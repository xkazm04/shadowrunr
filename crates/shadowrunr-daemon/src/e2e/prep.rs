//! `prep.*` over `/rpc`: a fake planner writes, the owner approves, the gate holds.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use shadowrunr_types::{
    Engine, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport, Role, Usage,
    WarmSession,
};

use super::{has_null, repo, rpc, start_state, test_config};
use crate::engine::EngineHandle;
use crate::server::AppState;

/// Answers the planner with the sample prep's material, `www.` lines included when `taint`.
struct SamplePlanner {
    taint: bool,
}

#[async_trait]
impl Engine for SamplePlanner {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
        assert_eq!(req.role, Role::Planner);
        let sample: Value = serde_json::from_str(
            &std::fs::read_to_string(repo().join("characters/malakor/prep.sample.json")).unwrap(),
        )
        .unwrap();
        let mut reply = json!({
            "cold_open": sample["cold_open"],
            "bits": sample["bits"],
            "connectors": sample["connectors"],
        });
        if self.taint {
            reply["connectors"][0]["line"] = json!("Tribute accepted at www.example.com tonight.");
        }
        Ok(EngineReply {
            text: reply.to_string(),
            engine: EngineKind::Scripted,
            account: None,
            model: "fake-planner".into(),
            first_text_ms: None,
            duration_ms: 0,
            usage: Usage::default(),
            cost_usd: None,
            fallback_reason: None,
            session_id: None,
        })
    }
    async fn open_session(&self, _: EngineRequest) -> Result<Box<dyn WarmSession>, EngineError> {
        Err(EngineError::Unavailable {
            reason: "fake planner".into(),
        })
    }
    async fn health(&self) -> Vec<HealthReport> {
        Vec::new()
    }
}

async fn serve(tmp: &std::path::Path, taint: bool) -> String {
    let cfg = test_config(tmp, repo().join("characters"));
    let engine = EngineHandle {
        engine: Arc::new(SamplePlanner { taint }),
        fleet: None,
    };
    start_state(AppState::new(cfg, engine)).await
}

#[tokio::test]
async fn prep_generate_list_get_approve_retire_over_rpc() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = serve(tmp.path(), false).await;

    assert_eq!(
        rpc(&addr, "prep.list", json!({})).await["result"],
        json!([])
    );
    let g = rpc(&addr, "prep.generate", json!({})).await;
    let p = &g["result"];
    assert_eq!(p["status"], "staged", "{g}");
    assert!(!has_null(p), "{p}");
    assert!(p.get("plan_report").is_none(), "no plan chosen yet");
    assert!(p.get("fictional").is_none());
    let id = p["prep_id"].as_str().unwrap().to_string();
    assert!(id.starts_with("prep-"));
    assert!(p["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["state"] == "pass"));

    let list = rpc(&addr, "prep.list", json!({})).await;
    assert_eq!(list["result"][0]["prep_id"], id.as_str());
    assert_eq!(list["result"][0]["title"], "The Rented Throne");
    assert_eq!(list["result"][0]["failed_checks"], 0);
    let got = rpc(&addr, "prep.get", json!({ "prep_id": id })).await;
    assert_eq!(got["result"], *p);

    let missing = rpc(
        &addr,
        "prep.get",
        json!({ "prep_id": "prep-20991231-000000" }),
    )
    .await;
    assert_eq!(missing["error"]["code"], "not_found");
    let bad = rpc(&addr, "prep.get", json!({ "prep_id": "../x" })).await;
    assert_eq!(bad["error"]["code"], "invalid_params");
    let unsigned = rpc(
        &addr,
        "prep.approve",
        json!({ "prep_id": id, "signature": " " }),
    )
    .await;
    assert_eq!(unsigned["error"]["code"], "invalid_params");

    let ok = rpc(
        &addr,
        "prep.approve",
        json!({ "prep_id": id, "signature": "owner" }),
    )
    .await;
    assert_eq!(ok["result"]["status"], "approved", "{ok}");
    assert_eq!(ok["result"]["approved_by"], "owner");
    assert!(ok["result"]["approved_ms"].is_i64());
    let again = rpc(
        &addr,
        "prep.approve",
        json!({ "prep_id": id, "signature": "owner" }),
    )
    .await;
    assert_eq!(again["error"]["code"], "conflict");

    let r = rpc(&addr, "prep.retire", json!({ "prep_id": id })).await;
    assert_eq!(r["result"]["status"], "retired");
    let r = rpc(&addr, "prep.retire", json!({ "prep_id": id })).await;
    assert_eq!(r["error"]["code"], "conflict");
}

#[tokio::test]
async fn a_failed_check_holds_the_approval_over_rpc() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = serve(tmp.path(), true).await;
    let g = rpc(&addr, "prep.generate", json!({})).await;
    let p = &g["result"];
    assert_eq!(
        p["status"], "staged",
        "a failed prep is staged so the owner sees why"
    );
    let urls = p["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "urls")
        .unwrap();
    assert_eq!(urls["state"], "fail");
    assert!(urls["detail"].as_str().unwrap().contains("conn-01"));
    let id = p["prep_id"].as_str().unwrap();
    let refused = rpc(
        &addr,
        "prep.approve",
        json!({ "prep_id": id, "signature": "owner" }),
    )
    .await;
    assert_eq!(refused["error"]["code"], "conflict", "{refused}");
    assert_eq!(refused["error"]["detail"]["failed"], json!(["urls"]));
    let list = rpc(&addr, "prep.list", json!({})).await;
    assert_eq!(list["result"][0]["status"], "staged");
    assert_eq!(list["result"][0]["failed_checks"], 1);
}

#[tokio::test]
async fn generate_from_an_unknown_report_is_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = serve(tmp.path(), false).await;
    let v = rpc(
        &addr,
        "prep.generate",
        json!({ "report_id": "rep-missing" }),
    )
    .await;
    assert_eq!(v["error"]["code"], "not_found", "{v}");
}
