// Multi-key reads over the query cache: cursor chains (`usePaged`, `usePagedMany`) and a fixed
// set of calls (`useRpcMany`). Every key goes through `QueryCache.watch`, so caching,
// invalidation, retry and the backstop behave exactly as for `useRpc`, and the folded snapshot
// keeps its identity until one of its keys changes (no per-render churn).
import { useMemo, useSyncExternalStore } from "react";
import type { RpcMethod, RpcParams, RpcResult } from "@shadowrunr/contract";
import { queryKey, type QueryCache, type QueryStatus } from "./cache";
import { viewOfError, type DataError, type ErrorView } from "./errors";
import { useDataLayer, type Query } from "./react";

export type PagedMethod = "runs.page" | "tournaments.page" | "ledger.page" | "reconcile.incidents";
export type ItemOf<M extends PagedMethod> = RpcResult<M>["items"][number];
export type BaseParams<M extends PagedMethod> = Omit<RpcParams<M>, "cursor">;

/** The default page cap of a chain: effectively "every page" (a cursor loop guard). */
export const MAX_PAGES = 10_000;

interface PageLike {
  readonly items: readonly unknown[];
  readonly next_cursor?: string;
}

function isPage(x: unknown): x is PageLike {
  return typeof x === "object" && x !== null && "items" in x && Array.isArray(x.items);
}

interface Fold<T> {
  readonly data: T;
  /** Every key answered at least once. */
  readonly ready: boolean;
  readonly error: DataError | undefined;
  readonly refreshing: boolean;
}

type Listener = () => void;

/** Watches a list of cache keys and folds their snapshots into one stable snapshot. */
abstract class KeySet<T> {
  protected readonly links: { key: string; params: unknown; unsub: () => void }[] = [];
  private readonly listeners = new Set<Listener>();
  private busy = false;
  private dirty = false;
  private datas: unknown[] = [];
  protected snap: Fold<T>;
  protected readonly cache: QueryCache;
  protected readonly method: string;

  constructor(cache: QueryCache, method: string, empty: T) {
    this.cache = cache;
    this.method = method;
    this.snap = { data: empty, ready: false, error: undefined, refreshing: false };
  }

  readonly subscribe = (l: Listener): (() => void) => {
    this.listeners.add(l);
    if (this.listeners.size === 1) this.recompute();
    return () => {
      this.listeners.delete(l);
      if (this.listeners.size === 0) this.truncate(0);
    };
  };

  readonly getSnapshot = (): Fold<T> => this.snap;

  readonly reload = (): void => {
    for (const l of this.links) {
      const s = this.cache.getSnapshot(l.key);
      if (s.status === "error" || s.data !== undefined) this.cache.refetch(this.method, l.params);
    }
  };

  protected ensure(i: number, params: unknown): void {
    const key = queryKey(this.method, params);
    if (this.links[i]?.key === key) return;
    this.truncate(i);
    this.links[i] = { key, params, unsub: this.cache.watch(this.method, params, this.onChange) };
  }

  protected truncate(i: number): void {
    for (const l of this.links.splice(i)) l.unsub();
  }

  private readonly onChange = () => {
    if (this.listeners.size > 0) this.recompute();
  };

  /** Walks the links; returns the folded data, the raw replies and whether all answered. */
  protected abstract walk(): { data: T; datas: unknown[]; ready: boolean };

