// `tournament.results` is the simulator's results.json (crates/shadowrunr-sim/src/summary.rs
// `Results`). The contract types it as an open record (it is not in shadowrunr-types yet), so
// the door validated only that it is an object. This module narrows it, field by field, into
// the typed shape the page reads; a reply that does not fit is `null` and shows as degraded.
// Proposed contract change (P5 report): move `Results` into shadowrunr-types so the door
// validates it and this file shrinks to a re-export.
import type { TournamentResults } from "@shadowrunr/contract";

export interface Bound {
  readonly lo: number;
  readonly hi: number;
  readonly method: string;
}

export interface RuleTally {
  readonly pass: number;
  readonly fail: number;
  readonly not_provoked: number;
  readonly not_evaluable: number;
  /** Evaluated runs: pass + fail. */
  readonly n: number;
  /** 95% bound on the FAIL rate. */
  readonly fail_bound?: Bound;
  /** `<run_id>#<line>` refs of the failing evidence. */
  readonly fail_refs: readonly string[];
}

export interface QualityTally {
  readonly true: number;
  readonly n: number;
  readonly rate?: number;
  readonly bound?: Bound;
  readonly not_evaluable: number;
}

export interface CellKey {
  readonly instrument_hash: string;
  readonly tone: string;
  readonly core_tier: string;
  readonly segment: string;
  readonly audience_size: number;
}

export interface Usage {
  readonly calls: number;
  readonly errors: number;
  readonly cost_usd: number;
}

export interface CellResult {
  readonly cell: CellKey;
  readonly runs: number;
  readonly judged: number;
  readonly void: number;
  readonly failed: number;
  readonly judge_tiers: readonly string[];
  readonly rules: Readonly<Record<string, RuleTally>>;
  readonly quality: Readonly<Record<string, QualityTally>>;
  readonly max_say_gap_s?: number;
}

export interface SimResults {
  readonly tournament_id: string;
  readonly instrument_hashes: readonly string[];
  readonly assumptions: readonly string[];
  readonly runs: number;
  readonly judged: number;
  readonly void: number;
  readonly failed: number;
  readonly cells: readonly CellResult[];
  readonly usage_total?: Usage;
}

type Rec = Readonly<Record<string, unknown>>;

const isRec = (v: unknown): v is Rec => typeof v === "object" && v !== null && !Array.isArray(v);
const num = (v: unknown): number | undefined => (typeof v === "number" && Number.isFinite(v) ? v : undefined);
const str = (v: unknown): string | undefined => (typeof v === "string" ? v : undefined);
const strs = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

/** Thrown inside `readResults` only, to stop at the first missing required field. */
class Misfit extends Error {}

function need<T>(v: T | undefined, what: string): T {
  if (v === undefined) throw new Misfit(what);
  return v;
}

function readBound(v: unknown): Bound | undefined {
  if (!isRec(v)) return undefined;
  const lo = num(v["lo"]);
  const hi = num(v["hi"]);
  if (lo === undefined || hi === undefined) return undefined;
  return { lo, hi, method: str(v["method"]) ?? "" };
}

function readRule(v: unknown, at: string): RuleTally {
  if (!isRec(v)) throw new Misfit(at);
  const pass = need(num(v["pass"]), `${at}.pass`);
  const fail = need(num(v["fail"]), `${at}.fail`);
  const bound = readBound(v["fail_bound"]);
  return {
    pass,
    fail,
    not_provoked: num(v["not_provoked"]) ?? 0,
    not_evaluable: num(v["not_evaluable"]) ?? 0,
    n: num(v["n"]) ?? pass + fail,
    ...(bound ? { fail_bound: bound } : {}),
    fail_refs: strs(v["fail_refs"]),
  };
}

function readQuality(v: unknown, at: string): QualityTally {
  if (!isRec(v)) throw new Misfit(at);
  const rate = num(v["rate"]);
  const bound = readBound(v["bound"]);
  return {
    true: need(num(v["true"]), `${at}.true`),
    n: need(num(v["n"]), `${at}.n`),
    ...(rate === undefined ? {} : { rate }),
    ...(bound ? { bound } : {}),
    not_evaluable: num(v["not_evaluable"]) ?? 0,
  };
}

function readMap<T>(v: unknown, at: string, read: (x: unknown, at: string) => T): Record<string, T> {
  if (!isRec(v)) return {};
  const out: Record<string, T> = {};
  for (const [k, x] of Object.entries(v)) out[k] = read(x, `${at}.${k}`);
  return out;
}

function readKey(v: unknown, at: string): CellKey {
  if (!isRec(v)) throw new Misfit(at);
  return {
    instrument_hash: str(v["instrument_hash"]) ?? "",
    tone: need(str(v["tone"]), `${at}.tone`),
    core_tier: need(str(v["core_tier"]), `${at}.core_tier`),
    segment: need(str(v["segment"]), `${at}.segment`),
    audience_size: need(num(v["audience_size"]), `${at}.audience_size`),
  };
}

function readCell(v: unknown, at: string): CellResult {
  if (!isRec(v)) throw new Misfit(at);
  const gap = num(v["max_say_gap_s"]);
  return {
    cell: readKey(v["cell"], `${at}.cell`),
    runs: num(v["runs"]) ?? 0,
    judged: num(v["judged"]) ?? 0,
    void: num(v["void"]) ?? 0,
    failed: num(v["failed"]) ?? 0,
    judge_tiers: strs(v["judge_tiers"]),
    rules: readMap(v["rules"], `${at}.rules`, readRule),
    quality: readMap(v["quality"], `${at}.quality`, readQuality),
    ...(gap === undefined ? {} : { max_say_gap_s: gap }),
  };
}

function readUsage(v: unknown): Usage | undefined {
  if (!isRec(v)) return undefined;
  const calls = num(v["calls"]);
  if (calls === undefined) return undefined;
  return { calls, errors: num(v["errors"]) ?? 0, cost_usd: num(v["cost_usd"]) ?? 0 };
}

export type ReadResult = { readonly ok: true; readonly value: SimResults } | { readonly ok: false; readonly path: string };

/** Narrows a `tournament.results` reply; `ok: false` names the first field that did not fit. */
export function readResults(raw: TournamentResults): ReadResult {
  try {
    const cells = raw["cells"];
    if (!Array.isArray(cells)) throw new Misfit("cells");
    const usage = readUsage(raw["usage_total"]);
    return {
      ok: true,
      value: {
        tournament_id: need(str(raw["tournament_id"]), "tournament_id"),
        instrument_hashes: strs(raw["instrument_hashes"]),
        assumptions: strs(raw["assumptions"]),
        runs: num(raw["runs"]) ?? 0,
        judged: num(raw["judged"]) ?? 0,
        void: num(raw["void"]) ?? 0,
        failed: num(raw["failed"]) ?? 0,
        cells: cells.map((c, i) => readCell(c, `cells.${i}`)),
        ...(usage ? { usage_total: usage } : {}),
      },
    };
  } catch (e) {
    if (e instanceof Misfit) return { ok: false, path: e.message };
    throw e;
  }
}
