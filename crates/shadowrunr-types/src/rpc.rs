//! `POST /rpc` and `GET /events`: every method's params and result, the paging shapes and the
//! event envelope. The method table is [`visit_methods`]; the JSON Schema export
//! (`cargo run -p shadowrunr-types --example export-schema`) and the daemon's dispatch test
//! both read it, so a method cannot exist on one side only.
//!
//! Paging is cursor-based: a reply carries `next_cursor` while more items follow, and the
//! client passes it back unchanged. Cursors are opaque; `limit` is capped at [`MAX_LIMIT`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::catalog::{
    CharacterDetail, CharacterIssue, CharacterSummary, RankTableInfo, RuleState, SegmentTables,
};
use crate::clips::{
    ClipCandidate, ClipIdParams, ClipPublishReport, ClipRunRecording, ClipScanReport,
    ClipsApproveParams, ClipsFeedbackParams, ClipsListParams, ClipsPage, ClipsPublishParams,
    ClipsRecordingParams, ClipsRejectParams, ClipsScanParams,
};
use crate::prep::{PrepApproveParams, PrepGenerateParams, PrepIdParams, PrepSummary, StreamPrep};
use crate::reconcile::{
    Directive, DirectiveKind, LordChatReply, NextStreamPlan, PlanOption, ReconcileReport,
    RuleIncident, Source,
};
use crate::results::TournamentResults;
use crate::review::{
    FeedbackListParams, FeedbackSetParams, LineFeedback, ReviewSession, ReviewSessionParams,
    ReviewSessionRow, ReviewSessionsParams, ReviewTagParams, SessionTags,
};
use crate::sandbox::{
    SandboxIdParams, SandboxStartParams, SandboxStarted, SandboxStatus, SandboxStatusParams,
};
use crate::tone::{
    ToneDirections, ToneDirectionsGetParams, ToneDirectionsSetParams, ToneInbox, ToneInboxParams,
    ToneLineWrite, ToneLines, ToneLinesAddParams, ToneLinesParams, ToneLinesSetStatusParams,
    ToneReviewItem, ToneReviewLineParams, ToneVariants, ToneVariantsParams,
};
use crate::{
    Effort, EngineKind, HealthReport, JudgeVerdict, ModelTier, Role, SegmentId, SimRunSpec,
    StreamEvent, TsMs, Usage,
};

/// The largest page any paged method returns.
pub const MAX_LIMIT: u32 = 500;

/// `{"method", "params"}`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RpcRequest {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Params of a method that takes none: `{}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EmptyParams {}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunsListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
}

