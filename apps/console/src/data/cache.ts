// Query cache over the RPC client. Entries refetch when an event or mutation invalidates them
// (see invalidation.ts: a rule names a method and may scope it to some params); an event's
// refetches wait out a short window so a burst costs one fetch per key. A slow backstop timer
// refreshes entries that are still watched and older than `backstopMs`, in case an event was
// missed. No per-tick work otherwise.
import type { RpcMethod } from "@shadowrunr/contract";
import type { DataError } from "./errors";
import { eventRules, type CachedReply, type Rule, type ScopeContext } from "./invalidation";
import type { RpcClient } from "./rpc";
import type { Timers } from "./socket";

export type QueryStatus = "idle" | "loading" | "ready" | "error";

export interface QuerySnapshot {
  readonly status: QueryStatus;
  /** The last good reply; kept while refetching and after a later error. */
  readonly data: unknown;
  readonly error: DataError | undefined;
  readonly updatedAt: number | undefined;
  readonly refreshing: boolean;
}

export const IDLE: QuerySnapshot = { status: "idle", data: undefined, error: undefined, updatedAt: undefined, refreshing: false };

interface Entry {
  readonly method: string;
  readonly params: unknown;
  snapshot: QuerySnapshot;
  listeners: Set<() => void>;
  inflight: AbortController | null;
  stale: boolean;
}

export interface QueryCacheOptions {
  readonly backstopMs?: number;
  readonly now?: () => number;
  readonly timers?: Timers;
  /** Unwatched entries kept for instant re-mounts; the oldest beyond this are dropped. */
  readonly maxIdleEntries?: number;
  /** How long an event's refetches wait for the rest of its burst (needs `timers`). */
  readonly coalesceMs?: number;
}

/** What a pushed event tells invalidation: its type and, when enveloped, its run. */
export interface EventScope {
  readonly type: string;
  readonly runId?: string;
}

export const DEFAULT_BACKSTOP_MS = 5 * 60_000;
export const DEFAULT_COALESCE_MS = 250;

/** Deterministic key: object keys sorted, so `{a,b}` and `{b,a}` share an entry. */
export function queryKey(method: string, params: unknown): string {
  return `${method} ${stableStringify(params ?? {})}`;
}

function stableStringify(v: unknown): string {
  if (Array.isArray(v)) return `[${v.map(stableStringify).join(",")}]`;
  if (typeof v === "object" && v !== null) {
    const rec = v as Record<string, unknown>;
    return `{${Object.keys(rec)
      .sort()
      .filter((k) => rec[k] !== undefined)
      .map((k) => `${JSON.stringify(k)}:${stableStringify(rec[k])}`)
      .join(",")}}`;
  }
  return JSON.stringify(v) ?? "null";
}

export class QueryCache {
  private readonly entries = new Map<string, Entry>();
  private readonly client: RpcClient;
  private readonly now: () => number;
  private readonly backstopMs: number;
  private readonly timers: Timers | undefined;
  private readonly maxIdle: number;
  private readonly coalesceMs: number;
  private backstop: number | null = null;
  /** Keys an event invalidated, fetched when the coalescing window closes. */
  private readonly due = new Set<string>();
  private window: number | null = null;
  /** Counts network calls; budgets and tests read it. */
  fetches = 0;

  constructor(client: RpcClient, opts: QueryCacheOptions = {}) {
    this.client = client;
    this.now = opts.now ?? Date.now;
    this.backstopMs = opts.backstopMs ?? DEFAULT_BACKSTOP_MS;
    this.timers = opts.timers;
    this.maxIdle = opts.maxIdleEntries ?? 256;
    this.coalesceMs = opts.coalesceMs ?? DEFAULT_COALESCE_MS;
  }

  getSnapshot(key: string): QuerySnapshot {
    return this.entries.get(key)?.snapshot ?? IDLE;
  }

  /** Watch a query; fetches when there is no reply yet or the reply is stale. */
  watch(method: string, params: unknown, listener: () => void): () => void {
    const key = queryKey(method, params);
    let e = this.entries.get(key);
    if (!e) {
      e = { method, params, snapshot: IDLE, listeners: new Set(), inflight: null, stale: true };
      this.entries.set(key, e);
    }
    e.listeners.add(listener);
    if (e.stale && !e.inflight) void this.fetch(key, e);
    const entry = e;
    return () => {
      entry.listeners.delete(listener);
      if (entry.listeners.size === 0) this.evictIdle();
    };
  }

  /** Marks every entry of these methods stale and refetches the watched ones. */
  invalidate(methods: readonly string[]): void {
    this.invalidateWhere(methods as readonly RpcMethod[], {}, false);
  }

  /**
   * Marks stale every entry a rule holds (its method, and its scope unless the scope proves the
   * entry unchanged) and refetches the watched ones: now, or once `coalesce`'s window closes.
   */
  invalidateWhere(rules: readonly Rule[], ctx: Omit<ScopeContext, "cached">, coalesce: boolean): void {
    if (rules.length === 0) return;
    const full: ScopeContext = { ...ctx, cached: (m) => this.cached(m) };
    const hit: [string, Entry][] = [];
    for (const rule of rules) {
      const method = typeof rule === "string" ? rule : rule.method;
      for (const [key, e] of this.entries) {
        if (e.method === method && (typeof rule === "string" || rule.scope(e.params, full))) hit.push([key, e]);
      }
    }
    // Every scope read the cache as the event found it; only now does anything turn stale.
    for (const [key, e] of hit) {
      e.stale = true;
      if (e.listeners.size === 0) continue;
      if (coalesce && this.timers && this.coalesceMs > 0) this.schedule(key);
      else void this.fetch(key, e);
    }
  }

