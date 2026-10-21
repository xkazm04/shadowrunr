// Typed hooks over the data layer. Components read snapshots through useSyncExternalStore, so a
// component re-renders only when its own query or the connection changes.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { IDLE, queryKey, type QuerySnapshot } from "./cache";
import { summarise, type ConnectionSnapshot, type ConnectionSummary } from "./connection";
import type { EventEnvelope } from "./door";
import { viewOfError, type DataError, type ErrorView } from "./errors";
import type { RpcMethod, RpcParams, RpcResult } from "@shadowrunr/contract";
import type { DataLayer } from "./layer";
import type { InflightMutation } from "./mutations";
import type { Result } from "./result";
import type { CallOptions } from "./rpc";

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
  /** When the reply in `data` landed (ms since the epoch); undefined before the first reply. */
  readonly updatedAt: number | undefined;
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
      updatedAt: snap.updatedAt,
      reload,
    }),
    [snap, reload],
  );
}

/** The method's calls in flight anywhere in the console, read from the layer (survives a remount). */
export function useInflight(method: RpcMethod): readonly InflightMutation[] {
  const { mutations } = useDataLayer();
  const get = useCallback(() => mutations.inflight(method), [mutations, method]);
  return useSyncExternalStore(mutations.subscribe, get, get);
}

/** An outcome that settled while no caller was mounted to receive it; `ack()` marks it read. */
export interface MissedOutcome<M extends RpcMethod> {
  readonly params: RpcParams<M>;
  readonly settledAt: number;
  readonly error: DataError | undefined;
  readonly ack: () => void;
}

/**
 * A mutating RPC; on success the cache refetches what it invalidates (invalidation.ts).
 * `opts.timeoutMs` overrides the 15 s default per call: the LLM-backed calls (`reconcile.run`,
 * `plan.choose`, `lord.chat`) run for minutes.
 *
 * `pending` is this instance's own call (per-row buttons keep using it). The layer owns the call
 * itself: `running` lists the method's calls in flight, which a remounted view still sees, and
 * `settled` is an outcome no mounted caller received (its view unmounted first), until `ack()`.
 */
export function useMutation<M extends RpcMethod>(
  method: M,
  opts?: CallOptions,
): {
  run: (params: RpcParams<M>) => Promise<Result<RpcResult<M>, DataError>>;
  pending: boolean;
  running: readonly InflightMutation[];
  settled: MissedOutcome<M> | undefined;
} {
  const layer = useDataLayer();
  const { mutations } = layer;
  const [pending, setPending] = useState(false);
  const running = useInflight(method);
  const getMissed = useCallback(() => mutations.unacked(method), [mutations, method]);
  const missed = useSyncExternalStore(mutations.subscribe, getMissed, getMissed);
  // Claims this instance holds; released on unmount so a call outliving it settles unread.
  const claims = useRef(new Set<() => void>());
  useEffect(() => {
    const held = claims.current;
    return () => {
      for (const release of held) release();
      held.clear();
    };
  }, []);
  const timeoutMs = opts?.timeoutMs;
  const signal = opts?.signal;
  const run = useCallback(
    async (params: RpcParams<M>): Promise<Result<RpcResult<M>, DataError>> => {
      setPending(true);
      const release = mutations.claim(queryKey(method, params));
      claims.current.add(release);
      try {
        const callOpts: CallOptions = { ...(timeoutMs === undefined ? {} : { timeoutMs }), ...(signal === undefined ? {} : { signal }) };
        const r = await layer.mutate(method, params, callOpts);
        // The door validated the reply against this method's schema.
        return r.ok ? { ok: true, value: r.value as RpcResult<M> } : r;
      } finally {
        release();
        claims.current.delete(release);
        setPending(false);
      }
    },
    [layer, mutations, method, timeoutMs, signal],
  );
  const settled = useMemo<MissedOutcome<M> | undefined>(
    () =>
      missed === undefined
        ? undefined
        : {
            params: missed.params as RpcParams<M>,
            settledAt: missed.settledAt,
            error: missed.result.ok ? undefined : missed.result.error,
            ack: () => mutations.ack(missed.id),
          },
    [missed, mutations],
  );
  return { run, pending, running, settled };
}

/** Per-call timeout for the reconciler's LLM calls (opus, 600 s in the routing table). */
export const LLM_TIMEOUT_MS = 11 * 60_000;

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
