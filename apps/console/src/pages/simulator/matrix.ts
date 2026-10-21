// The heatmap's map: a tournament's cells pivoted into rows of tone × tier and columns of
// segment × audience, one block per instrument hash (results from different instruments are
// never merged, as the tournament summary keeps them apart). Pure.
//
// A position's colour is the row's held derivation (runs.rs tournament_row, TournamentRow.held)
// applied to the cell's own runs: the mean over its judged runs of each run's verdict share,
// RunRow.health. Runs are the observations, never the pooled checks, so the run-weighted cells
// give back the tournament's held exactly (model.test.ts checks it). The figures beside the
// colour are the counted ones of ./cells.ts (fail over evaluated checks, judged runs): counted,
// not bounded, so the map claims no more than the worst-first table.
import type { ModelTier, RunRow } from "@shadowrunr/contract";
import { cellChecks, compareCellKey, isRedLine, isThin, ruleEntries } from "./cells";
import type { CellKey, CellResult } from "./results";

/** Tier order within a tone: the contract's ModelTier, cheapest first. */
const TIER_ORDER: readonly ModelTier[] = ["haiku", "sonnet", "opus"];

export interface MatrixRow {
  readonly tone: string;
  readonly tier: string;
}

export interface MatrixCol {
  readonly segment: string;
  readonly audience: number;
}

export interface Position {
  /** The coordinate, present whether or not the cell ran. */
  readonly key: CellKey;
  /** The cell's results; undefined: this combination was not run. */
  readonly cell: CellResult | undefined;
  /** Mean of the cell's judged runs' shares; undefined before a verdict or when unattributable. */
  readonly held: number | undefined;
  /** The runs `held` averages. */
  readonly heldRuns: number;
  readonly fail: number;
  readonly evaluated: number;
  /** A red-line rule failed in this cell. */
  readonly red: boolean;
  /** Fewer judged runs than THIN_N. */
  readonly thin: boolean;
}

/** A row's or column's marginal over the cells that ran; not-run positions are left out. */
export interface Margin {
  readonly cells: number;
  readonly fail: number;
  readonly evaluated: number;
  readonly judged: number;
  readonly thin: number;
}

export interface Block {
  readonly hash: string;
  readonly rows: readonly MatrixRow[];
  readonly cols: readonly MatrixCol[];
  /** rows × cols. */
  readonly positions: readonly (readonly Position[])[];
  readonly rowMargins: readonly Margin[];
  readonly colMargins: readonly Margin[];
}

const byName = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
const tierRank = (t: string) => {
  const i = TIER_ORDER.indexOf(t as ModelTier);
  return i < 0 ? TIER_ORDER.length : i;
};
const byTier = (a: string, b: string) => tierRank(a) - tierRank(b) || byName(a, b);
const distinct = <T>(xs: readonly T[]) => [...new Set(xs)];
const coord = (tone: string, tier: string, segment: string, audience: number) => `${tone}|${tier}|${segment}|${audience}`;

function margin(ps: readonly Position[]): Margin {
  let cells = 0;
  let fail = 0;
  let evaluated = 0;
  let judged = 0;
  let thin = 0;
  for (const p of ps) {
    if (!p.cell) continue;
    cells += 1;
    fail += p.fail;
    evaluated += p.evaluated;
    judged += p.cell.judged;
    if (p.thin) thin += 1;
  }
  return { cells, fail, evaluated, judged, thin };
}

/**
 * The cells as one block per instrument hash. Axes come from the cells present: tones and
 * segments by name, tiers in model order, audiences ascending. `runs` are the tournament's
 * RunRows; a run carries no instrument hash, so a coordinate that two blocks share gets no
 * held colour rather than a guessed one.
 */
export function pivot(cells: readonly CellResult[], runs: readonly RunRow[] = []): Block[] {
  const hashes = distinct(cells.map((c) => c.cell.instrument_hash)).sort(byName);
  const owners = new Map<string, number>();
  for (const c of cells) {
    const k = coord(c.cell.tone, c.cell.core_tier, c.cell.segment, c.cell.audience_size);
    owners.set(k, (owners.get(k) ?? 0) + 1);
  }
  const shares = new Map<string, { sum: number; n: number }>();
  for (const r of runs) {
    if (r.health === undefined) continue;
    const k = coord(r.spec.tone_variant, r.spec.core_tier, r.spec.segment, r.spec.audience_size);
    const s = shares.get(k) ?? { sum: 0, n: 0 };
    s.sum += r.health;
    s.n += 1;
    shares.set(k, s);
  }
  return hashes.map((hash) => {
    const mine = cells.filter((c) => c.cell.instrument_hash === hash);
    const at = new Map(mine.map((c) => [coord(c.cell.tone, c.cell.core_tier, c.cell.segment, c.cell.audience_size), c]));
    const tones = distinct(mine.map((c) => c.cell.tone)).sort(byName);
    const tiers = distinct(mine.map((c) => c.cell.core_tier)).sort(byTier);
    const segments = distinct(mine.map((c) => c.cell.segment)).sort(byName);
    const audiences = distinct(mine.map((c) => c.cell.audience_size)).sort((a, b) => a - b);
    const rows = tones.flatMap((tone) => tiers.map((tier) => ({ tone, tier })));
    const cols = segments.flatMap((segment) => audiences.map((audience) => ({ segment, audience })));
    const positions = rows.map((r) =>
      cols.map((c): Position => {
        const k = coord(r.tone, r.tier, c.segment, c.audience);
        const cell = at.get(k);
        const key: CellKey = { instrument_hash: hash, tone: r.tone, core_tier: r.tier, segment: c.segment, audience_size: c.audience };
        if (!cell) return { key, cell: undefined, held: undefined, heldRuns: 0, fail: 0, evaluated: 0, red: false, thin: false };
        const s = owners.get(k) === 1 ? shares.get(k) : undefined;
        const checks = cellChecks(cell);
        return {
          key,
          cell,
          held: s && s.n > 0 ? s.sum / s.n : undefined,
          heldRuns: s?.n ?? 0,
          fail: checks.fail,
          evaluated: checks.evaluated,
          red: ruleEntries(cell).some(([rule, t]) => isRedLine(rule) && t.fail > 0),
          thin: isThin(cell),
        };
      }),
    );
    return {
      hash,
      rows,
      cols,
      positions,
      rowMargins: positions.map(margin),
      colMargins: cols.map((_, j) => margin(positions.map((row) => row[j]).filter((p): p is Position => p !== undefined))),
    };
  });
}

/** The position a route's cell key names, as [row, col] in its block, or undefined. */
export function locate(b: Block, k: CellKey | undefined): [number, number] | undefined {
  if (!k) return undefined;
  for (let i = 0; i < b.positions.length; i++) {
    const j = b.positions[i]?.findIndex((p) => compareCellKey(p.key, k) === 0) ?? -1;
    if (j >= 0) return [i, j];
  }
  return undefined;
}
