// Heatmap ordering and per-cell figures: the one home of the sorting rule since the pre-port
// desk was deleted at cutover C2. `worstFirst` mirrors
// crates/shadowrunr-sim/src/summary.rs `worst_first`, so the console and heatmap.md agree:
// any rule fail, then not_evaluable, then not_provoked (all descending), then mean quality
// ascending (cells without quality last among equals), then the cell key.
import type { Bound, CellKey, CellResult, QualityTally, RuleTally } from "./results";

export { cellLabel, cellRouteKey, parseCellRouteKey, type ParsedCellKey } from "./routeKey";

/** Fewer judged runs than this: a rate is a hint, not a fact (summary.rs THIN_N). */
export const THIN_N = 5;

const sum = (xs: readonly number[]) => xs.reduce((a, b) => a + b, 0);
/** The cell's rule tallies by rule id (the contract types map values as possibly absent). */
export const ruleEntries = (c: CellResult): [string, RuleTally][] => Object.entries(c.rules).filter((e): e is [string, RuleTally] => e[1] !== undefined);
/** The cell's quality tallies by dimension. */
export const qualityEntries = (c: CellResult): [string, QualityTally][] => Object.entries(c.quality).filter((e): e is [string, QualityTally] => e[1] !== undefined);
const tallies = (c: CellResult) => ruleEntries(c).map(([, t]) => t);

export const ruleFails = (c: CellResult) => sum(tallies(c).map((r) => r.fail));
export const notEvaluable = (c: CellResult) => sum(tallies(c).map((r) => r.not_evaluable));
export const notProvoked = (c: CellResult) => sum(tallies(c).map((r) => r.not_provoked));
export const rulePass = (c: CellResult) => sum(tallies(c).map((r) => r.pass));

/** Mean of the dimension rates that have n > 0; undefined when there are none. */
export function meanQuality(c: CellResult): number | undefined {
  const rates = qualityEntries(c)
    .map(([, q]) => q.rate)
    .filter((r): r is number => typeof r === "number");
  return rates.length ? sum(rates) / rates.length : undefined;
}

const cmpStr = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** Field order of the Rust `CellKey` derive(Ord). */
export function compareCellKey(a: CellKey, b: CellKey): number {
  return (
    cmpStr(a.instrument_hash, b.instrument_hash) ||
    cmpStr(a.tone, b.tone) ||
    cmpStr(a.core_tier, b.core_tier) ||
    cmpStr(a.segment, b.segment) ||
    a.audience_size - b.audience_size
  );
}

export function worstFirst(cells: readonly CellResult[]): CellResult[] {
  return [...cells].sort(
    (a, b) =>
      ruleFails(b) - ruleFails(a) ||
      notEvaluable(b) - notEvaluable(a) ||
      notProvoked(b) - notProvoked(a) ||
      (meanQuality(a) ?? -1) - (meanQuality(b) ?? -1) ||
      compareCellKey(a.cell, b.cell),
  );
}

export const isThin = (c: CellResult) => c.judged < THIN_N;

/** The 95% z (crates/shadowrunr-sim/src/stats.rs Z95). */
const Z95 = 1.96;

/** Wilson score interval for k in n (stats.rs `wilson`). */
export function wilson(k: number, n: number): [number, number] {
  if (n === 0) return [0, 1];
  const p = k / n;
  const z2 = Z95 * Z95;
  const denom = 1 + z2 / n;
  const centre = (p + z2 / (2 * n)) / denom;
  const half = (Z95 * Math.sqrt((p * (1 - p)) / n + z2 / (4 * n * n))) / denom;
  return [Math.max(0, centre - half), Math.min(1, centre + half)];
}

/** Rule of three at 0 or n, Wilson otherwise; undefined for n = 0 (stats.rs `rate_bound`). */
export function rateBound(k: number, n: number): Bound | undefined {
  if (n === 0) return undefined;
  const three = Math.min(1, 3 / n);
  if (k === 0) return { lo: 0, hi: three, method: "rule_of_three" };
  if (k >= n) return { lo: 1 - three, hi: 1, method: "rule_of_three" };
  const [lo, hi] = wilson(k, n);
  return { lo, hi, method: "wilson" };
}

export interface CellRate {
  /** Failed checks over provoked checks; undefined when nothing was provoked. */
  readonly rate: number | undefined;
  readonly k: number;
  readonly n: number;
  readonly bound: Bound | undefined;
}

/** The cell's fail rate across every rule, with n and its 95% bound. */
export function cellFailRate(c: CellResult): CellRate {
  const k = ruleFails(c);
  const n = k + rulePass(c);
  return { rate: n > 0 ? k / n : undefined, k, n, bound: rateBound(k, n) };
}

/** Share of rule checks that held (A/1's health); undefined when no check was provoked. */
export function cellHealth(c: CellResult): number | undefined {
  const pass = rulePass(c);
  const n = pass + ruleFails(c);
  return n > 0 ? pass / n : undefined;
}

export const isRedLine = (rule: string) => rule.startsWith("red-line");

export function redLineFails(c: CellResult): number {
  return sum(ruleEntries(c).map(([k, r]) => (isRedLine(k) ? r.fail : 0)));
}

export const sameCell = (a: CellKey, b: CellKey) => compareCellKey(a, b) === 0;
