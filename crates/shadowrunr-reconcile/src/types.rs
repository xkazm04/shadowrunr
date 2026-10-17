//! Reconcile & Plan wire types (docs/spec/contract.md § Daemon protocol). Serde names are
//! the contract's, verbatim. Optional fields are omitted from JSON, never `null`.
//!
//! Fields marked "addition" are not in the contract's type sketch yet; they are proposed in
//! the WP6 report and only ever appear when they carry data.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use shadowrunr_types::{Rank, Role, SegmentId, TsMs};

/// What a reconcile run covers: a whole tournament or an explicit list of runs.
/// Wire: `{ "tournament_id": "..." }` or `{ "run_ids": ["..."] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Source {
    Tournament { tournament_id: String },
    Runs { run_ids: Vec<String> },
}

/// A claim plus the transcript lines that back it (`<run_id>#<line>`, 1-based).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub claim: String,
    #[serde(default)]
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentFinding {
    pub segment: SegmentId,
    pub runs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ses_mean: Option<f64>,
    #[serde(default)]
    pub what_worked: Vec<Evidence>,
    #[serde(default)]
    pub what_failed: Vec<Evidence>,
    /// Addition: the deterministic per-segment metrics the findings were drawn from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<SegmentMetrics>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundMethod {
    /// Wilson score interval, 95%.
    Wilson,
    /// Rule of three: zero events in n trials puts the 95% bound at about 3/n.
    RuleOfThree,
}

/// A 95% interval on a rate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bound {
    pub lo: f64,
    pub hi: f64,
    pub method: BoundMethod,
}

/// A rate is never reported without its n and its bound. Rates with n = 0 are omitted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rate {
    pub rate: f64,
    pub n: u32,
    pub bound: Bound,
}

/// Failed hard-rule checks of one rule within one tone cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleFailure {
    pub rule: String,
    pub fails: u32,
    /// Checks that were evaluated (pass + fail); not_provoked / not_evaluable excluded.
    pub n: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToneFinding {
    pub tone_variant: String,
    pub quality_rates: BTreeMap<String, Rate>,
    #[serde(default)]
    pub rule_failures: Vec<RuleFailure>,
    /// Addition: the instrument the cell was measured with. Different hashes are never
    /// merged, so one tone can appear once per hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument_hash: Option<String>,
    /// Addition: judged runs in this cell.
    #[serde(default)]
    pub runs: u32,
}

/// Engagement of one rank: who chatted, who got picked, who got answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankRow {
    pub rank: Rank,
    /// Distinct viewers (per run) of this rank who chatted.
    pub viewers: u32,
    pub messages: u32,
    /// Messages the chat picker chose.
    pub picked: u32,
    /// Messages Malakor (or a foil) answered via `replying_to`.
    pub answered: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick_rate: Option<Rate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_rate: Option<Rate>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RankEngagement {
    /// Lowest rank first; ranks with no messages are omitted.
    #[serde(default)]
    pub by_rank: Vec<RankRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentSource {
    /// A hard-rule check the judge marked `fail`.
    Verdict,
    /// A `moderation` event in the transcript.
    Moderation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleIncident {
    pub run_id: String,
    pub rule: String,
    pub source: IncidentSource,
    /// `input` or `output` for moderation incidents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default)]
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleLatency {
    pub role: Role,
    /// Successful calls.
    pub calls: u32,
    pub errors: u32,
    pub fallbacks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_p50_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_p95_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_text_p50_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_text_p95_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LatencyStats {
    /// Ledger rows read (success and failure).
    pub ledger_rows: u32,
    pub errors: u32,
    pub fallbacks: u32,
    #[serde(default)]
    pub by_role: Vec<RoleLatency>,
    /// In-stream `say` lines by Malakor that carried `first_text_ms`.
    pub say_n: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub say_first_text_p50_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub say_first_text_p95_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoverTiming {
    pub run_id: String,
    pub from: SegmentId,
    pub to: SegmentId,
    pub reason: String,
    /// Time from the `from` segment's start to the handover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<i64>,
    /// Time from the handover to the next `segment_start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap_ms: Option<i64>,
    #[serde(rename = "ref")]
    pub line_ref: String,
}

/// Deterministic per-segment metrics, summed over runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentMetrics {
    pub segment: SegmentId,
    pub runs: u32,
    pub minutes: f64,
    pub chat_msgs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_per_min: Option<f64>,
    pub malakor_lines: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub malakor_lines_per_min: Option<f64>,
    /// SES proxy: share of chat messages that got an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered: Option<Rate>,
    pub ops: u32,
    pub moderation_events: u32,
    pub polls: u32,
    /// Mean over closed polls of voters / audience size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_participation_mean: Option<f64>,
    #[serde(default)]
    pub handovers: Vec<HandoverTiming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ses_mean: Option<f64>,
    /// Segment ends that carried an SES value.
    pub ses_n: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience_size: Option<u32>,
    pub lines: u32,
    pub has_verdict: bool,
    pub has_ledger: bool,
}

/// Everything `analyze` computes. No LLM involved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    pub runs: Vec<RunSummary>,
    pub segments: Vec<SegmentMetrics>,
    pub tones: Vec<ToneFinding>,
    pub ranks: RankEngagement,
    pub red_lines: Vec<RuleIncident>,
    pub latency: LatencyStats,
}

/// An LLM-proposed item that did not survive mechanical verification (the "flag" half of
/// "drop or flag").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroppedItem {
    /// `evidence_ref`, `evidence`, `option` or `segment`.
    pub what: String,
    /// The claim, option title, or segment name.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub report_id: String,
    pub created_ms: TsMs,
    pub source: Source,
    pub segments: Vec<SegmentFinding>,
    pub tone: Vec<ToneFinding>,
    pub ranks: RankEngagement,
    pub red_lines: Vec<RuleIncident>,
    pub latency: LatencyStats,
    pub summary: String,
    /// Addition: refs, evidence and options the verifier dropped, with the reason.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<DroppedItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionKind {
    CharacterChange,
    SegmentChange,
    Topic,
    Experiment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOption {
    pub option_id: String,
    pub kind: OptionKind,
    pub title: String,
    pub rationale: String,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    /// Unified diff against `bible.md` or `tones/*.md`; never `constitution.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanSegment {
    pub segment: SegmentId,
    pub minutes: u32,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextStreamPlan {
    pub plan_id: String,
    pub created_ms: TsMs,
    pub lord_dispatch: String,
    pub segments: Vec<PlanSegment>,
    pub topics: Vec<String>,
    pub directives: Vec<Directive>,
    /// Addition: the report and options this plan was composed from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub option_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectiveKind {
    Prevent,
    Try,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Directive {
    pub id: String,
    pub kind: DirectiveKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<SegmentId>,
    pub created_ms: TsMs,
    pub active: bool,
}

/// `lord.chat` result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LordChatReply {
    pub reply: String,
    pub directives_added: Vec<Directive>,
}
