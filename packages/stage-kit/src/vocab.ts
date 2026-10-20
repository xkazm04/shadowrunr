// The wire vocabulary as runtime lists. The types come from `@shadowrunr/contract` (generated
// from crates/shadowrunr-types); these constant lists follow them, and the `Exhaustive` checks
// below fail the typecheck when the contract gains or loses a member.

import type {
  EndReason,
  EvidenceGrade,
  HexKind,
  ModelTier,
  Op,
  Platform,
  Rank,
  SegmentId,
  StreamEvent,
} from "@shadowrunr/contract";

export type TsMs = number;
export type OpName = Op["op"];
export type EventType = StreamEvent["type"];

/** True when `L` lists every member of `U` (and nothing else). */
type Exhaustive<U, L extends readonly U[]> = [U] extends [L[number]] ? true : false;
const exhaustive = <U>() => <L extends readonly U[]>(list: L & (Exhaustive<U, L> extends true ? unknown : never)): L => list;

export const SEGMENT_IDS = exhaustive<SegmentId>()(["SEG-01", "SEG-02A", "SEG-02B-I", "SEG-02B-II"] as const);
/** Lowest first. */
export const RANKS = exhaustive<Rank>()(["thrall", "acolyte", "cultist", "warlock", "lich", "dread_council"] as const);
export const EVIDENCE_GRADES = exhaustive<EvidenceGrade>()(["DOC", "REP", "CLM", "SPC"] as const);
export const MODEL_TIERS = exhaustive<ModelTier>()(["haiku", "sonnet", "opus"] as const);
export const PLATFORMS = exhaustive<Platform>()(["twitch", "youtube", "kick", "sim"] as const);
export const HEX_KINDS = exhaustive<HexKind>()(["entity", "wallet", "domain", "shell_company", "claim"] as const);
export const END_REASONS = exhaustive<EndReason>()(["scheduled", "fleet_lost", "operator", "sim_complete"] as const);

export const OP_NAMES = exhaustive<OpName>()([
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
] as const);

export const EVENT_TYPES = exhaustive<EventType>()([
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
  "health",
  "stream_end",
] as const);
