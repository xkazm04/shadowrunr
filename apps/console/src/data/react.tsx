// Typed hooks over the data layer. Components read snapshots through useSyncExternalStore, so a
// component re-renders only when its own query or the connection changes.
import { createContext, useCallback, useContext, useEffect, useMemo, useState, useSyncExternalStore, type ReactNode } from "react";
import { IDLE, queryKey, type QuerySnapshot } from "./cache";
import { summarise, type ConnectionSnapshot, type ConnectionSummary } from "./connection";
import type { EventEnvelope } from "./door";
import { viewOfError, type DataError, type ErrorView } from "./errors";
import type { RpcMethod, RpcParams, RpcResult } from "@shadowrunr/contract";
import type { DataLayer } from "./layer";
import type { Result } from "./result";

const DataContext = createContext<DataLayer | null>(null);

export function DataProvider({ layer, children }: { layer: DataLayer; children: ReactNode }) {
  useEffect(() => {
    layer.start();
    return () => layer.stop();
  }, [layer]);
  return <DataContext.Provider value={layer}>{children}</DataContext.Provider>;
}

export function useDataLayer(): DataLayer {
  const l = useContext(DataContext);
  if (l === null) throw new Error("useDataLayer outside <DataProvider>");
  return l;
}

export interface Query<T> {
  readonly status: QuerySnapshot["status"];
  readonly data: T | undefined;
  readonly error: DataError | undefined;
  /** How to show the error, when there is one. */
  readonly errorView: ErrorView | undefined;
  readonly refreshing: boolean;
  readonly reload: () => void;
}

/**
 * Reads one RPC through the cache, typed by the contract's method table. `params === null`
 * skips the call. The door validated the reply against this method's schema before it reached
 * the cache, which is what makes the narrowing below sound.
 */
export function useRpc<M extends RpcMethod>(method: M, params: RpcParams<M> | null): Query<RpcResult<M>> {
  const layer = useDataLayer();
  const key = params === null ? null : queryKey(method, params);
  const subscribe = useCallback(
    (l: () => void) => (params === null ? () => undefined : layer.cache.watch(method, params, l)),
    // `key` captures params by value, so a new object with the same content does not resubscribe.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [layer, method, key],
  );
  const get = useCallback(() => (key === null ? IDLE : layer.cache.getSnapshot(key)), [layer, key]);
  const snap = useSyncExternalStore(subscribe, get, get);
  const reload = useCallback(() => {
    if (params !== null) layer.cache.refetch(method, params);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [layer, method, key]);
  return useMemo(
    () => ({
      status: snap.status,
      data: snap.data as RpcResult<M> | undefined,
      error: snap.error,
      errorView: snap.error ? viewOfError(snap.error) : undefined,
      refreshing: snap.refreshing,
      reload,
    }),
    [snap, reload],
  );
}

/** A mutating RPC; on success the cache refetches what it invalidates (invalidation.ts). */
export function useMutation<M extends RpcMethod>(method: M): {
  run: (params: RpcParams<M>) => Promise<Result<RpcResult<M>, DataError>>;
  pending: boolean;
} {
  const layer = useDataLayer();
  const [pending, setPending] = useState(false);
  const run = useCallback(
    async (params: RpcParams<M>): Promise<Result<RpcResult<M>, DataError>> => {
      setPending(true);
      try {
        const r = await layer.mutate(method, params);
        return r.ok ? { ok: true, value: r.value as RpcResult<M> } : r;
      } finally {
        setPending(false);
      }
    },
    [layer, method],
  );
  return { run, pending };
}

export function useConnection(): ConnectionSnapshot & { summary: ConnectionSummary } {
  const { connection } = useDataLayer();
  const snap = useSyncExternalStore(connection.subscribe, connection.getSnapshot, connection.getSnapshot);
  return useMemo(() => ({ ...snap, summary: summarise(snap) }), [snap]);
}

/** Calls `listener` for every live event; the listener may change without resubscribing. */
export function useLiveEvents(listener: (env: EventEnvelope) => void): void {
  const layer = useDataLayer();
  const [ref] = useState(() => ({ current: listener }));
  useEffect(() => {
    ref.current = listener;
  });
  useEffect(() => layer.onLiveEvent((env) => ref.current(env)), [layer, ref]);
}
