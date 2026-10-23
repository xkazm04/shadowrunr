//! The chat moderator's wire types (docs/spec/contract.md, "Moderation").
//!
//! The moderator is a second character (`role = "moderator"`) that guards a platform chat
//! from its own bot account. A daemon lane screens every platform message against the
//! operator's [`ModRule`] list and, when [`ModMode::Armed`], deletes, times out or bans on its
//! own; in [`ModMode::Shadow`] it decides and logs only. Every decision is one
//! [`ModDecision`] in `<data>/moderator/decisions.jsonl` (append-only).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Platform, TsMs};

/// How a rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModRuleKind {
    /// `pattern` is a term or regex, matched on normalised text (instant, no model).
    Pattern,
    /// `text` is a plain-language rule a classifier model judges ("no spoilers").
    Plain,
}

/// What the moderator does to a message's author.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModActionKind {
    /// Delete the message only.
    Delete,
    /// Delete the message and time the author out for `timeout_s`.
    Timeout,
    /// Delete the message and ban the author from the channel.
    Ban,
}

/// Escalation: after `count` hits of the same rule by the same viewer within `window_s`, the
/// action becomes `then` (with `then_timeout_s` when `then` is a timeout).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModStrikes {
    pub count: u32,
    pub window_s: u32,
    pub then: ModActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub then_timeout_s: Option<u32>,
}

/// One operator rule. `pattern` is required for `kind = pattern`, `text` for `kind = plain`;
/// `timeout_s` is required when `action = timeout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModRule {
    /// Stable id, `[a-z0-9-]`.
    pub id: String,
    /// Short human name; the moderator's chat line names this, never the message text.
    pub name: String,
    pub kind: ModRuleKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub action: ModActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strikes: Option<ModStrikes>,
    pub enabled: bool,
}

/// Whether decisions are carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModMode {
    /// Decide and log ("would have"), act on nothing, post nothing. The default.
    #[default]
    Shadow,
    /// Carry decisions out on the platform and post the moderator's line.
    Armed,
}

/// One decision of the moderation lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModDecision {
    pub id: String,
    pub ts_ms: TsMs,
    pub platform: Platform,
    pub viewer_id: String,
    pub viewer_name: String,
    pub message_id: String,
    pub rule_id: String,
    /// The action after strike escalation.
    pub action: ModActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_s: Option<u32>,
    pub mode: ModMode,
    /// The platform call succeeded (always false in shadow mode and for `rules.test`).
    pub executed: bool,
    /// The moderator's chat line for this decision (posted when armed, logged in shadow).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// Why the platform call failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// An unban, appended to the same log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModUnban {
    pub ts_ms: TsMs,
    pub platform: Platform,
    pub viewer_id: String,
    pub executed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One line of `decisions.jsonl`, and one row of `moderator.log`. Struct variants (not
/// newtypes) so the generated TypeScript keeps the `kind` tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModLogEntry {
    Decision { decision: ModDecision },
    Unban { unban: ModUnban },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlatformState {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

/// A chat platform connection as the moderator sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlatformStatus {
    pub platform: Platform,
    pub state: PlatformState,
    /// The bot account's login, once known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_login: Option<String>,
    /// The broadcaster channel the bot moderates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Why it is disconnected or in error ("no client_id configured").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct ModCounts {
    pub decisions: u32,
    pub executed: u32,
}

/// `moderator.status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModeratorStatus {
    pub mode: ModMode,
    /// The moderator character's package id (`[moderator] character`).
    pub character: String,
    pub platforms: Vec<PlatformStatus>,
    /// Since the daemon started.
    pub counts: ModCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModRules {
    pub rules: Vec<ModRule>,
}

/// `moderator.rules.set`: replaces the whole list atomically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModRulesSetParams {
    pub rules: Vec<ModRule>,
}

/// `moderator.rules.test`: screens one sample message with no side effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModRulesTestParams {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModRulesTestResult {
    /// Empty when no rule matched.
    pub decisions: Vec<ModDecision>,
    /// The message was read as a self-disclosure: no rule acts on it (live-safety-rails).
    pub disclosure: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModModeSetParams {
    pub mode: ModMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct ModLogParams {
    /// Newest first; default 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModLog {
    /// Newest first.
    pub entries: Vec<ModLogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModUnbanParams {
    pub platform: Platform,
    pub viewer_id: String,
}

/// `twitch.connect`: the device-code login the operator completes in a browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TwitchConnect {
    pub verification_uri: String,
    pub user_code: String,
    pub expires_s: u32,
}
