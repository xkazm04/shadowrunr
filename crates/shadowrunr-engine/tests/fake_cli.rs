//! Drives the real process-spawning code path against `shadowrunr-fake-claude`, which replays
//! recorded stream-json. No quota is spent and nothing touches the network.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use shadowrunr_engine::ledger::LedgerRow;
use shadowrunr_engine::{AccountConfig, EngineConfig, Fleet, FleetMode};
use shadowrunr_types::{
    Effort, Engine, EngineError, EngineKind, EngineRequest, HealthState, ModelTier, Role,
};

const FAKE: &str = env!("CARGO_BIN_EXE_shadowrunr-fake-claude");

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

struct World {
    root: tempfile::TempDir,
    cfg: EngineConfig,
}

impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let cfg = EngineConfig {
            claude_bin: Some(FAKE.into()),
            work_dir: Some(root.path().join("work").to_string_lossy().into()),
            ledger_path: Some(root.path().join("ledger.jsonl").to_string_lossy().into()),
            ..Default::default()
        };
        Self { root, cfg }
    }

    /// Add an account whose config dir makes the fake CLI behave as `fake`.
    fn account(&mut self, label: &str, fake: Value) -> PathBuf {
        let dir = self.root.path().join(format!("acct-{label}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fake.json"), fake.to_string()).unwrap();
        self.cfg.accounts.push(AccountConfig {
            label: label.into(),
            config_dir: Some(dir.to_string_lossy().into()),
            fallback: false,
        });
        dir
    }

    fn ledger(&self) -> Vec<LedgerRow> {
        std::fs::read_to_string(self.root.path().join("ledger.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

fn calls(dir: &Path) -> Vec<Value> {
    // Only whole lines: a fake that is still writing has not ended its record yet.
    std::fs::read_to_string(dir.join("calls.jsonl"))
        .unwrap_or_default()
        .split_inclusive('\n')
        .filter(|l| l.ends_with('\n'))
        .map(|l| serde_json::from_str(l.trim_end()).unwrap())
        .collect()
}

fn argv(call: &Value) -> Vec<String> {
    call["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn req(timeout_ms: u64) -> EngineRequest {
    EngineRequest {
        role: Role::CoreThread,
        tier: ModelTier::Haiku,
        effort: Effort::Low,
        system: "You are Arch-Lich Malakor. You know you are streaming.".into(),
        prompt: "reply with the word ok".into(),
        allowed_tools: vec![],
        timeout_ms,
        json_schema: None,
        correlation_id: Some("test-run-1".into()),
    }
}

#[tokio::test]
async fn one_shot_spawns_cli_with_isolated_env_and_writes_ledger() {
    // The parent has an API key; the child must not see it.
    std::env::set_var("ANTHROPIC_API_KEY", "test-parent-key-not-real");
    // A parent Claude Code session exports its dev-mod plugin dirs; the child must not load them.
    std::env::set_var("CLAUDE_CODE_PLUGIN_DIRS", "C:/not-real/dev-mods");
    let mut w = World::new();
    let dir = w.account("main", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    let reply = fleet.complete(req(20_000)).await.unwrap();
    assert_eq!(reply.text, "ok");
    assert_eq!(reply.engine, EngineKind::ClaudeCli);
    assert_eq!(reply.account.as_deref(), Some("main"));
    assert_eq!(reply.model, "claude-haiku-5-5");
    assert!(reply.first_text_ms.is_some());
    assert_eq!(reply.cost_usd, Some(0.0001474));
    assert_eq!(
        reply.session_id.as_deref(),
        Some("548e4fea-c3e5-4bce-9075-b2de500c6ded")
    );
    assert!(reply.fallback_reason.is_none());

    let c = calls(&dir);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["api_key_present"], false);
    assert_eq!(c[0]["plugin_dirs_present"], false);
    let a = argv(&c[0]);
    for flag in [
        "-p",
        "--output-format",
        "--verbose",
        "--include-partial-messages",
        "--system-prompt-file",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--permission-prompts",
        "--setting-sources",
        "--safe-mode",
    ] {
        assert!(a.iter().any(|x| x == flag), "missing {flag} in {a:?}");
    }
    assert!(!a.iter().any(|x| x.contains("dangerously")));
    // The system prompt rides in a content-addressed file under the work dir.
    let i = a.iter().position(|x| x == "--system-prompt-file").unwrap();
    let sp = std::fs::read_to_string(&a[i + 1]).unwrap();
    assert!(sp.starts_with("You are Arch-Lich Malakor."));

    let rows = w.ledger();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].account.as_deref(), Some("main"));
    assert_eq!(rows[0].correlation_id.as_deref(), Some("test-run-1"));
    assert_eq!(rows[0].error_kind, None);
    let raw = std::fs::read_to_string(w.root.path().join("ledger.jsonl")).unwrap();
    assert!(!raw.contains("test-parent-key-not-real"));
}

#[tokio::test]
async fn rate_limited_process_rotates_to_next_account() {
    let mut w = World::new();
    w.account(
        "a",
        json!({"fixture": fixture("claude_rate_limit_reconstructed.jsonl")}),
    );
    w.account("b", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    let reply = fleet.complete(req(20_000)).await.unwrap();
    assert_eq!(reply.account.as_deref(), Some("b"));
    assert!(reply
        .fallback_reason
        .as_deref()
        .unwrap()
        .contains("account a rate-limited"));
    assert_eq!(fleet.fleet_state().mode, FleetMode::Primary);
    let rows = w.ledger();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].error_kind.as_deref(), Some("rate_limited"));
    assert_eq!(rows[1].account.as_deref(), Some("b"));
}

#[tokio::test]
async fn real_error_stream_is_returned_not_rotated() {
    let mut w = World::new();
    w.account(
        "a",
        json!({"fixture": fixture("claude_error_model_not_found.jsonl")}),
    );
    let dir_b = w.account("b", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    // Round-robin starts at a; a model error is the request's fault, so b is not tried.
    match fleet.complete(req(20_000)).await {
        Err(EngineError::Unavailable { reason }) => assert!(reason.contains("404"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(calls(&dir_b).is_empty());
    assert_eq!(w.ledger()[0].error_kind.as_deref(), Some("unavailable"));
}

#[tokio::test]
async fn timeout_kills_process_and_logs_failure() {
    let mut w = World::new();
    w.account(
        "slow",
        json!({"fixture": fixture("claude_success.jsonl"), "delay_ms": 5000}),
    );
    let fleet = Fleet::new(&w.cfg);
    match fleet.complete(req(300)).await {
        Err(EngineError::Timeout { after_ms }) => assert_eq!(after_ms, 300),
        other => panic!("{other:?}"),
    }
    let rows = w.ledger();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].error_kind.as_deref(), Some("timeout"));
}

#[tokio::test]
async fn warm_session_reuses_one_process_and_interrupts() {
    let mut w = World::new();
    let dir = w.account("main", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    let mut s = fleet.open_session(req(20_000)).await.unwrap();
    let r1 = s.send("first".into()).await.unwrap();
    assert_eq!(r1.text, "ok");
    s.interrupt().await.unwrap();
    let r2 = s.send("second".into()).await.unwrap();
    assert_eq!(r2.text, "ok");
    s.close().await.unwrap();
    let c = calls(&dir);
    assert_eq!(c.len(), 1, "one live process for the whole session");
    let a = argv(&c[0]);
    assert!(a
        .windows(2)
        .any(|x| x[0] == "--input-format" && x[1] == "stream-json"));
    assert!(!a.iter().any(|x| x == "--no-session-persistence"));
    let rows = w.ledger();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.warm));
}

#[tokio::test]
async fn warm_session_self_heals_with_resume() {
    let mut w = World::new();
    let dir = w.account(
        "main",
        json!({"fixture": fixture("claude_success.jsonl"), "die_on_turn": 2}),
    );
    let fleet = Fleet::new(&w.cfg);
    let mut s = fleet.open_session(req(20_000)).await.unwrap();
    s.send("one".into()).await.unwrap();
    let r = s.send("two".into()).await.unwrap();
    assert_eq!(r.text, "ok");
    let c = calls(&dir);
    assert_eq!(c.len(), 2, "died once, respawned once");
    let a = argv(&c[1]);
    let i = a
        .iter()
        .position(|x| x == "--resume")
        .expect("respawn resumes");
    assert_eq!(a[i + 1], "548e4fea-c3e5-4bce-9075-b2de500c6ded");
    let rows = w.ledger();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].error_kind.as_deref(), Some("transient"));
}

