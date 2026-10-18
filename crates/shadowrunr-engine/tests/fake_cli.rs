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
    std::fs::read_to_string(dir.join("calls.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
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
