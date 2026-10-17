//! The universal LLM engine seam. Claude Code CLI is the primary engine; every other engine
//! is a fallback behind the same trait.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    Haiku,
    Sonnet,
    Opus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

/// Who is asking. Routing (tier, effort, tools, timeout) is a table keyed by role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Malakor's fast conversational thread.
    CoreThread,
    /// Picks which chat messages the core thread answers (anti-bait).
    ChatPicker,
    /// Segment planning and directing (slow lane).
    Planner,
    /// Background research subagent.
    Researcher,
    /// Sleep-cycle memory consolidation.
    Consolidator,
    /// Plays simulated chatters.
    Chatter,
    /// Judges simulation transcripts.
    Judge,
    /// Between-streams Reconcile & Plan analysis.
    Reconciler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    ClaudeCli,
    AnthropicApi,
    CodexCli,
    AgyCli,
    LocalGemma,
    /// Deterministic, keyless; for tests and harness smoke runs.
    Scripted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRequest {
    pub role: Role,
    pub tier: ModelTier,
    pub effort: Effort,
    /// Stable prefix (constitution + identity). Kept byte-identical across turns so the
    /// provider's prompt cache holds.
    pub system: String,
    /// Per-turn context plus the actual ask.
    pub prompt: String,
    /// Tool allowlist for the spawned CLI. Empty = no tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    pub timeout_ms: u64,
    /// When set, the reply text must be JSON matching this schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<serde_json::Value>,
    /// Correlates the request with a stream, sim run or job, for the ledger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_creation_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineReply {
    pub text: String,
    pub engine: EngineKind,
    /// Fleet account label that served the call (never a credential).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_text_ms: Option<u64>,
    pub duration_ms: u64,
    #[serde(default)]
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Set when the call was served by something other than the first choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EngineError {
    #[error("rate limited on account {account}")]
    RateLimited {
        account: String,
        reset_at_ms: Option<i64>,
    },
    #[error("engine unavailable: {reason}")]
    Unavailable { reason: String },
    #[error("timed out after {after_ms} ms")]
    Timeout { after_ms: u64 },
    #[error("refused: {reason}")]
    Refused { reason: String },
    #[error("could not parse engine output: {detail}")]
    Parse { detail: String },
    #[error("io: {detail}")]
    Io { detail: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    Green,
    Amber,
    Red,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    /// Pre-flight check id, e.g. "PF-01".
    pub check: String,
    pub state: HealthState,
    pub detail: String,
}

/// One live, warm conversation (Claude Code `--input-format stream-json`).
#[async_trait]
pub trait WarmSession: Send {
    async fn send(&mut self, prompt: String) -> Result<EngineReply, EngineError>;
    async fn interrupt(&mut self) -> Result<(), EngineError>;
    async fn close(self: Box<Self>) -> Result<(), EngineError>;
}

#[async_trait]
pub trait Engine: Send + Sync {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError>;
    async fn open_session(
        &self,
        template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError>;
    async fn health(&self) -> Vec<HealthReport>;
}
