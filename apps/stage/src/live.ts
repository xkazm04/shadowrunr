// The live feed from the daemon WebSocket: a bounded event store with the scene reduced one event
// at a time, seq tracking for resume (contract § event resume), the watchdog behind the broadcast
// slates, and the `{"ack"}` frames the sandbox gates on (contract appendix (sandbox)).

import type { AckFrame, StreamEvent } from "@shadowrunr/contract";
import { initialScene, reduce, type SceneState } from "@shadowrunr/stage-kit";
import type { EventsFrame } from "@shadowrunr/stage-kit/parse";
import { connectDaemon, type DaemonLink, type SocketLike, type WsStatus } from "./source";

/** Events kept for the debug views; the scene itself covers the whole stream. */
export const RING_MAX = 2000;
export const ACK_EVERY_MS = 5000;
/** The "SIGNAL LOST" slate shows after the socket has been down this long. */
export const DISCONNECTED_SLATE_MS = 3000;
/** The stalled mark shows when the socket is open but no event has arrived for this long. */
export const STALLED_MS = 20_000;

/** A fixed-capacity ring: push is O(1), and the oldest entry goes once it is full. */
export class EventRing<T> {
  private buf: (T | undefined)[];
  /** Entries ever pushed since the last clear. */
  private pushed = 0;

  constructor(readonly capacity: number) {
    this.buf = new Array<T | undefined>(capacity);
  }

  get size(): number {
    return Math.min(this.pushed, this.capacity);
  }

  push(x: T): void {
    this.buf[this.pushed % this.capacity] = x;
    this.pushed++;
  }

  clear(): void {
    this.buf = new Array<T | undefined>(this.capacity);
    this.pushed = 0;
  }

  /** Oldest first. */
  toArray(): T[] {
    const out: T[] = [];
    for (let i = this.pushed - this.size; i < this.pushed; i++) out.push(this.buf[i % this.capacity]!);
    return out;
  }
}

/** What `LiveStore.accept` did with a frame. A `gap` asks the caller to resubscribe. */
export type Accepted = "applied" | "duplicate" | "gap" | "held";

export interface Watch {
  /** The socket has been down for at least DISCONNECTED_SLATE_MS (and the stream has not ended). */
  disconnected: boolean;
  /** Open, not ended, and no event for at least STALLED_MS. */
  stalled: boolean;
  /** How long the open socket has gone without an event (0 when closed or ended). */
  stalledMs: number;
}

export class LiveStore {
  scene: SceneState = initialScene();
  readonly ring = new EventRing<StreamEvent>(RING_MAX);
  /** The run the seqs belong to ("" before the first enveloped frame). */
  runId = "";
  /** The highest contiguous seq applied. */
  lastSeq = 0;
  /** Seqs found missing in this run, plus events a `busy` frame reported skipped. */
  gaps = 0;
  busyFrames = 0;
  /** Bumps on every applied event and every reset. */
  version = 0;
  /** The last applied event's timestamp and the wall time it arrived (drives the live clock). */
  lastEvent: { ts: number; wall: number } | null = null;
  /** Waiting for `lastSeq + 1` after a resubscribe: later seqs are held back, not counted again. */
  private resyncing = false;
  private lastArrival: number | null = null;
  private openSince: number | null = null;
  private downSince: number | null;
  private cache: { version: number; events: StreamEvent[] } | null = null;

  constructor(wall: number) {
    this.downSince = wall;
  }

  /** Apply one event frame once, in seq order. A new run id or a `stream_start` starts a new scene. */
  accept(f: Extract<EventsFrame, { kind: "event" }>, wall: number): Accepted {
    this.lastArrival = wall;
    if (f.runId !== undefined && f.runId !== this.runId) {
      this.runId = f.runId;
      this.lastSeq = 0;
      this.gaps = 0;
      this.resyncing = false;
      this.reset();
    }
    if (f.seq !== undefined) {
      if (f.seq <= this.lastSeq) return "duplicate";
      if (f.seq !== this.lastSeq + 1) {
        if (this.resyncing) return "held";
        this.gaps += f.seq - this.lastSeq - 1;
        this.resyncing = true;
        return "gap";
      }
      this.lastSeq = f.seq;
      this.resyncing = false;
    }
    if (f.event.type === "stream_start") this.reset();
    this.scene = reduce(this.scene, f.event);
    this.ring.push(f.event);
    this.lastEvent = { ts: f.event.ts_ms, wall };
    this.version++;
    return "applied";
  }