  private recompute() {
    // `watch` may answer synchronously from the cache while a walk is extending the chain.
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

interface ChainData {
  readonly items: readonly unknown[];
  readonly pages: number;
  readonly more: boolean;
}

const EMPTY_CHAIN: ChainData = { items: [], pages: 0, more: false };

/** Follows `next_cursor` from `base` for up to `maxPages` pages. */
class PageChain extends KeySet<ChainData> {
  private readonly base: object;
  private readonly maxPages: number;

  constructor(cache: QueryCache, method: string, base: object, maxPages: number) {
    super(cache, method, EMPTY_CHAIN);
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

/** One call per params entry, in order. */
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

/** Several stores read as one: the snapshot is their snapshots, stable until one changes. */
class Group<T> {
  private readonly stores: readonly KeySet<T>[];
  private snap: readonly Fold<T>[];

  constructor(stores: readonly KeySet<T>[]) {
    this.stores = stores;
    this.snap = stores.map((s) => s.getSnapshot());
  }

  readonly subscribe = (l: Listener): (() => void) => {
    const offs = this.stores.map((s) => s.subscribe(l));
    return () => offs.forEach((off) => off());
  };

  readonly getSnapshot = (): readonly Fold<T>[] => {
    const next = this.stores.map((s) => s.getSnapshot());
    if (next.length !== this.snap.length || next.some((s, i) => s !== this.snap[i])) this.snap = next;
    return this.snap;
  };
}

/** A cursor chain as a page reads it. */
export interface Paged<T> {
  /** For `QueryView`: `data` is every item loaded so far, once the first page answered. */
  readonly query: Query<readonly T[]>;
  readonly pages: number;
  /** Every page up to the cap has answered. */
  readonly done: boolean;
  /** The cap stopped the chain while the daemon had more pages. */
  readonly more: boolean;
}

function statusOf(f: Fold<unknown>, pages: number, idle: boolean): QueryStatus {
  if (idle) return "idle";
  if (pages > 0) return "ready";
  return f.error ? "error" : "loading";
}

function pagedOf<T>(f: Fold<ChainData>, reload: () => void, idle = false): Paged<T> {
  const { items, pages, more } = f.data;
  return {
    query: {
      status: statusOf(f, pages, idle),
      // The door validated every page against this method's schema, so the items are T.
      data: pages > 0 ? (items as readonly T[]) : undefined,
      error: f.error,
      errorView: f.error ? viewOfError(f.error) : undefined,
      refreshing: f.refreshing,
      reload,
    },
    pages,
    done: f.ready,
    more,
  };
}

const IDLE_FOLD: Fold<ChainData> = { data: EMPTY_CHAIN, ready: false, error: undefined, refreshing: false };
const NO_FOLDS: readonly Fold<ChainData>[] = [];
const NO_GROUP = { subscribe: () => () => undefined, getSnapshot: () => NO_FOLDS };

/** Follows several cursor chains at once (one per base); each stops after `maxPages` pages. */
export function usePagedMany<M extends PagedMethod>(method: M, bases: readonly BaseParams<M>[], maxPages = MAX_PAGES): readonly Paged<ItemOf<M>>[] {
  const { cache } = useDataLayer();
  const sig = bases.map((b) => queryKey(method, b)).join("\n");
  const chains = useMemo(
    () => bases.map((b) => new PageChain(cache, method, b, maxPages)),
    // `sig` carries `bases` by value: a new array with the same queries keeps the chains.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [cache, method, sig, maxPages],
  );
  const group = useMemo(() => (chains.length === 0 ? NO_GROUP : new Group(chains)), [chains]);
  const snaps = useSyncExternalStore(group.subscribe, group.getSnapshot, group.getSnapshot);
  return useMemo(() => chains.map((c, i) => pagedOf<ItemOf<M>>(snaps[i] ?? IDLE_FOLD, c.reload)), [chains, snaps]);
}

/** Follows one cursor chain for up to `maxPages` pages; `base === null` reads nothing. */
export function usePaged<M extends PagedMethod>(method: M, base: BaseParams<M> | null, maxPages = MAX_PAGES): Paged<ItemOf<M>> {
  const key = base === null ? "" : queryKey(method, base);
  // `key` carries `base` by value.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const bases = useMemo(() => (base === null ? [] : [base]), [key]);
  const out = usePagedMany(method, bases, maxPages);
  return out[0] ?? IDLE_PAGED;
}

const IDLE_PAGED: Paged<never> = pagedOf<never>(IDLE_FOLD, () => undefined, true);

/** A fixed set of calls read as one. */
export interface Many<T> {
  /** `data[i]` is undefined until call i answers. */
  readonly data: readonly (T | undefined)[];
  /** Every call answered at least once. */
  readonly ready: boolean;
  readonly error: DataError | undefined;
  readonly errorView: ErrorView | undefined;
  readonly refreshing: boolean;
  readonly reload: () => void;
}

const EMPTY_LIST: readonly never[] = [];
const IDLE_MANY: Fold<readonly unknown[]> = { data: EMPTY_LIST, ready: false, error: undefined, refreshing: false };

/** One call of `method` per params entry, in order; an empty list reads nothing. */
export function useRpcMany<M extends RpcMethod>(method: M, list: readonly RpcParams<M>[]): Many<RpcResult<M>> {
  const { cache } = useDataLayer();
  const sig = list.map((p) => queryKey(method, p)).join("\n");
  const store = useMemo(
    () => (list.length === 0 ? null : new EachSet(cache, method, list)),
    // `sig` carries `list` by value.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [cache, method, sig],
  );
  const subscribe = useMemo(() => store?.subscribe ?? (() => () => undefined), [store]);
  const get = useMemo(() => store?.getSnapshot ?? (() => IDLE_MANY), [store]);
  const snap = useSyncExternalStore(subscribe, get, get);
  return useMemo(
    () => ({
      // The door validated each reply against this method's schema.
      data: snap.data as readonly (RpcResult<M> | undefined)[],
      ready: snap.ready,
      error: snap.error,
      errorView: snap.error ? viewOfError(snap.error) : undefined,
      refreshing: snap.refreshing,
      reload: store?.reload ?? (() => undefined),
    }),
    [snap, store],
  );
}
