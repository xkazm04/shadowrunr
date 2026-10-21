//! Sandbox sessions over real sockets: `/events` carries the session live with its seq, and the
//! stage's ack frames drive the `stage_ack` check.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use shadowrunr_types::sandbox::{
    SandboxCheckState, SandboxStartParams, SandboxState, SandboxStatus,
};
use tokio_tungstenite::tungstenite::Message;

use super::{repo, rpc, start_state, test_config};
use crate::engine::EngineHandle;
use crate::sandbox::tests::{fake_judge, fake_run, hosting, params};
use crate::sandbox::{start_with, Hosting};
use crate::server::AppState;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Follow `session_id` from its first event, ack every frame's seq, and return the seqs of
/// the events received once `stream_end` arrives.
async fn watch_and_ack(ws: &mut Ws, session_id: &str, secs: u64) -> Vec<u64> {
    ws.send(Message::text(
        json!({ "subscribe": "live", "since": 0, "run_id": session_id }).to_string(),
    ))
    .await
    .unwrap();
    let mut seqs = Vec::new();
    let read = async {
        while let Some(Ok(m)) = ws.next().await {
            let Message::Text(t) = m else { continue };
            let v: Value = serde_json::from_str(&t).unwrap();
            assert!(v.get("error").is_none(), "{v}");
            let seq = v["seq"].as_u64().unwrap();
            assert_eq!(v["run_id"], session_id);
            seqs.push(seq);
            let ack = json!({ "ack": { "seq": seq, "gaps": 0, "stalled_ms": 0 } });
            ws.send(Message::text(ack.to_string())).await.unwrap();
            if v["event"]["type"] == "stream_end" {
                break;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(secs), read)
        .await
        .expect("the session ended in time");
    seqs
}

/// The next text frame, within 10 s.
async fn next_frame(ws: &mut Ws) -> Value {
    let read = async {
        loop {
            if let Some(Ok(Message::Text(t))) = ws.next().await {
                return serde_json::from_str::<Value>(&t).unwrap();
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .expect("a frame in time")
}
async fn until_ended(addr: &str, session_id: &str, secs: u64) -> SandboxStatus {
    let poll = async {
        loop {
            let v = rpc(addr, "sandbox.status", json!({ "session_id": session_id })).await;
            let st: SandboxStatus =
                serde_json::from_value(v["result"].clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
            if matches!(
                st.state,
                SandboxState::Done | SandboxState::Failed | SandboxState::Stopped
            ) {
                return st;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(secs), poll)
        .await
        .expect("the session reported in time")
}

fn check(st: &SandboxStatus, id: &str) -> SandboxCheckState {
    st.report
        .as_ref()
        .and_then(|r| r.checks.iter().find(|c| c.id == id))
        .map(|c| c.state)
        .unwrap_or_else(|| panic!("no {id} check: {st:?}"))
}

/// Hosted with a fake driver, so this runs now: acks over the socket pass `stage_ack`, and
/// ack frames never disturb the subscription.
#[tokio::test]
async fn acks_over_events_drive_the_stage_ack_check() {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::new(
        test_config(tmp.path(), repo().join("characters")),
        EngineHandle::scripted(),
    );
    let addr = start_state(state.clone()).await;
    let mut h = hosting(
        fake_run(6, Duration::from_secs(2), None, false),
        fake_judge(Some(0)),
    );
    h.ack_grace = Duration::from_secs(5);
    let started = start_with(state.clone(), params(true), h).await.unwrap();

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    let seqs = watch_and_ack(&mut ws, &started.session_id, 20).await;
    assert_eq!(seqs, (1..=6).collect::<Vec<u64>>());

    let end = until_ended(&addr, &started.session_id, 20).await;
    assert_eq!(end.state, SandboxState::Done, "{end:?}");
    assert_eq!(check(&end, "stage_ack"), SandboxCheckState::Pass, "{end:?}");
    assert_eq!(end.stage_ack_seq, Some(6));
    assert!(end.report.unwrap().pass);
}

/// An ack mid-stream keeps the subscription: events pushed after it still arrive, nothing
/// answers it, and the ack book has it. A malformed ack is refused without stopping the source.
#[tokio::test]
async fn an_ack_never_stops_the_source() {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::new(
        test_config(tmp.path(), repo().join("characters")),
        EngineHandle::scripted(),
    );
    let addr = start_state(state.clone()).await;
    let run = state.live.register("ack-run", 4).unwrap();
    let event = |n: i64| {
        crate::live::Frame::from(
            json!({ "type": "health", "ts_ms": n, "state": "healthy" }).to_string(),
        )
    };
    run.push(event(1));
    run.push(event(2));

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    ws.send(Message::text(
        json!({ "subscribe": "live", "since": 0, "run_id": "ack-run" }).to_string(),
    ))
    .await
    .unwrap();
    assert_eq!(next_frame(&mut ws).await["seq"], 1);
    assert_eq!(next_frame(&mut ws).await["seq"], 2);

    // Both acks are sent before the next events exist.
    let ack = json!({ "ack": { "seq": 2, "gaps": 0, "stalled_ms": 21000 } });
    ws.send(Message::text(ack.to_string())).await.unwrap();
    ws.send(Message::text(r#"{"ack":{"seq":"x"}}"#))
        .await
        .unwrap();
    let refused = next_frame(&mut ws).await;
    assert_eq!(refused["error"]["code"], "invalid_params", "{refused}");
    run.push(event(3));
    run.push(event(4));
    let (third, fourth) = (next_frame(&mut ws).await, next_frame(&mut ws).await);
    assert_eq!(
        (third["seq"].clone(), fourth["seq"].clone()),
        (json!(3), json!(4))
    );
    assert_eq!(third["run_id"], "ack-run");

    let recs = state.acks.for_run("ack-run");
    assert_eq!(recs.len(), 1, "{recs:?}");
    assert_eq!(
        (recs[0].seq, recs[0].gaps, recs[0].max_stalled_ms),
        (2, 0, 21_000)
    );
    state.live.remove("ack-run");
    run.finish(None);
}

fn sixty_seconds() -> Hosting {
    Hosting {
        planned_s: Some(60),
        ..Hosting::default()
    }
}

fn scripted_state(tmp: &std::path::Path) -> Arc<AppState> {
    AppState::new(
        test_config(tmp, repo().join("characters")),
        EngineHandle::scripted(),
    )
}

#[tokio::test]
async fn a_scripted_minute_with_an_acking_stage_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let state = scripted_state(tmp.path());
    let addr = start_state(state.clone()).await;
    let p = SandboxStartParams {
        minutes: 5,
        seed: Some(7),
        chaos: None,
        judge: true,
        tone: None,
        record: false,
    };
    let started = start_with(state.clone(), p, sixty_seconds()).await.unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    let seqs = watch_and_ack(&mut ws, &started.session_id, 120).await;
    assert_eq!(seqs, (1..=seqs.len() as u64).collect::<Vec<u64>>());

    let end = until_ended(&addr, &started.session_id, 300).await;
    assert_eq!(end.state, SandboxState::Done, "{end:?}");
    assert_eq!(check(&end, "stage_ack"), SandboxCheckState::Pass, "{end:?}");
    assert_eq!(end.stage_ack_seq, Some(seqs.len() as u64));
    // The scripted engine never signs off and does not reply with rubric JSON, so those two
    // checks fail here; everything the hosting owns passes.
    for id in ["say_gap", "core_errors", "stage_ack"] {
        assert_eq!(check(&end, id), SandboxCheckState::Pass, "{id}: {end:?}");
    }
    assert_eq!(
        check(&end, "chaos"),
        SandboxCheckState::Pass,
        "no chaos requested"
    );
    assert!(end.report.as_ref().unwrap().judge.is_none() || !end.report.as_ref().unwrap().pass);
    let on_disk =
        std::fs::read_to_string(std::path::Path::new(&started.dir).join("transcript.jsonl"))
            .unwrap();
    assert_eq!(
        on_disk.lines().filter(|l| !l.trim().is_empty()).count(),
        seqs.len()
    );
}

#[tokio::test]
async fn a_scripted_minute_nobody_watches_leaves_stage_ack_unmeasured() {
    let tmp = tempfile::tempdir().unwrap();
    let state = scripted_state(tmp.path());
    let addr = start_state(state.clone()).await;
    let p = SandboxStartParams {
        minutes: 5,
        seed: Some(7),
        chaos: None,
        judge: false,
        tone: None,
        record: false,
    };
    let started = start_with(state.clone(), p, sixty_seconds()).await.unwrap();
    let end = until_ended(&addr, &started.session_id, 300).await;
    assert_eq!(end.state, SandboxState::Done, "{end:?}");
    assert_eq!(check(&end, "stage_ack"), SandboxCheckState::Unmeasured);
    assert_eq!(check(&end, "judge"), SandboxCheckState::Unmeasured);
    assert!(!end.report.unwrap().pass);
}
