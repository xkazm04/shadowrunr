//! `GET /events`: one WebSocket session per client. The client sends
//! `{"subscribe": "<run>|live", "speed"?, "since"?, "run_id"?}`; each message replaces the
//! current source.
//!
//! - Legacy mode (no `since`): each frame is one bare `StreamEvent`.
//! - Enveloped mode (`since` present, `0` = from the start): each frame is
//!   `{"seq", "run_id", "event"}`, and the source resumes after `since`. `seq` is the event's
//!   1-based index in its run's transcript, the same for a live run and its stored replay.
//!
//! Error frames are `{"error": {code, message, detail?}}` in both modes. A subscriber that
//! falls more than `BROADCAST_CAPACITY` events behind a live run loses the overflow and gets a
//! `busy` frame with `detail: {skipped, resume_since}`; resubscribing with that `since`
//! backfills the gap from the run's history.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{close_code, CloseFrame, Message, Utf8Bytes, WebSocket};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use shadowrunr_types::rpc::Subscribe;
use shadowrunr_types::ErrorCode;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;

use crate::live::{Attach, Frame, LiveItem, LiveRun};
use crate::rpc::RpcError;
use crate::runs;
use crate::server::AppState;

/// Frames queued for one socket before the source waits for the writer.
const SESSION_QUEUE: usize = 256;
pub const SHUTDOWN_REASON: &str = "daemon shutting down";

/// What the writer task sends.
pub enum Out {
    Text(Frame),
    Close(&'static str),
}

/// Writes frames for one subscription, in its mode.
pub struct Emitter {
    tx: mpsc::Sender<Out>,
    enveloped: bool,
}

impl Emitter {
    pub fn new(tx: mpsc::Sender<Out>, enveloped: bool) -> Self {
        Self { tx, enveloped }
    }

    /// Send one event; false once the socket is gone.
    pub async fn event(&self, run_id: &str, seq: u64, frame: Frame) -> bool {
        let frame = if self.enveloped {
            let id = serde_json::to_string(run_id).unwrap_or_else(|_| "\"\"".into());
            Frame::from(format!(
                "{{\"seq\":{seq},\"run_id\":{id},\"event\":{frame}}}"
            ))
        } else {
            frame
        };
        self.raw(frame).await
    }

    pub async fn raw(&self, frame: Frame) -> bool {
        self.tx.send(Out::Text(frame)).await.is_ok()
    }

