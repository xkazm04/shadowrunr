// The engine floor's tables, derived from ledger rows (A/1 engineFloor, app.js:580-591). The
// daemon has no RPC for the configured routes or live account cooldowns yet (`engine.status` is
// proposed), so these tables say what the ledger shows the engine did.
import type { LedgerEntry } from "@shadowrunr/contract";
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
  readonly cost: number;
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
  let cacheRead = 0;
  let unrouted = 0;
  const roles = new Map<string, { calls: number; errors: number; fallbacks: number; tiers: Set<string>; efforts: Set<string>; engines: Set<string>; models: Set<string>; dur: number[]; first: number[] }>();
  const accounts = new Map<string, { engine: string; account: string; calls: number; errors: number; models: Map<string, number>; lastTs: number; lastError: string | undefined }>();
  const runStart = new Map<string, number>();
  for (const l of rows) {
    const failed = l.error_kind !== undefined;
    if (failed) errors++;
    if (l.fallback_reason !== undefined) fallbacks++;
    cost += l.cost_usd ?? 0;
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
  return { calls: rows.length, errors, fallbacks, cost, cacheRead, unrouted, routes, accounts: accountRows, runStart };
}
