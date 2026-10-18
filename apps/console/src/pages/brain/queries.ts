// Multi-key reads over the query cache: a cursor chain (`usePagedRpc`) and a fixed set of calls
// (`useRpcEach`). Each key goes through `QueryCache.watch`, so caching, invalidation, retry and
// the backstop behave exactly as for `useRpc`.
//
// Proposal for the Director: promote this file to `data/` (Simulator and Reconcile page too).
import { useMemo, useSyncExternalStore } from "react";
import type { RpcParams, RpcResult } from "@shadowrunr/contract";
import { queryKey, useDataLayer, viewOfError, type DataError, type ErrorView, type QueryCache } from "../../data";

export type PagedMethod = "runs.page" | "tournaments.page" | "ledger.page";
export type ItemOf<M extends PagedMethod> = RpcResult<M>["items"][number];
export type BaseParams<M extends PagedMethod> = Omit<RpcParams<M>, "cursor">;

interface PageLike {
  readonly items: readonly unknown[];
  readonly next_cursor?: string;
}

function isPage(x: unknown): x is PageLike {
  return typeof x === "object" && x !== null && Array.isArray((x as { items?: unknown }).items);
}

export interface MultiSnapshot<T> {
  readonly data: T;
  /** True once every key has answered at least once. */
  readonly ready: boolean;
  readonly error: DataError | undefined;
  readonly refreshing: boolean;
}

/** Watches a set of cache keys and folds their snapshots into one stable snapshot. */
abstract class KeySet<T> {
  protected readonly links: { key: string; unsub: () => void }[] = [];
  private readonly listeners = new Set<() => void>();
  private busy = false;
  private dirty = false;
  private datas: unknown[] = [];
  protected snap: MultiSnapshot<T>;
  protected readonly cache: QueryCache;
  protected readonly method: string;
  private readonly params = new Map<string, unknown>();

  constructor(cache: QueryCache, method: string, empty: T) {
    this.cache = cache;
    this.method = method;
    this.snap = { data: empty, ready: false, error: undefined, refreshing: false };
  }

  readonly subscribe = (l: () => void): (() => void) => {
    this.listeners.add(l);
    if (this.listeners.size === 1) this.recompute();
    return () => {
      this.listeners.delete(l);
      if (this.listeners.size === 0) this.truncate(0);
    };
  };

  readonly getSnapshot = (): MultiSnapshot<T> => this.snap;

  readonly reload = (): void => {
    for (const l of this.links) {
      const e = this.cache.getSnapshot(l.key);
      if (e.status === "error" || e.data !== undefined) this.refetchKey(l.key);
    }
  };

  protected ensure(i: number, params: unknown): void {
    const key = queryKey(this.method, params);
    if (this.links[i]?.key === key) return;
    this.truncate(i);
    this.params.set(key, params);
    this.links[i] = { key, unsub: this.cache.watch(this.method, params, this.onChange) };
  }

  protected truncate(i: number): void {
    for (const l of this.links.splice(i)) l.unsub();
  }

  private refetchKey(key: string) {
    this.cache.refetch(this.method, this.params.get(key));
  }

  private readonly onChange = () => {
    if (this.listeners.size === 0) return;
    this.recompute();
  };

  /** Walks the links; returns the folded data and whether every link answered. */
  protected abstract walk(): { data: T; datas: unknown[]; ready: boolean };

  private recompute() {
    if (this.busy) {
      this.dirty = true;
      return;
    }
    this.busy = true;
    let out: ReturnType<KeySet<T>["walk"]>;
    try {
      do {
        this.dirty = false;
        out = this.walk();
      } while (this.dirty);
    } finally {
      this.busy = false;
    }
    let error: DataError | undefined;
    let refreshing = false;
    for (const l of this.links) {
      const s = this.cache.getSnapshot(l.key);
      error ??= s.error;
      refreshing ||= s.refreshing;
    }
    const same =
      out.datas.length === this.datas.length &&
      out.datas.every((d, i) => d === this.datas[i]) &&
      error === this.snap.error &&
      refreshing === this.snap.refreshing &&
      out.ready === this.snap.ready;
    if (same) return;
    this.datas = out.datas;
    this.snap = { data: out.data, ready: out.ready, error, refreshing };
    for (const l of this.listeners) l();
  }
}

