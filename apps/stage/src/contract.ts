// TypeScript mirror of the wire contract (`crates/shadowrunr-types`, docs/spec/contract.md).
// The Rust crate is the source of truth; this file only follows it. Optional fields are
// omitted on the wire, never `null`.

export type TsMs = number;

export const SEGMENT_IDS = ["SEG-01", "SEG-02A", "SEG-02B-I", "SEG-02B-II"] as const;
export type SegmentId = (typeof SEGMENT_IDS)[number];

/** Lowest first. */
export const RANKS = ["thrall", "acolyte", "cultist", "warlock", "lich", "dread_council"] as const;
export type Rank = (typeof RANKS)[number];

export const EVIDENCE_GRADES = ["DOC", "REP", "CLM", "SPC"] as const;
export type EvidenceGrade = (typeof EVIDENCE_GRADES)[number];

export const MODEL_TIERS = ["haiku", "sonnet", "opus"] as const;
export type ModelTier = (typeof MODEL_TIERS)[number];

export const PLATFORMS = ["twitch", "youtube", "kick", "sim"] as const;
export type Platform = (typeof PLATFORMS)[number];

export const HEX_KINDS = ["entity", "wallet", "domain", "shell_company", "claim"] as const;
export type HexKind = (typeof HEX_KINDS)[number];

export const END_REASONS = ["scheduled", "fleet_lost", "operator", "sim_complete"] as const;
export type EndReason = (typeof END_REASONS)[number];

export interface Viewer {
  id: string;
  name: string;
  platform: Platform;
  rank: Rank;
  sub_tier?: number;
  months: number;
  gifted: number;
  is_mod: boolean;
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

export type OpName = Op["op"];

export const OP_NAMES: readonly OpName[] = [
  "highlight",
  "start_poll",
  "close_poll",
  "summon_foil",
  "dismiss_foil",
  "reveal_hex",
  "dispatch_research",
  "show_card",
  "mark_moment",
  "switch_segment",
  "sign_off",
];

export interface VoteTally {
  option: string;
  voters: number;
  /** Sum of rank-weighted votes. */
  weight: number;
}

export type ModStage = "input" | "output";
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
  /** SEG-02A: the Case being audited, emitted once right after its `segment_start`. */
  | { type: "case_open"; ts_ms: TsMs; case: Case }
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

export type EventType = StreamEvent["type"];

export const EVENT_TYPES: readonly EventType[] = [
  "stream_start",
  "segment_start",
  "segment_end",
  "case_open",
  "chat",
  "picked",
  "say",
  "op",
  "vote",
  "moderation",
  "handover",
  "job",
  "stream_end",
];

export interface Hex {
  id: string;
  q: number;
  r: number;
  kind: HexKind;
  label: string;
  grade?: EvidenceGrade;
  fog: boolean;
  sources?: string[];
}

export interface Case {
  id: string;
  title: string;
  /** True for fabricated demo or simulation cases. Must never be presented as real. */
  fictional: boolean;
  hexes: Hex[];
  links?: [string, string][];
}
