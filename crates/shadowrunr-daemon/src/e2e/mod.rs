//! End-to-end daemon tests: `/rpc` and `/events` over real sockets on 127.0.0.1, scripted
//! engine only. They live in the lib (not `tests/`) so they run without linking the
//! `shadowrunr` binary, which a live tournament may hold open.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use shadowrunr_types::StreamEvent;
use tokio_tungstenite::tungstenite::Message;

use crate::config::{Config, FileConfig, PathsConfig};
use crate::engine::EngineHandle;
use crate::server::{self, AppState};

mod catalog;
mod events;
mod paging;
mod resilience;
mod review;
mod sandbox;
mod voice;

pub(crate) fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub(crate) fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sim")
}

pub(crate) fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

fn s(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

/// Repo character package, policy, calibration and segment tables; runs and data in `tmp`.
pub(crate) fn test_config(tmp: &Path, characters: PathBuf) -> Config {
    copy_dir(&fixture_dir(), &tmp.join("runs").join("sim"));
    let mut fc = FileConfig {
        paths: PathsConfig {
            characters: s(characters),
            runs: s(tmp.join("runs")),
            data: s(tmp.join("data")),
            policy: s(repo().join("policy")),
            sim: s(repo().join("sim")),
            segments: s(repo().join("crates/shadowrunr-segments/data")),
        },
        ..FileConfig::default()
    };
    fc.engine.scripted = true;
    fc.sim.tick_s = 5;
    fc.sim.max_featured = 2;
    Config::from_file_config(fc, tmp)
}

pub(crate) async fn start(cfg: Config) -> String {
    start_state(AppState::new(cfg, EngineHandle::scripted())).await
}

/// Whether any value in the tree is JSON `null` (optional fields must be omitted instead).
pub(crate) fn has_null(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Array(a) => a.iter().any(has_null),
        Value::Object(o) => o.values().any(has_null),
        _ => false,
    }
}

/// Serve `state` on a free port; returns `127.0.0.1:<port>`.
pub(crate) async fn start_state(state: Arc<AppState>) -> String {
    let (addr, serve) = server::bind(state, 0).await.unwrap();
    tokio::spawn(serve);
    format!("127.0.0.1:{}", addr.port())
}

pub(crate) async fn rpc(addr: &str, method: &str, params: Value) -> Value {
    reqwest::Client::new()
        .post(format!("http://{addr}/rpc"))
        .json(&json!({ "method": method, "params": params }))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()
}

#[tokio::test]
async fn preflight_rpc_returns_eight_reports() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;
    let v = rpc(&addr, "preflight.run", json!({})).await;
    let reports = v["result"].as_array().expect("result array");
    assert_eq!(reports.len(), 8, "{v}");
    for (i, r) in reports.iter().enumerate() {
        assert_eq!(r["check"], format!("PF-0{}", i + 1));
        assert_ne!(r["state"], "red", "{r}");
    }
    assert_eq!(reports[7]["state"], "amber");
    assert!(reports[7]["detail"]
        .as_str()
        .unwrap()
        .contains("not in media-less phase"));
}

