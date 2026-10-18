//! The fleet: Claude Code CLI accounts in health-aware round-robin, then the Anthropic API
//! key, then the experimental CLI fallbacks. One `Engine` that callers never think about.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use shadowrunr_types::{
    Engine, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport, WarmSession,
};
use tokio::io::AsyncWriteExt;

use crate::alt_cli::{self, AltCommand};
use crate::api::{AnthropicApi, HttpTransport, ReqwestTransport};
use crate::claude_cli::{read_tail, Account, ClaudeCli, CliRunner};
use crate::config::{EngineConfig, FallbackDriver, DEFAULT_AGY_MODEL, DEFAULT_CODEX_MODEL};
use crate::ledger::{Ledger, LedgerRow};
use crate::limits::Failure;
use crate::model::model_id;
use crate::session::{ApiSession, ClaudeWarmSession};
use crate::stream::TurnOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FleetMode {
    /// The last call was served by a Claude Code CLI account.
    Primary,
    /// The last call was served by a fallback (API key or an alternative CLI).
    Degraded,
    /// The last call found nothing able to serve it.
    Down,
}

/// What the brain reads to apply the fleet-loss policy (chat on the API key for a while,
/// then sign off). `since_ms` is when the fleet entered `mode` (epoch ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetState {
    pub mode: FleetMode,
    pub since_ms: i64,
}

/// Epoch-ms clock; injectable so tests can move time.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    })
}

/// Per-account health as the fleet sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountStatus {
    pub label: String,
    /// Epoch ms until which the account is skipped; `None` = healthy.
    pub cooling_until_ms: Option<i64>,
}

/// Shared fleet internals. Warm sessions hold an `Arc` to mark accounts cooling.
pub struct FleetInner {
    pub cfg: EngineConfig,
    pub cli: ClaudeCli,
    pub runner: Arc<dyn CliRunner>,
    pub accounts: Vec<Account>,
    cooling: Mutex<Vec<Option<i64>>>,
    rr: AtomicUsize,
    pub api: Option<AnthropicApi>,
    pub fallbacks: Vec<FallbackDriver>,
    pub ledger: Ledger,
    state: Mutex<FleetState>,
    pub clock: Clock,
}

impl FleetInner {
    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    /// Next healthy account in round-robin order, skipping `tried`.
    pub fn pick_account(&self, tried: &[usize]) -> Option<usize> {
        let n = self.accounts.len();
        if n == 0 {
            return None;
        }
        let now = self.now();
        let start = self.rr.fetch_add(1, Ordering::Relaxed) % n;
        let mut cooling = self.cooling.lock().unwrap_or_else(|e| e.into_inner());
        for k in 0..n {
            let i = (start + k) % n;
            if tried.contains(&i) {
                continue;
            }
            match cooling[i] {
                Some(until) if until > now => continue,
                Some(_) => cooling[i] = None,
                None => {}
            }
            return Some(i);
        }
        None
    }

    /// Cool an account until its reset time, or for the default cooldown.
    pub fn cool(&self, i: usize, reset_at_ms: Option<i64>) {
        let now = self.now();
        let until = reset_at_ms
            .filter(|r| *r > now)
            .unwrap_or(now + self.cfg.cooldown_ms() as i64);
        let mut cooling = self.cooling.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(slot) = cooling.get_mut(i) {
            *slot = Some(until);
        }
    }

    pub fn account_status(&self) -> Vec<AccountStatus> {
        let now = self.now();
        let cooling = self.cooling.lock().unwrap_or_else(|e| e.into_inner());
        self.accounts
            .iter()
            .zip(cooling.iter())
            .map(|(a, c)| AccountStatus {
                label: a.label.clone(),
                cooling_until_ms: c.filter(|u| *u > now),
            })
            .collect()
    }

    pub fn set_mode(&self, mode: FleetMode) {
        let now = self.now();
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if st.mode != mode {
            *st = FleetState {
                mode,
                since_ms: now,
            };
        }
    }

