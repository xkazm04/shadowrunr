// The engine floor's tables (A/1 engineFloor, app.js:580-591): what the ledger shows the engine
// did. The whole source comes from the daemon's `ledger.summary` (fromLedgerSummary); when that
// fails the floor folds the rows it paged in (summariseLedger), with the same definitions.
import type { LedgerEntry, LedgerSummary } from "@shadowrunr/contract";
import { percentile } from "./brainModel";

export interface ObservedRoute {
  readonly role: string;
  readonly calls: number;
  readonly errors: number;
  readonly fallbacks: number;
  readonly tiers: readonly string[];
  readonly efforts: readonly string[];
  readonly engines: readonly string[];
  readonly models: readonly string[];
  readonly durationP50: number | undefined;
  readonly durationP95: number | undefined;
  readonly firstTextP50: number | undefined;
}

export interface AccountRow {
  readonly key: string;
  readonly engine: string;
  readonly account: string;
  readonly calls: number;
  readonly errors: number;
  readonly models: readonly { readonly model: string; readonly calls: number }[];
  readonly lastTs: number;
  /** The error kind of the account's latest call, when it failed. */
  readonly lastError: string | undefined;
}

export interface EngineSummary {
  readonly calls: number;
  readonly errors: number;
  readonly fallbacks: number;
  /** The sum over the rows that carry a cost. */
  readonly cost: number;
  /** Rows without a cost: unknown, never read as $0. */
  readonly uncosted: number;
  readonly cacheRead: number;
  readonly unrouted: number;
  readonly routes: readonly ObservedRoute[];
  readonly accounts: readonly AccountRow[];
  /** Earliest row per run, so a run's calls read as stream time. */
  readonly runStart: ReadonlyMap<string, number>;
}

const sortedSet = (s: Set<string>) => [...s].sort();

export function summariseLedger(rows: readonly LedgerEntry[]): EngineSummary {
  let errors = 0;
  let fallbacks = 0;
  let cost = 0;
  let uncosted = 0;
  let cacheRead = 0;
  let unrouted = 0;
  const roles = new Map<string, { calls: number; errors: number; fallbacks: number; tiers: Set<string>; efforts: Set<string>; engines: Set<string>; models: Set<string>; dur: number[]; first: number[] }>();
  const accounts = new Map<string, { engine: string; account: string; calls: number; errors: number; models: Map<string, number>; lastTs: number; lastError: string | undefined }>();
  const runStart = new Map<string, number>();
  for (const l of rows) {
    const failed = l.error_kind !== undefined;
    if (failed) errors++;
    if (l.fallback_reason !== undefined) fallbacks++;
    if (l.cost_usd === undefined) uncosted++;
    else cost += l.cost_usd;
    cacheRead += l.usage.cache_read_tokens ?? 0;
    if (l.run_id !== undefined) {
      const t = runStart.get(l.run_id);
      if (t === undefined || l.ts_ms < t) runStart.set(l.run_id, l.ts_ms);
    }
    let r = roles.get(l.role);
    if (!r) {
      r = { calls: 0, errors: 0, fallbacks: 0, tiers: new Set(), efforts: new Set(), engines: new Set(), models: new Set(), dur: [], first: [] };
      roles.set(l.role, r);
    }
    r.calls++;
    if (failed) r.errors++;
    if (l.fallback_reason !== undefined) r.fallbacks++;
    if (l.tier) r.tiers.add(l.tier);
    if (l.effort) r.efforts.add(l.effort);
    if (l.engine) r.engines.add(l.engine);
    if (l.model) r.models.add(l.model);
    if (!failed) r.dur.push(l.duration_ms);
    if (l.first_text_ms !== undefined) r.first.push(l.first_text_ms);
    if (l.engine === undefined || l.account === undefined) {
      unrouted++;
      continue;
    }
    const key = `${l.engine} · ${l.account}`;
    const a = accounts.get(key) ?? { engine: l.engine, account: l.account, calls: 0, errors: 0, models: new Map<string, number>(), lastTs: 0, lastError: undefined };
    a.calls++;
    if (failed) a.errors++;
    if (l.model) a.models.set(l.model, (a.models.get(l.model) ?? 0) + 1);
    if (l.ts_ms >= a.lastTs) {
      a.lastTs = l.ts_ms;
      a.lastError = l.error_kind;
    }
    accounts.set(key, a);
  }
  const routes = [...roles].map(([role, r]) => {
    const dur = [...r.dur].sort((x, y) => x - y);
    const first = [...r.first].sort((x, y) => x - y);
    return {
      role,
      calls: r.calls,
      errors: r.errors,
      fallbacks: r.fallbacks,
      tiers: sortedSet(r.tiers),
      efforts: sortedSet(r.efforts),
      engines: sortedSet(r.engines),
      models: sortedSet(r.models),
      durationP50: percentile(dur, 50),
      durationP95: percentile(dur, 95),
      firstTextP50: percentile(first, 50),
    };
  });
  routes.sort((a, b) => b.calls - a.calls);
  const accountRows = [...accounts].map(([key, a]) => ({
    key,
    engine: a.engine,
    account: a.account,
    calls: a.calls,
    errors: a.errors,
    models: [...a.models].map(([model, calls]) => ({ model, calls })).sort((x, y) => y.calls - x.calls),
    lastTs: a.lastTs,
    lastError: a.lastError,
  }));
  accountRows.sort((a, b) => b.calls - a.calls);
  return { calls: rows.length, errors, fallbacks, cost, uncosted, cacheRead, unrouted, routes, accounts: accountRows, runStart };
}

