// Pure model of the Segments & ranks page: lifecycle grouping, the rank pyramid, the command
// and op grids and the vote calculator. Everything here is derived from `segments.tables`,
// `ranks.table` and `reconcile.report`; nothing is invented.
import type { Rank, RankInfo, RankRow, RankTableInfo, SegmentStatus, SegmentTableInfo, SegmentTables } from "@shadowrunr/contract";
import type { Tone } from "../../ui/Chip";
import { LIFECYCLE, type Lifecycle } from "./counts";

export { glance, LIFECYCLE, statusCounts, type Lifecycle } from "./counts";

/** Ticks shown per lifecycle column before a "+N more" tick takes over (DOM stays flat at 10x). */
export const TICKS_PER_COLUMN = 3;
/** Descoped hexes drawn on the track's siding before a "+N" hex takes over. */
export const DESCOPED_SHOWN = 6;

export interface Column {
  readonly status: Lifecycle;
  readonly total: number;
  readonly shown: readonly SegmentTableInfo[];
  readonly more: number;
}

/**
 * Groups segments into the five lifecycle columns. Descoped segments (SEG-02B) are not retired:
 * they never ran, so they leave the columns and ride the track's siding instead.
 */
export function lifecycleColumns(segments: readonly SegmentTableInfo[], perColumn = TICKS_PER_COLUMN): Column[] {
  return LIFECYCLE.map((status) => {
    const here = segments.filter((s) => s.status === status);
    return { status, total: here.length, shown: here.slice(0, perColumn), more: Math.max(0, here.length - perColumn) };
  });
}

/** The chip tone of each lifecycle state (descoped reads as a caution, never as retired). */
export const STATUS_TONE: Readonly<Record<SegmentStatus, Tone>> = {
  pilot: "ok",
  trial: "neutral",
  regular: "neutral",
  flagship: "signal",
  retired: "muted",
  descoped: "caution",
};

export const descoped = (segments: readonly SegmentTableInfo[]) => segments.filter((s) => s.status === "descoped");

/** The rank order of the wire contract, lowest first. */
export const RANK_ORDER: readonly Rank[] = ["thrall", "acolyte", "cultist", "warlock", "lich", "dread_council"];
export const rankIndex = (r: Rank) => RANK_ORDER.indexOf(r);

/** CSS-module key of each rank's ink (`--sr-rank-*`). */
export const RANK_KEY: Readonly<Record<Rank, string>> = {
  thrall: "rkThrall",
  acolyte: "rkAcolyte",
  cultist: "rkCultist",
  warlock: "rkWarlock",
  lich: "rkLich",
  dread_council: "rkDreadCouncil",
};

export interface Step {
  readonly rank: Rank;
  readonly title: string;
  readonly weight: number;
  /** The winner's step width in percent: 28% at the apex, +14% per step down. */
  readonly widthPct: number;
  /** This rank may `!propose`. */
  readonly proposes: boolean;
  /** The lowest proposing rank: the ziggurat's ledge sits under this step. */
  readonly ledge: boolean;
}

/** The rank pyramid, apex first (l1.js `SR.ranks.slice().reverse()`, width `28 + i * 14`%). */
export function pyramid(table: RankTableInfo): Step[] {
  const gate = rankIndex(table.propose_min_rank);
  return table.ranks
    .slice()
    .reverse()
    .map((r, i) => ({
      rank: r.rank,
      title: r.title,
      weight: r.weight,
      widthPct: 28 + i * 14,
      proposes: rankIndex(r.rank) >= gate,
      ledge: r.rank === table.propose_min_rank,
    }));
}

export const fmtWeight = (w: number) => `×${w.toFixed(1)}`;

/** How a rank qualifies, in words (`tier 1, 3+ months · 5+ gifted`), or the base rank. */
export function qualifies(r: RankInfo): { tiers: string; gifted: string } {
  const tiers = (r.tiers ?? []).map((t) => `tier ${t.tier}${t.min_months ? `, ${t.min_months}+ months` : ""}`).join(" · ");
  return { tiers, gifted: r.min_gifted === undefined ? "" : `${r.min_gifted}+ gifted` };
}

