// Wire types for the Shadowrunr daemon, written by hand.
//
// Source of truth: docs/spec/contract.md (§ The transcript, § Daemon protocol) and
// crates/shadowrunr-types/src/*. Field and tag names mirror the serde names exactly.
// Optional fields are OMITTED on the wire, never null, so they are typed `?:` here.
//
// Three groups are NOT yet pinned by the contract and are mirrored from the code that will
// produce them; each is marked "unpinned" and proposed for the contract in the WP7 report:
//   - TournamentResults: crates/shadowrunr-sim/src/summary.rs `Results` (results.json).
//   - RankEngagement, RuleIncident, LatencyStats, ToneFinding.rule_failures, the quality
//     `bound`: named in the contract without fields (WP6 owns them).
//   - `directive.deactivate`: a method the console needs and the contract does not list.

// ---------------------------------------------------------------------------------------
// shadowrunr-types: engine.rs, chat.rs, segment.rs

export type TsMs = number;

export type ModelTier = "haiku" | "sonnet" | "opus";
export type Effort = "low" | "medium" | "high" | "xhigh" | "max";
export type Role =
  | "core_thread"
  | "chat_picker"
  | "planner"
  | "researcher"
  | "consolidator"
  | "chatter"
  | "judge"
  | "reconciler";

export type HealthState = "green" | "amber" | "red";

export interface HealthReport {
  /** Pre-flight check id, e.g. "PF-01". */
  check: string;
  state: HealthState;
  detail: string;
}

export type Platform = "twitch" | "youtube" | "kick" | "sim";

/** Lowest first. */
export const RANKS = ["thrall", "acolyte", "cultist", "warlock", "lich", "dread_council"] as const;
export type Rank = (typeof RANKS)[number];

export interface Viewer {
  id: string;
  name: string;
  platform: Platform;
  rank: Rank;
  sub_tier?: number;
  months?: number;
  gifted?: number;
  is_mod?: boolean;
}

export type EvidenceGrade = "DOC" | "REP" | "CLM" | "SPC";

/** SEG-02A case (`shadowrunr_types::Case`), carried by the `case_open` event. */
export interface CaseHex {
  id: string;
  q: number;
  r: number;
  kind: "entity" | "wallet" | "domain" | "shell_company" | "claim";
  label: string;
  grade?: EvidenceGrade;
  fog: boolean;
  sources?: string[];
}
export interface CaseFile {
  id: string;
  title: string;
  fictional: boolean;
  hexes: CaseHex[];
  links?: [string, string][];
}

export type Command =
  | { cmd: "summon"; foil: string }
  | { cmd: "hint"; hex: string; text: string }
  | { cmd: "grade"; hex: string; grade: EvidenceGrade }
  | { cmd: "propose"; text: string }
  | { cmd: "vote"; option: string };

export interface ChatMsg {
  id: string;
  ts_ms: TsMs;
  viewer: Viewer;
  text: string;
  command?: Command;
}

export type SegmentId = "SEG-01" | "SEG-02A" | "SEG-02B-I" | "SEG-02B-II";
/** The segments in scope (SEG-02B is descoped). */
export const ACTIVE_SEGMENTS: SegmentId[] = ["SEG-01", "SEG-02A"];

// ---------------------------------------------------------------------------------------
// shadowrunr-types: event.rs (transcript.jsonl, one StreamEvent per line, 1-based lines)

export type Op =
  | { op: "highlight"; msg_id: string }
  | {
      op: "start_poll";
      poll_id: string;
      question: string;
      options: string[];
      min_rank: Rank;
      secs: number;
    }
  | { op: "close_poll"; poll_id: string }
  | { op: "summon_foil"; foil: string }
  | { op: "dismiss_foil" }
  | { op: "reveal_hex"; hex: string; grade: EvidenceGrade }
  | { op: "dispatch_research"; job_id: string; brief: string }
  | { op: "show_card"; card_id: string; title: string; body: string }
  | { op: "mark_moment"; label: string }
  | { op: "switch_segment"; to: SegmentId }
  | { op: "sign_off"; reason: string };

export interface VoteTally {
  option: string;
  voters: number;
  weight: number;
}

export type ModStage = "input" | "output";
export type EndReason = "scheduled" | "fleet_lost" | "operator" | "sim_complete";
export type Speaker = "malakor" | "foil";