/** The daemon's whole-source `ledger.summary` as the floor's summary; `runStart` comes from the
 * rows the call ledger paged in (it only times those rows). */
export function fromLedgerSummary(s: LedgerSummary, runStart: ReadonlyMap<string, number>): EngineSummary {
  return {
    calls: s.calls,
    errors: s.errors,
    fallbacks: s.fallbacks,
    cost: s.cost_usd,
    uncosted: s.uncosted_calls,
    cacheRead: s.cache_read_tokens,
    unrouted: s.unrouted,
    routes: s.routes.map((r) => ({
      role: r.role,
      calls: r.calls,
      errors: r.errors,
      fallbacks: r.fallbacks,
      tiers: r.tiers,
      efforts: r.efforts,
      engines: r.engines,
      models: r.models,
      durationP50: r.duration_p50_ms,
      durationP95: r.duration_p95_ms,
      firstTextP50: r.first_text_p50_ms,
    })),
    accounts: s.accounts.map((a) => ({
      key: `${a.engine} · ${a.account}`,
      engine: a.engine,
      account: a.account,
      calls: a.calls,
      errors: a.errors,
      models: a.models,
      lastTs: a.last_ts_ms,
      lastError: a.last_error,
    })),
    runStart,
  };
}

/** Cost as known: `$x`, `$x + n uncosted`, or `unknown` when no row carries a cost. */
export function costText(s: Pick<EngineSummary, "calls" | "cost" | "uncosted">, usd: (x: number) => string): string {
  if (s.uncosted === 0) return usd(s.cost);
  if (s.uncosted >= s.calls) return "unknown";
  return `${usd(s.cost)} + ${s.uncosted.toLocaleString("en-US")} uncosted`;
}

// The compare (diff-comparison, pair-and-baseline-selection): a baseline ledger against the
// candidate, role by role. A delta shows only where both sides carry `minN` calls; a role on
// one side only is new or gone, never a delta against zero.

export type Direction = "better" | "worse" | "same";
/** both: a delta per column; new / gone: one side only; thin: under the minimum sample. */
export type CompareState = "both" | "new" | "gone" | "thin";
export const STATE_LABEL: Readonly<Record<CompareState, string>> = { both: "", new: "new", gone: "gone", thin: "too few calls" };

/** One column of the compare: each side's value and, when the sample carries it, the change. */
export interface DeltaCell {
  readonly base: number | undefined;
  readonly cand: number | undefined;
  /** cand - base (ms, or percentage points for a rate); undefined where no delta is honest. */
  readonly delta: number | undefined;
  readonly direction: Direction | undefined;
}