/// A run by `run_id`, `<tournament_id>/<run_id>`, or `run_id` plus `tournament_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunParams {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TournamentParams {
    pub tournament_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SimStartParams {
    pub spec: SimRunSpec,
    /// Judge the run when it ends. Default false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub judge: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SimStartResult {
    pub run_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReconcileRunParams {
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReconcileRunResult {
    pub report_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReportParams {
    pub report_id: String,
}

/// One stored Reconcile report, newest first, for `reconcile.list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReportSummary {
    pub report_id: String,
    pub created_ms: i64,
    pub source: Source,
    pub proposals: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanChooseParams {
    pub report_id: String,
    pub option_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DirectiveAddParams {
    pub kind: DirectiveKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<SegmentId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DirectiveIdParams {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterRulesParams {
    pub character: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterRulesSetParams {
    pub character: String,
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LordChatParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterGetParams {
    pub id: String,
}

/// `character.validate`: check a package (`<characters>/<id>/`) the way `character validate`
/// does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterValidateParams {
    pub id: String,
}

/// `character.tone.set`: the operator's tone for a character, written to
/// `<data>/characters/<id>/active.json`. The tone must be one of the package's tones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharacterToneSetParams {
    pub id: String,
    pub tone: String,
}

/// `reconcile.incidents`: one report's red-line incidents, paged in report order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReconcileIncidentsParams {
    pub report_id: String,
    /// Only incidents of this rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// 1..=500; larger values are capped at 500.
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IncidentsPage {
    pub items: Vec<RuleIncident>,
    /// Incidents matching the filter, over every page.
    pub total: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RanksTableParams {
    /// Whose rank titles to show; the configured default character when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
}

/// One row of `runs.list` and `runs.page`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunRow {
    pub run_id: String,
    pub tournament_id: String,
    pub spec: SimRunSpec,
    pub has_verdict: bool,
    /// Share of the verdict's rule checks that held: pass / (pass + fail). Absent without a
    /// verdict or when no rule was provoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<f64>,
    /// Failed `red-line:*` rule checks in the verdict; omitted when 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub red_line_fails: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// One row of `tournament.list` and `tournaments.page`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TournamentRow {
    pub tournament_id: String,
    /// The tournament's base instrument hash; `mixed` or `unknown` for an ad-hoc directory.
    pub instrument_hash: String,
    pub runs: usize,
    /// Runs that finished (simulated, or judged when judging was on).
    pub completed: usize,
    /// Runs whose verdict parses. Omitted by a daemon older than the field.
    #[serde(default)]
    pub judged: usize,
    /// The tournament's held share: the MEAN over judged runs of each run's
    /// pass / (pass + fail) ([`RunRow::health`]). A run is the observation, not a check, so
    /// this is never the pooled checks. A judged run with no evaluated check is left out of
    /// the denominator; absent when no judged run has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<f64>,
    /// Failed `red-line:*` checks summed over the judged runs; omitted when 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub red_line_fails: u32,
    /// The character package the tournament ran: `tournament.json`'s `character`, else (an
    /// ad-hoc directory) the one character every live run's spec names. Absent when the runs
    /// disagree or nothing says; a picker never defaults to such a row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
    /// The newest live run's `run.json` mtime: when the tournament last produced a result.
    /// Absent when no live run has written one. Tournament ids carry no date; this does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_ms: Option<TsMs>,
}

/// `runs.page` filter; every present field must match.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_tier: Option<ModelTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<SegmentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_verdict: Option<bool>,
    /// Case-insensitive substring of the run id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunsPageParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<RunFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// 1..=500; larger values are capped at 500.
    pub limit: u32,
}

/// Runs ordered by `(tournament_id, run_id)`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunsPage {
    pub items: Vec<RunRow>,
    /// Present while more items follow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TournamentsPageParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub limit: u32,
}

/// Tournaments ordered by `tournament_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TournamentsPage {
    pub items: Vec<TournamentRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// `ledger.page` source: one run's `ledger.jsonl` (`run_id`, optionally qualified by
/// `tournament_id`), every run of a tournament in run order (`tournament_id` alone), or the
/// daemon's process-wide engine ledger (neither).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LedgerPageParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    pub limit: u32,
}

/// One engine call. Never a prompt, a reply or a credential: accounts by label only.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LedgerEntry {
    pub ts_ms: TsMs,
    pub role: Role,
    /// Present for per-run (simulation) rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<ModelTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<EngineKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_text_ms: Option<u64>,
    pub duration_ms: u64,
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// The engine error kind when the call failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    /// The call was a turn of a warm session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub warm: bool,
    /// The run the row belongs to (per-run sources).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LedgerPage {
    pub items: Vec<LedgerEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// `ledger.summary` source: the same grammar as [`LedgerPageParams`] without the paging
/// (one run, a tournament's runs, or the daemon's engine ledger).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LedgerSummaryParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
}

/// One role's calls in a [`LedgerSummary`]. Durations leave failed calls out; percentiles are
/// nearest-rank; a role with no sample has no percentile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RouteSummary {
    pub role: Role,
    pub calls: u64,
    pub errors: u64,
    pub fallbacks: u64,
    pub tiers: Vec<ModelTier>,
    pub efforts: Vec<Effort>,
    pub engines: Vec<EngineKind>,
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_p50_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_p95_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_text_p50_ms: Option<u64>,
}

/// Calls one model served for an account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelCalls {
    pub model: String,
    pub calls: u64,
}

/// One engine account's calls in a [`LedgerSummary`] (the account by label only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccountSummary {
    pub engine: EngineKind,
    pub account: String,
    pub calls: u64,
    pub errors: u64,
    /// Most calls first.
    pub models: Vec<ModelCalls>,
    pub last_ts_ms: TsMs,
    /// The error kind of the account's latest call, when it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// `ledger.summary`: every complete row of the source folded once (no cursor, no sample).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LedgerSummary {
    pub calls: u64,
    pub errors: u64,
    pub fallbacks: u64,
    /// The sum over the rows that carry a cost; see `uncosted_calls`.
    pub cost_usd: f64,
    /// Rows without a cost: an unknown cost, never folded into `cost_usd` as zero.
    pub uncosted_calls: u64,
    pub cache_read_tokens: u64,
    /// Rows without an engine or an account.
    pub unrouted: u64,
    /// Most calls first.
    pub routes: Vec<RouteSummary>,
    /// Most calls first.
    pub accounts: Vec<AccountSummary>,
}