  /** A `busy` frame: the daemon dropped `skipped` events; the caller resubscribes from lastSeq. */
  noteBusy(skipped: number): void {
    this.busyFrames++;
    this.gaps += Math.max(1, skipped);
    this.resyncing = true;
  }

  resume(): { since: number; runId: string } {
    return { since: this.lastSeq, runId: this.runId };
  }

  status(s: WsStatus, wall: number): void {
    if (s === "open") {
      this.openSince = wall;
      this.downSince = null;
    } else {
      this.openSince = null;
      this.downSince ??= wall;
    }
  }

  watch(wall: number): Watch {
    // An ended stream keeps its end card: a socket the daemon closes after the end is not news.
    const ended = this.scene.ended !== null;
    const disconnected = !ended && this.downSince !== null && wall - this.downSince >= DISCONNECTED_SLATE_MS;
    let stalledMs = 0;
    if (this.openSince !== null && !ended) {
      stalledMs = Math.max(0, wall - Math.max(this.openSince, this.lastArrival ?? this.openSince));
    }
    return { disconnected, stalled: stalledMs >= STALLED_MS, stalledMs };
  }

  ack(wall: number): AckFrame {
    return { ack: { seq: this.lastSeq, gaps: this.gaps, stalled_ms: Math.round(this.watch(wall).stalledMs) } };
  }

  /** The held events, oldest first (copied once per change, for the debug views). */
  events(): StreamEvent[] {
    if (this.cache?.version !== this.version) this.cache = { version: this.version, events: this.ring.toArray() };
    return this.cache.events;
  }

  private reset(): void {
    this.scene = initialScene();
    this.ring.clear();
    this.lastEvent = null;
    this.version++;
  }
}

function skippedOf(detail: unknown): number {
  if (typeof detail === "object" && detail !== null && "skipped" in detail) {
    const n = Number((detail as { skipped: unknown }).skipped);
    if (Number.isFinite(n)) return n;
  }
  return 1;
}

export interface LiveOptions {
  url: string;
  run: string;
  speed: number;
  /** The wall clock (ms). */
  now: () => number;
  socket?: (url: string) => SocketLike;
  onStatus?: (s: WsStatus, detail?: string) => void;
  onBadMessage?: (detail: string) => void;
  onErrorFrame?: (code: string, message: string) => void;
}

/** The store on a daemon link: resubscribes on a gap or `busy`, and acks every 5 s and at the end. */
export class LiveSession {
  readonly store: LiveStore;
  private link: DaemonLink | null = null;
  private ackTimer: ReturnType<typeof setInterval> | null = null;

  constructor(private readonly opts: LiveOptions) {
    this.store = new LiveStore(opts.now());
  }

  start(): void {
    const o = this.opts;
    this.link = connectDaemon(
      o.url,
      o.run,
      o.speed,
      {
        onFrame: (f) => this.frame(f),
        onStatus: (s, d) => {
          this.store.status(s, o.now());
          o.onStatus?.(s, d);
        },
        onBadMessage: (d) => o.onBadMessage?.(d),
        resume: () => this.store.resume(),
      },
      o.socket,
    );
    this.ackTimer = setInterval(() => this.ack(), ACK_EVERY_MS);
  }

  /** Send the ack now; false when the socket is not open. */
  ack(): boolean {
    return this.link?.send(this.store.ack(this.opts.now())) ?? false;
  }

  stop(): void {
    if (this.ackTimer !== null) clearInterval(this.ackTimer);
    this.ackTimer = null;
    this.link?.close();
    this.link = null;
  }

  private frame(f: EventsFrame): void {
    if (f.kind === "error") {
      if (f.code === "busy") {
        this.store.noteBusy(skippedOf(f.detail));
        this.link?.resubscribe();
      }
      this.opts.onErrorFrame?.(f.code, f.message);
      return;
    }
    const r = this.store.accept(f, this.opts.now());
    if (r === "gap") this.link?.resubscribe();
    else if (r === "applied" && f.event.type === "stream_end") this.ack();
  }
}
