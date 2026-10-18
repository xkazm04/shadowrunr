// Query cache over the RPC client. Entries refetch when an event or mutation invalidates their
// method (see invalidation.ts); a slow backstop timer refreshes entries that are still watched
// and older than `backstopMs`, in case an event was missed. No per-tick work otherwise.
import type { DataError } from "./errors";
import { invalidatedByEvent } from "./invalidation";
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
}

export const DEFAULT_BACKSTOP_MS = 5 * 60_000;

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
  private backstop: number | null = null;
  /** Counts network calls; budgets and tests read it. */
  fetches = 0;

  constructor(client: RpcClient, opts: QueryCacheOptions = {}) {
    this.client = client;
    this.now = opts.now ?? Date.now;
    this.backstopMs = opts.backstopMs ?? DEFAULT_BACKSTOP_MS;
    this.timers = opts.timers;
    this.maxIdle = opts.maxIdleEntries ?? 256;
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
    if (methods.length === 0) return;
    for (const [key, e] of this.entries) {
      if (!methods.includes(e.method)) continue;
      e.stale = true;
      if (e.listeners.size > 0) void this.fetch(key, e);
    }
  }

  /** Refetches watched entries whose last call failed (the daemon came back, say). */
  retryFailed(): void {
    for (const [key, e] of this.entries) {
      if (e.snapshot.status === "error" && e.listeners.size > 0 && !e.inflight) void this.fetch(key, e);
    }
  }

  onEvent(type: string): void {
    this.invalidate(invalidatedByEvent(type));
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
    this.backstop = null;
    for (const e of this.entries.values()) e.inflight?.abort();
    this.entries.clear();
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