export interface CommandRow {
  readonly name: string;
  /** The argument shape, from the daemon's command table; undefined when it sends none. */
  readonly args: string | undefined;
  /** What the command does, in one line. */
  readonly what: string | undefined;
  /** Segments whose table accepts the command. */
  readonly segments: readonly string[];
  /** The lowest rank allowed, or undefined for "any rank". */
  readonly minRank: Rank | undefined;
}

/**
 * One row per command the runtime knows, with where it is accepted and who may use it. The
 * arguments, meaning and rank gate come from `segments.tables` `commands` (the parser's own
 * table in shadowrunr-segments `command.rs`).
 */
export function commandRows(tables: SegmentTables): CommandRow[] {
  const info = new Map(tables.commands.map((c) => [c.name, c]));
  return tables.command_names.map((name) => {
    const c = info.get(name);
    return {
      name,
      args: c?.args,
      what: c?.what,
      segments: tables.segments.filter((s) => s.commands.includes(name)).map((s) => s.id),
      minRank: c?.min_rank,
    };
  });
}

/** The op grid's columns: segments that can run (descoped tables allow nothing). */
export const runnable = (segments: readonly SegmentTableInfo[]) => segments.filter((s) => s.status !== "descoped");

export interface OpRow {
  readonly op: string;
  readonly allowed: readonly boolean[];
  readonly count: number;
}

/** Every op tag of the action grammar against every runnable segment. */
export function opGrid(tables: SegmentTables): { columns: readonly SegmentTableInfo[]; rows: OpRow[] } {
  const columns = runnable(tables.segments);
  const rows = tables.op_tags.map((op) => {
    const allowed = columns.map((s) => s.allowed_ops.includes(op));
    return { op, allowed, count: allowed.filter(Boolean).length };
  });
  return { columns, rows };
}

export type Votes = Readonly<Record<Rank, { readonly a: number; readonly b: number }>>;

/** The winner's starting votes: six thralls back A; two thralls, two warlocks and a councillor back B. */
export const START_VOTES: Votes = {
  thrall: { a: 6, b: 2 },
  acolyte: { a: 0, b: 0 },
  cultist: { a: 0, b: 0 },
  warlock: { a: 0, b: 2 },
  lich: { a: 0, b: 0 },
  dread_council: { a: 0, b: 1 },
};

export interface Tally {
  readonly heads: { readonly a: number; readonly b: number };
  readonly weight: { readonly a: number; readonly b: number };
  readonly max: number;
  readonly verdict: string;
}

const clampVotes = (n: number) => (Number.isFinite(n) ? Math.min(999, Math.max(0, Math.floor(n))) : 0);
export const parseVotes = (s: string) => clampVotes(Number(s));

/** Head-count against weighted count (l2b.js `calc`): where the two disagree is the point. */
export function tally(ranks: readonly RankInfo[], votes: Votes): Tally {
  const heads = { a: 0, b: 0 };
  const weight = { a: 0, b: 0 };
  for (const r of ranks) {
    const v = votes[r.rank];
    heads.a += v.a;
    heads.b += v.b;
    weight.a += v.a * r.weight;
    weight.b += v.b * r.weight;
  }
  const headWord = heads.a === heads.b ? "The head-count ties." : `Head-count: ${heads.a > heads.b ? "A" : "B"} wins.`;
  const weightWord = Math.abs(weight.a - weight.b) < 0.05 ? "The weight ties." : `Weight: ${weight.a > weight.b ? "A" : "B"} wins.`;
  return { heads, weight, max: Math.max(heads.a, heads.b, weight.a, weight.b, 1), verdict: `${headWord} ${weightWord}` };
}

/** Engagement rows keyed by rank (the report omits ranks with no messages). */
export function engagementByRank(rows: readonly RankRow[] | undefined): ReadonlyMap<Rank, RankRow> {
  return new Map((rows ?? []).map((r) => [r.rank, r]));
}

export function fmtSecs(s: number): string {
  const m = Math.floor(s / 60);
  return `${m}:${String(s % 60).padStart(2, "0")}`;
}

export interface HandoverMarks {
  /** Percent positions along the ruler (0..100). */
  readonly sustainPct: number;
  readonly maxPct: number;
  readonly endS: number;
}

