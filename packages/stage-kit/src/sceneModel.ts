// The scene model: what the stage shows at a moment, as plain data. `reduce` (scene.ts) builds it
// from StreamEvents; nothing here reads a clock.

import type { Case, EndReason, EvidenceGrade, ModelTier, Rank, ScreenId, SegmentId, ShownRecord, VoteTally } from "@shadowrunr/contract";
import { ChatMemory } from "./chatMemory";
import type { TsMs } from "./vocab";

/** What the canvas draws: `boot` before the first segment, then the main screen (contract
 * `ScreenId`): `host` (the big-avatar room), `hexmap`, `browser` (a staged record in a browser
 * window) or `desk` (the browser window in front of the hexmap window). */
export type LayoutId = "boot" | ScreenId;

/** The screens with content behind Malakor: he stands cut out in the bottom-left corner. */
export const CONTENT_SCREENS = ["hexmap", "browser", "desk"] as const satisfies readonly ScreenId[];
export type ContentScreen = (typeof CONTENT_SCREENS)[number];

export function isContentScreen(layout: LayoutId): layout is ContentScreen {
  return (CONTENT_SCREENS as readonly string[]).includes(layout);
}

/** The hex the map window zooms to now: the `focus` on `hexmap`; on `desk` the `focus`, else the
 * open record's hex (the desk's map shows where the record points). Null: the overview. */
export function mapFocusOf(s: Pick<SceneState, "layout" | "mapFocus" | "activeRecord" | "records">): string | null {
  if (s.layout === "hexmap") return s.mapFocus;
  if (s.layout !== "desk") return null;
  if (s.mapFocus !== null) return s.mapFocus;
  return s.activeRecord === null ? null : (s.records[s.activeRecord]?.hex ?? null);
}

/** The screen a segment opens on. The runtime sends its own `show_screen` too; this default keeps
 * transcripts without one drawable. */
export function defaultScreen(segment: SegmentId): ScreenId {
  return segment === "SEG-02A" ? "hexmap" : "host";
}

export interface Transition {
  id: number;
  from: LayoutId;
  to: LayoutId;
  at: TsMs;
  /** What triggered it: a handover (with Malakor's reason), a plain segment start, or a
   * `show_screen` (or the record it waited for) inside a segment. */
  cause: "handover" | "segment_start" | "screen";
  /** The segment it opens (handover and segment_start). */
  segment?: SegmentId;
  reason?: string;
}

/** A `show_screen browser|desk` whose record has not arrived: the screen waits on hexmap. */
export interface PendingScreen {
  screen: "browser" | "desk";
  /** The record it names; absent: the next record that arrives. */
  recordId?: string;
  at: TsMs;
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
  /** What the canvas draws (see LayoutId). */
  layout: LayoutId;
  /** The main screen the segment is on (`host` before the first segment, drawn as `boot`). */
  screen: ScreenId;
  /** A browser or desk screen waiting for its record (contract: the stage tolerates either order). */
  pendingScreen: PendingScreen | null;
  /** Staged records received (`record_shown`), by id; at most RECORD_MAX, the open tabs kept. */
  records: Record<string, ShownRecord>;
  /** The browser window's tabs: record ids, most recent first (at most BROWSER_TABS_MAX). */
  tabs: string[];
  /** The record the browser window shows (the first tab once one is open). */
  activeRecord: string | null;
  /** The hex the map is zoomed to, from `show_screen`'s `focus` on `hexmap` or `desk` (null: the
   * overview). `mapFocusOf` resolves what the map draws. */
  mapFocus: string | null;
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
  /** Most recent chat message ids, newest last: every line received, judged or not. The screen
   * draws only `shownLane(scene)`, the lines the runtime's input filter has already judged. */
  lane: string[];
  /** The input filter's watermark: the latest closed beat (`to`), the one before it (`prev`), and
   * the beat whose verdicts are still arriving (`open`), from the `picked` and input `moderation`
   * events. A beat closes when any later event arrives, so a screen that renders between two of
   * its verdicts never shows a line its next verdict drops. A line at or before `to` is judged;
   * `closedAt` is when `to` closed, the moment its lines came on screen. */
  judged: { to: TsMs | null; prev: TsMs | null; open: TsMs | null; closedAt: TsMs | null };
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
/** Tabs the browser window keeps open. */
export const BROWSER_TABS_MAX = 4;
/** Staged records the scene remembers. */
export const RECORD_MAX = 32;
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
    screen: "host",
    pendingScreen: null,
    records: {},
    tabs: [],
    activeRecord: null,
    mapFocus: null,
    transition: null,
    pendingSwitch: null,
    caption: null,
    captions: [],
    foil: null,
    foilLine: null,
    chats: ChatMemory.EMPTY,
    lane: [],
    judged: { to: null, prev: null, open: null, closedAt: null },
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