/// Client -> `/events`: choose the source. Each message replaces the current source.
///
/// Sending `since` switches the session to enveloped frames ([`EventFrame`]) and resumes
/// after that seq. With `"subscribe": "live"`, `since` applies only while the run in flight
/// is `run_id` (so `run_id` is required with it); a different run streams from its start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Subscribe {
    /// `<run_id>`, `<tournament_id>/<run_id>` or `live`.
    pub subscribe: String,
    /// Replay speed for stored runs; 1.0 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    /// Resume after this seq (0 = from the start, enveloped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    /// The run `since` refers to, for `live`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

/// `/events` -> client in enveloped mode: one transcript event with its position. `seq` is the
/// 1-based index of the event in its run's transcript, so live and stored runs agree and a
/// reconnecting client resumes with `since` = the last seq it saw.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EventFrame {
    pub seq: u64,
    pub run_id: String,
    pub event: StreamEvent,
}

/// `engine.status`: one row of the role routing table as the engine applies it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EngineRoute {
    pub role: Role,
    pub tier: ModelTier,
    pub effort: Effort,
    pub timeout_ms: u64,
    /// The config's `[engine.routes]` table has an entry for this role.
    pub overridden: bool,
}

/// `engine.status`: one Claude Code account, by label only (never a config dir or a
/// credential).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EngineAccount {
    pub label: String,
    /// Epoch ms until which the account is skipped after a rate limit; absent = healthy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooling_until_ms: Option<TsMs>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EngineFleetMode {
    /// The last call was served by a Claude Code CLI account.
    Primary,
    /// The last call was served by a fallback (API key or an alternative CLI).
    Degraded,
    /// The last call found nothing able to serve it.
    Down,
    /// The daemon runs the deterministic scripted engine; no fleet exists.
    Scripted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EngineFleet {
    pub mode: EngineFleetMode,
    /// When the fleet entered `mode` (epoch ms); 0 for `scripted`.
    pub since_ms: TsMs,
}

/// `engine.status` result: the configured routes, account cooldowns and fleet state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EngineStatus {
    /// Every role, in contract order.
    pub routes: Vec<EngineRoute>,
    /// In rotation order; empty for the scripted engine.
    pub accounts: Vec<EngineAccount>,
    pub fleet: EngineFleet,
    /// Recognised `cli_fallbacks` drivers in configured order (`codex`, `agy`, `local_gemma`).
    pub fallbacks: Vec<String>,
    /// An Anthropic API key env var is configured as the first fallback.
    pub api_fallback: bool,
}

