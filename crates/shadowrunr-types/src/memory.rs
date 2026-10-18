//! Memory atoms. Markdown files are the source of truth; SQLite is a rebuildable index.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::TsMs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeKind {
    /// A stretch of stream: chat plus Malakor's lines.
    Stream,
    /// A background job's result.
    Job,
    /// Reconcile & Plan session with the owner.
    Reconcile,
    /// Machine-generated note (importance 1).
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Episode {
    pub id: String,
    pub ts_ms: TsMs,
    pub stream_id: String,
    pub kind: EpisodeKind,
    pub text: String,
    /// 1..=5
    pub importance: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FactScope {
    /// About one viewer (keyed by viewer id).
    Viewer,
    /// About Malakor himself (lore he has established on stream).
    Character,
    /// About the show's world and running jokes.
    World,
    /// About a SEG-02A case.
    Case,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Fact {
    pub id: String,
    pub scope: FactScope,
    pub key: String,
    pub value: String,
    /// 0.0..=1.0
    pub confidence: f32,
    /// Episode ids. Never empty: a fact without a source is rejected.
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
}
