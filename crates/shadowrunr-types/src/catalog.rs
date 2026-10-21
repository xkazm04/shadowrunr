//! Read-only catalogue data the operator app shows: character packages, segment tables and
//! the rank ladder (`character.list`, `character.get`, `segments.tables`, `ranks.table`).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::character::{
    CharacterAvatar, CharacterIdentity, CharacterVoice, Provenance, RankThreshold, SegmentFlavour,
};
use crate::{Rank, SegmentId, SegmentStatus};

/// One row of `character.list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterSummary {
    pub id: String,
    /// Display name, e.g. "Arch-Lich Malakor".
    pub name: String,
    /// One line under the name (`character.toml` `tagline`); omitted when the package has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
    pub default_tone: String,
    /// The tone the operator chose (`<data>/characters/<id>/active.json`), else `default_tone`.
    pub active_tone: String,
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

/// A package rule with its effective state (`character.rules`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuleState {
    pub id: String,
    pub title: String,
    pub enabled: bool,
    /// The package default from `character.toml` `[rules]`.
    pub default: bool,
    /// A rail (`character.toml` `rails`): always on; the operator override cannot reach it and
    /// `character.rules.set` refuses it. Absent (an older daemon): false.
    #[serde(default)]
    pub locked: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
    pub default_tone: String,
    /// The tone the operator chose, else `default_tone` (see [`CharacterSummary`]).
    pub active_tone: String,
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
    /// `[identity]` as written; omitted when the package has none (the neutral identity).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<CharacterIdentity>,
    /// `[voice]` as written: each field set overrides the daemon's `[voice]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<CharacterVoice>,
    /// `[avatar]` as written: `face` overrides `[stage] avatar`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<CharacterAvatar>,
    /// `[ranks]` overrides of the engine ladder; omitted when the package sets none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank_overrides: Option<RankOverrides>,
    /// Every tuning key in table order, with the package's value over the engine default.
    /// Absent (an older daemon): not shown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tuning: Vec<TuningRow>,
    /// `[segments.<id>]`: flavour appended to each segment's prompts; omitted when none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub segment_flavour: BTreeMap<SegmentId, SegmentFlavour>,
}

/// What a package's `[ranks]` changes in the engine ladder (`segments/data/ranks.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RankOverrides {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weights: BTreeMap<Rank, f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub thresholds: BTreeMap<Rank, RankThreshold>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propose_min_rank: Option<Rank>,
}

/// One tuning key of a package: the value the engine runs with and where it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TuningRow {
    /// e.g. `carry_ms` (`_ms` milliseconds, `_s` seconds, else a count).
    pub key: String,
    /// The value in effect: the package's when it sets the key, else the engine default.
    pub value: u64,
    /// The engine default (`TUNING_DEFAULTS`).
    pub default: u64,
    /// The package sets this key in `[tuning]`.
    pub set: bool,
    /// The package's provenance when set, else the default's.
    pub provenance: Provenance,
    /// The run or lesson behind the value; omitted when none is named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// What the key governs, in one line.
    pub doc: String,
}

/// How bad a package issue is: an `error` fails `character validate`; a `warning` (a
/// template placeholder still to fill) does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Error,
    Warning,
}

/// One problem `character validate` found: `file: key: reason`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterIssue {
    /// Relative to the package, with `/` separators, e.g. `character.toml`.
    pub file: String,
    /// The dotted key (`tuning.carry_ms`) or `line <n>`; omitted when the issue is the file.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    pub reason: String,
    pub severity: IssueSeverity,
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
    /// Bone-clerk searches per hex per stream before a Plebiscite takes the hex's next move.
    #[serde(default)]
    pub max_searches_per_hex: u32,
    /// Length of that Plebiscite.
    #[serde(default)]
    pub plebiscite_secs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handover: Option<HandoverInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_open: Option<ColdOpenInfo>,
}

/// SEG-01's cold open: ends at `to` once `active_chatters` distinct chatters (each wrote in
/// the last `window_s` seconds) have held for `sustain_s`, never before `min_s`, and at
/// `max_s` regardless. The handover's audience path waits for it; its ceiling does not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColdOpenInfo {
    pub to: String,
    pub min_s: u32,
    pub max_s: u32,
    pub active_chatters: u32,
    pub window_s: u32,
    pub sustain_s: u32,
}

/// What one state narrows (`[state_rules.<state>]`); an absent list means the segment's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StateRulesInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_ops: Option<Vec<String>>,
    /// The line the state summary carries while the segment is in this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screens: Option<Vec<String>>,
}

/// SEG-02A's multi-act arc for a long stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArcInfo {
    /// The arc runs only when the segment is planned for at least this long.
    pub min_planned_s: u32,
    pub acts: Vec<String>,
    /// The seal window at the end of the planned time; `sign_off` waits for it.
    pub seal_s: u32,
    /// One reveal slot per hex staged for an act (`reveal_not_due`).
    pub reveal_pacing: bool,
    /// Least gap between two reveals, in percent of a slot; 0 is off.
    pub reveal_gap_pct: u32,
    /// A reveal needs a search or a closed crowd vote on the hex first (`reveal_needs_work`).
    pub reveal_needs_work: bool,
    /// Direction only: at most one act recap per this many seconds; 0 is off.
    pub recap_cooldown_s: u32,
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
    /// Screens `show_screen` may put up; the first is the default at `segment_start`.
    #[serde(default)]
    pub screens: Vec<String>,
    /// Chat command names accepted in this segment.
    pub commands: Vec<String>,
    pub switch_to: Vec<SegmentId>,
    pub rules_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_notes: Option<String>,
    pub clock: ClockInfo,
    pub transitions: Vec<TransitionInfo>,
    /// Per-state rules, keyed by state name; omitted when the table declares none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub state_rules: BTreeMap<String, StateRulesInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<ArcInfo>,
}

/// One chat command as the runtime's parser reads it (`shadowrunr-segments` `command.rs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CommandInfo {
    /// Without the `!`, e.g. `summon`.
    pub name: String,
    /// The argument shape, e.g. `<hex> <DOC|REP|CLM|SPC>`.
    pub args: String,
    /// What the command does, in one line.
    pub what: String,
    /// Lowest rank allowed to use it; every rank when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_rank: Option<Rank>,
}

/// `segments.tables`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentTables {
    pub segments: Vec<SegmentTableInfo>,
    /// Every `Op` tag in the action grammar.
    pub op_tags: Vec<String>,
    /// Every chat command name.
    pub command_names: Vec<String>,
    /// Every chat command with its arguments and meaning, in `command_names` order.
    pub commands: Vec<CommandInfo>,
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
