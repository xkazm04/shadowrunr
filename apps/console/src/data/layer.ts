// Composes the data layer: one RPC client, one query cache, one live `/events` stream that drives
// cache invalidation and connection state, and a fan-out for pages that want the live events.
import { QueryCache } from "./cache";
import { ConnectionStore } from "./connection";
import type { DoorSource, EventEnvelope } from "./door";
import type { DataError } from "./errors";
import { mutationRules } from "./invalidation";
import { MutationStore } from "./mutations";
import type { Result } from "./result";
import { createRpcClient, type CallOptions, type FetchLike, type RpcClient } from "./rpc";
import { openEventStream, wsUrl, type EventStream, type SocketFactory, type Timers } from "./socket";

export interface DataLayerOptions {
  readonly baseUrl: string;
  readonly door: DoorSource;
  readonly fetch?: FetchLike;
  readonly socket: SocketFactory;
  readonly timers: Timers;
  readonly random?: () => number;
  readonly backstopMs?: number;
  readonly timeoutMs?: number;
  /** The clock stamping replies and reopens; `Date.now` by default. */
  readonly now?: () => number;
}

export interface DataLayer {
  readonly client: RpcClient;
  readonly cache: QueryCache;
  readonly connection: ConnectionStore;
  /** Mutations in flight and recently settled, so a long call outlives the view that started it. */
  readonly mutations: MutationStore;
  /**
   * A mutating call: on success, refetches what it invalidates. A call whose key
   * (`queryKey(method, params)`) is already in flight joins that flight instead of sending again.
   */
  mutate(method: string, params: unknown, opts?: CallOptions): Promise<Result<unknown, DataError>>;
  /** Live events (after the cache has seen them). */
  onLiveEvent(listener: (env: EventEnvelope) => void): () => void;
  /**
   * A dedicated `live` subscription owned by its caller: it backfills the run in flight from
   * seq 1 (`since: 0, run_id: ""`), resumes after a drop, and reports gaps via `onIssue`.
   * The caller closes it; nothing else does.
   */
  openLive(onEvent: (env: EventEnvelope) => void, onIssue?: (issue: DataError) => void): EventStream;
  /** Replays one run's transcript as events, with the same reconnect and resume. */
  openReplay(runId: string, onEvent: (env: EventEnvelope) => void, speed?: number): EventStream;
  /**
   * When `/events` last reopened after a drop (undefined until the first reopen). A reply older
   * than this may have missed events while the feed was down; the reopen revalidates it.
   */
  reopenedAt(): number | undefined;
  start(): void;
  stop(): void;
}

export function createDataLayer(opts: DataLayerOptions): DataLayer {
  const connection = new ConnectionStore();
  const client = createRpcClient({
    baseUrl: opts.baseUrl,
    door: opts.door,
    ...(opts.fetch ? { fetch: opts.fetch } : {}),
    ...(opts.timeoutMs === undefined ? {} : { timeoutMs: opts.timeoutMs }),
    onReachability: (rpc) => connection.update({ rpc }),
  });
  const now = opts.now ?? Date.now;
  const cache = new QueryCache(client, {
    timers: opts.timers,
    now,
    ...(opts.backstopMs === undefined ? {} : { backstopMs: opts.backstopMs }),
  });
  const mutations = new MutationStore(now);
  const live = new Set<(env: EventEnvelope) => void>();
  let stream: EventStream | null = null;
  let everOpened = false;
  let reopenedAt: number | undefined;

  const streamOpts = (target: string, onEvent: (env: EventEnvelope) => void, speed?: number) => ({
    url: wsUrl(opts.baseUrl),
    target,
    door: opts.door,
    socket: opts.socket,
    timers: opts.timers,
    onEvent,
    ...(opts.random ? { random: opts.random } : {}),
    ...(speed === undefined ? {} : { speed }),
  });

  return {
    client,
    cache,
    connection,
    mutations,
    mutate(method, params, callOpts) {
      return mutations.run(method, params, async () => {
        const r = await client.call(method, params, callOpts);
        if (r.ok) cache.invalidateWhere(mutationRules(method), { params }, false);
        return r;
      });
    },
    onLiveEvent(listener) {
      live.add(listener);
      return () => live.delete(listener);
    },
    openLive(onEvent, onIssue) {
      return openEventStream({ ...streamOpts("live", onEvent), ...(onIssue ? { onIssue } : {}) });
    },
    openReplay(runId, onEvent, speed) {
      return openEventStream(streamOpts(runId, onEvent, speed));
    },
    reopenedAt: () => reopenedAt,
    start() {
      if (stream) return;
      cache.startBackstop();
      stream = openEventStream({
        ...streamOpts("live", (env) => {
          cache.onEvent(env.runId === undefined ? { type: env.event.type } : { type: env.event.type, runId: env.runId });
          for (const l of live) l(env);
        }),
        onStatus: (status, info) => {
          // Stamped before the connection changes, so a region re-rendering on it sees the reopen.
          const reopened = status === "open" && everOpened;
          if (reopened) reopenedAt = now();
          connection.update({ stream: status, retryInMs: info.delayMs, ...(status === "open" ? { issue: undefined } : {}) });
          if (status === "open") {
            // Events were missed during the gap: every reply on screen refetches, not only failed ones.
            if (reopened) cache.revalidateWatched();
            everOpened = true;
          }
        },
        onIssue: (issue) => connection.update({ issue }),
      });
    },
    stop() {
      stream?.close();
      stream = null;
      cache.dispose();
      live.clear();
    },
  };
}
