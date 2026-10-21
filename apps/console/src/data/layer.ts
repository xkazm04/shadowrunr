// Composes the data layer: one RPC client, one query cache, one live `/events` stream that drives
// cache invalidation and connection state, and a fan-out for pages that want the live events.
import { QueryCache } from "./cache";
import { ConnectionStore } from "./connection";
import type { DoorSource, EventEnvelope } from "./door";
import type { DataError } from "./errors";
import { invalidatedByMutation } from "./invalidation";
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
}

export interface DataLayer {
  readonly client: RpcClient;
  readonly cache: QueryCache;
  readonly connection: ConnectionStore;
  /** A mutating call: on success, refetches what it invalidates. */
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
  const cache = new QueryCache(client, {
    timers: opts.timers,
    ...(opts.backstopMs === undefined ? {} : { backstopMs: opts.backstopMs }),
  });
  const live = new Set<(env: EventEnvelope) => void>();
  let stream: EventStream | null = null;
  let everOpened = false;

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
    async mutate(method, params, callOpts) {
      const r = await client.call(method, params, callOpts);
      if (r.ok) cache.invalidate(invalidatedByMutation(method));
      return r;
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
    start() {
      if (stream) return;
      cache.startBackstop();
      stream = openEventStream({
        ...streamOpts("live", (env) => {
          cache.onEvent(env.event.type);
          for (const l of live) l(env);
        }),
        onStatus: (status, info) => {
          connection.update({ stream: status, retryInMs: info.delayMs, ...(status === "open" ? { issue: undefined } : {}) });
          if (status === "open") {
            if (everOpened) cache.retryFailed();
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