export interface Chain<T> {
  readonly items: readonly T[];
  readonly pages: number;
  /** The daemon has more pages than this chain may read. */
  readonly more: boolean;
}

class PageChain extends KeySet<Chain<unknown>> {
  private readonly base: object;
  private readonly maxPages: number;

  constructor(cache: QueryCache, method: string, base: object, maxPages: number) {
    super(cache, method, { items: [], pages: 0, more: false });
    this.base = base;
    this.maxPages = maxPages;
  }

  protected walk() {
    if (this.links.length === 0) this.ensure(0, this.base);
    const items: unknown[] = [];
    const datas: unknown[] = [];
    let more = false;
    let ready = false;
    for (let i = 0; i < this.links.length; i++) {
      const link = this.links[i];
      const d = link ? this.cache.getSnapshot(link.key).data : undefined;
      if (!isPage(d)) break;
      datas.push(d);
      items.push(...d.items);
      if (d.next_cursor === undefined) {
        this.truncate(i + 1);
        ready = true;
      } else if (i + 1 < this.maxPages) {
        this.ensure(i + 1, { ...this.base, cursor: d.next_cursor });
      } else {
        more = true;
        ready = true;
      }
    }
    return { data: { items, pages: datas.length, more }, datas, ready };
  }
}

class EachSet extends KeySet<readonly unknown[]> {
  private readonly list: readonly unknown[];

  constructor(cache: QueryCache, method: string, list: readonly unknown[]) {
    super(cache, method, []);
    this.list = list;
  }

  protected walk() {
    this.list.forEach((p, i) => this.ensure(i, p));
    const datas = this.links.map((l) => this.cache.getSnapshot(l.key).data);
    return { data: datas, datas, ready: datas.every((d) => d !== undefined) };
  }
}

export interface MultiQuery<T> extends MultiSnapshot<T> {
  readonly errorView: ErrorView | undefined;
  readonly reload: () => void;
}

function useKeySet<T>(store: KeySet<T> | null, empty: T): MultiQuery<T> {
  const idle = useMemo<MultiSnapshot<T>>(() => ({ data: empty, ready: false, error: undefined, refreshing: false }), [empty]);
  const subscribe = useMemo(() => store?.subscribe ?? (() => () => undefined), [store]);
  const get = useMemo(() => store?.getSnapshot ?? (() => idle), [store, idle]);
  const snap = useSyncExternalStore(subscribe, get, get);
  return useMemo(
    () => ({ ...snap, errorView: snap.error ? viewOfError(snap.error) : undefined, reload: store?.reload ?? (() => undefined) }),
    [snap, store],
  );
}

const EMPTY_CHAIN: Chain<never> = { items: [], pages: 0, more: false };
const EMPTY_LIST: readonly never[] = [];

/** Follows `next_cursor` for up to `maxPages` pages; `params === null` skips. */
export function usePagedRpc<M extends PagedMethod>(method: M, params: BaseParams<M> | null, maxPages: number): MultiQuery<Chain<ItemOf<M>>> {
  const { cache } = useDataLayer();
  const key = params === null ? null : queryKey(method, params);
  const store = useMemo(
    () => (params === null ? null : new PageChain(cache, method, params, maxPages)),
    // `key` captures params by value.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [cache, method, key, maxPages],
  );
  // The door validated every page against this method's schema, so the items are ItemOf<M>.
  return useKeySet(store, EMPTY_CHAIN) as MultiQuery<Chain<ItemOf<M>>>;
}

/** One call per params entry, in order; `data[i]` is undefined until call i answers. */
export function useRpcEach<M extends "run.transcript" | "run.verdict">(method: M, list: readonly RpcParams<M>[]): MultiQuery<readonly (RpcResult<M> | undefined)[]> {
  const { cache } = useDataLayer();
  const key = list.map((p) => queryKey(method, p)).join("\n");
  const store = useMemo(
    () => (list.length === 0 ? null : new EachSet(cache, method, list)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [cache, method, key],
  );
  return useKeySet(store, EMPTY_LIST) as MultiQuery<readonly (RpcResult<M> | undefined)[]>;
}
