//! The daemon: axum on 127.0.0.1 only, serving `GET /events` (WebSocket) and `POST /rpc`
//! (docs/spec/contract.md § Daemon protocol).
//!
//! Browser access is limited to `http://localhost:*`, `tauri://localhost` and
//! `http://tauri.localhost` (the Tauri v2 window origin on Windows), plus any
//! `[daemon] extra_origins`: CORS answers only those origins, and a request or WebSocket
//! upgrade that carries any other `Origin` is refused, so a web page cannot drive the daemon
//! (and spend quota) from the operator's browser. Non-browser clients send no `Origin`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use shadowrunr_reconcile::{ReconcileConfig, Reconciler};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::config::Config;
use crate::engine::EngineHandle;
use crate::live::{Frame, LiveRegistry};
use crate::rpc::{self, RpcError, RpcRequest};
use crate::runs;

pub struct AppState {
    pub cfg: Arc<Config>,
    pub engine: EngineHandle,
    pub reconciler: Reconciler,
    pub live: LiveRegistry,
}

impl AppState {
    pub fn new(cfg: Config, engine: EngineHandle) -> Arc<Self> {
        let reconciler = Reconciler::new(
            ReconcileConfig {
                data_dir: cfg.paths.data.clone(),
                runs_root: cfg.paths.sim_runs(),
                characters_root: Some(cfg.paths.characters.clone()),
            },
            engine.engine.clone(),
        );
        Arc::new(Self {
            cfg: Arc::new(cfg),
            engine,
            reconciler,
            live: LiveRegistry::default(),
        })
    }
}

/// Built-in exact origins: the Tauri v2 window origin on macOS/Linux and on Windows.
pub const TAURI_ORIGINS: [&str; 2] = ["tauri://localhost", "http://tauri.localhost"];

/// `http://localhost` with an optional numeric port, a Tauri window, or an exact extra.
pub fn origin_allowed(origin: &str, extra: &[String]) -> bool {
    if TAURI_ORIGINS.contains(&origin) || extra.iter().any(|e| e == origin) {
        return true;
    }
    match origin.strip_prefix("http://localhost") {
        Some("") => true,
        Some(rest) => rest.strip_prefix(':').is_some_and(|p| {
            !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit())
        }),
        None => false,
    }
}

fn origin_ok(headers: &HeaderMap, extra: &[String]) -> bool {
    match headers.get(header::ORIGIN) {
        None => true,
        Some(v) => v.to_str().is_ok_and(|o| origin_allowed(o, extra)),
    }
}

fn error_response(status: StatusCode, e: RpcError) -> Response {
    (status, Json(e.body())).into_response()
}

pub fn router(state: Arc<AppState>) -> Router {
    let extra = state.cfg.daemon.extra_origins.clone();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |o: &HeaderValue, _| {
            o.to_str().is_ok_and(|o| origin_allowed(o, &extra))
        }))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE]);
    Router::new()
        .route("/rpc", post(rpc_handler))
        .route("/events", get(events_handler))
        .layer(cors)
        .with_state(state)
}

async fn rpc_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !origin_ok(&headers, &state.cfg.daemon.extra_origins) {
        return error_response(
            StatusCode::FORBIDDEN,
            RpcError::new("forbidden_origin", "origin not allowed"),
        );
    }
    // Requiring application/json forces a CORS preflight for any cross-origin browser call.
    let json_ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.trim_start()
                .to_ascii_lowercase()
                .starts_with("application/json")
        });
    if !json_ct {
        return error_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            RpcError::new(
                "unsupported_media_type",
                "content-type must be application/json",
            ),
        );
    }
    let req: RpcRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                RpcError::new("parse_error", e.to_string()),
            )
        }
    };
    let body = match rpc::dispatch(&state, req).await {
        Ok(result) => serde_json::json!({ "result": result }),
        Err(e) => e.body(),
    };
    Json(body).into_response()
}

#[derive(Debug, Deserialize)]
struct Subscribe {
    subscribe: String,
    #[serde(default)]
    speed: Option<f64>,
}

async fn events_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_ok(&headers, &state.cfg.daemon.extra_origins) {
        return error_response(
            StatusCode::FORBIDDEN,
            RpcError::new("forbidden_origin", "origin not allowed"),
        );
    }
    ws.on_upgrade(move |socket| events_session(state, socket))
}

