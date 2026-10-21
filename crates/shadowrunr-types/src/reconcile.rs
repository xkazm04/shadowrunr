//! Reconcile & Plan wire types (docs/spec/contract.md § Daemon protocol), the results of
//! `reconcile.*`, `plan.*`, `directive.*` and `lord.chat`. Serde names are the contract's,
//! verbatim. Optional fields are omitted from JSON, never `null`.
//!
//! These are the schema source for the generated TS contract. `shadowrunr-reconcile` still
//! declares identical copies in its `types.rs` (proposed to become re-exports of this module);
//! the daemon test `reconcile_types_match_the_contract` round-trips its real output through
//! these types so the two cannot drift.
//!
//! Fields marked "addition" were proposed in the WP6 report and only ever appear when they
//! carry data.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Rank, Role, SegmentId, TsMs};

/// What a reconcile run covers: a whole tournament or an explicit list of runs.
/// Wire: `{ "tournament_id": "..." }` or `{ "run_ids": ["..."] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Source {
    Tournament { tournament_id: String },
    Runs { run_ids: Vec<String> },
}

/// A claim plus the transcript lines that back it (`<run_id>#<line>`, 1-based).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    pub claim: String,
    #[serde(default)]
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BoundMethod {
    /// Wilson score interval, 95%.
    Wilson,
    /// Rule of three: zero events in n trials puts the 95% bound at about 3/n.
    RuleOfThree,
}

/// A 95% interval on a rate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Bound {
    pub lo: f64,
    pub hi: f64,
    pub method: BoundMethod,
}

/// A rate is never reported without its n and its bound. Rates with n = 0 are omitted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rate {
    pub rate: f64,
    pub n: u32,
    pub bound: Bound,
}

/// Failed hard-rule checks of one rule within one tone cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuleFailure {
    pub rule: String,
    pub fails: u32,
    /// Checks that were evaluated (pass + fail); not_provoked / not_evaluable excluded.
    pub n: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RankEngagement {
    /// Lowest rank first; ranks with no messages are omitted.
    #[serde(default)]
    pub by_rank: Vec<RankRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncidentSource {
    /// A hard-rule check the judge marked `fail`.
    Verdict,
    /// A `moderation` event in the transcript.
    Moderation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentMetrics {
    pub segment: SegmentId,
    pub runs: u32,
    pub minutes: f64,
    pub chat_msgs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_per_min: Option<f64>,
    /// Lines the host said (`say` with `speaker: host`). Reads the stored v1 name
    /// `malakor_lines` too.
    #[serde(alias = "malakor_lines")]
    pub host_lines: u32,
    #[serde(
        default,
        alias = "malakor_lines_per_min",
        skip_serializing_if = "Option::is_none"
    )]
    pub host_lines_per_min: Option<f64>,
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

/// An LLM-proposed item that did not survive mechanical verification (the "flag" half of
/// "drop or flag").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DroppedItem {
    /// `evidence_ref`, `evidence`, `option` or `segment`.
    pub what: String,
    /// The claim, option title, or segment name.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    /// Wire addition (WPI): every incident counted by `(rule, source)`, most first. Present
    /// on `reconcile.report` replies; the stored report never carries it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub red_line_counts: Vec<IncidentCount>,
    /// Wire addition (WPI): the report's incident total when `red_lines` was cut to its first
    /// page on the wire; page the rest with `reconcile.incidents`. Absent when nothing was cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red_lines_total: Option<u32>,
    /// Wire addition (WPI): what the synthesis prompt carried and what it cut to fit its
    /// budget. Absent for reports stored before WP12.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synthesis_input: Option<SynthesisInput>,
    /// Addition (WP15): what happened to each of the Lord's directives in the source runs
    /// (held or attempted, with n and bound). Absent when no run carried a brief.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directives: Vec<DirectiveOutcome>,
}

/// Incidents of one rule from one source, counted over the whole report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IncidentCount {
    pub rule: String,
    pub source: IncidentSource,
    pub count: u32,
    /// Distinct runs with at least one such incident.
    pub runs: u32,
}