export type StreamEvent =
  | {
      type: "stream_start";
      ts_ms: TsMs;
      stream_id: string;
      character: string;
      tone_variant: string;
      core_tier: ModelTier;
      audience_size: number;
    }
  | { type: "segment_start"; ts_ms: TsMs; segment: SegmentId }
  | { type: "segment_end"; ts_ms: TsMs; segment: SegmentId; ses?: number }
  | { type: "case_open"; ts_ms: TsMs; case: CaseFile }
  | { type: "chat"; ts_ms: TsMs; msg: ChatMsg }
  | { type: "picked"; ts_ms: TsMs; msg_ids: string[]; reason: string }
  | {
      type: "say";
      ts_ms: TsMs;
      speaker: Speaker;
      foil?: string;
      text: string;
      replying_to?: string[];
      first_text_ms?: number;
    }
  | { type: "op"; ts_ms: TsMs; op: Op }
  | { type: "vote"; ts_ms: TsMs; poll_id: string; tallies: VoteTally[]; closed: boolean }
  | {
      type: "moderation";
      ts_ms: TsMs;
      stage: ModStage;
      rule: string;
      msg_id?: string;
      action: string;
    }
  | { type: "handover"; ts_ms: TsMs; from: SegmentId; to: SegmentId; reason: string }
  | { type: "job"; ts_ms: TsMs; job_id: string; status: string; summary?: string }
  | { type: "stream_end"; ts_ms: TsMs; reason: EndReason };

export type StreamEventType = StreamEvent["type"];

// ---------------------------------------------------------------------------------------
// shadowrunr-types: sim.rs

export interface SimRunSpec {
  run_id: string;
  character: string;
  tone_variant: string;
  core_tier: ModelTier;
  core_effort: Effort;
  segment: SegmentId;
  audience_size: number;
  seed: number;
  duration_s: number;
  population: string;
}

export type CheckState = "pass" | "fail" | "not_provoked" | "not_evaluable";

export interface RuleCheck {
  rule: string;
  state: CheckState;
  /** Transcript refs `<run_id>#<line>`. */
  evidence?: string[];
}

export interface QualityFact {
  dimension: string;
  value: boolean;
  turn_ref: string;
  quote: string;
}

export interface JudgeVerdict {
  run_id: string;
  judge_tier: ModelTier;
  rules: RuleCheck[];
  quality: QualityFact[];
  notes?: string;
}

// ---------------------------------------------------------------------------------------
// results.json (unpinned: mirrors crates/shadowrunr-sim/src/summary.rs + stats.rs)

export type BoundMethod = "wilson" | "rule_of_three";

export interface Bound {
  lo: number;
  hi: number;
  method: BoundMethod;
}

export interface CellKey {
  instrument_hash: string;
  tone: string;
  core_tier: string;
  segment: string;
  audience_size: number;
}

export interface RuleTally {
  pass: number;
  fail: number;
  not_provoked: number;
  not_evaluable: number;
  /** Evaluated runs: pass + fail. */
  n: number;
  pass_rate?: number;
  /** 95% bound on the FAIL rate. */
  fail_bound?: Bound;
  fail_refs?: string[];
}

export interface QualityTally {
  true: number;
  n: number;
  rate?: number;
  bound?: Bound;
  not_evaluable: number;
}

export interface LatencySummary {
  n: number;
  p50_ms?: number;
  p95_ms?: number;
}

export interface UsageTotals {
  calls: number;
  errors: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  cost_usd: number;
}

export interface CellResult {
  cell: CellKey;
  runs: number;
  judged: number;
  void: number;
  failed: number;
  judge_tiers: string[];
  rules: Record<string, RuleTally>;
  quality: Record<string, QualityTally>;
  core_latency: LatencySummary;
  usage: UsageTotals;
  usage_by_role: Record<string, UsageTotals>;
  max_say_gap_s?: number;
}

export type CalibrationOrigin = { kind: "file"; path: string } | { kind: "default_assumption" };

export interface TournamentResults {
  tournament_id: string;
  instrument_hashes: string[];
  calibration: CalibrationOrigin[];
  assumptions?: string[];
  runs: number;
  judged: number;
  void: number;
  failed: number;
  cells: CellResult[];
  latency_by_tier: Record<string, Record<string, LatencySummary>>;
  usage_total: UsageTotals;
  usage_by_role: Record<string, UsageTotals>;
}

// ---------------------------------------------------------------------------------------
// Daemon protocol result rows

export interface RunRow {
  run_id: string;
  tournament_id: string;
  spec: SimRunSpec;
  has_verdict: boolean;
}

export interface TournamentRow {
  tournament_id: string;
  instrument_hash: string;
  runs: number;
  completed: number;
}

