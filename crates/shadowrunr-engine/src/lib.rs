//! Universal LLM wrapper. One [`Engine`] that callers never have to think about:
//!
//! 1. the Claude Code CLI fleet (accounts in health-aware round-robin, one-shot calls and
//!    warm stream-json sessions),
//! 2. then the Anthropic Messages API, when `api_key_env` is set,
//! 3. then the experimental codex / agy CLI drivers, when named in `cli_fallbacks`.
//!
//! Local Gemma is out of scope in v0.1 and answers with a clear error. Every call, success or
//! failure, appends one row to the ledger. Tests use [`ScriptedEngine`] or the
//! `shadowrunr-fake-claude` binary and never spend quota.

pub mod alt_cli;
pub mod api;
pub mod claude_cli;
pub mod config;
pub mod fleet;
pub mod health;
pub mod ledger;
pub mod limits;
pub mod model;
pub mod scripted;
pub mod session;
pub mod stream;

use std::sync::Arc;

use shadowrunr_types::Engine;

pub use config::{route, AccountConfig, EngineConfig, FallbackDriver, RouteOverride};
pub use fleet::{AccountStatus, Fleet, FleetMode, FleetState};
pub use scripted::ScriptedEngine;

/// Build the engine stack (fleet + fallbacks) from config; `scripted = true` gives the
/// deterministic engine. Use [`build_fleet`] when you need [`Fleet::fleet_state`].
pub fn build_engine(cfg: &EngineConfig) -> Arc<dyn Engine> {
    if cfg.scripted {
        return Arc::new(ScriptedEngine);
    }
    build_fleet(cfg)
}

/// The concrete fleet, for callers that read [`Fleet::fleet_state`]. It coerces to
/// `Arc<dyn Engine>`.
pub fn build_fleet(cfg: &EngineConfig) -> Arc<Fleet> {
    Arc::new(Fleet::new(cfg))
}
