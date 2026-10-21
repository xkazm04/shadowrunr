// The scene model: what the stage shows at a moment, as plain data. `reduce` (scene.ts) builds it
// from StreamEvents; nothing here reads a clock.

import type { Case, EndReason, EvidenceGrade, ModelTier, Rank, SegmentId, VoteTally } from "@shadowrunr/contract";
import { ChatMemory } from "./chatMemory";
import type { TsMs } from "./vocab";

export type LayoutId = "boot" | "improv" | "hexmap" | "descoped";

export function layoutFor(segment: SegmentId): LayoutId {
  switch (segment) {
    case "SEG-01":
      return "improv";
    case "SEG-02A":
      return "hexmap";
    default:
      return "descoped";
  }
}

export interface Transition {
  id: number;
  from: LayoutId;
  to: LayoutId;
  at: TsMs;
  /** What triggered it: a handover (with Malakor's reason) or a plain segment start. */
  cause: "handover" | "segment_start";
  reason?: string;
}

export interface Caption {
  id: number;
  text: string;
  at: TsMs;
  /** Display names of the chatters this line answers. */
  replyingTo: string[];
}

/** A caption on the stage's caption queue: `at` is when it shows, `saidAt` when it was said. */
export interface ScheduledCaption extends Caption {
  saidAt: TsMs;
  /** When its last word has landed, from `at` (ms). */
  spokenMs: number;
}

export interface FoilLine {
  id: number;
  foil: string;
  text: string;
  at: TsMs;
}

export interface Decree {
  key: string;
  msgId: string;
  kind: "highlight" | "picked";
  at: TsMs;
  reason?: string;
}

export interface Poll {
  id: string;
  question: string;
  options: string[];
  minRank: Rank;
  secs: number;
  openedAt: TsMs;
  closedAt?: TsMs;
  /** Authoritative tallies from the latest `vote` event (empty until one arrives). */
  tallies: VoteTally[];
  /** Rank-weighted votes observed in chat (`!vote` / `!summon`), per option, per rank. */
  observed: Record<string, Partial<Record<Rank, number>>>;
  /** viewer id → option, so a changed vote moves rather than doubles. */
  ballots: Record<string, { option: string; rank: Rank }>;
}

export interface HexState {
  grade: EvidenceGrade;
  revealedAt: TsMs;
}

export interface Hint {
  count: number;
  lastAt: TsMs;
  lastText: string;
  lastBy: string;
}

export interface Job {
  id: string;
  brief?: string;
  status: string;
  summary?: string;
  at: TsMs;
  updatedAt: TsMs;
}

export interface Card {
  id: string;
  title: string;
  body: string;
  at: TsMs;
}

export interface Moment {
  label: string;
  at: TsMs;
}

export interface Redaction {
  key: string;
  at: TsMs;
  rule: string;
  action: string;
}

export interface Seal {
  segment: SegmentId;
  ses?: number;
  at: TsMs;
}

export interface StreamMeta {
  streamId: string;
  character: string;
  toneVariant: string;
  coreTier: ModelTier;
  audienceSize: number;
  at: TsMs;
}

export interface SceneState {
  t0: TsMs | null;
  lastTs: TsMs | null;
  /** Count of events applied (handy for tests and the debug overlay). */
  applied: number;
  meta: StreamMeta | null;
  segment: SegmentId | null;
  /** The SEG-02A case from the latest `case_open` (null for transcripts without one). */
  caseFile: Case | null;
  layout: LayoutId;
  transition: Transition | null;
  pendingSwitch: { to: SegmentId; at: TsMs } | null;
  caption: Caption | null;
  /** The caption queue: the caption showing at the latest event (first), then those waiting,
   * each scheduled to show once the one before has revealed and held. `captionAt` reads it. */
  captions: ScheduledCaption[];
  foil: { id: string; since: TsMs } | null;
  foilLine: FoilLine | null;
  /** Remembered chat lines by id, oldest first (at most CHAT_MEMORY_MAX + CHAT_EVICT_SLACK). */
  chats: ChatMemory;
  /** Most recent chat message ids, newest last. */
  lane: string[];
  /** When each remembered line was picked for the core thread. */
  heard: ReadonlyMap<string, TsMs>;
  purged: Record<string, string>;
  decrees: Decree[];
  polls: Record<string, Poll>;
  activePoll: string | null;
  hexes: Record<string, HexState>;
  revealOrder: string[];
  hints: Record<string, Hint>;
  /** Community `!grade` ballots per hex, viewer id → grade, so a repeated grade moves rather than adds. */
  gradeVotes: Record<string, Record<string, EvidenceGrade>>;
  /** Authoritative jury tallies per hex, from the runtime's `grade:<hex>` vote events. */
  jury: Record<string, VoteTally[]>;
  jobs: Job[];
  card: Card | null;
  moments: Moment[];
  redactions: Redaction[];
  seals: Seal[];
  outro: { reason: string; at: TsMs } | null;
  ended: { reason: EndReason; at: TsMs } | null;
  seq: number;
}

export const LANE_MAX = 40;
export const CHAT_MEMORY_MAX = 600;
/** The memory may exceed CHAT_MEMORY_MAX by at most this many lines (young lines plus what is on screen). */
export const CHAT_EVICT_SLACK = 200;
export const DECREE_MAX = 3;
export const JOB_MAX = 12;
export const MOMENT_MAX = 200;
export const REDACTION_MAX = 20;
/** Captions that may wait behind the one on screen; a burst beyond this skips the oldest waiting. */
export const CAPTION_QUEUE_MAX = 3;
/** How long a revealed caption holds when another is waiting behind it. */
export const QUEUED_HOLD_MS = 2000;

export function initialScene(): SceneState {
  return {
    t0: null,
    lastTs: null,
    applied: 0,
    meta: null,
    segment: null,
    caseFile: null,
    layout: "boot",
    transition: null,
    pendingSwitch: null,
    caption: null,
    captions: [],
    foil: null,
    foilLine: null,
    chats: ChatMemory.EMPTY,
    lane: [],
    heard: new Map(),
    purged: {},
    decrees: [],
    polls: {},
    activePoll: null,
    hexes: {},
    revealOrder: [],
    hints: {},
    gradeVotes: {},
    jury: {},
    jobs: [],
    card: null,
    moments: [],
    redactions: [],
    seals: [],
    outro: null,
    ended: null,
    seq: 0,
  };
}
