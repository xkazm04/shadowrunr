// A results cell's rule checks, counted: the core of the simulator's per-cell figures
// (pages/simulator/cells.ts re-exports these and stays their page home), here so another page
// can count a cell the same way without importing the simulator (pages never import pages).
// Counts only: no rate, no held share, so nothing here is a second derivation of `held`.
import type { CellResult, RuleTally } from "@shadowrunr/contract";

/** Fewer judged runs than this: a rate is a hint, not a fact (summary.rs THIN_N). */
export const THIN_N = 5;

const sum = (xs: readonly number[]) => xs.reduce((a, b) => a + b, 0);
/** The cell's rule tallies by rule id (the contract types map values as possibly absent). */
export const ruleEntries = (c: CellResult): [string, RuleTally][] => Object.entries(c.rules).filter((e): e is [string, RuleTally] => e[1] !== undefined);
const tallies = (c: CellResult) => ruleEntries(c).map(([, t]) => t);

export const ruleFails = (c: CellResult) => sum(tallies(c).map((r) => r.fail));
export const notEvaluable = (c: CellResult) => sum(tallies(c).map((r) => r.not_evaluable));
export const notProvoked = (c: CellResult) => sum(tallies(c).map((r) => r.not_provoked));
export const rulePass = (c: CellResult) => sum(tallies(c).map((r) => r.pass));

/**
 * The cell's rule checks, counted and never bounded. Every rule is judged once per run, so the
 * checks inside one run share its host, seed and room and are not independent: a cell of two
 * judged runs and 95 checks is two observations, not 95, and a bound over the pooled checks
 * would claim the precision of 95. heatmap.md prints the same counts and no cell bound; the
 * bound that holds is each rule's own `fail_bound`, whose n is that rule's evaluated runs.
 */
export interface CellChecks {
  readonly fail: number;
  /** Evaluated checks (pass + fail); not provoked and not evaluable are never passes. */
  readonly evaluated: number;
}

export function cellChecks(c: CellResult): CellChecks {
  const fail = ruleFails(c);
  return { fail, evaluated: fail + rulePass(c) };
}

export const isRedLine = (rule: string) => rule.startsWith("red-line");