/// One WebSocket session. Each `{"subscribe", "speed"}` message replaces the current source.
async fn events_session(state: Arc<AppState>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Frame>(256);
    let writer = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if sink
                .send(Message::Text(frame.as_ref().into()))
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let mut source: Option<tokio::task::JoinHandle<()>> = None;
    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };
        if let Some(h) = source.take() {
            h.abort();
        }
        match serde_json::from_str::<Subscribe>(&text) {
            Ok(sub) => {
                let state = state.clone();
                let tx = tx.clone();
                source = Some(tokio::spawn(async move {
                    run_source(state, sub, tx).await;
                }));
            }
            Err(e) => {
                let frame =
                    RpcError::invalid(format!("expected {{\"subscribe\", \"speed\"}}: {e}"))
                        .body()
                        .to_string();
                let _ = tx.send(Arc::from(frame)).await;
            }
        }
    }
    if let Some(h) = source.take() {
        h.abort();
    }
    drop(tx);
    let _ = writer.await;
}

async fn send_error(tx: &mpsc::Sender<Frame>, e: RpcError) {
    let _ = tx.send(Arc::from(e.body().to_string())).await;
}

async fn run_source(state: Arc<AppState>, sub: Subscribe, tx: mpsc::Sender<Frame>) {
    if sub.subscribe == "live" {
        follow_live(&state, &tx).await;
        return;
    }
    // A run in flight streams live (speed does not apply); otherwise replay from disk.
    let live_id = match sub.subscribe.split_once('/') {
        None => Some(sub.subscribe.as_str()),
        Some((t, r)) if t == state.cfg.sim.adhoc_dir => Some(r),
        Some(_) => None,
    };
    if let Some(run) = live_id.and_then(|id| state.live.get(id)) {
        let (history, recv) = run.attach();
        for f in history {
            if tx.send(f).await.is_err() {
                return;
            }
        }
        if let Some(mut recv) = recv {
            while let Some(f) = recv.recv().await {
                if tx.send(f).await.is_err() {
                    return;
                }
            }
        }
        return;
    }
    let speed = match sub.speed {
        None => 1.0,
        Some(s) if s.is_finite() && s > 0.0 => s,
        Some(s) => {
            send_error(
                &tx,
                RpcError::invalid(format!("speed must be > 0, got {s}")),
            )
            .await;
            return;
        }
    };
    replay(&state, &sub.subscribe, speed, &tx).await;
}

/// Replay a stored transcript, pacing events by their `ts_ms` gaps divided by `speed`.
async fn replay(state: &AppState, run_ref: &str, speed: f64, tx: &mpsc::Sender<Frame>) {
    let lines = runs::find_run(&state.cfg.paths.sim_runs(), run_ref, None)
        .and_then(|e| runs::read_transcript_lines(&e.dir.join("transcript.jsonl")));
    let lines = match lines {
        Ok(l) => l,
        Err(e) => {
            send_error(tx, e).await;
            return;
        }
    };
    let mut prev: Option<i64> = None;
    for (line, ev) in lines {
        let ts = shadowrunr_sim::transcript::ts(&ev);
        if let Some(p) = prev {
            let gap_ms = (ts - p).max(0) as f64 / speed;
            if gap_ms >= 1.0 {
                tokio::time::sleep(Duration::from_secs_f64(gap_ms / 1000.0)).await;
            }
        }
        prev = Some(prev.map_or(ts, |p| p.max(ts)));
        if tx.send(Arc::from(line)).await.is_err() {
            return;
        }
    }
}

/// `{"subscribe": "live"}`: the run in flight (from its start), then each next run.
async fn follow_live(state: &AppState, tx: &mpsc::Sender<Frame>) {
    let mut latest = state.live.latest();
    loop {
        let current = latest.borrow_and_update().clone();
        if let Some(run) = current {
            let (history, recv) = run.attach();
            if let Some(mut recv) = recv {
                for f in history {
                    if tx.send(f).await.is_err() {
                        return;
                    }
                }
                while let Some(f) = recv.recv().await {
                    if tx.send(f).await.is_err() {
                        return;
                    }
                }
            }
        }
        if latest.changed().await.is_err() {
            return;
        }
    }
}

/// Bind 127.0.0.1:`port` (0 = any free port) and return the bound address and the server
/// future.
pub async fn bind(
    state: Arc<AppState>,
    port: u16,
) -> std::io::Result<(
    SocketAddr,
    impl std::future::Future<Output = std::io::Result<()>>,
)> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    let addr = listener.local_addr()?;
    let app = router(state);
    Ok((addr, async move { axum::serve(listener, app).await }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_and_tauri_origins_are_allowed() {
        for ok in [
            "http://localhost",
            "http://localhost:1420",
            "http://localhost:5173",
            "tauri://localhost",
            "http://tauri.localhost",
        ] {
            assert!(origin_allowed(ok, &[]), "{ok}");
        }
        for bad in [
            "https://evil.example",
            "http://localhost.evil.example",
            "http://localhost:80x",
            "http://localhost:",
            "https://localhost:1420",
            "http://127.0.0.1:1420",
            "null",
        ] {
            assert!(!origin_allowed(bad, &[]), "{bad}");
        }
        assert!(origin_allowed("http://x.local", &["http://x.local".into()]));
    }
}
