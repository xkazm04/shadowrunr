// Connection state as one small store: is the daemon answering RPC, and is `/events` open.
import type { DataError } from "./errors";
import type { Reachability } from "./rpc";
import type { StreamStatus } from "./socket";

export interface ConnectionSnapshot {
  readonly rpc: Reachability;
  readonly stream: StreamStatus;
  /** Milliseconds until the next reconnect attempt, while reconnecting. */
  readonly retryInMs: number | undefined;
  /** The latest stream issue (a gap, a bad frame); cleared when the stream reopens. */
  readonly issue: DataError | undefined;
}

export type ConnectionSummary = "online" | "offline" | "degraded" | "connecting";

export function summarise(c: ConnectionSnapshot): ConnectionSummary {
  if (c.rpc === "offline") return "offline";
  if (c.stream === "open" && c.issue === undefined) return c.rpc === "online" || c.rpc === "unknown" ? "online" : "degraded";
  if (c.stream === "reconnecting") return c.rpc === "online" ? "degraded" : "offline";
  if (c.stream === "open") return "degraded";
  return "connecting";
}

export class ConnectionStore {
  private snap: ConnectionSnapshot = { rpc: "unknown", stream: "idle", retryInMs: undefined, issue: undefined };
  private readonly listeners = new Set<() => void>();

  subscribe = (l: () => void): (() => void) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };

  getSnapshot = (): ConnectionSnapshot => this.snap;

  update(patch: Partial<ConnectionSnapshot>): void {
    const next = { ...this.snap, ...patch };
    if (
      next.rpc === this.snap.rpc &&
      next.stream === this.snap.stream &&
      next.retryInMs === this.snap.retryInMs &&
      next.issue === this.snap.issue
    ) {
      return;
    }
    this.snap = next;
    for (const l of this.listeners) l();
  }
}
