//! Engine configuration (`shadowrunr.toml` `[engine]`) and the role routing table.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use shadowrunr_types::{Effort, ModelTier, Role};

/// Default routing per role: (tier, effort, timeout_ms). The tournament recalibrates the
/// core-thread entry: the night tournament (2026-10-18, n=60 per tier, Opus judge) put Sonnet
/// at 0.4% red-line fails against Haiku's 1.9%. The judge timeout sits above the measured
/// whole-transcript p95 (502 s) with headroom; at 600 s one transcript never finished.
/// Everything else is a starting point.
pub fn route(role: Role) -> (ModelTier, Effort, u64) {
    match role {
        Role::CoreThread => (ModelTier::Sonnet, Effort::Low, 30_000),
        Role::ChatPicker => (ModelTier::Haiku, Effort::Low, 20_000),
        Role::Planner => (ModelTier::Sonnet, Effort::Medium, 120_000),
        Role::Researcher => (ModelTier::Sonnet, Effort::Medium, 240_000),
        Role::Consolidator => (ModelTier::Sonnet, Effort::Medium, 300_000),
        Role::Chatter => (ModelTier::Haiku, Effort::Low, 60_000),
        Role::Judge => (ModelTier::Sonnet, Effort::Medium, 900_000),
        Role::Reconciler => (ModelTier::Opus, Effort::High, 600_000),
    }
}

/// Every role, in contract order (for tables such as the daemon's `engine.status`).
pub const ROLES: [Role; 8] = [
    Role::CoreThread,
    Role::ChatPicker,
    Role::Planner,
    Role::Researcher,
    Role::Consolidator,
    Role::Chatter,
    Role::Judge,
    Role::Reconciler,
];

/// One row of the route override table. Absent fields keep the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<ModelTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// The fallback drivers that `cli_fallbacks` may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackDriver {
    /// OpenAI Codex CLI. Experimental.
    Codex,
    /// Antigravity CLI. Experimental.
    Agy,
    /// Local Gemma. Out of scope in v0.1 (GPU work is excluded); always unavailable.
    LocalGemma,
}

impl FallbackDriver {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "codex" | "codex_cli" => Some(Self::Codex),
            "agy" | "agy_cli" => Some(Self::Agy),
            "local_gemma" | "gemma" => Some(Self::LocalGemma),
            _ => None,
        }
    }

    /// The canonical `cli_fallbacks` name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Agy => "agy",
            Self::LocalGemma => "local_gemma",
        }
    }
}

pub const DEFAULT_MAX_LIVE_PROCESSES: usize = 6;
pub const DEFAULT_IDLE_TIMEOUT_MS: u64 = 20 * 60 * 1000;
pub const DEFAULT_COOLDOWN_MS: u64 = 15 * 60 * 1000;
pub const DEFAULT_API_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_CODEX_MODEL: &str = "gpt-5.6-sol";
pub const DEFAULT_AGY_MODEL: &str = "gemini-3.8-flash";

/// Engine configuration, loaded from `shadowrunr.toml` `[engine]` by the daemon. Every
/// optional field is omitted from serialised output when unset.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Claude Code accounts in rotation order: label + CLAUDE_CONFIG_DIR.
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    /// Env var name holding an Anthropic API key for the first fallback (never the key itself).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Further fallbacks after the API key, in order: `codex`, `agy` (experimental), and
    /// `local_gemma` (always unavailable in v0.1).
    #[serde(default)]
    pub cli_fallbacks: Vec<String>,
    /// Use the deterministic scripted engine (tests, harness smoke runs).
    #[serde(default)]
    pub scripted: bool,
    /// Path or name of the `claude` executable. Default `claude` (resolved on PATH).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_bin: Option<String>,
    /// Working directory for spawned CLIs and the system-prompt files. Default
    /// `<temp>/shadowrunr-engine`, so no project CLAUDE.md is picked up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_dir: Option<String>,
    /// Ledger path. Default `ledger.jsonl` in the process working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger_path: Option<String>,
    /// Global cap on live CLI processes (one-shot calls and warm sessions). Default 6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_live_processes: Option<usize>,
    /// Warm-session idle timeout. Default 20 minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_timeout_ms: Option<u64>,
    /// Cooldown for a rate-limited account when the CLI gives no reset time. Default 15 min.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_ms: Option<u64>,
    /// Pass `--safe-mode` to spawned `claude` (no CLAUDE.md, skills, hooks, MCP servers or
    /// installed plugins from the operator's setup). Default true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_mode: Option<bool>,
    /// Route override table keyed by role (`core_thread = { tier = "sonnet" }`). When a role
    /// has an entry, the engine applies it to every request for that role.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub routes: HashMap<Role, RouteOverride>,
    /// Messages API base URL. Default `https://api.anthropic.com`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base_url: Option<String>,
    /// Command head for codex (for example `["node", "C:/.../codex.js"]`). Default `["codex"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_bin: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_model: Option<String>,
    /// Command head for agy. Default `["agy"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agy_bin: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agy_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub label: String,
    /// Path for CLAUDE_CONFIG_DIR; omitted = the default login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
}