export interface CompareRow {
  readonly role: string;
  readonly state: CompareState;
  readonly baseCalls: number | undefined;
  readonly candCalls: number | undefined;
  /** Percent of calls that failed; the delta in percentage points. */
  readonly errorRate: DeltaCell;
  readonly fallbackRate: DeltaCell;
  readonly p50: DeltaCell;
  readonly p95: DeltaCell;
  readonly firstText: DeltaCell;
}

export interface Comparison {
  readonly rows: readonly CompareRow[];
  readonly total: { readonly errorRate: DeltaCell; readonly fallbackRate: DeltaCell };
  /** The known cost of each side; a side with uncosted calls makes the delta a lower bound. */
  readonly cost: { readonly base: number; readonly cand: number; readonly delta: number; readonly lowerBound: boolean };
}

type Side = Pick<EngineSummary, "calls" | "errors" | "fallbacks" | "cost" | "uncosted" | "routes">;

/** Lower is better for every compared column (latency, error and fallback rates). */
function cell(base: number | undefined, cand: number | undefined, show: boolean): DeltaCell {
  if (!show || base === undefined || cand === undefined) return { base, cand, delta: undefined, direction: undefined };
  const delta = cand - base;
  return { base, cand, delta, direction: delta > 0 ? "worse" : delta < 0 ? "better" : "same" };
}

const rate = (n: number, of: number) => (of === 0 ? undefined : (n / of) * 100);

export function compareSummaries(base: Side, cand: Side, opts: { minN?: number } = {}): Comparison {
  const minN = opts.minN ?? 20;
  const b = new Map(base.routes.map((r) => [r.role, r]));
  const c = new Map(cand.routes.map((r) => [r.role, r]));
  const roles = [...new Set([...cand.routes.map((r) => r.role), ...base.routes.map((r) => r.role)])];
  const rows = roles.map((role): CompareRow => {
    const x = b.get(role);
    const y = c.get(role);
    const state: CompareState = x === undefined ? "new" : y === undefined ? "gone" : x.calls < minN || y.calls < minN ? "thin" : "both";
    const show = state === "both";
    return {
      role,
      state,
      baseCalls: x?.calls,
      candCalls: y?.calls,
      errorRate: cell(x && rate(x.errors, x.calls), y && rate(y.errors, y.calls), show),
      fallbackRate: cell(x && rate(x.fallbacks, x.calls), y && rate(y.fallbacks, y.calls), show),
      p50: cell(x?.durationP50, y?.durationP50, show),
      p95: cell(x?.durationP95, y?.durationP95, show),
      firstText: cell(x?.firstTextP50, y?.firstTextP50, show),
    };
  });
  const whole = base.calls >= minN && cand.calls >= minN;
  return {
    rows,
    total: { errorRate: cell(rate(base.errors, base.calls), rate(cand.errors, cand.calls), whole), fallbackRate: cell(rate(base.fallbacks, base.calls), rate(cand.fallbacks, cand.calls), whole) },
    cost: { base: base.cost, cand: cand.cost, delta: cand.cost - base.cost, lowerBound: base.uncosted > 0 || cand.uncosted > 0 },
  };
}

const sign = (x: number) => (x > 0 ? "+" : x < 0 ? "−" : "±");

/** A latency change: `+400 ms`, `−1.2 s`, `±0 ms`; `–` where there is no delta. */
export function signedMs(d: number | undefined): string {
  if (d === undefined) return "–";
  const a = Math.abs(d);
  return `${sign(d)}${a >= 10_000 ? `${(a / 1000).toFixed(1)} s` : `${Math.round(a)} ms`}`;
}

/** A rate change in percentage points: `+8.0 pp`; `–` where there is no delta. */
export function signedPp(d: number | undefined): string {
  if (d === undefined) return "–";
  return `${sign(d)}${Math.abs(d).toFixed(1)} pp`;
}
