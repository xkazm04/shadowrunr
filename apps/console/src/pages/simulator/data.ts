// The simulator's data hooks over the shared cache. The grid needs many queries at once (every
// tournament's runs, paged, and its results), which `useRpc` reads one at a time, so this file
// watches a list of queries through one subscription. Proposed for `data/` (P5 report) as
// `useRpcMany` and `usePaged`; it uses only the data layer's public surface.
import { useCallback, useMemo, useRef, useSyncExternalStore } from "react";
import type { RpcMethod, RpcParams, RpcResult, TournamentResults } from "@shadowrunr/contract";
import { IDLE, queryKey, useDataLayer, viewOfError, type Query, type QueryCache, type QuerySnapshot } from "../../data";
import { buildTile, tilesInView, type Tile } from "./grid";
import { readResults, type SimResults } from "./results";

/** The daemon caps a page at 500 rows. */
export const PAGE_LIMIT = 500;

const sameList = (a: readonly QuerySnapshot[], b: readonly QuerySnapshot[]) => a.length === b.length && a.every((s, i) => s === b[i]);

/** Watches every query in `list` (one subscription) and returns their snapshots in order. */
export function useSnapshots<M extends RpcMethod>(method: M, list: readonly RpcParams<M>[]): readonly QuerySnapshot[] {
  const layer = useDataLayer();
  const keys = list.map((p) => queryKey(method, p));
  const sig = keys.join("\n");
  const last = useRef<readonly QuerySnapshot[]>([]);
  const subscribe = useCallback(
    (l: () => void) => {
      const offs = list.map((p) => layer.cache.watch(method, p, l));
      return () => offs.forEach((off) => off());
    },
    // `sig` carries `list` by value: a new array with the same queries does not resubscribe.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [layer, method, sig],
  );
  const get = useCallback(() => {
    const next = keys.map((k) => layer.cache.getSnapshot(k));
    if (sameList(last.current, next)) return last.current;
    last.current = next;
    return next;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [layer, sig]);
  return useSyncExternalStore(subscribe, get, get);
}

/** A snapshot as the `Query` that `QueryView` shows (loading, offline, degraded, data). */
export function asQuery<T>(snap: QuerySnapshot, data: T | undefined, reload: () => void): Query<T> {
  return {
    status: snap.status,
    data,
    error: snap.error,
    errorView: snap.error ? viewOfError(snap.error) : undefined,
    refreshing: snap.refreshing,
    reload,
  };
}

type PagedMethod = "runs.page" | "tournaments.page";
type ItemOf<M extends PagedMethod> = RpcResult<M>["items"][number];

/** The page params of a cursor chain, read from what the cache already holds. */
function chain<M extends PagedMethod>(cache: QueryCache, method: M, base: RpcParams<M>): RpcParams<M>[] {
  const out: RpcParams<M>[] = [base];
  for (;;) {
    const last = out[out.length - 1];
    const page = cache.getSnapshot(queryKey(method, last)).data as RpcResult<M> | undefined;
    const next = page?.next_cursor;
    if (next === undefined || out.length > 10_000) return out;
    out.push({ ...base, cursor: next } as RpcParams<M>);
  }
}

export interface Paged<T> {
  /** The first page's state, for `QueryView`; `data` is every item loaded so far. */
  readonly query: Query<readonly T[]>;
  /** Every page has arrived. */
  readonly done: boolean;
}

/** Follows several cursor chains at once; `bases` null or empty reads nothing. */
export function usePagedMany<M extends PagedMethod>(method: M, bases: readonly RpcParams<M>[]): readonly Paged<ItemOf<M>>[] {
  const { cache } = useDataLayer();
  const chains = bases.map((b) => chain(cache, method, b));
  const snaps = useSnapshots(method, chains.flat());
  return useMemo(() => {
    const groups: (readonly QuerySnapshot[])[] = [];
    let at = 0;
    for (const c of chains) {
      groups.push(snaps.slice(at, at + c.length));
      at += c.length;
    }
    return chains.map((c, i) => readChain(cache, method, c, groups[i] ?? []));
    // `snaps` changes whenever any page does; the chains are derived from the same cache.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snaps, cache, method]);
}

function readChain<M extends PagedMethod>(cache: QueryCache, method: M, c: readonly RpcParams<M>[], mine: readonly QuerySnapshot[]): Paged<ItemOf<M>> {
  const items: ItemOf<M>[] = [];
  let done = true;
  for (const s of mine) {
    const page = s.data as RpcResult<M> | undefined;
    if (page) items.push(...(page.items as ItemOf<M>[]));
    else done = false;
  }
  const lastPage = mine[mine.length - 1]?.data as RpcResult<M> | undefined;
  if (lastPage?.next_cursor !== undefined) done = false;
  const first = mine[0] ?? IDLE;
  const reload = () => {
    for (const p of c) cache.refetch(method, p);
  };
  return { query: asQuery<readonly ItemOf<M>[]>(first, first.data === undefined ? undefined : items, reload), done };
}

export function usePaged<M extends PagedMethod>(method: M, base: RpcParams<M> | null): Paged<ItemOf<M>> {
  const baseKey = base === null ? "" : queryKey(method, base);
  // `baseKey` carries `base` by value.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const bases = useMemo(() => (base === null ? [] : [base]), [baseKey]);
  const out = usePagedMany(method, bases);
  return out[0] ?? { query: asQuery<readonly ItemOf<M>[]>(IDLE, undefined, () => undefined), done: false };
}

/** Narrowed tournament results by id; ids whose reply did not fit map to their misfit path. */
export function useResults(ids: readonly string[]): ReadonlyMap<string, SimResults | { readonly misfit: string }> {
  const idsKey = ids.join("\n");
  // `idsKey` carries `ids` by value.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const list = useMemo(() => ids.map((tournament_id) => ({ tournament_id })), [idsKey]);
  const snaps = useSnapshots("tournament.results", list);
  return useMemo(() => {
    const out = new Map<string, SimResults | { readonly misfit: string }>();
    snaps.forEach((s, i) => {
      const id = ids[i];
      if (id === undefined || s.data === undefined) return;
      const r = readResults(s.data as TournamentResults);
      out.set(id, r.ok ? r.value : { misfit: r.path });
    });
    return out;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snaps]);
}

export const isResults = (r: SimResults | { readonly misfit: string } | undefined): r is SimResults => r !== undefined && !("misfit" in r);

export interface SimGrid {
  /** The tournaments page chain: drives the page states. */
  readonly tournaments: Paged<RpcResult<"tournaments.page">["items"][number]>;
  /** One tile per tournament in view (the latest 40). */
  readonly tiles: readonly Tile[];
  /** Every page and every result has arrived. */
  readonly complete: boolean;
}

/** Layer 1's data: the latest 40 tournaments, all their runs, and their results, aggregated. */
export function useSimGrid(): SimGrid {
  const tournaments = usePaged("tournaments.page", { limit: PAGE_LIMIT });
  const all = tournaments.query.data;
  const inView = useMemo(() => tilesInView(all ?? []), [all]);
  const ids = useMemo(() => inView.map((t) => t.tournament_id), [inView]);
  const runBases = useMemo(() => ids.map((tournament_id) => ({ tournament_id, limit: PAGE_LIMIT })), [ids]);
  const runs = usePagedMany("runs.page", runBases);
  const results = useResults(ids);
  const tiles = useMemo(
    () =>
      inView.map((t, i) => {
        const r = results.get(t.tournament_id);
        return buildTile(t.tournament_id, t.runs, runs[i]?.query.data ?? [], isResults(r) ? r : undefined);
      }),
    [inView, runs, results],
  );
  const complete = tournaments.done && runs.every((r) => r.done) && ids.every((id) => results.has(id));
  return { tournaments, tiles, complete };
}
