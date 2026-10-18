//! Read-only catalogue data the operator app shows: character packages, segment tables and
//! the rank ladder (`character.list`, `character.get`, `segments.tables`, `ranks.table`).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Rank, SegmentId, SegmentStatus};

/// One row of `character.list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterSummary {
    pub id: String,
    /// Display name, e.g. "Arch-Lich Malakor".
    pub name: String,
    pub default_tone: String,
    /// Tone variant ids (`tones/<id>.md`), sorted.
    pub tones: Vec<String>,
    /// Foil ids declared in `character.toml`, in order.
    pub foils: Vec<String>,
    /// Optional rules enabled after the operator's override.
    pub rules_enabled: u32,
    pub rules_total: u32,
}

/// One file of a character package, relative to the package directory with `/` separators.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PackageFile {
    pub path: String,
    pub bytes: u64,
}

/// A tone variant (`tones/<id>.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToneInfo {
    pub id: String,
    pub text: String,
}

/// A SEG-01 guest foil (`foils/<id>.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FoilInfo {
    pub id: String,
    /// Listed in `character.toml` `foils` (summonable); an undeclared file is a draft.
    pub declared: bool,
    pub text: String,
}

/// One situation of `situations.json` (format in docs/spec/contract.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Situation {
    pub id: String,
    pub behaviour: String,
    pub archetype: String,
    #[serde(default)]
    pub near_miss: bool,
    pub required: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invariants: Vec<String>,
    /// `message`, `silence`, `vote` or `wave`; inferred by the sim when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stimulus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silence_s: Option<u32>,
    /// Segments the situation is scheduled in; every segment when absent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<SegmentId>,
}

/// An optional constitution rule with its effective state (`character.rules`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuleState {
    pub id: String,
    pub title: String,
    pub enabled: bool,
    /// The package default from `character.toml` `[rules]`.
    pub default: bool,
}

/// An optional rule with its text (`character.get`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterRuleInfo {
    pub id: String,
    pub title: String,
    pub enabled: bool,
    pub default: bool,
    pub text: String,
}

/// `character.get`: the whole package as the operator sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterDetail {
    pub id: String,
    pub name: String,
    pub default_tone: String,
    /// Every file of the package, sorted by path.
    pub files: Vec<PackageFile>,
    /// `constitution.md`: immutable, never edited by the brain.
    pub constitution: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bible: Option<String>,
    pub tones: Vec<ToneInfo>,
    /// Every `foils/*.md`, sorted by id.
    pub foils: Vec<FoilInfo>,
    pub situations: Vec<Situation>,
    pub rules: Vec<CharacterRuleInfo>,
    /// Rank wire name -> the title the character says.
    pub rank_titles: BTreeMap<Rank, String>,
}

/// One transition row: `(from, on[, guard]) -> to`; `from = "*"` matches any state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TransitionInfo {
    pub from: String,
    /// Trigger name, e.g. `poll_opened`.
    pub on: String,
    /// Guard name, e.g. `foil_active`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<String>,
    pub to: String,
}

/// SEG-01's handover clock rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HandoverInfo {
    pub to: SegmentId,
    pub ccv_min: u32,
    pub sustain_s: u32,
    pub max_elapsed_s: u32,
    pub grace_s: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClockInfo {
    pub foil_vote_secs: u32,
    pub poll_min_secs: u32,
    pub poll_max_secs: u32,
    pub max_poll_options: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handover: Option<HandoverInfo>,
}

/// One parsed segment table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentTableInfo {
    pub id: SegmentId,
    pub title: String,
    pub status: SegmentStatus,
    pub initial: String,
    pub states: Vec<String>,
    /// `Op` tags the brain may emit in this segment.
    pub allowed_ops: Vec<String>,
    /// Chat command names accepted in this segment.
    pub commands: Vec<String>,
    pub switch_to: Vec<SegmentId>,
    pub rules_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_notes: Option<String>,
    pub clock: ClockInfo,
    pub transitions: Vec<TransitionInfo>,
}

/// `segments.tables`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentTables {
    pub segments: Vec<SegmentTableInfo>,
    /// Every `Op` tag in the action grammar.
    pub op_tags: Vec<String>,
    /// Every chat command name.
    pub command_names: Vec<String>,
}

/// One way to qualify for a rank through a subscription.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TierRuleInfo {
    pub tier: u8,
    pub min_months: u32,
}

/// One rung of the ladder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RankInfo {
    pub rank: Rank,
    /// The character's title for the rank.
    pub title: String,
    /// Vote weight.
    pub weight: f64,
    /// Gifting at least this many subscriptions qualifies on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_gifted: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<TierRuleInfo>,
}

/// `ranks.table`: thresholds, weights and titles, lowest rank first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RankTableInfo {
    /// The character whose titles are shown.
    pub character: String,
    /// Lowest rank allowed to use `!propose`.
    pub propose_min_rank: Rank,
    pub ranks: Vec<RankInfo>,
}