impl EngineConfig {
    /// Routing for a role: the default table with this config's overrides applied.
    pub fn route(&self, role: Role) -> (ModelTier, Effort, u64) {
        let (tier, effort, timeout) = route(role);
        match self.routes.get(&role) {
            Some(o) => (
                o.tier.unwrap_or(tier),
                o.effort.unwrap_or(effort),
                o.timeout_ms.unwrap_or(timeout),
            ),
            None => (tier, effort, timeout),
        }
    }

    /// Whether the route override table has an entry for `role` (the engine then applies
    /// [`EngineConfig::route`] to every request of that role).
    pub fn route_overridden(&self, role: Role) -> bool {
        self.routes.contains_key(&role)
    }

    pub fn claude_bin(&self) -> String {
        self.claude_bin.clone().unwrap_or_else(|| "claude".into())
    }

    pub fn work_dir(&self) -> PathBuf {
        self.work_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("shadowrunr-engine"))
    }

    pub fn ledger_path(&self) -> PathBuf {
        PathBuf::from(self.ledger_path.as_deref().unwrap_or("ledger.jsonl"))
    }

    pub fn max_live_processes(&self) -> usize {
        self.max_live_processes
            .unwrap_or(DEFAULT_MAX_LIVE_PROCESSES)
            .max(1)
    }

    pub fn idle_timeout_ms(&self) -> u64 {
        self.idle_timeout_ms.unwrap_or(DEFAULT_IDLE_TIMEOUT_MS)
    }

    pub fn cooldown_ms(&self) -> u64 {
        self.cooldown_ms.unwrap_or(DEFAULT_COOLDOWN_MS)
    }

    pub fn safe_mode(&self) -> bool {
        self.safe_mode.unwrap_or(true)
    }

    pub fn api_base_url(&self) -> String {
        self.api_base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_API_BASE_URL.into())
    }

    /// Recognised fallback drivers, in configured order; unknown names are skipped.
    pub fn fallback_drivers(&self) -> Vec<FallbackDriver> {
        self.cli_fallbacks
            .iter()
            .filter_map(|s| FallbackDriver::parse(s))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_overrides_load_from_toml_shaped_json() {
        let cfg: EngineConfig = serde_json::from_str(
            r#"{"routes":{"core_thread":{"tier":"sonnet"},"judge":{"effort":"high","timeout_ms":5}}}"#,
        )
        .unwrap();
        assert_eq!(
            cfg.route(Role::CoreThread),
            (ModelTier::Sonnet, Effort::Low, 30_000)
        );
        assert_eq!(cfg.route(Role::Judge), (ModelTier::Sonnet, Effort::High, 5));
        assert_eq!(cfg.route(Role::Planner), route(Role::Planner));
        assert!(cfg.route_overridden(Role::Judge));
        assert!(!cfg.route_overridden(Role::Planner));
        for d in [
            FallbackDriver::Codex,
            FallbackDriver::Agy,
            FallbackDriver::LocalGemma,
        ] {
            assert_eq!(FallbackDriver::parse(d.name()), Some(d));
        }
        assert_eq!(ROLES.len(), 8);
    }

    #[test]
    fn defaults_and_omitted_fields() {
        let cfg = EngineConfig::default();
        assert_eq!(cfg.max_live_processes(), 6);
        assert_eq!(cfg.idle_timeout_ms(), 1_200_000);
        assert_eq!(cfg.cooldown_ms(), 900_000);
        assert!(cfg.safe_mode());
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(!json.contains("null"), "{json}");
        assert_eq!(
            FallbackDriver::parse("local_gemma"),
            Some(FallbackDriver::LocalGemma)
        );
    }
}
