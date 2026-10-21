//! Session review over real sockets: the five `review.*` methods on a fixture session, and a
//! scripted sandbox minute that is tagged after its judge.

use std::time::Duration;

use serde_json::{json, Value};
use shadowrunr_types::review::{ReviewSession, ReviewSessionRow, SessionTags};
use shadowrunr_types::sandbox::{SandboxStartParams, SandboxState};

use super::{has_null, repo, rpc, start_state, test_config};
use crate::engine::EngineHandle;
use crate::review::tests::{write_session, Mode, TagEngine, MALAKOR_LINES, SID};
use crate::sandbox::{start_with, Hosting};
use crate::server::AppState;

#[tokio::test]
async fn the_review_methods_answer_over_rpc() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = TagEngine::new(Mode::Good);
    let state = AppState::new(
        test_config(tmp.path(), repo().join("characters")),
        EngineHandle {
            engine: engine.clone(),
            fleet: None,
        },
    );
    write_session(&state.cfg.paths.sandbox_runs(), SID, 1_000);
    let addr = start_state(state.clone()).await;

    let list = rpc(&addr, "review.sessions", json!({})).await;
    let rows: Vec<ReviewSessionRow> = serde_json::from_value(list["result"].clone()).unwrap();
    assert_eq!(rows.len(), 1, "{list}");
    assert!(!rows[0].tagged);
    assert!(!has_null(&list));

    let tagged = rpc(&addr, "review.tag", json!({ "session_id": SID })).await;
    let tags: SessionTags = serde_json::from_value(tagged["result"].clone())
        .unwrap_or_else(|e| panic!("{e}: {tagged}"));
    assert_eq!(
        tags.lines.iter().map(|t| t.line).collect::<Vec<_>>(),
        MALAKOR_LINES
    );
    assert_eq!(engine.calls(), 1);

    let set = rpc(
        &addr,
        "review.feedback.set",
        json!({ "session_id": SID, "line": 4, "tags": ["too_mean"], "note": "ouch" }),
    )
    .await;
    assert_eq!(set["result"]["tags"], json!(["too_mean"]), "{set}");
    let refused = rpc(
        &addr,
        "review.feedback.set",
        json!({ "session_id": SID, "line": 5, "tags": ["landed"] }),
    )
    .await;
    assert_eq!(refused["error"]["code"], "invalid_params", "{refused}");
    let listed = rpc(&addr, "review.feedback.list", json!({ "session_id": SID })).await;
    assert_eq!(listed["result"].as_array().unwrap().len(), 1, "{listed}");

    let whole = rpc(&addr, "review.session", json!({ "session_id": SID })).await;
    assert!(!has_null(&whole), "{whole}");
    let s: ReviewSession = serde_json::from_value(whole["result"].clone()).unwrap();
    assert!(s.row.tagged && s.row.marked == 1);
    assert_eq!(s.tags.unwrap(), tags);
    assert_eq!(s.feedback.len(), 1);
    // The tagger's call joined the session's ledger.
    assert_eq!(s.calls.len(), 3);

    for (params, code) in [
        (
            json!({ "session_id": "sbx-19700101-000000-5m" }),
            "not_found",
        ),
        (json!({ "session_id": "../../secrets" }), "invalid_params"),
        (json!({}), "invalid_params"),
    ] {
        let v = rpc(&addr, "review.session", params.clone()).await;
        assert_eq!(v["error"]["code"], code, "{params}: {v}");
    }
}

/// A scripted sandbox minute with the judge on: once it is done, the daemon tags it in the
/// background and `review.sessions` reports it tagged.
#[tokio::test]
async fn a_judged_sandbox_session_is_tagged_after_it_ends() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = TagEngine::new(Mode::Good);
    let state = AppState::new(
        test_config(tmp.path(), repo().join("characters")),
        EngineHandle {
            engine: engine.clone(),
            fleet: None,
        },
    );
    let addr = start_state(state.clone()).await;
    let p = SandboxStartParams {
        minutes: 5,
        seed: Some(7),
        chaos: None,
        judge: true,
        tone: None,
        record: false,
    };
    let hosting = Hosting {
        planned_s: Some(60),
        ..Hosting::default()
    };
    let started = start_with(state.clone(), p, hosting).await.unwrap();
    let id = started.session_id.clone();

    let poll = async {
        loop {
            let v = rpc(&addr, "sandbox.status", json!({ "session_id": id })).await;
            let st = v["result"]["state"].clone();
            if st == json!("done") || st == json!("stopped") || st == json!("failed") {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };
    let end: Value = tokio::time::timeout(Duration::from_secs(300), poll)
        .await
        .expect("the session ended in time");
    let state_now: SandboxState = serde_json::from_value(end["result"]["state"].clone()).unwrap();
    assert_eq!(state_now, SandboxState::Done, "{end}");

    let tagged = async {
        loop {
            let v = rpc(&addr, "review.sessions", json!({})).await;
            let rows: Vec<ReviewSessionRow> = serde_json::from_value(v["result"].clone()).unwrap();
            if let Some(r) = rows.iter().find(|r| r.session_id == id && r.tagged) {
                return r.clone();
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };
    let row = tokio::time::timeout(Duration::from_secs(60), tagged)
        .await
        .expect("the session was tagged in time");
    let tags: SessionTags = serde_json::from_str(
        &std::fs::read_to_string(std::path::Path::new(&started.dir).join("tags.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(tags.lines.len() as u32 + tags.untagged, row.lines);
    assert!(row.lines > 0, "the scripted host spoke");
    assert_eq!(tags.untagged, 0);
    assert!(engine.calls() >= 1);
}
