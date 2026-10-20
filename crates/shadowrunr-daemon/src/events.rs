//! `GET /events`: one WebSocket session per client. The client sends
//! `{"subscribe": "<run>|live", "speed"?, "since"?, "run_id"?}`; each message replaces the
//! current source.
//!
//! A client may also send `{"ack": {"seq", "gaps", "stalled_ms"}}` at any time: what it has
//! applied of the run it follows. Acks never change the source and get no reply; the
//! [`AckBook`] keeps them per connection for the sandbox's `stage_ack` check. A frame that
//! is neither (or a malformed ack) gets an `invalid_params` error frame.
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

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{close_code, CloseFrame, Message, Utf8Bytes, WebSocket};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use shadowrunr_types::rpc::Subscribe;
use shadowrunr_types::sandbox::{AckFrame, StageAck};
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

/// The run a connection last received an event from: the run its acks refer to.
pub type Following = Arc<Mutex<Option<String>>>;

/// What one connection acknowledged of one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AckRecord {
    /// The highest seq acked.
    pub seq: u64,
    /// The gaps the client reported (a running total, so the highest report).
    pub gaps: u32,
    /// The largest `stalled_ms` reported: ms since the client's last event. Routine acks carry
    /// a few seconds; the stage counts 20 s and more as a stall.
    pub max_stalled_ms: u64,
    /// Epoch ms of the last ack.
    pub last_ms: i64,
}

/// Stage acks per run id, per connection.
#[derive(Default)]
pub struct AckBook {
    next_conn: AtomicU64,
    runs: Mutex<HashMap<String, HashMap<u64, AckRecord>>>,
}

impl AckBook {
    fn runs(&self) -> std::sync::MutexGuard<'_, HashMap<String, HashMap<u64, AckRecord>>> {
        self.runs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A fresh connection id.
    pub fn connection(&self) -> u64 {
        self.next_conn.fetch_add(1, Ordering::Relaxed)
    }

    pub fn record(&self, run_id: &str, conn: u64, ack: StageAck, now_ms: i64) {
        let mut runs = self.runs();
        let r = runs
            .entry(run_id.to_string())
            .or_default()
            .entry(conn)
            .or_insert(AckRecord {
                seq: 0,
                gaps: 0,
                max_stalled_ms: 0,
                last_ms: now_ms,
            });
        r.seq = r.seq.max(ack.seq);
        r.gaps = r.gaps.max(ack.gaps);
        r.max_stalled_ms = r.max_stalled_ms.max(ack.stalled_ms);
        r.last_ms = now_ms;
    }

    /// Every connection's record for a run.
    pub fn for_run(&self, run_id: &str) -> Vec<AckRecord> {
        self.runs()
            .get(run_id)
            .map(|m| m.values().copied().collect())
            .unwrap_or_default()
    }

    /// The highest seq any connection acked for a run.
    pub fn max_seq(&self, run_id: &str) -> Option<u64> {
        self.for_run(run_id).iter().map(|r| r.seq).max()
    }
}

/// A frame a client sends.
#[derive(Debug, PartialEq)]
enum ClientFrame {
    Subscribe(Subscribe),
    Ack(StageAck),
    /// An `ack` frame that does not parse: an error frame, the source is kept.
    BadAck(String),
    /// Anything else: an error frame, and the source stops.
    Bad(String),
}

fn parse_frame(text: &str) -> ClientFrame {
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return ClientFrame::Bad(format!("expected a JSON object: {e}")),
    };
    if v.get("ack").is_some() {
        return match serde_json::from_value::<AckFrame>(v) {
            Ok(f) => ClientFrame::Ack(f.ack),
            Err(e) => ClientFrame::BadAck(format!(
                "expected {{\"ack\": {{\"seq\", \"gaps\", \"stalled_ms\"}}}}: {e}"
            )),
        };
    }
    match serde_json::from_value::<Subscribe>(v) {
        Ok(sub) => ClientFrame::Subscribe(sub),
        Err(e) => ClientFrame::Bad(format!(
            "expected {{\"subscribe\", \"speed\"?, \"since\"?}}: {e}"
        )),
    }
}

/// Writes frames for one subscription, in its mode.
pub struct Emitter {
    tx: mpsc::Sender<Out>,
    enveloped: bool,
    following: Following,
}