/// What a method visitor is handed for each method.
pub trait MethodVisitor {
    fn method<P: JsonSchema, R: JsonSchema>(&mut self, name: &'static str);
}

/// The method table: `(name, params, result)` for every daemon method, in contract order.
pub fn visit_methods(v: &mut impl MethodVisitor) {
    v.method::<EmptyParams, Vec<HealthReport>>("preflight.run");
    v.method::<RunsListParams, Vec<RunRow>>("runs.list");
    v.method::<RunParams, Vec<StreamEvent>>("run.transcript");
    v.method::<RunParams, JudgeVerdict>("run.verdict");
    v.method::<EmptyParams, Vec<TournamentRow>>("tournament.list");
    v.method::<TournamentParams, TournamentResults>("tournament.results");
    v.method::<SimStartParams, SimStartResult>("sim.start");
    v.method::<ReconcileRunParams, ReconcileRunResult>("reconcile.run");
    v.method::<ReportParams, ReconcileReport>("reconcile.report");
    v.method::<EmptyParams, Vec<ReportSummary>>("reconcile.list");
    v.method::<ReconcileIncidentsParams, IncidentsPage>("reconcile.incidents");
    v.method::<EmptyParams, NextStreamPlan>("plan.current");
    v.method::<ReportParams, Vec<PlanOption>>("plan.proposals");
    v.method::<PlanChooseParams, NextStreamPlan>("plan.choose");
    v.method::<DirectiveAddParams, Directive>("directive.add");
    v.method::<EmptyParams, Vec<Directive>>("directive.list");
    v.method::<DirectiveIdParams, Directive>("directive.deactivate");
    v.method::<LordChatParams, LordChatReply>("lord.chat");
    v.method::<CharacterRulesParams, Vec<RuleState>>("character.rules");
    v.method::<CharacterRulesSetParams, Vec<RuleState>>("character.rules.set");
    v.method::<EmptyParams, Vec<CharacterSummary>>("character.list");
    v.method::<CharacterGetParams, CharacterDetail>("character.get");
    v.method::<CharacterToneSetParams, CharacterSummary>("character.tone.set");
    v.method::<CharacterValidateParams, Vec<CharacterIssue>>("character.validate");
    v.method::<EmptyParams, SegmentTables>("segments.tables");
    v.method::<RanksTableParams, RankTableInfo>("ranks.table");
    v.method::<LedgerPageParams, LedgerPage>("ledger.page");
    v.method::<LedgerSummaryParams, LedgerSummary>("ledger.summary");
    v.method::<RunsPageParams, RunsPage>("runs.page");
    v.method::<TournamentsPageParams, TournamentsPage>("tournaments.page");
    v.method::<EmptyParams, EngineStatus>("engine.status");
    v.method::<ClipsListParams, ClipsPage>("clips.list");
    v.method::<ClipIdParams, ClipCandidate>("clips.get");
    v.method::<ClipsScanParams, ClipScanReport>("clips.scan");
    v.method::<ClipIdParams, ClipCandidate>("clips.render");
    v.method::<ClipsApproveParams, ClipCandidate>("clips.approve");
    v.method::<ClipsRejectParams, ClipCandidate>("clips.reject");
    v.method::<ClipsPublishParams, ClipPublishReport>("clips.publish");
    v.method::<ClipsFeedbackParams, ClipCandidate>("clips.feedback");
    v.method::<ClipsRecordingParams, ClipRunRecording>("clips.recording");
    v.method::<SandboxStartParams, SandboxStarted>("sandbox.start");
    v.method::<SandboxIdParams, SandboxStatus>("sandbox.stop");
    v.method::<SandboxStatusParams, SandboxStatus>("sandbox.status");
    v.method::<ReviewSessionsParams, Vec<ReviewSessionRow>>("review.sessions");
    v.method::<ReviewSessionParams, ReviewSession>("review.session");
    v.method::<FeedbackSetParams, LineFeedback>("review.feedback.set");
    v.method::<FeedbackListParams, Vec<LineFeedback>>("review.feedback.list");
    v.method::<ReviewTagParams, SessionTags>("review.tag");
    v.method::<PrepGenerateParams, StreamPrep>("prep.generate");
    v.method::<EmptyParams, Vec<PrepSummary>>("prep.list");
    v.method::<PrepIdParams, StreamPrep>("prep.get");
    v.method::<PrepApproveParams, StreamPrep>("prep.approve");
    v.method::<PrepIdParams, StreamPrep>("prep.retire");
    v.method::<ToneVariantsParams, ToneVariants>("tone.variants");
    v.method::<ToneLinesParams, ToneLines>("tone.lines.list");
    v.method::<ToneLinesAddParams, ToneLineWrite>("tone.lines.add");
    v.method::<ToneLinesSetStatusParams, ToneLineWrite>("tone.lines.set_status");
    v.method::<ToneDirectionsGetParams, ToneDirections>("tone.directions.get");
    v.method::<ToneDirectionsSetParams, ToneDirections>("tone.directions.set");
    v.method::<ToneInboxParams, ToneInbox>("tone.inbox");
    v.method::<ToneReviewLineParams, ToneReviewItem>("tone.review_line");
}

/// Every method name, in table order.
pub fn method_names() -> Vec<&'static str> {
    struct Names(Vec<&'static str>);
    impl MethodVisitor for Names {
        fn method<P: JsonSchema, R: JsonSchema>(&mut self, name: &'static str) {
            self.0.push(name);
        }
    }
    let mut n = Names(Vec::new());
    visit_methods(&mut n);
    n.0
}

/// `character.rules.set` -> `CharacterRulesSet`: the prefix of the method's schema names
/// (`<Pascal>Params`, `<Pascal>Result`).
pub fn pascal_name(method: &str) -> String {
    method
        .split(['.', '_'])
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_names_are_unique_and_pascal_names_too() {
        let names = method_names();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
        let mut pascal: Vec<String> = names.iter().map(|n| pascal_name(n)).collect();
        pascal.sort();
        pascal.dedup();
        assert_eq!(pascal.len(), names.len());
        assert_eq!(pascal_name("character.rules.set"), "CharacterRulesSet");
    }

    #[test]
    fn optional_params_are_omitted() {
        let p = SimStartParams {
            spec: serde_json::from_value(serde_json::json!({
                "run_id": "r", "character": "malakor", "tone_variant": "theatrical",
                "core_tier": "haiku", "core_effort": "low", "segment": "SEG-01",
                "audience_size": 3, "seed": 1, "duration_s": 60, "population": "p"
            }))
            .unwrap(),
            judge: false,
        };
        assert!(serde_json::to_value(&p).unwrap().get("judge").is_none());
        let s = Subscribe {
            subscribe: "live".into(),
            speed: None,
            since: None,
            run_id: None,
        };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"subscribe":"live"}"#
        );
    }
}