    pub fn state(&self) -> FleetState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Apply the config's route override for the request's role, when there is one.
    pub fn apply_route(&self, mut req: EngineRequest) -> EngineRequest {
        if self.cfg.routes.contains_key(&req.role) {
            let (tier, effort, timeout) = self.cfg.route(req.role);
            req.tier = tier;
            req.effort = effort;
            req.timeout_ms = timeout;
        }
        req
    }

    pub fn record(&self, row: LedgerRow) {
        self.ledger.append(&row);
    }

    pub fn failure_row(
        &self,
        req: &EngineRequest,
        engine: EngineKind,
        account: Option<String>,
        duration_ms: u64,
        kind: &str,
        warm: bool,
    ) -> LedgerRow {
        LedgerRow {
            ts_ms: self.now(),
            role: req.role,
            engine,
            account,
            model: model_id(req.tier).to_string(),
            first_text_ms: None,
            duration_ms,
            usage: Default::default(),
            cost_usd: None,
            fallback_reason: None,
            correlation_id: req.correlation_id.clone(),
            error_kind: Some(kind.to_string()),
            warm,
        }
    }

    pub fn cli_reply(
        &self,
        account: &Account,
        req: &EngineRequest,
        o: TurnOutcome,
        duration_ms: u64,
        fallback_reason: Option<String>,
    ) -> EngineReply {
        EngineReply {
            text: o.text,
            engine: EngineKind::ClaudeCli,
            account: Some(account.label.clone()),
            model: o.model.unwrap_or_else(|| model_id(req.tier).to_string()),
            first_text_ms: o.first_text_ms,
            duration_ms,
            usage: o.usage,
            cost_usd: o.cost_usd,
            fallback_reason,
            session_id: o.session_id,
        }
    }

    async fn run_alt(
        &self,
        driver: FallbackDriver,
        req: &EngineRequest,
    ) -> Result<EngineReply, Failure> {
        let work_dir = self.cfg.work_dir();
        let (cmd, engine, model) = match driver {
            FallbackDriver::LocalGemma => {
                return Err(Failure::Transient {
                    detail: "local_gemma is not implemented in v0.1 (GPU work is out of scope)"
                        .into(),
                })
            }
            FallbackDriver::Codex => {
                let head = self
                    .cfg
                    .codex_bin
                    .clone()
                    .unwrap_or_else(|| vec!["codex".into()]);
                let model = self
                    .cfg
                    .codex_model
                    .clone()
                    .unwrap_or_else(|| DEFAULT_CODEX_MODEL.into());
                (
                    alt_cli::codex_command(&head, &model, req, &work_dir),
                    EngineKind::CodexCli,
                    model,
                )
            }
            FallbackDriver::Agy => {
                let head = self
                    .cfg
                    .agy_bin
                    .clone()
                    .unwrap_or_else(|| vec!["agy".into()]);
                let model = self
                    .cfg
                    .agy_model
                    .clone()
                    .unwrap_or_else(|| DEFAULT_AGY_MODEL.into());
                let slug = alt_cli::agy_model(&model, req.effort);
                (
                    alt_cli::agy_command(&head, &model, req),
                    EngineKind::AgyCli,
                    slug,
                )
            }
        };
        let started = Instant::now();
        let (stdout, exit_ok) = spawn_alt(&self.cli, &cmd, work_dir, req.timeout_ms).await?;
        let parsed = match engine {
            EngineKind::CodexCli => alt_cli::parse_codex(&stdout),
            _ => alt_cli::parse_agy(&stdout),
        };
        if let Some(f) = alt_cli::classify(&parsed, exit_ok) {
            return Err(f);
        }
        Ok(EngineReply {
            text: parsed.text,
            engine,
            account: None,
            model,
            first_text_ms: None,
            duration_ms: started.elapsed().as_millis() as u64,
            usage: parsed.usage,
            cost_usd: None,
            fallback_reason: None,
            session_id: None,
        })
    }
}

