//! Universal LLM wrapper. WP1 owns this crate; the skeleton below fixes the public surface
//! that brain, sim and daemon build against in parallel.

pub mod scripted;

use std::sync::Arc;

use shadowrunr_types::{Effort, Engine, ModelTier, Role};

pub use scripted::ScriptedEngine;

/// Default routing per role: (tier, effort, timeout_ms). The tournament recalibrates the
/// core-thread entry; everything else is a starting point.
pub fn route(role: Role) -> (ModelTier, Effort, u64) {
    match role {
        Role::CoreThread => (ModelTier::Haiku, Effort::Low, 30_000),
        Role::ChatPicker => (ModelTier::Haiku, Effort::Low, 20_000),
        Role::Planner => (ModelTier::Sonnet, Effort::Medium, 120_000),
        Role::Researcher => (ModelTier::Sonnet, Effort::Medium, 240_000),
        Role::Consolidator => (ModelTier::Sonnet, Effort::Medium, 300_000),
        Role::Chatter => (ModelTier::Haiku, Effort::Low, 60_000),
        Role::Judge => (ModelTier::Sonnet, Effort::Medium, 180_000),
        Role::Reconciler => (ModelTier::Opus, Effort::High, 600_000),
    }
}

/// Engine configuration, loaded from `shadowrunr.toml` `[engine]` by the daemon.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EngineConfig {
    /// Claude Code accounts in rotation order: label + CLAUDE_CONFIG_DIR.
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    /// Env var name holding an Anthropic API key for the first fallback (never the key itself).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Enable the codex / agy CLI fallbacks (stubbed in v0.1).
    #[serde(default)]
    pub cli_fallbacks: Vec<String>,
    /// Use the deterministic scripted engine (tests, harness smoke runs).
    #[serde(default)]
    pub scripted: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountConfig {
    pub label: String,
    /// Path for CLAUDE_CONFIG_DIR; omitted = the default login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
}

/// Build the engine stack (fleet + fallbacks) from config. WP1 implements the real fleet.
pub fn build_engine(cfg: &EngineConfig) -> Arc<dyn Engine> {
    if cfg.scripted {
        return Arc::new(ScriptedEngine);
    }
    todo!("WP1: Claude Code CLI fleet with fallback chain")
}