#[tokio::test]
async fn websocket_replay_delivers_every_event_in_order() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;
    let expected: Vec<String> =
        std::fs::read_to_string(fixture_dir().join("fixture-t/fixture-run/transcript.jsonl"))
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(String::from)
            .collect();
    assert_eq!(expected.len(), 11);

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    ws.send(Message::text(
        json!({ "subscribe": "fixture-run", "speed": 50.0 }).to_string(),
    ))
    .await
    .unwrap();
    let mut got = Vec::new();
    let read = async {
        while got.len() < expected.len() {
            match ws.next().await {
                Some(Ok(Message::Text(t))) => got.push(t.to_string()),
                Some(Ok(_)) => {}
                other => panic!("socket ended early: {other:?}"),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(20), read)
        .await
        .expect("replay finished in time");
    assert_eq!(got, expected);
    for line in &got {
        serde_json::from_str::<StreamEvent>(line).unwrap();
    }
}

#[tokio::test]
async fn rpc_reads_runs_and_rejects_bad_requests() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;

    let runs = rpc(&addr, "runs.list", json!({})).await;
    let rows = runs["result"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{runs}");
    assert_eq!(rows[0]["run_id"], "fixture-run");
    assert_eq!(rows[0]["tournament_id"], "fixture-t");
    assert_eq!(rows[0]["has_verdict"], false);

    let t = rpc(&addr, "run.transcript", json!({ "run_id": "fixture-run" })).await;
    assert_eq!(t["result"].as_array().unwrap().len(), 11);

    let list = rpc(&addr, "tournament.list", json!({})).await;
    assert_eq!(list["result"][0]["tournament_id"], "fixture-t");

    let missing = rpc(&addr, "run.verdict", json!({ "run_id": "fixture-run" })).await;
    assert_eq!(missing["error"]["code"], "not_found");

    let bad_id = rpc(&addr, "run.transcript", json!({ "run_id": "../x" })).await;
    assert_eq!(bad_id["error"]["code"], "invalid_params");

    let unknown = rpc(&addr, "no.such", json!({})).await;
    assert_eq!(unknown["error"]["code"], "method_not_found");

    let client = reqwest::Client::new();
    let r = client
        .post(format!("http://{addr}/rpc"))
        .header("origin", "https://evil.example")
        .json(&json!({ "method": "preflight.run", "params": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let refused: Value = r.json().await.unwrap();
    assert_eq!(refused["error"]["code"], "invalid_params", "{refused}");
    assert_eq!(refused["error"]["detail"]["reason"], "forbidden_origin");
    let r = client
        .post(format!("http://{addr}/rpc"))
        .header("content-type", "text/plain")
        .body(r#"{"method":"preflight.run","params":{}}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 415);
    let r = client
        .post(format!("http://{addr}/rpc"))
        .header("origin", "http://localhost:1420")
        .json(&json!({ "method": "directive.list", "params": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok()),
        Some("http://localhost:1420")
    );
}

#[tokio::test]
async fn reconcile_and_directives_are_wired() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;

    let d = rpc(
        &addr,
        "directive.add",
        json!({ "kind": "prevent", "text": "No more skeleton puns in SEG-01", "scope": "SEG-01" }),
    )
    .await;
    assert_eq!(d["result"]["kind"], "prevent", "{d}");
    assert_eq!(d["result"]["active"], true);
    let list = rpc(&addr, "directive.list", json!({})).await;
    assert_eq!(list["result"].as_array().unwrap().len(), 1);
    let id = d["result"]["id"].as_str().unwrap().to_string();
    let off = rpc(&addr, "directive.deactivate", json!({ "id": id })).await;
    assert_eq!(off["result"]["id"], id.as_str(), "{off}");
    assert_eq!(off["result"]["active"], false);
    let gone = rpc(&addr, "directive.deactivate", json!({ "id": "dir-none" })).await;
    assert_eq!(gone["error"]["code"], "not_found");
    let descoped = rpc(
        &addr,
        "directive.add",
        json!({ "kind": "try", "text": "x", "scope": "SEG-02B-I" }),
    )
    .await;
    assert_eq!(descoped["error"]["code"], "invalid_params");

    // The scripted engine cannot synthesise, so only the deterministic report is stored.
    let run = rpc(
        &addr,
        "reconcile.run",
        json!({ "source": { "tournament_id": "fixture-t" } }),
    )
    .await;
    let report_id = run["result"]["report_id"]
        .as_str()
        .unwrap_or_else(|| panic!("{run}"))
        .to_string();
    let report = rpc(&addr, "reconcile.report", json!({ "report_id": report_id })).await;
    assert_eq!(report["result"]["report_id"], report_id.as_str());
    let proposals = rpc(&addr, "plan.proposals", json!({ "report_id": report_id })).await;
    assert!(proposals["result"].is_array(), "{proposals}");
}

pub(crate) fn spec(run_id: &str) -> Value {
    json!({
        "run_id": run_id,
        "character": "malakor",
        "tone_variant": "theatrical",
        "core_tier": "haiku",
        "core_effort": "low",
        "segment": "SEG-01",
        "audience_size": 3,
        "seed": 7,
        "duration_s": 60,
        "population": "small_new_channel"
    })
}

#[tokio::test]
async fn sim_start_streams_live_on_events() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/events"))
        .await
        .unwrap();
    ws.send(Message::text(
        json!({ "subscribe": "live", "speed": 1.0 }).to_string(),
    ))
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let started = rpc(
        &addr,
        "sim.start",
        json!({ "spec": spec("live-1"), "judge": false }),
    )
    .await;
    assert_eq!(started["result"]["run_id"], "live-1", "{started}");

    let mut events: Vec<StreamEvent> = Vec::new();
    let read = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(t))) => {
                    let v: Value = serde_json::from_str(&t).unwrap();
                    assert!(v.get("error").is_none(), "{v}");
                    let ev: StreamEvent = serde_json::from_value(v).unwrap();
                    let end = matches!(ev, StreamEvent::StreamEnd { .. });
                    events.push(ev);
                    if end {
                        break;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended early: {other:?}"),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(120), read)
        .await
        .expect("live run finished in time");
    assert!(matches!(events[0], StreamEvent::StreamStart { .. }));

    let transcript = tmp.path().join("runs/sim/adhoc/live-1/transcript.jsonl");
    let on_disk = std::fs::read_to_string(&transcript).unwrap();
    assert_eq!(
        on_disk.lines().filter(|l| !l.trim().is_empty()).count(),
        events.len()
    );
}

#[tokio::test]
async fn sim_start_is_refused_while_preflight_is_red() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), tmp.path().join("no-characters"))).await;
    let v = rpc(
        &addr,
        "sim.start",
        json!({ "spec": spec("red-1"), "judge": false }),
    )
    .await;
    assert_eq!(v["error"]["code"], "preflight_red", "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("PF-02"));
    let descoped = {
        let mut sp = spec("d-1");
        sp["segment"] = json!("SEG-02B-II");
        rpc(&addr, "sim.start", json!({ "spec": sp })).await
    };
    assert_eq!(descoped["error"]["code"], "invalid_params");
}

// ── WP10: character rules and installed lists ──────────────────────────

#[tokio::test]
async fn character_rules_rpc_lists_and_toggles_into_the_data_override() {
    let tmp = tempfile::tempdir().unwrap();
    let chars = tmp.path().join("characters");
    copy_dir(
        &repo().join("characters").join("malakor"),
        &chars.join("malakor"),
    );
    let toml_before = std::fs::read(chars.join("malakor/character.toml")).unwrap();
    let addr = start(test_config(tmp.path(), chars.clone())).await;

    let v = rpc(&addr, "character.rules", json!({ "character": "malakor" })).await;
    assert_eq!(
        v["result"],
        json!([
            { "id": "crisis-chat-ban", "title": "Crisis chat ban", "enabled": true, "default": true },
            { "id": "crisis-handoff", "title": "Crisis handoff", "enabled": true, "default": true },
            { "id": "no-financial-promotion", "title": "No financial promotion", "enabled": true, "default": true }
        ]),
        "{v}"
    );

    let v = rpc(
        &addr,
        "character.rules.set",
        json!({ "character": "malakor", "id": "crisis-handoff", "enabled": false }),
    )
    .await;
    assert_eq!(v["result"][1]["id"], "crisis-handoff", "{v}");
    assert_eq!(v["result"][1]["enabled"], false, "{v}");
    assert_eq!(v["result"][1]["default"], true);
    let over = tmp.path().join("data/characters/malakor/rules.json");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&over).unwrap()).unwrap();
    assert_eq!(saved, json!({ "crisis-handoff": false }));
    assert_eq!(
        std::fs::read(chars.join("malakor/character.toml")).unwrap(),
        toml_before,
        "the tracked package is never rewritten by a toggle"
    );
    let v = rpc(&addr, "character.rules", json!({ "character": "malakor" })).await;
    assert_eq!(v["result"][1]["enabled"], false, "the override wins");

    let v = rpc(
        &addr,
        "character.rules.set",
        json!({ "character": "malakor", "id": "no-such-rule", "enabled": true }),
    )
    .await;
    assert_eq!(v["error"]["code"], "not_found", "{v}");
    let v = rpc(&addr, "character.rules", json!({ "character": "../x" })).await;
    assert_eq!(v["error"]["code"], "invalid_params", "{v}");
    let v = rpc(&addr, "character.rules", json!({ "character": "nobody" })).await;
    assert_eq!(v["error"]["code"], "not_found", "{v}");
}