impl Emitter {
    pub fn new(tx: mpsc::Sender<Out>, enveloped: bool) -> Self {
        Self::following(tx, enveloped, Following::default())
    }

    /// An emitter that notes, in `following`, the run each event came from.
    pub fn following(tx: mpsc::Sender<Out>, enveloped: bool, following: Following) -> Self {
        Self {
            tx,
            enveloped,
            following,
        }
    }

    /// Send one event; false once the socket is gone.
    pub async fn event(&self, run_id: &str, seq: u64, frame: Frame) -> bool {
        {
            let mut f = self.following.lock().unwrap_or_else(|p| p.into_inner());
            if f.as_deref() != Some(run_id) {
                *f = Some(run_id.to_string());
            }
        }
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
    let conn = state.acks.connection();
    let following = Following::default();
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
        let sub = match parse_frame(&text) {
            ClientFrame::Ack(ack) => {
                let run = following.lock().unwrap_or_else(|p| p.into_inner()).clone();
                if let Some(run) = run {
                    state.acks.record(&run, conn, ack, runs::now_ms());
                }
                continue;
            }
            ClientFrame::BadAck(msg) => {
                let _ = Emitter::new(tx.clone(), false)
                    .error(RpcError::invalid(msg))
                    .await;
                continue;
            }
            ClientFrame::Subscribe(sub) => Ok(sub),
            ClientFrame::Bad(msg) => Err(msg),
        };
        if let Some(h) = source.take() {
            h.abort();
        }
        match sub {
            Ok(sub) => {
                let state = state.clone();
                let emit = Emitter::following(tx.clone(), sub.since.is_some(), following.clone());
                source = Some(tokio::spawn(async move {
                    run_source(state, sub, emit).await;
                }));
            }
            Err(msg) => {
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

async fn run_source(state: Arc<AppState>, sub: Subscribe, emit: Emitter) {
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

    #[test]
    fn client_frames_are_subscribe_ack_or_refused() {
        assert!(matches!(
            parse_frame(r#"{"subscribe":"live"}"#),
            ClientFrame::Subscribe(s) if s.subscribe == "live"
        ));
        assert_eq!(
            parse_frame(r#"{"ack":{"seq":9,"gaps":1,"stalled_ms":20}}"#),
            ClientFrame::Ack(StageAck {
                seq: 9,
                gaps: 1,
                stalled_ms: 20
            })
        );
        assert!(matches!(
            parse_frame(r#"{"ack":{"seq":"x"}}"#),
            ClientFrame::BadAck(_)
        ));
        assert!(matches!(parse_frame(r#"{"hello":1}"#), ClientFrame::Bad(_)));
        assert!(matches!(parse_frame("not json"), ClientFrame::Bad(_)));
    }

    #[test]
    fn the_ack_book_keeps_each_connection_per_run() {
        let book = AckBook::default();
        let (a, b) = (book.connection(), book.connection());
        assert_ne!(a, b);
        assert_eq!(book.max_seq("r"), None);
        let ack = |seq, gaps, stalled_ms| StageAck {
            seq,
            gaps,
            stalled_ms,
        };
        book.record("r", a, ack(10, 0, 0), 1);
        book.record("r", a, ack(7, 2, 900), 2);
        book.record("r", b, ack(12, 0, 0), 3);
        book.record("other", b, ack(99, 0, 0), 4);
        let mut recs = book.for_run("r");
        recs.sort_by_key(|r| r.seq);
        assert_eq!(
            recs,
            vec![
                AckRecord {
                    seq: 10,
                    gaps: 2,
                    max_stalled_ms: 900,
                    last_ms: 2
                },
                AckRecord {
                    seq: 12,
                    gaps: 0,
                    max_stalled_ms: 0,
                    last_ms: 3
                },
            ]
        );
        assert_eq!(book.max_seq("r"), Some(12));
    }

    #[tokio::test]
    async fn an_emitter_notes_the_run_it_follows() {
        let (tx, _rx) = mpsc::channel(4);
        let following = Following::default();
        let emit = Emitter::following(tx, true, following.clone());
        assert!(emit.event("sbx-1", 1, Frame::from("{}")).await);
        assert_eq!(following.lock().unwrap().as_deref(), Some("sbx-1"));
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