    pub async fn error(&self, e: RpcError) -> bool {
        self.raw(Frame::from(e.body().to_string())).await
    }
}

/// The slow-consumer signal: `skipped` events were dropped after `resume_since`.
pub fn lag_error(skipped: u64, resume_since: u64) -> RpcError {
    RpcError::new(
        ErrorCode::Busy,
        format!("slow consumer: {skipped} events dropped; resubscribe with since={resume_since}"),
    )
    .with_detail(json!({ "skipped": skipped, "resume_since": resume_since }))
}

/// One WebSocket session.
pub async fn session(state: Arc<AppState>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Out>(SESSION_QUEUE);
    let writer = tokio::spawn(async move {
        while let Some(out) = rx.recv().await {
            let msg = match out {
                Out::Text(f) => Message::Text(f.as_ref().into()),
                Out::Close(reason) => {
                    let frame = CloseFrame {
                        code: close_code::AWAY,
                        reason: Utf8Bytes::from_static(reason),
                    };
                    let _ = sink.send(Message::Close(Some(frame))).await;
                    break;
                }
            };
            if sink.send(msg).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let shutdown = state.shutdown_signal();
    tokio::pin!(shutdown);
    let mut source: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        let msg = tokio::select! {
            m = stream.next() => m,
            () = &mut shutdown => {
                if let Some(h) = source.take() {
                    h.abort();
                }
                let _ = tx.send(Out::Close(SHUTDOWN_REASON)).await;
                break;
            }
        };
        let text = match msg {
            Some(Ok(Message::Text(t))) => t.to_string(),
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
            Some(Ok(_)) => continue,
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
                let msg = format!("expected {{\"subscribe\", \"speed\"?, \"since\"?}}: {e}");
                let _ = Emitter::new(tx.clone(), false)
                    .error(RpcError::invalid(msg))
                    .await;
            }
        }
    }
    if let Some(h) = source.take() {
        h.abort();
    }
    drop(tx);
    let _ = writer.await;
}

async fn run_source(state: Arc<AppState>, sub: Subscribe, tx: mpsc::Sender<Out>) {
    let emit = Emitter::new(tx, sub.since.is_some());
    let since = sub.since.unwrap_or(0);
    if sub.subscribe == "live" {
        if sub.since.is_some() && sub.run_id.is_none() {
            let e = RpcError::invalid("`since` with \"live\" needs the `run_id` it refers to");
            emit.error(e).await;
            return;
        }
        follow_live(&state, &emit, since, sub.run_id.as_deref()).await;
        return;
    }
    // A run in flight streams live (speed does not apply); otherwise replay from disk.
    let live_id = match sub.subscribe.split_once('/') {
        None => Some(sub.subscribe.as_str()),
        Some((t, r)) if t == state.cfg.sim.adhoc_dir => Some(r),
        Some(_) => None,
    };
    if let Some(run) = live_id.and_then(|id| state.live.get(id)) {
        let attach = run.attach(since);
        pump(&run.run_id, attach, since, &emit).await;
        return;
    }
    let speed = match sub.speed {
        None => 1.0,
        Some(s) if s.is_finite() && s > 0.0 => s,
        Some(s) => {
            emit.error(RpcError::invalid(format!("speed must be > 0, got {s}")))
                .await;
            return;
        }
    };
    replay(&state, &sub.subscribe, speed, since, &emit).await;
}

/// Deliver an attached live run: its history after `since`, then what follows, then its end.
/// Returns false once the socket is gone.
pub async fn pump(run_id: &str, attach: Attach, since: u64, emit: &Emitter) -> bool {
    let mut last = since;
    for (seq, f) in attach.history {
        last = seq;
        if !emit.event(run_id, seq, f).await {
            return false;
        }
    }
    let Some(mut rx) = attach.rx else {
        return match attach.error {
            Some(e) => emit.raw(e).await,
            None => true,
        };
    };
    loop {
        match rx.recv().await {
            Ok(LiveItem::Event(seq, f)) => {
                if seq <= last {
                    continue;
                }
                last = seq;
                if !emit.event(run_id, seq, f).await {
                    return false;
                }
            }
            Ok(LiveItem::End(error)) => {
                return match error {
                    Some(e) => emit.raw(e).await,
                    None => true,
                };
            }
            Err(RecvError::Lagged(skipped)) => {
                if !emit.error(lag_error(skipped, last)).await {
                    return false;
                }
            }
            Err(RecvError::Closed) => return true,
        }
    }
}

/// Replay a stored transcript after `since`, pacing events by their `ts_ms` gaps divided by
/// `speed`.
async fn replay(state: &AppState, run_ref: &str, speed: f64, since: u64, emit: &Emitter) {
    let found = runs::find_run(&state.cfg.paths.sim_runs(), run_ref, None).and_then(|e| {
        let lines = runs::read_transcript_lines(&e.dir.join("transcript.jsonl"))?;
        Ok((e.row.run_id, lines))
    });
    let (run_id, lines) = match found {
        Ok(f) => f,
        Err(e) => {
            emit.error(e).await;
            return;
        }
    };
    let mut prev: Option<i64> = None;
    let skip = usize::try_from(since).unwrap_or(usize::MAX);
    for (i, (line, ev)) in lines.into_iter().enumerate().skip(skip) {
        let ts = shadowrunr_sim::transcript::ts(&ev);
        if let Some(p) = prev {
            let gap_ms = (ts - p).max(0) as f64 / speed;
            if gap_ms >= 1.0 {
                tokio::time::sleep(Duration::from_secs_f64(gap_ms / 1000.0)).await;
            }
        }
        prev = Some(prev.map_or(ts, |p| p.max(ts)));
        if !emit.event(&run_id, i as u64 + 1, Frame::from(line)).await {
            return;
        }
    }
}

/// `{"subscribe": "live"}`: the run in flight (from its start, or after `since` when it is
/// `resume_run`), then each next run.
async fn follow_live(state: &AppState, emit: &Emitter, since: u64, resume_run: Option<&str>) {
    let mut latest = state.live.latest();
    let mut first = true;
    loop {
        let current: Option<Arc<LiveRun>> = latest.borrow_and_update().clone();
        if let Some(run) = current.filter(|r| !r.is_done()) {
            let from = if first && resume_run == Some(run.run_id.as_str()) {
                since
            } else {
                0
            };
            if !pump(&run.run_id, run.attach(from), from, emit).await {
                return;
            }
        }
        first = false;
        if latest.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(rx: &mut mpsc::Receiver<Out>) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(o) = rx.try_recv() {
            if let Out::Text(f) = o {
                out.push(f.to_string());
            }
        }
        out
    }

    #[tokio::test]
    async fn a_lagging_subscriber_is_told_where_to_resume() {
        let run = LiveRun::new("r1".into());
        let attach = run.attach(0);
        let total = crate::live::BROADCAST_CAPACITY as u64 + 100;
        for i in 0..total {
            run.push(Frame::from(format!("{{\"n\":{i}}}")));
        }
        run.finish(None);
        let (tx, mut rx) = mpsc::channel(4096);
        assert!(pump("r1", attach, 0, &Emitter::new(tx, true)).await);
        let got = texts(&mut rx);
        let signal: serde_json::Value = serde_json::from_str(&got[0]).unwrap();
        assert_eq!(signal["error"]["code"], "busy", "{signal}");
        assert_eq!(signal["error"]["detail"]["resume_since"], 0);
        let skipped = signal["error"]["detail"]["skipped"].as_u64().unwrap();
        assert!(skipped > 0);
        let first: serde_json::Value = serde_json::from_str(&got[1]).unwrap();
        assert_eq!(first["seq"], skipped + 1);
        assert_eq!(first["run_id"], "r1");
        let last: serde_json::Value = serde_json::from_str(got.last().unwrap()).unwrap();
        assert_eq!(last["seq"], total);

        // Resubscribing with `since` backfills exactly the dropped events.
        let (tx, mut rx) = mpsc::channel(4096);
        let backfill = run.attach(0);
        assert!(backfill.rx.is_none());
        pump("r1", backfill, 0, &Emitter::new(tx, true)).await;
        assert_eq!(texts(&mut rx).len() as u64, total);
    }

    #[tokio::test]
    async fn legacy_mode_sends_bare_frames() {
        let run = LiveRun::new("r2".into());
        run.push(Frame::from("{\"a\":1}"));
        run.finish(Some(RpcError::new(ErrorCode::Engine, "sim panicked: x")));
        let (tx, mut rx) = mpsc::channel(16);
        pump("r2", run.attach(0), 0, &Emitter::new(tx, false)).await;
        let got = texts(&mut rx);
        assert_eq!(got[0], "{\"a\":1}");
        assert!(got[1].contains("\"code\":\"engine\""), "{}", got[1]);
    }
}
