// The scene model: what the stage shows at a moment, as plain data. `reduce` (scene.ts) builds it
// from StreamEvents; nothing here reads a clock.

import type { Case, Command, EndReason, EvidenceGrade, LinkRel, ModelTier, Rank, ScreenId, SegmentId, ShownRecord, VoteTally } from "@shadowrunr/contract";
import { ChatMemory } from "./chatMemory";
import { ViewerBook } from "./viewerBook";
import type { TsMs } from "./vocab";

/** What the canvas draws: `boot` before the first segment, then the main screen (contract
 * `ScreenId`): `host` (the big-avatar room), `board` (the evidence board), `hexmap`, `browser` (a
 * staged record in a browser window) or `desk` (the browser window in front of the board, or of
 * the hexmap when that was the last map shown). */
export type LayoutId = "boot" | ScreenId;

/** The screens with content behind Malakor: he stands cut out in the bottom-left corner. */
export const CONTENT_SCREENS = ["board", "hexmap", "browser", "desk"] as const satisfies readonly ScreenId[];

/** The map a segment works on: the evidence board, or the hexmap of older transcripts. The desk
 * shows it behind the browser window, and a screen waiting for its record shows it meanwhile. */
export type MapScreen = "board" | "hexmap";
export type ContentScreen = (typeof CONTENT_SCREENS)[number];

export function isContentScreen(layout: LayoutId): layout is ContentScreen {
  return (CONTENT_SCREENS as readonly string[]).includes(layout);
}

/** The hex (or card) the map window zooms to now: the `focus` on `hexmap` and `board`; on `desk`
 * the `focus`, else the open record's hex (the desk's map shows where the record points). Null:
 * the overview. */
export function mapFocusOf(s: Pick<SceneState, "layout" | "mapFocus" | "activeRecord" | "records">): string | null {
  if (s.layout === "hexmap" || s.layout === "board") return s.mapFocus;
  if (s.layout !== "desk") return null;
  if (s.mapFocus !== null) return s.mapFocus;
  return s.activeRecord === null ? null : (s.records[s.activeRecord]?.hex ?? null);
}

/** The screen a segment opens on. The runtime sends its own `show_screen` too; this default keeps
 * transcripts without one drawable. */
export function defaultScreen(segment: SegmentId): ScreenId {
  return segment === "SEG-02A" ? "board" : "host";
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

/** A `show_screen browser|desk` whose record has not arrived: the screen waits on the map. */
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
  /** Chat ballots per option, kept beside `observed` so a bar reads its voters without a scan. */
  observedVoters: Record<string, number>;
  /** viewer id → option, so a changed vote moves rather than doubles (a persistent book: a
   * ballot costs amortized O(1), not a copy of every voter). */
  ballots: ViewerBook<{ option: string; rank: Rank }>;
}

/** One hex's community `!grade` ballots: viewer id → grade, and the running count per grade. */
export interface GradeBook {
  ballots: ViewerBook<EvidenceGrade>;
  counts: Record<EvidenceGrade, number>;
}

export const EMPTY_GRADE_BOOK: GradeBook = { ballots: ViewerBook.empty(), counts: { DOC: 0, REP: 0, CLM: 0, SPC: 0 } };

/** `book` with `viewer`'s grade set to `grade`; a repeated grade moves rather than adds. */
export function castGrade(book: GradeBook, viewer: string, grade: EvidenceGrade): GradeBook {
  const prev = book.ballots.get(viewer);
  const counts = { ...book.counts };
  if (prev !== undefined) counts[prev] -= 1;
  counts[grade] += 1;
  return { ballots: book.ballots.set(viewer, grade), counts };
}

/** A ballot command from chat, held until the poll it may count in opens. */
export interface PendingBallot {
  voter: Voter;
  cmd: Command;
}

/** Who cast a ballot: the rule reads only the viewer's id and rank. */
export interface Voter {
  id: string;
  rank: Rank;
}

/** At most this many pending ballots are held; the oldest go first. */
export const PENDING_BALLOT_MAX = 256;

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

/** A string on the evidence board (`link_pinned`): accepted by the runtime, citing its record. */
export interface PinnedLink {
  /** `from>to>rel`: the runtime refuses a duplicate, so this names one string. */
  key: string;
  from: string;
  to: string;
  rel: LinkRel;
  recordId: string;
  /** The record's grade: below DOC the string is drawn as alleged. */
  grade: EvidenceGrade;
  at: TsMs;
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
  /** The host's short name (`stream_start.host_name`); absent from older streams. */
  hostName?: string;
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
  /** The map the segment works on (the last of `board` and `hexmap` shown; a segment opens on its
   * default). The desk shows it behind the browser window. */
  mapScreen: MapScreen;
  /** A browser or desk screen waiting for its record (contract: the stage tolerates either order). */
  pendingScreen: PendingScreen | null;
  /** Staged records received (`record_shown`), by id; at most RECORD_MAX, the open tabs kept. */
  records: Record<string, ShownRecord>;
  /** The browser window's tabs: record ids, most recent first (at most BROWSER_TABS_MAX). */
  tabs: string[];
  /** The record the browser window shows (the first tab once one is open). */
  activeRecord: string | null;
  /** The hex the map is zoomed to, from `show_screen`'s `focus` on `board`, `hexmap` or `desk`
   * (null: the overview). `mapFocusOf` resolves what the map draws. */
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
  /** `!vote`/`!summon` lines seen while no poll was open, since the runtime last answered a chat
   * batch. A transcript logs a batch's chat before what the runtime emitted for it, so the
   * `!summon` that opens a foil vote arrives before its `start_poll`; that op casts these
   * (ballots.ts `openFoilVote`). Any other event but an input verdict clears them. */
  pendingBallots: readonly PendingBallot[];
  hexes: Record<string, HexState>;
  revealOrder: string[];
  /** The evidence board's strings, in the order pinned (at most LINK_MAX). */
  links: PinnedLink[];
  hints: Record<string, Hint>;
  /** Community `!grade` ballots per hex (viewer id → grade), so a repeated grade moves rather than adds. */
  gradeVotes: Record<string, GradeBook>;
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
/** Strings the board keeps; past this the oldest leaves. */
export const LINK_MAX = 64;
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
    mapScreen: "board",
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
    pendingBallots: [],
    hexes: {},
    revealOrder: [],
    links: [],
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
