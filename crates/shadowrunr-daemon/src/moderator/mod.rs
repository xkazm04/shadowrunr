//! The chat moderator (docs/spec/contract.md "Moderation", docs/guide/moderator.md).
//!
//! One lane in the daemon, ahead of everything else that reads chat. Every platform message
//! goes to [`lane`], which runs whether or not a show is on. Pattern rules decide at once,
//! plain-language rules go to a classifier in batches, and a self-disclosure is never acted
//! on. When armed, the lane deletes, times out or bans through [`crate::platform`], and
//! [`voice`] writes the moderator character's line. Every decision is appended to
//! `<data>/moderator/decisions.jsonl` ([`store`]).

pub mod lane;
pub mod rules;
pub mod store;
pub mod voice;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use shadowrunr_types::moderation::{
    ModLog, ModLogParams, ModModeSetParams, ModRules, ModRulesSetParams, ModRulesTestParams,
    ModRulesTestResult, ModUnban, ModUnbanParams, ModeratorStatus, TwitchConnect,
};

use crate::config::Config;
use crate::platform::twitch::TwitchPlatform;
use crate::rpc::RpcError;
use crate::server::AppState;

/// `[moderator]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeratorConfig {
    /// The moderator character's package id (`characters/<id>/`, `role = "moderator"`).
    pub character: String,
    /// The `shadowrunr mcp` moderator write tools run only when true.
    pub mcp_allow_writes: bool,
    /// How long the classifier collects messages before one call (plain-language rules).
    pub classifier_batch_ms: u64,
}

impl Default for ModeratorConfig {
    fn default() -> Self {
        Self {
            character: "moderator".into(),
            mcp_allow_writes: false,
            classifier_batch_ms: 1500,
        }
    }
}

/// The moderator's state in the daemon (one per process).
pub struct ModeratorHub {
    pub twitch: Arc<TwitchPlatform>,
}

impl ModeratorHub {
    pub fn new(cfg: &Config) -> Arc<Self> {
        Arc::new(Self {
            twitch: TwitchPlatform::new(cfg.twitch.clone()),
        })
    }
}

/// Starts the lane and the configured platforms. Called once by the daemon after bind.
pub fn start(_state: &Arc<AppState>) {}

fn not_yet(method: &str) -> RpcError {
    RpcError::new(
        shadowrunr_types::ErrorCode::Internal,
        format!("{method} is not implemented yet"),
    )
}

pub async fn rpc_status(_state: &Arc<AppState>) -> Result<ModeratorStatus, RpcError> {
    Err(not_yet("moderator.status"))
}

pub async fn rpc_rules_list(_state: &Arc<AppState>) -> Result<ModRules, RpcError> {
    Err(not_yet("moderator.rules.list"))
}

pub async fn rpc_rules_set(
    _state: &Arc<AppState>,
    _p: ModRulesSetParams,
) -> Result<ModRules, RpcError> {
    Err(not_yet("moderator.rules.set"))
}

pub async fn rpc_rules_test(
    _state: &Arc<AppState>,
    _p: ModRulesTestParams,
) -> Result<ModRulesTestResult, RpcError> {
    Err(not_yet("moderator.rules.test"))
}

pub async fn rpc_mode_set(
    _state: &Arc<AppState>,
    _p: ModModeSetParams,
) -> Result<ModeratorStatus, RpcError> {
    Err(not_yet("moderator.mode.set"))
}

pub async fn rpc_log(_state: &Arc<AppState>, _p: ModLogParams) -> Result<ModLog, RpcError> {
    Err(not_yet("moderator.log"))
}

pub async fn rpc_unban(_state: &Arc<AppState>, _p: ModUnbanParams) -> Result<ModUnban, RpcError> {
    Err(not_yet("moderator.unban"))
}

pub async fn rpc_twitch_connect(_state: &Arc<AppState>) -> Result<TwitchConnect, RpcError> {
    Err(not_yet("twitch.connect"))
}

pub async fn rpc_twitch_disconnect(_state: &Arc<AppState>) -> Result<ModeratorStatus, RpcError> {
    Err(not_yet("twitch.disconnect"))
}