/// Invented tokens only: no real slur ever appears in a fixture.
const FIXTURE_RULES: &str = r#"{ "version": 1, "platforms": {}, "term_rules": [
  { "id": "slur-operator-list", "category": "slur",
    "pattern": "\\b\\B__OPERATOR_INSTALLED_LIST__fixture-list__slur_racial__",
    "platforms": ["twitch"], "action": "block" } ] }"#;

#[tokio::test]
async fn pf05_reports_placeholder_only_then_the_installed_list() {
    let tmp = tempfile::tempdir().unwrap();
    let chars = tmp.path().join("characters");
    copy_dir(
        &repo().join("characters").join("malakor"),
        &chars.join("malakor"),
    );
    let policy = tmp.path().join("policy");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(policy.join("platform-rules.json"), FIXTURE_RULES).unwrap();
    let mut cfg = test_config(tmp.path(), chars);
    cfg.paths.policy = policy.clone();

    let r = crate::preflight::pf05_policy(&cfg);
    assert_eq!(
        r.state,
        shadowrunr_types::HealthState::Amber,
        "{}",
        r.detail
    );
    assert!(r.detail.contains("placeholder only"), "{}", r.detail);

    let src = tmp.path().join("fixture.txt");
    std::fs::write(
        &src,
        "zorblax\tslur_racial\nquexfin\tslur_racial\nwibblet\tinsult\n",
    )
    .unwrap();
    let meta =
        crate::policy_install::install(&policy, "fixture-list", Some(src.to_str().unwrap()), 0)
            .await
            .unwrap();
    assert_eq!(meta.terms, 3);
    let r = crate::preflight::pf05_policy(&cfg);
    assert_eq!(
        r.state,
        shadowrunr_types::HealthState::Green,
        "{}",
        r.detail
    );
    assert!(
        r.detail.contains("installed list fixture-list (2 terms)"),
        "{}",
        r.detail
    );
}