  /** Refetches watched entries whose last call failed (the daemon came back, say). */
  retryFailed(): void {
    for (const [key, e] of this.entries) {
      if (e.snapshot.status === "error" && e.listeners.size > 0 && !e.inflight) void this.fetch(key, e);
    }
  }

  /**
   * Refetches every watched entry, ready or failed: after the live feed was down, any reply on
   * screen may have missed the events that would have invalidated it. Unwatched entries wait
   * until they are watched again (they are marked stale so a re-mount fetches).
   */
  revalidateWatched(): void {
    for (const [key, e] of this.entries) {
      if (e.listeners.size === 0) e.stale = true;
      else if (!e.inflight) void this.fetch(key, e);
    }
  }

  onEvent(event: EventScope): void {
    this.invalidateWhere(eventRules(event.type), event.runId === undefined ? {} : { runId: event.runId }, true);
  }

  refetch(method: string, params: unknown): void {
    const key = queryKey(method, params);
    const e = this.entries.get(key);
    if (e) void this.fetch(key, e);
  }

  startBackstop(): void {
    if (!this.timers || this.backstop !== null) return;
    const timers = this.timers;
    const tick = () => {
      const cutoff = this.now() - this.backstopMs;
      for (const [key, e] of this.entries) {
        if (e.listeners.size > 0 && !e.inflight && (e.snapshot.updatedAt ?? 0) <= cutoff) void this.fetch(key, e);
      }
      this.backstop = timers.setTimeout(tick, this.backstopMs);
    };
    this.backstop = timers.setTimeout(tick, this.backstopMs);
  }

  dispose(): void {
    if (this.timers && this.backstop !== null) this.timers.clearTimeout(this.backstop);
    if (this.timers && this.window !== null) this.timers.clearTimeout(this.window);
    this.backstop = null;
    this.window = null;
    this.due.clear();
    for (const e of this.entries.values()) e.inflight?.abort();
    this.entries.clear();
  }

  private *cached(method: RpcMethod): Iterable<CachedReply> {
    for (const e of this.entries.values()) {
      if (e.method !== method) continue;
      const fresh = e.snapshot.status === "ready" && !e.stale && !e.inflight;
      yield { params: e.params, data: e.snapshot.data, fresh };
    }
  }

  private schedule(key: string): void {
    this.due.add(key);
    const timers = this.timers;
    if (!timers || this.window !== null) return;
    this.window = timers.setTimeout(() => {
      this.window = null;
      const keys = [...this.due];
      this.due.clear();
      for (const k of keys) {
        const e = this.entries.get(k);
        // A watch or a mutation may have fetched it since; that reply is newer than the event.
        if (e?.stale && e.listeners.size > 0) void this.fetch(k, e);
      }
    }, this.coalesceMs);
  }

  private async fetch(key: string, e: Entry): Promise<void> {
    e.inflight?.abort();
    const ctrl = new AbortController();
    e.inflight = ctrl;
    e.stale = false;
    this.set(e, { ...e.snapshot, status: e.snapshot.status === "ready" ? "ready" : "loading", refreshing: true });
    this.fetches += 1;
    const r = await this.client.call(e.method, e.params, { signal: ctrl.signal });
    if (ctrl.signal.aborted || this.entries.get(key) !== e) return;
    e.inflight = null;
    if (r.ok) {
      // An identical reply keeps the old object, so memos and canvases downstream do not redraw.
      const data = e.snapshot.data !== undefined && sameValue(e.snapshot.data, r.value) ? e.snapshot.data : r.value;
      this.set(e, { status: "ready", data, error: undefined, updatedAt: this.now(), refreshing: false });
    } else {
      e.stale = true;
      this.set(e, { ...e.snapshot, status: "error", error: r.error, refreshing: false });
    }
  }

  private set(e: Entry, s: QuerySnapshot) {
    e.snapshot = s;
    for (const l of e.listeners) l();
  }

  private evictIdle() {
    const idle = [...this.entries].filter(([, e]) => e.listeners.size === 0 && !e.inflight);
    const excess = idle.length - this.maxIdle;
    for (let i = 0; i < excess; i++) {
      const item = idle[i];
      if (item) this.entries.delete(item[0]);
    }
  }
}

/** Structural equality over JSON-shaped values (what the contract door hands back). */
export function sameValue(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (Array.isArray(a)) {
    if (!Array.isArray(b) || a.length !== b.length) return false;
    for (let i = 0; i < a.length; i += 1) if (!sameValue(a[i], b[i])) return false;
    return true;
  }
  if (Array.isArray(b)) return false;
  const ka = Object.keys(a);
  if (ka.length !== Object.keys(b).length) return false;
  const ra = a as Record<string, unknown>;
  const rb = b as Record<string, unknown>;
  for (const k of ka) if (!Object.hasOwn(rb, k) || !sameValue(ra[k], rb[k])) return false;
  return true;
}