/** The handover ruler runs to `max_elapsed_s + grace_s`; sustain and max-elapsed are marks on it. */
export function handoverMarks(h: { sustain_s: number; max_elapsed_s: number; grace_s: number }): HandoverMarks {
  const endS = Math.max(1, h.max_elapsed_s + h.grace_s);
  return { sustainPct: (h.sustain_s / endS) * 100, maxPct: (h.max_elapsed_s / endS) * 100, endS };
}

export const humanState = (s: string) => s.replace(/_/g, " ");

/**
 * The state a `[clock.cold_open]` rule clocks (shadowrunr-segments `table::COLD_OPEN_STATE`).
 * The wire's ColdOpenInfo carries the rule's target but not its source, so the source is named
 * here once; the walk draws the clock edge only when the table has this state.
 */
export const COLD_OPEN_STATE = "cold_open";

/** One edge of a segment's state machine, as the walk prints it. */
export interface StateEdge {
  readonly from: string;
  /** The trigger, or `cold open clock` for the clock rule's edge. */
  readonly on: string;
  readonly guard: string | undefined;
  readonly to: string;
  /** Expanded from a `from = "*"` row. */
  readonly any: boolean;
  /** A clock rule, not a transition row. */
  readonly clock: boolean;
}

/**
 * Every edge of the table, in table order with the cold-open clock first: `*` rows expand into
 * one edge per state. Nothing is decided here; the rows are the daemon's.
 */
export function stateEdges(seg: SegmentTableInfo): StateEdge[] {
  const out: StateEdge[] = [];
  const cold = seg.clock.cold_open;
  if (cold && seg.states.includes(COLD_OPEN_STATE)) out.push({ from: COLD_OPEN_STATE, on: "cold open clock", guard: undefined, to: cold.to, any: false, clock: true });
  for (const t of seg.transitions) {
    const froms = t.from === "*" ? seg.states : [t.from];
    for (const from of froms) out.push({ from, on: t.on, guard: t.guard, to: t.to, any: t.from === "*", clock: false });
  }
  return out;
}

/** The states a walk from `initial` reaches over the table's edges. */
export function reachable(seg: SegmentTableInfo, edges: readonly StateEdge[] = stateEdges(seg)): ReadonlySet<string> {
  const seen = new Set([seg.initial]);
  const queue = [seg.initial];
  for (let s = queue.shift(); s !== undefined; s = queue.shift()) {
    for (const e of edges) {
      if (e.from === s && !seen.has(e.to)) {
        seen.add(e.to);
        queue.push(e.to);
      }
    }
  }
  return seen;
}

export interface StateView {
  readonly state: string;
  readonly initial: boolean;
  readonly reachable: boolean;
  readonly ops: readonly string[];
  readonly opsNarrowed: boolean;
  readonly screens: readonly string[];
  readonly screensNarrowed: boolean;
  readonly commands: readonly string[];
  readonly commandsNarrowed: boolean;
  /** The line the state summary carries while in this state. */
  readonly prompt: string | undefined;
  /** Edges leaving the state (a self-loop stays). */
  readonly exits: readonly StateEdge[];
  /** Edges from other states into this one. */
  readonly entries: readonly StateEdge[];
}

/** What is open in one state and where it leads: the segment's lists narrowed by `state_rules`. */
export function stateView(seg: SegmentTableInfo, state: string): StateView {
  const r = seg.state_rules?.[state];
  const edges = stateEdges(seg);
  return {
    state,
    initial: state === seg.initial,
    reachable: reachable(seg, edges).has(state),
    ops: r?.allowed_ops ?? seg.allowed_ops,
    opsNarrowed: r?.allowed_ops !== undefined,
    screens: r?.screens ?? seg.screens ?? [],
    screensNarrowed: r?.screens !== undefined,
    commands: r?.commands ?? seg.commands,
    commandsNarrowed: r?.commands !== undefined,
    prompt: r?.prompt?.trim(),
    exits: edges.filter((e) => e.from === state),
    entries: edges.filter((e) => e.to === state && e.from !== state),
  };
}
