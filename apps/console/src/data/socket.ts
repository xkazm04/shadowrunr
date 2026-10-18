// `GET /events` client: reconnects with exponential backoff and jitter, and resumes from the
// last `seq` it saw, dropping replayed duplicates.
//
// It always subscribes in enveloped mode (`since` present, docs/spec/contract.md § Event
// resume): `since: 0` on a fresh connection, `since: <last seq>` after a drop. `seq` counts
// within one run, so the cursor is the pair (run_id, seq); for `live`, the subscribe carries the
// run_id the seq belongs to ("" before the first run is seen: a different run, so it streams
// from seq 1). A `busy` frame with `detail.resume_since` (slow consumer) resubscribes from there.
import { isDoorPromise, type Door, type DoorSource, type EventEnvelope } from "./door";
import { dataError, type DataError } from "./errors";

/** The slice of the browser WebSocket the client uses; tests pass a fake. */
export interface SocketLike {
  send(data: string): void;
  close(code?: number, reason?: string): void;
  onopen: ((ev: Event) => void) | null;
  onmessage: ((ev: MessageEvent) => void) | null;
  onclose: ((ev: CloseEvent) => void) | null;
  onerror: ((ev: Event) => void) | null;
}
export type SocketFactory = (url: string) => SocketLike;

export interface Timers {
  setTimeout(fn: () => void, ms: number): number;
  clearTimeout(handle: number): void;
}

export type StreamStatus = "idle" | "connecting" | "open" | "reconnecting" | "closed";

export interface BackoffPolicy {
  readonly baseMs: number;
  readonly capMs: number;
}
export const DEFAULT_BACKOFF: BackoffPolicy = { baseMs: 500, capMs: 30_000 };

/**
 * Exponential backoff with "equal jitter": half the window is fixed, half is random, so a
 * reconnect storm spreads out yet never retries in a tight loop.
 */
export function backoffDelay(attempt: number, policy: BackoffPolicy, random: () => number): number {
  const window = Math.min(policy.capMs, policy.baseMs * 2 ** Math.max(0, attempt));
  return Math.round(window / 2 + random() * (window / 2));
}

export interface EventStreamOptions {
  readonly url: string;
  /** `live`, or a run id to replay. */
  readonly target: string;
  /** The door, or a promise of it: the first connection waits for it. */
  readonly door: DoorSource;
  readonly socket: SocketFactory;
  readonly onEvent: (env: EventEnvelope) => void;
  readonly onStatus?: (status: StreamStatus, info: { attempt: number; delayMs?: number }) => void;
  readonly onIssue?: (issue: DataError) => void;
  readonly backoff?: BackoffPolicy;
  readonly random?: () => number;
  readonly timers?: Timers;
  readonly speed?: number;
}

export interface EventStream {
  close(): void;
  lastSeq(): number | undefined;
  lastRunId(): string | undefined;
  status(): StreamStatus;
}

const browserTimers: Timers = {
  setTimeout: (fn, ms) => window.setTimeout(fn, ms),
  clearTimeout: (h) => window.clearTimeout(h),
};

export function wsUrl(baseUrl: string): string {
  return `${baseUrl.replace(/\/+$/, "").replace(/^http/, "ws")}/events`;
}

export function openEventStream(opts: EventStreamOptions): EventStream {
  const policy = opts.backoff ?? DEFAULT_BACKOFF;
  const random = opts.random ?? Math.random;
  const timers = opts.timers ?? browserTimers;
  let status: StreamStatus = "idle";
  let attempt = 0;
  let lastSeq: number | undefined;
  let lastRunId: string | undefined;
  let socket: SocketLike | null = null;
  let retry: number | null = null;
  let closedByUs = false;

  const setStatus = (s: StreamStatus, delayMs?: number) => {
    status = s;
    opts.onStatus?.(s, delayMs === undefined ? { attempt } : { attempt, delayMs });
  };

  let door: Door | null = isDoorPromise(opts.door) ? null : opts.door;

  function connect() {
    if (door === null) return;
    const parser = door;
    retry = null;
    setStatus(attempt === 0 ? "connecting" : "reconnecting");
    const ws = opts.socket(opts.url);
    socket = ws;
    const subscribe = () => {
      const hello = {
        subscribe: opts.target,
        speed: opts.speed ?? 1,
        since: lastSeq ?? 0,
        ...(opts.target === "live" ? { run_id: lastRunId ?? "" } : {}),
      };
      ws.send(JSON.stringify(hello));
    };
    ws.onopen = () => {
      attempt = 0;
      subscribe();
      setStatus("open");
    };
    ws.onmessage = (m) => {
      const parsed = parser.parseEvent(typeof m.data === "string" ? m.data : String(m.data));
      if (!parsed.ok) {
        const resume = parsed.error.detail?.["resume_since"];
        if (typeof resume === "number") {
          // Slow consumer: the daemon dropped events after `resume`; backfill from there.
          lastSeq = resume;
          subscribe();
        }
        opts.onIssue?.(parsed.error);
        return;
      }
      const env = parsed.value;
      if (env.runId !== undefined && env.runId !== lastRunId) {
        lastRunId = env.runId; // a new run: its seq starts again at 1
        lastSeq = undefined;
      }
      if (env.seq !== undefined) {
        if (lastSeq !== undefined && env.seq <= lastSeq) return; // replayed on resume
        const expected = (lastSeq ?? 0) + 1;
        if (env.seq > expected) {
          opts.onIssue?.(dataError("unavailable", `Missed events ${expected}..${env.seq - 1}`, { reason: "gap" }));
        }
        lastSeq = env.seq;
      }
      opts.onEvent(env);
    };
    ws.onerror = () => {
      // The close event follows; reconnecting is decided there.
    };
    ws.onclose = () => {
      if (socket !== ws) return;
      socket = null;
      if (closedByUs) return;
      const delayMs = backoffDelay(attempt, policy, random);
      attempt += 1;
      setStatus("reconnecting", delayMs);
      retry = timers.setTimeout(connect, delayMs);
    };
  }

  if (isDoorPromise(opts.door)) {
    setStatus("connecting");
    opts.door.then(
      (d) => {
        door = d;
        if (!closedByUs) connect();
      },
      (e: unknown) => {
        opts.onIssue?.(dataError("internal", `The contract door failed to load: ${String(e)}`));
        setStatus("closed");
      },
    );
  } else {
    connect();
  }

  return {
    close() {
      closedByUs = true;
      if (retry !== null) timers.clearTimeout(retry);
      retry = null;
      const ws = socket;
      socket = null;
      ws?.close(1000, "client closed");
      setStatus("closed");
    },
    lastSeq: () => lastSeq,
    lastRunId: () => lastRunId,
    status: () => status,
  };
}
