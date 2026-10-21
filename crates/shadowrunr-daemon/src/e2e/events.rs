//! `/events` resume: enveloped frames carry a seq, and `since` resumes without gaps for
//! stored replays and live runs alike.

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use shadowrunr_types::rpc::EventFrame;
use tokio_tungstenite::tungstenite::Message;

use super::{fixture_dir, repo, rpc, spec, start, test_config};

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn subscribe(addr: &str, msg: Value) -> Ws {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    ws.send(Message::text(msg.to_string())).await.unwrap();
    ws
}

/// Read text frames until `stop` says so (or the socket ends), within `secs`.
async fn read_until(ws: &mut Ws, secs: u64, mut stop: impl FnMut(&Value) -> bool) -> Vec<Value> {
    let mut got = Vec::new();
    let read = async {
        while let Some(Ok(m)) = ws.next().await {
            if let Message::Text(t) = m {
                let v: Value = serde_json::from_str(&t).unwrap();
                let done = stop(&v);
                got.push(v);
                if done {
                    break;
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(secs), read)
        .await
        .expect("frames in time");
    got
}

fn is_end(v: &Value) -> bool {
    v["event"]["type"] == "stream_end" || v.get("error").is_some()
}

#[tokio::test]
async fn replay_resumes_from_a_seq() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;
    let lines: Vec<Value> =
        std::fs::read_to_string(fixture_dir().join("fixture-t/fixture-run/transcript.jsonl"))
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

    let mut ws = subscribe(
        &addr,
        json!({ "subscribe": "fixture-run", "speed": 1000.0, "since": 0 }),
    )
    .await;
    let all = read_until(&mut ws, 20, is_end).await;
    assert_eq!(all.len(), lines.len());
    for (i, f) in all.iter().enumerate() {
        let frame: EventFrame = serde_json::from_value(f.clone()).unwrap();
        assert_eq!(frame.seq, i as u64 + 1);
        assert_eq!(frame.run_id, "fixture-run");
        assert_eq!(f["event"], lines[i]);
    }

    // Reconnect after seq 6: exactly the rest, nothing repeated.
    let mut ws = subscribe(
        &addr,
        json!({ "subscribe": "fixture-t/fixture-run", "speed": 1000.0, "since": 6 }),
    )
    .await;
    let rest = read_until(&mut ws, 20, is_end).await;
    let seqs: Vec<u64> = rest.iter().map(|f| f["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, (7..=lines.len() as u64).collect::<Vec<_>>());
    assert_eq!(rest[0]["event"], lines[6]);

    // `live` resume needs the run the seq belongs to.
    let mut ws = subscribe(&addr, json!({ "subscribe": "live", "since": 3 })).await;
    let err = read_until(&mut ws, 5, |_| true).await;
    assert_eq!(err[0]["error"]["code"], "invalid_params", "{err:?}");
}

#[tokio::test]
async fn live_run_resumes_from_a_seq_after_a_disconnect() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;
    let started = rpc(
        &addr,
        "sim.start",
        json!({ "spec": spec("resume-1"), "judge": false }),
    )
    .await;
    assert_eq!(started["result"]["run_id"], "resume-1", "{started}");

    let mut ws = subscribe(
        &addr,
        json!({ "subscribe": "resume-1", "speed": 1000.0, "since": 0 }),
    )
    .await;
    let head = read_until(&mut ws, 60, |v| v["seq"] == 3 || is_end(v)).await;
    drop(ws);
    let last = head.last().unwrap()["seq"].as_u64().unwrap();

    // The run may still be in flight or already on disk: either way the seq continues.
    let mut ws = subscribe(
        &addr,
        json!({ "subscribe": "resume-1", "speed": 1000.0, "since": last }),
    )
    .await;
    let tail = read_until(&mut ws, 120, is_end).await;
    assert!(tail.iter().all(|f| f.get("error").is_none()), "{tail:?}");
    let all: Vec<&Value> = head.iter().chain(tail.iter()).collect();
    for (i, f) in all.iter().enumerate() {
        assert_eq!(f["seq"].as_u64().unwrap(), i as u64 + 1, "contiguous seq");
    }
    let on_disk =
        std::fs::read_to_string(tmp.path().join("runs/sim/adhoc/resume-1/transcript.jsonl"))
            .unwrap();
    let disk: Vec<Value> = on_disk
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(all.len(), disk.len());
    for (f, d) in all.iter().zip(&disk) {
        assert_eq!(&f["event"], d);
    }
}

/// The scripted engine, except that its first model call waits for `gate`.
struct GatedEngine {
    gate: std::sync::Arc<tokio::sync::Notify>,
    first: std::sync::atomic::AtomicBool,
}

impl GatedEngine {
    async fn wait_first(&self) {
        if self.first.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.gate.notified().await;
        }
    }
}

#[async_trait::async_trait]
impl shadowrunr_types::Engine for GatedEngine {
    async fn complete(
        &self,
        req: shadowrunr_types::EngineRequest,
    ) -> Result<shadowrunr_types::EngineReply, shadowrunr_types::EngineError> {
        self.wait_first().await;
        shadowrunr_engine::ScriptedEngine.complete(req).await
    }
    async fn open_session(
        &self,
        template: shadowrunr_types::EngineRequest,
    ) -> Result<Box<dyn shadowrunr_types::WarmSession>, shadowrunr_types::EngineError> {
        self.wait_first().await;
        shadowrunr_engine::ScriptedEngine
            .open_session(template)
            .await
    }
    async fn health(&self) -> Vec<shadowrunr_types::HealthReport> {
        shadowrunr_engine::ScriptedEngine.health().await
    }
}

/// Opens the gate when the test ends, pass or fail: the runtime waits for the sim's blocking
/// thread on shutdown, so a gate left shut would hang the test instead of failing it.
struct OpenOnDrop(std::sync::Arc<tokio::sync::Notify>);

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

/// A live sim's events reach subscribers as they are written, not when a buffer fills: with
/// the engine's first answer held back, `stream_start` and the stage's opening are already
/// on the socket.
#[tokio::test]
async fn live_sim_events_arrive_before_the_engine_answers() {
    let tmp = tempfile::tempdir().unwrap();
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    let _open = OpenOnDrop(gate.clone());
    let engine = crate::engine::EngineHandle {
        engine: std::sync::Arc::new(GatedEngine {
            gate: gate.clone(),
            first: std::sync::atomic::AtomicBool::new(true),
        }),
        fleet: None,
    };
    let state =
        crate::server::AppState::new(test_config(tmp.path(), repo().join("characters")), engine);
    let addr = super::start_state(state).await;
    let started = rpc(
        &addr,
        "sim.start",
        json!({ "spec": spec("gated-1"), "judge": false }),
    )
    .await;
    assert_eq!(started["result"]["run_id"], "gated-1", "{started}");

    let mut ws = subscribe(&addr, json!({ "subscribe": "gated-1", "since": 0 })).await;
    // The gate, not the clock, is the claim: the engine is held until the frames are read, so
    // the deadline only bounds a failure (2 s timed out under a loaded workspace test run).
    let head = read_until(&mut ws, 20, |v| {
        v["event"]["type"] == "segment_start" || is_end(v)
    })
    .await;
    assert_eq!(head[0]["seq"], 1, "{head:?}");
    assert_eq!(head[0]["event"]["type"], "stream_start", "{head:?}");
    let last = head.last().unwrap();
    assert_eq!(last["event"]["type"], "segment_start", "{head:?}");
    for (i, f) in head.iter().enumerate() {
        assert_eq!(f["seq"].as_u64().unwrap(), i as u64 + 1, "{head:?}");
    }

    // Released, the run completes on the same socket.
    gate.notify_one();
    let rest = read_until(&mut ws, 120, is_end).await;
    assert_eq!(
        rest.last().unwrap()["event"]["type"],
        "stream_end",
        "{rest:?}"
    );
}

/// Every live frame's seq is its line in the run's `transcript.jsonl`, and its event is that
/// line: N frames for N lines, no gaps.
#[tokio::test]
async fn live_seq_is_the_transcript_line() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;
    let started = rpc(
        &addr,
        "sim.start",
        json!({ "spec": spec("seq-1"), "judge": false }),
    )
    .await;
    assert_eq!(started["result"]["run_id"], "seq-1", "{started}");
    let mut ws = subscribe(&addr, json!({ "subscribe": "seq-1", "since": 0 })).await;
    let all = read_until(&mut ws, 120, is_end).await;
    assert!(all.iter().all(|f| f.get("error").is_none()), "{all:?}");
    let disk: Vec<Value> =
        std::fs::read_to_string(tmp.path().join("runs/sim/adhoc/seq-1/transcript.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
    assert_eq!(all.len(), disk.len(), "N frames for N lines");
    for (i, (f, d)) in all.iter().zip(&disk).enumerate() {
        let frame: EventFrame = serde_json::from_value(f.clone()).unwrap();
        assert_eq!(frame.seq, i as u64 + 1, "seq is the 1-based line");
        assert_eq!(frame.run_id, "seq-1");
        assert_eq!(&f["event"], d, "line {}", i + 1);
    }
}