/// Spawn an alternative CLI under the shared process cap and collect its stdout.
async fn spawn_alt(
    cli: &ClaudeCli,
    cmd: &AltCommand,
    work_dir: PathBuf,
    timeout_ms: u64,
) -> Result<(String, bool), Failure> {
    let _permit = cli
        .sem
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| Failure::Transient {
            detail: "process semaphore closed".into(),
        })?;
    let Some((exe, args)) = cmd.argv.split_first() else {
        return Err(Failure::Transient {
            detail: "empty alt argv".into(),
        });
    };
    let mut c = tokio::process::Command::new(exe);
    c.args(args)
        .envs(cmd.env.iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let _ = std::fs::create_dir_all(&work_dir);
    if work_dir.is_dir() {
        c.current_dir(&work_dir);
    }
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    let mut child = c.spawn().map_err(|e| Failure::Transient {
        detail: format!("spawn {exe}: {e}"),
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(cmd.stdin.as_bytes()).await;
        drop(stdin);
    }
    let stdout = child.stdout.take();
    let run = async {
        let out = match stdout {
            Some(s) => read_tail(s, 4 * 1024 * 1024).await,
            None => String::new(),
        };
        let ok = child.wait().await.map(|s| s.success()).unwrap_or(false);
        (out, ok)
    };
    tokio::time::timeout(Duration::from_millis(timeout_ms), run)
        .await
        .map_err(|_| {
            Failure::Fatal(EngineError::Timeout {
                after_ms: timeout_ms,
            })
        })
}

/// The production engine stack. Hold it concretely (`Arc<Fleet>`) to read
/// [`Fleet::fleet_state`]; it coerces to `Arc<dyn Engine>` for everything else.
#[derive(Clone)]
pub struct Fleet {
    inner: Arc<FleetInner>,
}

impl Fleet {
    /// The real stack: `claude` processes, reqwest for the API.
    pub fn new(cfg: &EngineConfig) -> Self {
        let cli = ClaudeCli::new(
            cfg.claude_bin(),
            cfg.work_dir(),
            cfg.safe_mode(),
            cfg.max_live_processes(),
        );
        let runner: Arc<dyn CliRunner> = Arc::new(cli.clone());
        Self::with_parts(
            cfg,
            cli,
            runner,
            Arc::new(ReqwestTransport::new()),
            system_clock(),
        )
    }

    /// Assemble from explicit parts (tests and embedders): `runner` serves one-shot calls,
    /// `cli` serves warm sessions and health, `transport` carries API calls.
    pub fn with_parts(
        cfg: &EngineConfig,
        cli: ClaudeCli,
        runner: Arc<dyn CliRunner>,
        transport: Arc<dyn HttpTransport>,
        clock: Clock,
    ) -> Self {
        let accounts: Vec<Account> = cfg
            .accounts
            .iter()
            .map(|a| Account {
                label: a.label.clone(),
                config_dir: a.config_dir.as_ref().map(PathBuf::from),
            })
            .collect();
        let api = cfg
            .api_key_env
            .as_ref()
            .map(|env| AnthropicApi::new(env.clone(), cfg.api_base_url(), transport.clone()));
        let now = clock();
        let inner = FleetInner {
            cooling: Mutex::new(vec![None; accounts.len()]),
            accounts,
            cli,
            runner,
            rr: AtomicUsize::new(0),
            api,
            fallbacks: cfg.fallback_drivers(),
            ledger: Ledger::new(Some(cfg.ledger_path())),
            state: Mutex::new(FleetState {
                mode: if cfg.accounts.is_empty() {
                    FleetMode::Degraded
                } else {
                    FleetMode::Primary
                },
                since_ms: now,
            }),
            clock,
            cfg: cfg.clone(),
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Current fleet state for the brain's fleet-loss policy.
    pub fn fleet_state(&self) -> FleetState {
        self.inner.state()
    }

    /// Per-account cooling status.
    pub fn accounts(&self) -> Vec<AccountStatus> {
        self.inner.account_status()
    }

    /// Whether some Claude Code account is not cooling (read-only). A fleet with no accounts
    /// serves from its fallbacks only, and counts as able to serve.
    pub fn can_serve(&self) -> bool {
        let st = self.inner.account_status();
        st.is_empty() || st.iter().any(|a| a.cooling_until_ms.is_none())
    }

    /// The earliest instant (epoch ms) a cooling account comes back; `None` when no account
    /// is cooling (read-only).
    pub fn earliest_reset_ms(&self) -> Option<i64> {
        self.inner
            .account_status()
            .iter()
            .filter_map(|a| a.cooling_until_ms)
            .min()
    }

    /// The engine configuration the fleet was built from (read-only).
    pub fn config(&self) -> &EngineConfig {
        &self.inner.cfg
    }

    pub fn inner(&self) -> &Arc<FleetInner> {
        &self.inner
    }
}

#[async_trait]
impl Engine for Fleet {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
        let inner = &self.inner;
        let req = inner.apply_route(req);
        let mut reasons: Vec<String> = Vec::new();
        let mut tried: Vec<usize> = Vec::new();

        while let Some(i) = inner.pick_account(&tried) {
            tried.push(i);
            let account = inner.accounts[i].clone();
            let started = Instant::now();
            let res = inner.runner.run(&account, &req).await;
            let duration_ms = started.elapsed().as_millis() as u64;
            match res {
                Ok(o) => {
                    let fallback = (!reasons.is_empty()).then(|| reasons.join("; "));
                    let reply = inner.cli_reply(&account, &req, o, duration_ms, fallback);
                    inner.record(LedgerRow::from_reply(
                        inner.now(),
                        req.role,
                        &reply,
                        req.correlation_id.clone(),
                    ));
                    inner.set_mode(FleetMode::Primary);
                    return Ok(reply);
                }
                Err(f) => {
                    inner.record(inner.failure_row(
                        &req,
                        EngineKind::ClaudeCli,
                        Some(account.label.clone()),
                        duration_ms,
                        f.kind(),
                        false,
                    ));
                    match f {
                        Failure::RateLimited { reset_at_ms, rule } => {
                            inner.cool(i, reset_at_ms);
                            reasons
                                .push(format!("account {} rate-limited ({rule})", account.label));
                        }
                        Failure::Auth { detail } => {
                            inner.cool(i, None);
                            reasons.push(format!("account {} auth: {detail}", account.label));
                        }
                        Failure::Transient { detail } => {
                            reasons.push(format!("account {}: {detail}", account.label));
                        }
                        Failure::Fatal(e) => return Err(e),
                    }
                }
            }
        }
        if reasons.is_empty() {
            reasons.push(if inner.accounts.is_empty() {
                "no claude cli accounts configured".into()
            } else {
                "all claude cli accounts cooling".into()
            });
        }

        if let Some(api) = &inner.api {
            let started = Instant::now();
            match api.complete(&req, &[], inner.now()).await {
                Ok(o) => {
                    let reply = EngineReply {
                        text: o.text,
                        engine: EngineKind::AnthropicApi,
                        account: None,
                        model: o.model,
                        first_text_ms: None,
                        duration_ms: o.duration_ms,
                        usage: o.usage,
                        cost_usd: Some(o.cost_usd),
                        fallback_reason: Some(reasons.join("; ")),
                        session_id: None,
                    };
                    inner.record(LedgerRow::from_reply(
                        inner.now(),
                        req.role,
                        &reply,
                        req.correlation_id.clone(),
                    ));
                    inner.set_mode(FleetMode::Degraded);
                    return Ok(reply);
                }
                Err(f) => {
                    inner.record(inner.failure_row(
                        &req,
                        EngineKind::AnthropicApi,
                        None,
                        started.elapsed().as_millis() as u64,
                        f.kind(),
                        false,
                    ));
                    if let Failure::Fatal(e) = f {
                        return Err(e);
                    }
                    reasons.push(format!("api: {}", f.into_engine_error("api")));
                }
            }
        }

        for driver in inner.fallbacks.clone() {
            let started = Instant::now();
            let engine = match driver {
                FallbackDriver::Codex => EngineKind::CodexCli,
                FallbackDriver::Agy => EngineKind::AgyCli,
                FallbackDriver::LocalGemma => EngineKind::LocalGemma,
            };
            match inner.run_alt(driver, &req).await {
                Ok(mut reply) => {
                    reply.fallback_reason = Some(reasons.join("; "));
                    inner.record(LedgerRow::from_reply(
                        inner.now(),
                        req.role,
                        &reply,
                        req.correlation_id.clone(),
                    ));
                    inner.set_mode(FleetMode::Degraded);
                    return Ok(reply);
                }
                Err(f) => {
                    inner.record(inner.failure_row(
                        &req,
                        engine,
                        None,
                        started.elapsed().as_millis() as u64,
                        f.kind(),
                        false,
                    ));
                    reasons.push(format!("{engine:?}: {}", f.into_engine_error("alt")));
                }
            }
        }

        inner.set_mode(FleetMode::Down);
        Err(EngineError::Unavailable {
            reason: reasons.join("; "),
        })
    }

    async fn open_session(
        &self,
        template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError> {
        let inner = &self.inner;
        let template = inner.apply_route(template);
        let mut reasons: Vec<String> = Vec::new();
        let mut tried = Vec::new();
        while let Some(i) = inner.pick_account(&tried) {
            tried.push(i);
            match ClaudeWarmSession::open(inner.clone(), i, template.clone()).await {
                Ok(s) => {
                    inner.set_mode(FleetMode::Primary);
                    return Ok(Box::new(s));
                }
                Err(e) => reasons.push(format!("account {}: {e}", inner.accounts[i].label)),
            }
        }
        if reasons.is_empty() {
            reasons.push(if inner.accounts.is_empty() {
                "no claude cli accounts configured".into()
            } else {
                "all claude cli accounts cooling".into()
            });
        }
        if inner.api.is_some() {
            inner.set_mode(FleetMode::Degraded);
            return Ok(Box::new(ApiSession::new(
                inner.clone(),
                template,
                reasons.join("; "),
            )));
        }
        inner.set_mode(FleetMode::Down);
        Err(EngineError::Unavailable {
            reason: reasons.join("; "),
        })
    }

    async fn health(&self) -> Vec<HealthReport> {
        crate::health::preflight(&self.inner).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::tests::{req, StubTransport};
    use crate::config::AccountConfig;
    use crate::stream::parse_stream;
    use shadowrunr_types::{Effort, ModelTier, Role};
    use std::collections::HashMap;
    use std::sync::atomic::AtomicI64;

    const SUCCESS: &str = include_str!("../tests/fixtures/claude_success.jsonl");
    const RATE_LIMIT: &str =
        include_str!("../tests/fixtures/claude_rate_limit_reconstructed.jsonl");

    /// Replays a fixture per account label; counts calls.
    struct FixtureRunner {
        by_label: HashMap<String, &'static str>,
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl CliRunner for FixtureRunner {
        async fn run(&self, a: &Account, _r: &EngineRequest) -> Result<TurnOutcome, Failure> {
            self.calls.lock().unwrap().push(a.label.clone());
            parse_stream(self.by_label[&a.label])
        }
    }

    struct Setup {
        fleet: Fleet,
        runner: Arc<FixtureRunner>,
        stub: Arc<StubTransport>,
        now: Arc<AtomicI64>,
        _dir: tempfile::TempDir,
    }

    fn setup(labels: &[(&str, &'static str)], api_env: Option<&str>) -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let cfg = EngineConfig {
            accounts: labels
                .iter()
                .map(|(l, _)| AccountConfig {
                    label: (*l).into(),
                    config_dir: None,
                })
                .collect(),
            api_key_env: api_env.map(str::to_string),
            ledger_path: Some(dir.path().join("ledger.jsonl").to_string_lossy().into()),
            work_dir: Some(dir.path().to_string_lossy().into()),
            ..Default::default()
        };
        let runner = Arc::new(FixtureRunner {
            by_label: labels.iter().map(|(l, f)| ((*l).to_string(), *f)).collect(),
            calls: Mutex::new(vec![]),
        });
        let stub = Arc::new(StubTransport::ok("Kneel, minion."));
        let now = Arc::new(AtomicI64::new(1_700_000_000_000));
        let n2 = now.clone();
        let cli = ClaudeCli::new("claude".into(), dir.path().into(), true, 2);
        let fleet = Fleet::with_parts(
            &cfg,
            cli,
            runner.clone(),
            stub.clone(),
            Arc::new(move || n2.load(Ordering::SeqCst)),
        );
        Setup {
            fleet,
            runner,
            stub,
            now,
            _dir: dir,
        }
    }

    fn ledger(s: &Setup) -> Vec<LedgerRow> {
        let p = s.fleet.inner.ledger.path().unwrap().to_path_buf();
        std::fs::read_to_string(p)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn core() -> EngineRequest {
        req(Role::CoreThread, ModelTier::Haiku, Effort::Low)
    }

    #[tokio::test]
    async fn rate_limited_account_rotates_to_next_with_fallback_reason() {
        let s = setup(&[("A", RATE_LIMIT), ("B", SUCCESS)], None);
        let reply = s.fleet.complete(core()).await.unwrap();
        assert_eq!(reply.account.as_deref(), Some("B"));
        assert_eq!(reply.engine, EngineKind::ClaudeCli);
        assert_eq!(reply.text, "ok");
        let reason = reply.fallback_reason.expect("fallback_reason set");
        assert!(reason.contains("account A rate-limited"), "{reason}");
        // A cools until the reset in the stream (1791999000 s), not the default 15 min.
        let st = s.fleet.accounts();
        assert_eq!(st[0].cooling_until_ms, Some(1_791_999_000_000));
        assert_eq!(st[1].cooling_until_ms, None);
        assert_eq!(s.fleet.fleet_state().mode, FleetMode::Primary);
        assert!(s.fleet.can_serve());
        assert_eq!(s.fleet.earliest_reset_ms(), Some(1_791_999_000_000));
        // Next call skips A entirely.
        let again = s.fleet.complete(core()).await.unwrap();
        assert_eq!(again.account.as_deref(), Some("B"));
        assert!(again.fallback_reason.is_none());
        assert_eq!(*s.runner.calls.lock().unwrap(), ["A", "B", "B"]);
        // Ledger: failure row for A, success rows for B.
        let rows = ledger(&s);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].error_kind.as_deref(), Some("rate_limited"));
        assert_eq!(rows[0].account.as_deref(), Some("A"));
        assert_eq!(rows[1].error_kind, None);
        assert!(rows[1].fallback_reason.is_some());
    }

    #[tokio::test]
    async fn all_cooling_falls_to_api_key() {
        std::env::set_var("SHADOWRUNR_TEST_KEY_FLEET_A", "test-not-a-key");
        let s = setup(
            &[("A", RATE_LIMIT), ("B", RATE_LIMIT)],
            Some("SHADOWRUNR_TEST_KEY_FLEET_A"),
        );
        let t0 = s.fleet.fleet_state().since_ms;
        s.now.fetch_add(5_000, Ordering::SeqCst);
        let reply = s.fleet.complete(core()).await.unwrap();
        assert_eq!(reply.engine, EngineKind::AnthropicApi);
        assert_eq!(reply.text, "Kneel, minion.");
        assert_eq!(reply.model, "claude-haiku-5-5");
        let reason = reply.fallback_reason.unwrap();
        assert!(
            reason.contains("account A") && reason.contains("account B"),
            "{reason}"
        );
        let st = s.fleet.fleet_state();
        assert_eq!(st.mode, FleetMode::Degraded);
        assert_eq!(st.since_ms, t0 + 5_000);
        // The stub saw a core-thread body with thinking disabled and a cached system block.
        let seen = s.stub.seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].0.ends_with("/v1/messages"));
        assert_eq!(seen[0].1["thinking"]["type"], "disabled");
        assert_eq!(seen[0].1["system"][0]["cache_control"]["type"], "ephemeral");
        // While A and B cool, the next call goes straight to the API.
        let again = s.fleet.complete(core()).await.unwrap();
        assert_eq!(again.engine, EngineKind::AnthropicApi);
        assert!(again
            .fallback_reason
            .unwrap()
            .contains("all claude cli accounts cooling"));
        assert_eq!(s.runner.calls.lock().unwrap().len(), 2);
        let rows = ledger(&s);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[2].engine, EngineKind::AnthropicApi);
        assert!(rows[2].cost_usd.is_some());
    }

    #[tokio::test]
    async fn cooldown_expires_and_account_returns() {
        std::env::set_var("SHADOWRUNR_TEST_KEY_FLEET_B", "test-not-a-key");
        let s = setup(&[("A", RATE_LIMIT)], Some("SHADOWRUNR_TEST_KEY_FLEET_B"));
        let r = s.fleet.complete(core()).await.unwrap();
        assert_eq!(r.engine, EngineKind::AnthropicApi);
        s.now.store(1_791_999_000_001, Ordering::SeqCst);
        assert_eq!(s.fleet.accounts()[0].cooling_until_ms, None);
        let _ = s.fleet.complete(core()).await;
        assert_eq!(s.runner.calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn all_down_is_unavailable_and_fleet_down() {
        let s = setup(&[("A", RATE_LIMIT), ("B", RATE_LIMIT)], None);
        match s.fleet.complete(core()).await {
            Err(EngineError::Unavailable { reason }) => {
                assert!(reason.contains("account A") && reason.contains("account B"))
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(s.fleet.fleet_state().mode, FleetMode::Down);
        assert!(!s.fleet.can_serve());
        assert!(s.fleet.earliest_reset_ms().is_some());
        assert!(matches!(
            s.fleet.open_session(core()).await,
            Err(EngineError::Unavailable { .. })
        ));
        let rows = ledger(&s);
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|r| r.error_kind.as_deref() == Some("rate_limited")));
    }

    #[tokio::test]
    async fn api_failure_after_fleet_loss_is_down_and_logged() {
        // Key env var deliberately unset: the API attempt fails as auth, then Unavailable.
        let s = setup(&[("A", RATE_LIMIT)], Some("SHADOWRUNR_TEST_KEY_NEVER_SET"));
        assert!(matches!(
            s.fleet.complete(core()).await,
            Err(EngineError::Unavailable { .. })
        ));
        assert_eq!(s.fleet.fleet_state().mode, FleetMode::Down);
        let rows = ledger(&s);
        assert_eq!(rows[1].engine, EngineKind::AnthropicApi);
        assert_eq!(rows[1].error_kind.as_deref(), Some("auth"));
    }

    #[tokio::test]
    async fn local_gemma_fallback_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = EngineConfig {
            cli_fallbacks: vec!["local_gemma".into()],
            ledger_path: Some(dir.path().join("l.jsonl").to_string_lossy().into()),
            work_dir: Some(dir.path().to_string_lossy().into()),
            ..Default::default()
        };
        let fleet = Fleet::new(&cfg);
        match fleet.complete(core()).await {
            Err(EngineError::Unavailable { reason }) => {
                assert!(
                    reason.contains("local_gemma is not implemented"),
                    "{reason}"
                )
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn route_override_from_config_is_applied() {
        let mut s = setup(&[("A", SUCCESS)], None);
        let mut cfg = s.fleet.inner.cfg.clone();
        cfg.routes.insert(
            Role::CoreThread,
            crate::config::RouteOverride {
                tier: Some(ModelTier::Sonnet),
                ..Default::default()
            },
        );
        let cli = s.fleet.inner.cli.clone();
        s.fleet = Fleet::with_parts(&cfg, cli, s.runner.clone(), s.stub.clone(), system_clock());
        let r = s.fleet.inner.apply_route(core());
        assert_eq!(r.tier, ModelTier::Sonnet);
        assert_eq!(r.effort, Effort::Low);
    }
}