// ---------------------------------------------------------------------------------------
// Reconcile & Plan (shadowrunr-reconcile, contract § Daemon protocol)

export type ReconcileSource = { tournament_id: string } | { run_ids: string[] };

export interface Evidence {
  claim: string;
  /** `<run_id>#<line>` */
  refs: string[];
}

export interface SegmentFinding {
  segment: SegmentId;
  runs: number;
  ses_mean?: number;
  what_worked: Evidence[];
  what_failed: Evidence[];
}

/** `{rate, n, bound}`; bound shape unpinned (assumed `Bound`, as in results.json). */
export interface QualityRate {
  rate: number;
  n: number;
  bound: Bound;
}

export interface ToneFinding {
  tone_variant: string;
  quality_rates: Record<string, QualityRate>;
  /** Unpinned: assumed a count of failed rule checks. */
  rule_failures: number;
}

/** Unpinned (proposed). */
export interface RankEngagementRow {
  rank: Rank;
  chatters: number;
  messages: number;
  picked: number;
  answered: number;
}

/** Unpinned (proposed). */
export interface RankEngagement {
  by_rank: RankEngagementRow[];
}

/** Unpinned (proposed). */
export interface RuleIncident {
  rule: string;
  run_id: string;
  state: CheckState;
  refs: string[];
  note?: string;
}

/** Unpinned (proposed). */
export interface LatencyStats {
  core_first_text_ms: LatencySummary;
  by_role?: Record<string, LatencySummary>;
}

export interface ReconcileReport {
  report_id: string;
  created_ms: TsMs;
  source: ReconcileSource;
  segments: SegmentFinding[];
  tone: ToneFinding[];
  ranks: RankEngagement;
  red_lines: RuleIncident[];
  latency: LatencyStats;
  summary: string;
}

export type PlanOptionKind = "character_change" | "segment_change" | "topic" | "experiment";

export interface PlanOption {
  option_id: string;
  kind: PlanOptionKind;
  title: string;
  rationale: string;
  evidence: Evidence[];
  /** Unified diff; never touches constitution.md. */
  diff?: string;
}

export interface PlanSegment {
  segment: SegmentId;
  minutes: number;
  notes: string;
}

export type DirectiveKind = "prevent" | "try";

export interface Directive {
  id: string;
  kind: DirectiveKind;
  text: string;
  scope?: SegmentId;
  created_ms: TsMs;
  active: boolean;
}

export interface NextStreamPlan {
  plan_id: string;
  created_ms: TsMs;
  lord_dispatch: string;
  segments: PlanSegment[];
  topics: string[];
  directives: Directive[];
}

export interface LordChatReply {
  reply: string;
  directives_added: Directive[];
}

// ---------------------------------------------------------------------------------------
// The RPC method table: `POST /rpc {"method", "params"}` -> `{"result"}` | `{"error"}`

export interface RpcMethods {
  "preflight.run": { params: Record<string, never>; result: HealthReport[] };
  "runs.list": { params: { tournament_id?: string }; result: RunRow[] };
  "run.transcript": { params: { run_id: string }; result: StreamEvent[] };
  "run.verdict": { params: { run_id: string }; result: JudgeVerdict };
  "tournament.list": { params: Record<string, never>; result: TournamentRow[] };
  "tournament.results": { params: { tournament_id: string }; result: TournamentResults };
  "sim.start": { params: { spec: SimRunSpec; judge: boolean }; result: { run_id: string } };
  "reconcile.run": { params: { source: ReconcileSource }; result: { report_id: string } };
  "reconcile.report": { params: { report_id: string }; result: ReconcileReport };
  "plan.proposals": { params: { report_id: string }; result: PlanOption[] };
  "plan.choose": { params: { report_id: string; option_ids: string[] }; result: NextStreamPlan };
  "directive.add": {
    params: { kind: DirectiveKind; text: string; scope?: SegmentId };
    result: Directive;
  };
  "directive.list": { params: Record<string, never>; result: Directive[] };
  /** Unpinned: proposed in the WP7 report. */
  "directive.deactivate": { params: { id: string }; result: Directive };
  "lord.chat": { params: { report_id?: string; message: string }; result: LordChatReply };
}

export type RpcMethod = keyof RpcMethods;
export type RpcParams<M extends RpcMethod> = RpcMethods[M]["params"];
export type RpcResult<M extends RpcMethod> = RpcMethods[M]["result"];

export interface RpcErrorBody {
  code: string;
  message: string;
}

/** `GET /events` client -> server message. */
export interface SubscribeMessage {
  subscribe: string; // "<run_id>" | "live"
  speed: number;
}
