// The simulator grid model (A/1 app.js:446-475): one tile per tournament, one cell per run.
// Pure: built from `tournaments.page` and `runs.page` (each row carries its verdict's health),
// aggregated once, then drawn. Geometry and hit-testing are A/1's, kept exact at 40 tiles x 120 runs and
// generalised past it so the 10x scale (48k runs) still draws one rect per run.
import type { RunRow } from "@shadowrunr/contract";
import { rulePass, ruleFails } from "./cells";
import type { SimResults } from "./results";

export interface RunCell {
  readonly runId: string;
  /** Share of the run's rule checks held; undefined while it has no verdict. */
  readonly health: number | undefined;
  /** A red-line rule failed in this run. */
  readonly red: boolean;
}

export interface Tile {
  readonly id: string;
  /** Mean share of rule checks held over its judged runs; undefined before any verdict. */
  readonly health: number | undefined;
  /** Red-line fails across the tournament. */
  readonly red: number;
  /** Runs the daemon reports for the tournament. */
  readonly declared: number;
  readonly runs: readonly RunCell[];
}

/** The A/1 tile count: 8 columns by 5 rows. */
export const TILE_COLS = 8;
export const TILE_ROWS = 5;
export const MAX_TILES = TILE_COLS * TILE_ROWS;
/** A/1's run grid inside a tile: 12 by 10, so 120 runs. */
export const RUN_COLS = 12;
export const RUN_ROWS = 10;
/** The label band at the top of a tile (A/1: 16 CSS px). */
export const LABEL_BAND = 16;

/** Share of rule checks held across a tournament's cells (pooled); undefined before any verdict. */
export function resultsHealth(results: SimResults | undefined): number | undefined {
  let pass = 0;
  let fail = 0;
  for (const c of results?.cells ?? []) {
    pass += rulePass(c);
    fail += ruleFails(c);
  }
  return pass + fail > 0 ? pass / (pass + fail) : undefined;
}

/**
 * One tournament as a tile, from its run rows: each run's health and red-line fails come from
 * its own verdict (the daemon reads them). The tile's health is the mean over judged runs.
 * `declared` is the run count the daemon reports for the tournament.
 */
export function buildTile(id: string, declared: number, runs: readonly RunRow[]): Tile {
  let red = 0;
  let sum = 0;
  let judged = 0;
  const cells: RunCell[] = runs.map((r) => {
    const fails = r.red_line_fails ?? 0;
    red += fails;
    if (r.health !== undefined) {
      sum += r.health;
      judged += 1;
    }
    return { runId: r.run_id, health: r.health, red: fails > 0 };
  });
  return { id, health: judged > 0 ? sum / judged : undefined, red, declared, runs: cells };
}

/** The tiles in view: the last `MAX_TILES` tournaments of the daemon's order. */
export function tilesInView<T>(all: readonly T[]): readonly T[] {
  return all.length > MAX_TILES ? all.slice(all.length - MAX_TILES) : all;
}

export interface GridLayout {
  readonly cols: number;
  readonly rows: number;
  readonly gap: number;
  readonly tw: number;
  readonly th: number;
}

/** A/1 `geo`: 8 x 5 tiles over the grid rect, the gap 0.8% of its width (at least 4). */
export function gridLayout(w: number, h: number): GridLayout {
  const cols = TILE_COLS;
  const rows = TILE_ROWS;
  const gap = Math.max(4, w * 0.008);
  return { cols, rows, gap, tw: (w - gap * (cols - 1)) / cols, th: (h - gap * (rows - 1)) / rows };
}

export interface TileOrigin {
  readonly ox: number;
  readonly oy: number;
}

export function tileOrigin(l: GridLayout, i: number): TileOrigin {
  return { ox: (i % l.cols) * (l.tw + l.gap), oy: Math.floor(i / l.cols) * (l.th + l.gap) };
}

/**
 * The run grid inside a tile. Up to 120 runs it is A/1's 12 x 10; beyond, it keeps the cells
 * near-square to the tile so a 1,200-run tournament still gets one cell per run.
 */
export function runGrid(count: number, tw: number, th: number): { cols: number; rows: number } {
  if (count <= RUN_COLS * RUN_ROWS) return { cols: RUN_COLS, rows: RUN_ROWS };
  const bandH = Math.max(1, th - LABEL_BAND);
  const cols = Math.max(RUN_COLS, Math.ceil(Math.sqrt((count * Math.max(1, tw)) / bandH)));
  return { cols, rows: Math.max(RUN_ROWS, Math.ceil(count / cols)) };
}

/** A/1 `at`: the tile under a point of the grid rect, or -1. */
export function hitTest(l: GridLayout, x: number, y: number, count: number): number {
  const col = Math.floor(x / (l.tw + l.gap));
  const row = Math.floor(y / (l.th + l.gap));
  const i = row * l.cols + col;
  return col >= 0 && col < l.cols && row >= 0 && i >= 0 && i < count ? i : -1;
}

export interface Rect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/**
 * Where the grid sits on the screen. Wide: A/1's `pos(2, 13, 74, 84)` of the scene. Narrow
 * (the screen under `narrowPx`): full width under the heading, the HUD below it.
 */
export function gridRect(w: number, h: number, narrow: boolean): Rect {
  return narrow ? { x: w * 0.03, y: h * 0.16, w: w * 0.94, h: h * 0.5 } : { x: w * 0.02, y: h * 0.13, w: w * 0.74, h: h * 0.84 };
}

export interface GridSummary {
  readonly runs: number;
  readonly red: number;
  readonly best: Tile | undefined;
  readonly worst: Tile | undefined;
}

export function summarise(tiles: readonly Tile[]): GridSummary {
  let runs = 0;
  let red = 0;
  let best: Tile | undefined;
  let worst: Tile | undefined;
  for (const t of tiles) {
    runs += t.runs.length;
    red += t.red;
    if (t.health === undefined) continue;
    if (!best || t.health > (best.health ?? -1)) best = t;
    if (!worst || t.health < (worst.health ?? 2)) worst = t;
  }
  return { runs, red, best, worst };
}
