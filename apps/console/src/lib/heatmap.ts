// Heatmap ordering for tournament results. `worstFirst` mirrors
// crates/shadowrunr-sim/src/summary.rs `worst_first` so the console and heatmap.md agree:
// any rule fail, then not_evaluable, then not_provoked (all descending), then mean quality
// ascending (cells without quality last among equals), then the cell key.

import type { CellKey, CellResult, QualityTally, TournamentResults } from "../contract";

/** Fewer judged runs than this: a rate is a hint, not a fact (summary.rs THIN_N). */
export const THIN_N = 5;

const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);

export const ruleFails = (c: CellResult) => sum(Object.values(c.rules).map((r) => r.fail));
export const notEvaluable = (c: CellResult) => sum(Object.values(c.rules).map((r) => r.not_evaluable));
export const notProvoked = (c: CellResult) => sum(Object.values(c.rules).map((r) => r.not_provoked));
export const rulePass = (c: CellResult) => sum(Object.values(c.rules).map((r) => r.pass));
export const ruleN = (c: CellResult) => sum(Object.values(c.rules).map((r) => r.n));

/** Mean of the dimension rates that have n > 0; undefined when there are none. */
export function meanQuality(c: CellResult): number | undefined {
  const rates = Object.values(c.quality)
    .map((q) => q.rate)
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

export interface QualityMargin {
  dimension: string;
  trueCount: number;
  n: number;
  rate: number | undefined;
}

/** Quality dimensions across all cells, worst aggregate rate first (summary.rs quality_margins). */
export function qualityColumns(res: Pick<TournamentResults, "cells">): QualityMargin[] {
  const m = new Map<string, { t: number; n: number }>();
  for (const c of res.cells) {
    for (const [dim, q] of Object.entries(c.quality)) {
      const e = m.get(dim) ?? { t: 0, n: 0 };
      e.t += q.true;
      e.n += q.n;
      m.set(dim, e);
    }
  }
  return [...m.entries()]
    .map(([dimension, { t, n }]) => ({ dimension, trueCount: t, n, rate: n > 0 ? t / n : undefined }))
    .sort((a, b) => (a.rate ?? -1) - (b.rate ?? -1) || cmpStr(a.dimension, b.dimension));
}

export type Heat = "good" | "warn" | "bad" | "none";

/** Colour band for a quality rate. Thresholds are display-only. */
export function heatOf(q: Pick<QualityTally, "rate" | "n"> | undefined): Heat {
  if (!q || q.n === 0 || typeof q.rate !== "number") return "none";
  if (q.rate >= 0.8) return "good";
  if (q.rate >= 0.6) return "warn";
  return "bad";
}

export const isThin = (c: CellResult) => c.judged < THIN_N;

export function cellLabel(k: CellKey): string {
  return `${k.tone} · ${k.core_tier} · ${k.segment} · a${k.audience_size}`;
}

export function cellId(k: CellKey): string {
  return `${k.instrument_hash}|${k.tone}|${k.core_tier}|${k.segment}|${k.audience_size}`;
}