/// Items that made it into the synthesis prompt, and items cut to fit its budget.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Counted {
    pub included: u32,
    pub omitted: u32,
}

/// What the synthesis prompt carried (WP12), stored with the report as `synthesis_input`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SynthesisInput {
    /// The budget the prompt was sized against, in estimated tokens.
    pub budget_tokens: u32,
    /// The prompt's estimated size (characters / 3.5), system prefix and schema included.
    pub estimated_tokens: u32,
    /// Runs with at least one excerpt line.
    pub runs: Counted,
    pub excerpt_lines: u32,
    /// Transcript lines the judge cited for rule failures.
    pub failure_refs: Counted,
    /// Turns the judge marked as failed quality facts.
    pub quality_refs: Counted,
    /// Incidents listed individually (all are counted by rule).
    pub incidents: Counted,
    pub tone_cells: Counted,
    /// Rule-failure rows (per segment and per tone cell) listed in the top-N tables.
    pub rule_failures: Counted,
    /// What the prompt told the Reconciler was omitted; empty when nothing was cut.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OptionKind {
    CharacterChange,
    SegmentChange,
    Topic,
    Experiment,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanSegment {
    pub segment: SegmentId,
    pub minutes: u32,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DirectiveKind {
    Prevent,
    Try,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Directive {
    pub id: String,
    pub kind: DirectiveKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<SegmentId>,
    pub created_ms: TsMs,
    pub active: bool,
    /// Addition (WP14): proposed by the Reconciler in a `lord.chat` reply and not yet decided
    /// by the owner. A proposed directive is inactive (`active: false`), so no stream reads
    /// it. `directive.add` with the same kind, text and scope keeps it (it becomes active and
    /// loses `proposed`); `directive.deactivate` drops it. Omitted when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub proposed: bool,
}

/// What one stream receives from the Lord of the Darkness (WP15): the latest chosen plan's
/// dispatch, segments and topics, plus the directives that are active when the stream starts
/// (never proposed or inactive ones). The brain renders it once, in its stable prefix; the sim
/// hashes its canonical JSON into the instrument. Built by `shadowrunr_brain::build_stream_brief`
/// (the one builder, used by the live path and the sim). A brief with nothing in it is no brief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StreamBrief {
    /// The Lord's message for the stream, in character. LLM-composed from report text, so the
    /// brain frames it as a message, never as a rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lord_dispatch: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<PlanSegment>,
    #[serde(default)]
    pub topics: Vec<String>,
    /// Active directives only, oldest first.
    #[serde(default)]
    pub directives: Vec<Directive>,
}

/// How a directive fared over the judged runs that carried it (WP15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DirectiveVerdict {
    /// A prevent directive no evaluated run broke.
    Held,
    /// A prevent directive at least one evaluated run broke.
    Broken,
    /// A try directive at least one run attempted.
    Attempted,
    /// A try directive no run attempted.
    Ignored,
    /// The judge never evaluated it (n = 0).
    Unmeasured,
}

/// Per-directive outcome aggregate (WP15): a prevent directive is the judge's hard rule
/// `directive:prevent:<id>`, a try directive its run fact `directive:try:<id>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DirectiveOutcome {
    pub id: String,
    pub kind: DirectiveKind,
    /// The judge check the outcome is counted from.
    pub check: String,
    /// Judged runs whose verdict carried the check, in any state.
    pub runs: u32,
    /// Prevent: share of evaluated runs (pass + fail) where it held. Try: share of runs where
    /// it was attempted. Omitted when n is 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<Rate>,
    pub verdict: DirectiveVerdict,
    /// Prevent: refs of the failing evidence. Try: refs of the attempts. At most 20.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

/// `lord.chat` result. `directives_added` are the directives the reply proposed: each is
/// `proposed: true, active: false` until the owner keeps it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LordChatReply {
    pub reply: String,
    pub directives_added: Vec<Directive>,
}
