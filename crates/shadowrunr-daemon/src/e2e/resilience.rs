//! Daemon resilience: graceful shutdown closes `/events` with a reason, and a panicking sim is
//! an `engine` error frame, never a dead daemon.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use shadowrunr_types::{
    Engine, EngineError, EngineReply, EngineRequest, HealthReport, HealthState, WarmSession,
};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::Message;

use super::{repo, rpc, spec, start_state, test_config};
use crate::engine::EngineHandle;
use crate::events::SHUTDOWN_REASON;
use crate::server::{self, AppState};

#[tokio::test]
async fn shutdown_closes_event_sockets_with_a_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::new(
        test_config(tmp.path(), repo().join("characters")),
        EngineHandle::scripted(),
    );
    let (addr, serve) = server::bind(state.clone(), 0).await.unwrap();
    let server = tokio::spawn(serve);
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    ws.send(Message::text(json!({ "subscribe": "live" }).to_string()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    state.shutdown();
    let close = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(frame))) => return frame,
                Some(Ok(_)) => continue,
                other => panic!("socket ended without a close frame: {other:?}"),
            }
        }
    })
    .await
    .expect("close frame in time")
    .expect("close frame carries a reason");
    assert_eq!(close.code, CloseCode::Away);
    assert_eq!(close.reason.as_str(), SHUTDOWN_REASON);
    drop(ws);
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .expect("server drained in time")
        .unwrap()
        .unwrap();
}

/// Healthy for pre-flight, then panics on the first model call.
struct PanickingEngine;

#[async_trait]
impl Engine for PanickingEngine {
    async fn complete(&self, _req: EngineRequest) -> Result<EngineReply, EngineError> {
        panic!("engine exploded");
    }
    async fn open_session(
        &self,
        _template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError> {
        panic!("engine exploded");
    }
    async fn health(&self) -> Vec<HealthReport> {
        vec![HealthReport {
            check: "test".into(),
            state: HealthState::Green,
            detail: "panicking test engine".into(),
        }]
    }
}

#[tokio::test]
async fn a_panicking_sim_does_not_kill_the_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = EngineHandle {
        engine: Arc::new(PanickingEngine),
        fleet: None,
    };
    let state = AppState::new(test_config(tmp.path(), repo().join("characters")), engine);
    let addr = start_state(state).await;

    let started = rpc(
        &addr,
        "sim.start",
        json!({ "spec": spec("panic-1"), "judge": false }),
    )
    .await;
    assert_eq!(started["result"]["run_id"], "panic-1", "{started}");
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    ws.send(Message::text(
        json!({ "subscribe": "panic-1", "since": 0 }).to_string(),
    ))
    .await
    .unwrap();
    let error = tokio::time::timeout(Duration::from_secs(60), async {
        while let Some(Ok(m)) = ws.next().await {
            if let Message::Text(t) = m {
                let v: Value = serde_json::from_str(&t).unwrap();
                if v.get("error").is_some() {
                    return v;
                }
            }
        }
        panic!("socket ended without an error frame");
    })
    .await
    .expect("error frame in time");
    assert_eq!(error["error"]["code"], "engine", "{error}");
    let msg = error["error"]["message"].as_str().unwrap_or_default();
    assert!(msg.contains("sim panicked: engine exploded"), "{error}");

    // The daemon still answers, and the run's slot was released.
    let v = rpc(&addr, "preflight.run", json!({})).await;
    assert_eq!(v["result"].as_array().map(Vec::len), Some(8), "{v}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let again = rpc(
        &addr,
        "sim.start",
        json!({ "spec": spec("panic-1"), "judge": false }),
    )
    .await;
    assert_eq!(again["result"]["run_id"], "panic-1", "{again}");
}
