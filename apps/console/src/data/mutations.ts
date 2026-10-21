// The data layer's record of mutations, so a long call outlives the button that started it.
// In flight: one entry per key (`queryKey(method, params)`); a second call for the same key joins
// the first flight instead of sending again (the guard is per key, never per method, and holds
// only while the call is in flight: a settled call answers no new caller). Settled: a bounded
// ring of outcomes, so an error that lands while no view is mounted is still read once.
// Client-only: the daemon has no reconcile.run guard, so two console tabs are not protected.
import { queryKey } from "./cache";
import type { DataError } from "./errors";
import type { Result } from "./result";

export interface InflightMutation {
  readonly key: string;
  readonly method: string;
  readonly params: unknown;
  /** When the call was sent (ms since the epoch). */
  readonly startedAt: number;
}

export interface SettledMutation {
  /** Unique per settle, for `ack`. */
  readonly id: number;
  readonly key: string;
  readonly method: string;
  readonly params: unknown;
  readonly result: Result<unknown, DataError>;
  readonly settledAt: number;
  readonly acked: boolean;
}

/** Settled outcomes kept; in-flight entries are never evicted. */
export const SETTLED_LIMIT = 32;

const NONE: readonly never[] = [];

export class MutationStore {
  private readonly flights = new Map<string, InflightMutation & { readonly promise: Promise<Result<unknown, DataError>> }>();
  private settledRing: SettledMutation[] = [];
  private nextId = 1;
  private readonly listeners = new Set<() => void>();
  /** Keys a mounted caller is awaiting: its outcome reaches that caller, so it settles acknowledged. */
  private readonly claims = new Map<string, number>();
  /** Snapshots per method, rebuilt only after a change, so `useSyncExternalStore` sees stable values. */
  private inflightCache = new Map<string, readonly InflightMutation[]>();
  private unackedCache = new Map<string, SettledMutation | undefined>();

  private readonly now: () => number;
  private readonly limit: number;

  constructor(now: () => number = Date.now, limit: number = SETTLED_LIMIT) {
    this.now = now;
    this.limit = limit;
  }

  /** Runs `call` unless the same key is already in flight, in which case it joins that flight. */
  run(method: string, params: unknown, call: () => Promise<Result<unknown, DataError>>): Promise<Result<unknown, DataError>> {
    const key = queryKey(method, params);
    const joined = this.flights.get(key);
    if (joined) return joined.promise;
    const promise = call().then(
      (result) => {
        this.settle(key, method, params, result);
        return result;
      },
      (e: unknown) => {
        const result: Result<unknown, DataError> = { ok: false, error: { code: "internal", message: e instanceof Error ? e.message : String(e) } };
        this.settle(key, method, params, result);
        return result;
      },
    );
    this.flights.set(key, { key, method, params, startedAt: this.now(), promise });
    this.changed();
    return promise;
  }

  /** The method's calls in flight, oldest first. */
  inflight(method: string): readonly InflightMutation[] {
    let snap = this.inflightCache.get(method);
    if (snap === undefined) {
      const list: InflightMutation[] = [];
      for (const f of this.flights.values()) {
        if (f.method === method) list.push({ key: f.key, method: f.method, params: f.params, startedAt: f.startedAt });
      }
      snap = list.length === 0 ? NONE : list;
      this.inflightCache.set(method, snap);
    }
    return snap;
  }

  /** The newest outcome of the method nobody has acknowledged yet. */
  unacked(method: string): SettledMutation | undefined {
    if (this.unackedCache.has(method)) return this.unackedCache.get(method);
    let found: SettledMutation | undefined;
    for (let i = this.settledRing.length - 1; i >= 0; i--) {
      const s = this.settledRing[i];
      if (s && s.method === method && !s.acked) {
        found = s;
        break;
      }
    }
    this.unackedCache.set(method, found);
    return found;
  }

  /** Every settled outcome kept, oldest first. */
  settled(): readonly SettledMutation[] {
    return this.settledRing;
  }

  /** Marks one outcome read. */
  ack(id: number): void {
    const i = this.settledRing.findIndex((s) => s.id === id && !s.acked);
    if (i < 0) return;
    const s = this.settledRing[i] as SettledMutation;
    this.settledRing = this.settledRing.map((x, j) => (j === i ? { ...s, acked: true } : x));
    this.changed();
  }

  /**
   * A mounted caller awaits `key`: if it is still holding the claim when the call settles, the
   * outcome is recorded acknowledged (the caller handles it). Returns the release.
   */
  claim(key: string): () => void {
    this.claims.set(key, (this.claims.get(key) ?? 0) + 1);
    let held = true;
    return () => {
      if (!held) return;
      held = false;
      const n = (this.claims.get(key) ?? 1) - 1;
      if (n <= 0) this.claims.delete(key);
      else this.claims.set(key, n);
    };
  }

  subscribe = (l: () => void): (() => void) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };

  private settle(key: string, method: string, params: unknown, result: Result<unknown, DataError>) {
    this.flights.delete(key);
    const next = [...this.settledRing, { id: this.nextId++, key, method, params, result, settledAt: this.now(), acked: this.claims.has(key) }];
    this.settledRing = next.length > this.limit ? next.slice(next.length - this.limit) : next;
    this.changed();
  }

  private changed() {
    this.inflightCache = new Map();
    this.unackedCache = new Map();
    for (const l of this.listeners) l();
  }
}