#[tokio::test]
async fn idle_timeout_reaps_and_next_send_resumes() {
    let mut w = World::new();
    w.cfg.idle_timeout_ms = Some(60);
    let dir = w.account("main", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    let mut s = fleet.open_session(req(20_000)).await.unwrap();
    s.send("one".into()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    s.send("two".into()).await.unwrap();
    let c = calls(&dir);
    assert_eq!(c.len(), 2);
    assert!(argv(&c[1]).iter().any(|x| x == "--resume"));
}

#[tokio::test]
async fn warm_session_moves_off_rate_limited_account() {
    let mut w = World::new();
    w.account(
        "a",
        json!({"fixture": fixture("claude_rate_limit_reconstructed.jsonl")}),
    );
    w.account("b", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    let mut s = fleet.open_session(req(20_000)).await.unwrap();
    let r = s.send("hello".into()).await.unwrap();
    assert_eq!(r.account.as_deref(), Some("b"));
    assert!(r
        .fallback_reason
        .unwrap()
        .contains("account a rate-limited"));
    assert!(fleet.accounts()[0].cooling_until_ms.is_some());
}

#[tokio::test]
async fn preflight_reports_cli_and_logins() {
    let mut w = World::new();
    w.account("in", json!({"logged_in": true}));
    w.account("out", json!({"logged_in": false}));
    w.cfg.api_key_env = Some("SHADOWRUNR_TEST_KEY_NEVER_SET_PF".into());
    let fleet = Fleet::new(&w.cfg);
    let h = fleet.health().await;
    assert!(h.iter().all(|r| r.check == "PF-01"));
    let find = |p: &str| h.iter().find(|r| r.detail.starts_with(p)).unwrap();
    assert_eq!(find("cli:").state, HealthState::Green);
    assert!(find("cli:").detail.contains("2.1.302"));
    assert_eq!(find("account in:").state, HealthState::Green);
    assert_eq!(find("account out:").state, HealthState::Red);
    assert_eq!(find("api:").state, HealthState::Amber);
}

#[tokio::test]
async fn missing_cli_is_red_without_api() {
    let mut w = World::new();
    w.cfg.claude_bin = Some(
        w.root
            .path()
            .join("no-such-claude")
            .to_string_lossy()
            .into(),
    );
    w.account("main", json!({}));
    let fleet = Fleet::new(&w.cfg);
    let h = fleet.health().await;
    assert_eq!(h[0].state, HealthState::Red);
    // And a call fails over (spawn failure is transient) to nothing: Unavailable + Down.
    assert!(matches!(
        fleet.complete(req(5_000)).await,
        Err(EngineError::Unavailable { .. })
    ));
    assert_eq!(fleet.fleet_state().mode, FleetMode::Down);
}

#[tokio::test]
async fn global_semaphore_caps_live_processes() {
    let mut w = World::new();
    w.cfg.max_live_processes = Some(1);
    w.account(
        "main",
        json!({"fixture": fixture("claude_success.jsonl"), "delay_ms": 400}),
    );
    let fleet = std::sync::Arc::new(Fleet::new(&w.cfg));
    let started = std::time::Instant::now();
    let (a, b) = tokio::join!(fleet.complete(req(20_000)), fleet.complete(req(20_000)));
    assert!(a.is_ok() && b.is_ok());
    // With one permit the two 400 ms processes cannot overlap.
    assert!(started.elapsed() >= Duration::from_millis(800));
}

// ---- One failure ladder for one-shot and warm turns ----

/// A loopback Messages API responder: answers every POST with one canned reply and keeps
/// the request bodies. Nothing leaves the machine.
struct ApiStub {
    base_url: String,
    bodies: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
}

async fn api_stub() -> ApiStub {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = bodies.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head_end, len) = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (i + 4, len);
                    }
                };
                while buf.len() < head_end + len {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let end = (head_end + len).min(buf.len());
                let body: Value =
                    serde_json::from_slice(&buf[head_end..end]).unwrap_or(Value::Null);
                seen.lock().unwrap().push(body);
                let reply = json!({
                    "content": [{"type": "text", "text": "Kneel, minion."}],
                    "stop_reason": "end_turn",
                    "usage": {"input_tokens": 12, "output_tokens": 4}
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    ApiStub { base_url, bodies }
}

fn set_fake(dir: &Path, fake: Value) {
    std::fs::write(dir.join("fake.json"), fake.to_string()).unwrap();
}

#[tokio::test]
async fn warm_turn_overloaded_rotates_to_healthy_account() {
    let mut w = World::new();
    w.account(
        "a",
        json!({"fixture": fixture("claude_overloaded_reconstructed.jsonl")}),
    );
    w.account("b", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    let mut s = fleet.open_session(req(20_000)).await.unwrap();
    let r = s.send("hello".into()).await.expect("b serves the turn");
    assert_eq!(r.account.as_deref(), Some("b"));
    assert_eq!(r.engine, EngineKind::ClaudeCli);
    let reason = r.fallback_reason.expect("fallback_reason set");
    assert!(reason.contains("account a"), "{reason}");
    // An overload is no reason to cool the seat.
    assert_eq!(fleet.accounts()[0].cooling_until_ms, None);
    let rows = w.ledger();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].account.as_deref(), Some("a"));
    assert_eq!(rows[0].error_kind.as_deref(), Some("transient"));
    assert!(rows[0].warm);
    assert_eq!(rows[1].account.as_deref(), Some("b"));
    assert_eq!(rows[1].error_kind, None);
    assert!(rows[1].warm);
}

#[tokio::test]
async fn warm_turn_not_logged_in_cools_and_rotates() {
    let mut w = World::new();
    w.account(
        "a",
        json!({"fixture": fixture("claude_not_logged_in_reconstructed.jsonl")}),
    );
    w.account("b", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    let mut s = fleet.open_session(req(20_000)).await.unwrap();
    let r = s.send("hello".into()).await.expect("b serves the turn");
    assert_eq!(r.account.as_deref(), Some("b"));
    assert!(fleet.accounts()[0].cooling_until_ms.is_some());
    assert_eq!(fleet.accounts()[1].cooling_until_ms, None);
}

#[tokio::test]
async fn warm_session_out_of_accounts_degrades_in_place_to_api_with_its_turns() {
    std::env::set_var("SHADOWRUNR_TEST_KEY_WARM_LADDER", "test-not-a-key");
    let stub = api_stub().await;
    let mut w = World::new();
    w.cfg.idle_timeout_ms = Some(60);
    w.cfg.api_key_env = Some("SHADOWRUNR_TEST_KEY_WARM_LADDER".into());
    w.cfg.api_base_url = Some(stub.base_url.clone());
    let dir_a = w.account("a", json!({"fixture": fixture("claude_success.jsonl")}));
    let dir_b = w.account("b", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    let mut s = fleet.open_session(req(20_000)).await.unwrap();
    let first = s.send("first words".into()).await.unwrap();
    assert_eq!(first.engine, EngineKind::ClaudeCli);
    // Let the idle reaper end the process, then every account hits its limit.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let limited = json!({"fixture": fixture("claude_rate_limit_reconstructed.jsonl")});
    set_fake(&dir_a, limited.clone());
    set_fake(&dir_b, limited);
    let r = s
        .send("second words".into())
        .await
        .expect("the api rung serves");
    assert_eq!(r.engine, EngineKind::AnthropicApi);
    assert_eq!(r.text, "Kneel, minion.");
    let reason = r.fallback_reason.expect("fallback_reason set");
    assert!(
        reason.contains("account a") && reason.contains("account b"),
        "{reason}"
    );
    assert_eq!(fleet.fleet_state().mode, FleetMode::Degraded);
    let bodies = stub.bodies.lock().unwrap().clone();
    assert_eq!(bodies.len(), 1);
    let msgs = bodies[0]["messages"].as_array().unwrap().clone();
    assert_eq!(msgs.len(), 3, "{msgs:?}");
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "first words");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[1]["content"], "ok");
    assert_eq!(msgs[2]["role"], "user");
    assert_eq!(msgs[2]["content"], "second words");
    let rows = w.ledger();
    let last = rows.last().unwrap();
    assert_eq!(last.engine, EngineKind::AnthropicApi);
    assert!(last.warm);
}

#[tokio::test]
async fn a_warm_cli_turn_moves_the_fleet_back_to_primary() {
    std::env::set_var("SHADOWRUNR_TEST_KEY_WARM_MODE", "test-not-a-key");
    let stub = api_stub().await;
    let mut w = World::new();
    w.cfg.api_key_env = Some("SHADOWRUNR_TEST_KEY_WARM_MODE".into());
    w.cfg.api_base_url = Some(stub.base_url.clone());
    let dir = w.account("a", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    // The warm process reads its behaviour at start-up (one turn proves it is up), so it
    // stays healthy after the account's config changes.
    let mut s = fleet.open_session(req(20_000)).await.unwrap();
    s.send("warm up".into()).await.unwrap();
    // A one-shot finds the seat overloaded and is served by the API.
    set_fake(
        &dir,
        json!({"fixture": fixture("claude_overloaded_reconstructed.jsonl")}),
    );
    let one = fleet.complete(req(20_000)).await.unwrap();
    assert_eq!(one.engine, EngineKind::AnthropicApi);
    assert_eq!(fleet.fleet_state().mode, FleetMode::Degraded);
    let r = s.send("hello".into()).await.unwrap();
    assert_eq!(r.engine, EngineKind::ClaudeCli);
    assert_eq!(fleet.fleet_state().mode, FleetMode::Primary);
}

/// Guard: a request-side failure still never rotates.
#[tokio::test]
async fn warm_turn_model_not_found_is_returned_not_rotated() {
    let mut w = World::new();
    w.account(
        "a",
        json!({"fixture": fixture("claude_error_model_not_found.jsonl")}),
    );
    let dir_b = w.account("b", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = Fleet::new(&w.cfg);
    let mut s = fleet.open_session(req(20_000)).await.unwrap();
    match s.send("hello".into()).await {
        Err(EngineError::Unavailable { reason }) => assert!(reason.contains("404"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(calls(&dir_b).is_empty());
}

// ---- Admission by lane, deadline at enqueue ----
//
// Ordering, not wall-clock bounds, carries the verdicts: a slow fake CLI holds its slot for
// far longer than a fast one takes, and the tests assert which process had started or
// finished when an answer came back.

fn role_req(role: Role, timeout_ms: u64) -> EngineRequest {
    EngineRequest {
        role,
        ..req(timeout_ms)
    }
}

/// Wait (bounded) until `cond` holds. A wait, never a verdict.
async fn until(what: &str, mut cond: impl FnMut() -> bool) {
    let give_up = std::time::Instant::now() + Duration::from_secs(20);
    while !cond() {
        assert!(
            std::time::Instant::now() < give_up,
            "timed out waiting: {what}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn raw_ledger(w: &World) -> Vec<Value> {
    std::fs::read_to_string(w.root.path().join("ledger.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
async fn a_core_turn_is_admitted_ahead_of_queued_judges() {
    let mut w = World::new();
    w.cfg.max_live_processes = Some(2);
    w.cfg.live_reserved_processes = Some(1);
    let dir = w.account(
        "main",
        json!({"fixture": fixture("claude_success.jsonl"), "delay_ms": 4000}),
    );
    let fleet = std::sync::Arc::new(Fleet::new(&w.cfg));
    let adm = fleet.inner().cli.admission.clone();
    let f1 = fleet.clone();
    let judge1 = tokio::spawn(async move { f1.complete(role_req(Role::Judge, 60_000)).await });
    until("judge 1 spawned", || calls(&dir).len() == 1).await;
    let f2 = fleet.clone();
    let judge2 = tokio::spawn(async move { f2.complete(role_req(Role::Judge, 60_000)).await });
    until("judge 2 queued", || adm.waiting() == 1).await;
    // The core thread's process runs fast.
    set_fake(&dir, json!({"fixture": fixture("claude_success.jsonl")}));
    let core = fleet
        .complete(role_req(Role::CoreThread, 20_000))
        .await
        .unwrap();
    assert_eq!(core.text, "ok");
    assert!(
        !judge1.is_finished(),
        "the core turn did not wait for a judge"
    );
    assert_eq!(calls(&dir).len(), 2, "judge 2 is the one still waiting");
    assert_eq!(adm.waiting(), 1);
    assert!(judge1.await.unwrap().is_ok());
    assert!(judge2.await.unwrap().is_ok());
    let core_row = w
        .ledger()
        .into_iter()
        .find(|r| r.role == Role::CoreThread)
        .unwrap();
    assert_eq!(core_row.queue_ms, 0);
}

#[tokio::test]
async fn a_core_turn_that_waits_past_its_budget_times_out_in_the_queue() {
    let mut w = World::new();
    w.cfg.max_live_processes = Some(1);
    let dir = w.account(
        "main",
        json!({"fixture": fixture("claude_success.jsonl"), "delay_ms": 4000}),
    );
    let fleet = std::sync::Arc::new(Fleet::new(&w.cfg));
    let f1 = fleet.clone();
    let research =
        tokio::spawn(async move { f1.complete(role_req(Role::Researcher, 60_000)).await });
    until("research spawned", || calls(&dir).len() == 1).await;
    match fleet.complete(role_req(Role::CoreThread, 300)).await {
        Err(EngineError::Timeout { after_ms }) => assert_eq!(after_ms, 300),
        other => panic!("{other:?}"),
    }
    assert!(
        !research.is_finished(),
        "the core turn's budget covered its wait, not the research job"
    );
    assert_eq!(
        calls(&dir).len(),
        1,
        "no process was spawned for the core turn"
    );
    let rows = w.ledger();
    let row = rows.iter().find(|r| r.role == Role::CoreThread).unwrap();
    assert_eq!(row.error_kind.as_deref(), Some("timeout"));
    assert!(row.queue_ms >= 250, "{row:?}");
    assert!(research.await.unwrap().is_ok());
}

#[tokio::test]
async fn a_chat_pick_gets_in_while_a_warm_core_holds_a_slot_and_judges_fill_the_batch_lane() {
    let mut w = World::new();
    w.cfg.max_live_processes = Some(3);
    w.cfg.live_reserved_processes = Some(2);
    let dir = w.account("main", json!({"fixture": fixture("claude_success.jsonl")}));
    let fleet = std::sync::Arc::new(Fleet::new(&w.cfg));
    let adm = fleet.inner().cli.admission.clone();
    let mut core = fleet
        .open_session(role_req(Role::CoreThread, 20_000))
        .await
        .unwrap();
    core.send("warm up".into()).await.unwrap();
    set_fake(
        &dir,
        json!({"fixture": fixture("claude_success.jsonl"), "delay_ms": 4000}),
    );
    let f1 = fleet.clone();
    let judge1 = tokio::spawn(async move { f1.complete(role_req(Role::Judge, 60_000)).await });
    until("judge 1 spawned", || calls(&dir).len() == 2).await;
    let f2 = fleet.clone();
    let judge2 = tokio::spawn(async move { f2.complete(role_req(Role::Judge, 60_000)).await });
    until("judge 2 queued", || adm.waiting() == 1).await;
    set_fake(&dir, json!({"fixture": fixture("claude_success.jsonl")}));
    let pick = fleet
        .complete(role_req(Role::ChatPicker, 20_000))
        .await
        .expect("the chat pick is admitted on the live reserve");
    assert_eq!(pick.text, "ok");
    assert!(!judge1.is_finished());
    assert_eq!(adm.waiting(), 1, "judge 2 still waits");
    assert!(judge1.await.unwrap().is_ok());
    assert!(judge2.await.unwrap().is_ok());
    core.close().await.unwrap();
}

#[tokio::test]
async fn batch_roles_never_take_a_reserved_slot() {
    let mut w = World::new();
    w.cfg.max_live_processes = Some(2);
    w.cfg.live_reserved_processes = Some(1);
    w.account(
        "main",
        json!({"fixture": fixture("claude_success.jsonl"), "delay_ms": 400}),
    );
    let fleet = std::sync::Arc::new(Fleet::new(&w.cfg));
    let started = std::time::Instant::now();
    let (a, b, c) = tokio::join!(
        fleet.complete(role_req(Role::Judge, 60_000)),
        fleet.complete(role_req(Role::Judge, 60_000)),
        fleet.complete(role_req(Role::Judge, 60_000)),
    );
    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    // One shared slot: three 400 ms judges run one after another.
    assert!(started.elapsed() >= Duration::from_millis(1200));
    // Exactly two of the three queued, each behind the one before it.
    let rows = w.ledger();
    let mut queued: Vec<u64> = rows.iter().map(|r| r.queue_ms).collect();
    queued.sort_unstable();
    assert_eq!(queued[0], 0, "{rows:?}");
    assert!(queued[1] >= 350 && queued[2] >= 750, "{queued:?}");
}

#[tokio::test]
async fn queue_time_is_ledgered_only_when_a_call_waited() {
    let mut w = World::new();
    w.cfg.max_live_processes = Some(1);
    let dir = w.account(
        "main",
        json!({"fixture": fixture("claude_success.jsonl"), "delay_ms": 300}),
    );
    let fleet = std::sync::Arc::new(Fleet::new(&w.cfg));
    let adm = fleet.inner().cli.admission.clone();
    let f1 = fleet.clone();
    let first = tokio::spawn(async move { f1.complete(role_req(Role::Planner, 20_000)).await });
    until("first spawned", || calls(&dir).len() == 1).await;
    let f2 = fleet.clone();
    let second = tokio::spawn(async move { f2.complete(role_req(Role::Tagger, 20_000)).await });
    until("second queued", || adm.waiting() == 1).await;
    assert!(first.await.unwrap().is_ok());
    assert!(second.await.unwrap().is_ok());
    let raw = raw_ledger(&w);
    let row = |role: &str| raw.iter().find(|r| r["role"] == role).unwrap().clone();
    assert!(row("planner").get("queue_ms").is_none(), "{raw:?}");
    assert!(row("tagger")["queue_ms"].as_u64().unwrap() > 0, "{raw:?}");
}
