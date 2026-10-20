//! The daemon: axum on 127.0.0.1 only, serving `GET /events` (WebSocket, see `events`) and
//! `POST /rpc` (docs/spec/contract.md § Daemon protocol).
//!
//! Browser access is limited to `http://localhost:*`, `tauri://localhost` and
//! `http://tauri.localhost` (the Tauri v2 window origin on Windows), plus any
//! `[daemon] extra_origins`: CORS answers only those origins, and a request or WebSocket
//! upgrade that carries any other `Origin` is refused, so a web page cannot drive the daemon
//! (and spend quota) from the operator's browser. Non-browser clients send no `Origin`.
//!
//! Shutdown is graceful: [`AppState::shutdown`] stops accepting connections, and every open
//! `/events` socket is closed with code 1001 and the reason `daemon shutting down`.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use shadowrunr_reconcile::{ReconcileConfig, Reconciler};
use shadowrunr_types::rpc::RpcRequest;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::config::Config;
use crate::engine::EngineHandle;
use crate::events::{self, AckBook};
use crate::live::LiveRegistry;
use crate::rpc::{self, RpcError};
use crate::sandbox::SandboxRegistry;

pub struct AppState {
    pub cfg: Arc<Config>,
    pub engine: EngineHandle,
    pub reconciler: Reconciler,
    pub live: LiveRegistry,
    /// Stage acks from `/events` clients, per run.
    pub acks: AckBook,
    /// Sandbox sessions hosted since the daemon started.
    pub sandbox: SandboxRegistry,
    shutdown: watch::Sender<bool>,
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
            acks: AckBook::default(),
            sandbox: SandboxRegistry::default(),
            shutdown: watch::Sender::new(false),
        })
    }

    /// Begin a graceful shutdown: the server stops accepting and `/events` sockets close.
    pub fn shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    /// Resolves once [`AppState::shutdown`] was called (immediately if it already was).
    pub fn shutdown_signal(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let mut rx = self.shutdown.subscribe();
        async move {
            let _ = rx.wait_for(|down| *down).await;
        }
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

pub(crate) fn origin_ok(headers: &HeaderMap, extra: &[String]) -> bool {
    match headers.get(header::ORIGIN) {
        None => true,
        Some(v) => v.to_str().is_ok_and(|o| origin_allowed(o, extra)),
    }
}

fn error_response(status: StatusCode, e: RpcError) -> Response {
    (status, Json(e.body())).into_response()
}

/// A request refused before dispatch: `invalid_params` with the old code as the reason.
fn refused(status: StatusCode, reason: &str, message: impl Into<String>) -> Response {
    error_response(
        status,
        RpcError::invalid(message).with_detail(json!({ "reason": reason })),
    )
}

fn forbidden_origin() -> Response {
    refused(
        StatusCode::FORBIDDEN,
        "forbidden_origin",
        "origin not allowed",
    )
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
        .route("/voice", get(crate::voice::voice_handler))
        .route("/avatar/{file}", get(crate::voice::avatar_handler))
        .layer(cors)
        .with_state(state)
}

async fn rpc_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !origin_ok(&headers, &state.cfg.daemon.extra_origins) {
        return forbidden_origin();
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
        return refused(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "content-type must be application/json",
        );
    }
    let req: RpcRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return refused(StatusCode::BAD_REQUEST, "parse_error", e.to_string()),
    };
    let body = match rpc::dispatch(&state, req).await {
        Ok(result) => json!({ "result": result }),
        Err(e) => e.body(),
    };
    Json(body).into_response()
}

async fn events_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_ok(&headers, &state.cfg.daemon.extra_origins) {
        return forbidden_origin();
    }
    ws.on_upgrade(move |socket| events::session(state, socket))
}

/// Bind 127.0.0.1:`port` (0 = any free port) and return the bound address and the server
/// future, which completes after [`AppState::shutdown`] once connections have drained.
pub async fn bind(
    state: Arc<AppState>,
    port: u16,
) -> std::io::Result<(
    SocketAddr,
    impl std::future::Future<Output = std::io::Result<()>>,
)> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    let addr = listener.local_addr()?;
    let signal = state.shutdown_signal();
    let app = router(state);
    Ok((addr, async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(signal)
            .await
    }))
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
