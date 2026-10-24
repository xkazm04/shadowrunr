// The live feed from the daemon WebSocket: a bounded event store with the scene reduced one event
// at a time, seq tracking for resume (contract § event resume), the watchdog behind the broadcast
// slates, and the `{"ack"}` frames the sandbox gates on (contract appendix (sandbox)).

import type { AckFrame, StreamEvent } from "@shadowrunr/contract";
import { initialScene, reduce, type SceneState } from "@shadowrunr/stage-kit";
import type { EventsFrame } from "@shadowrunr/stage-kit/parse";
import { connectDaemon, type DaemonLink, type SocketLike, type WsStatus } from "./source";
import type { SaidLine } from "./voice/captions";

/** Events kept for the debug views and the preview's DVR window; the scene itself covers the
 * whole stream. */
export const RING_MAX = 2000;
/** The DVR keeps the scene after every this many applied events, so a seek re-folds at most this
 * many (time-travel-replay: keyframes bound the seek cost by their spacing, not the stream). */
export const KEYFRAME_EVERY = 100;
export const ACK_EVERY_MS = 5000;
/** The "SIGNAL LOST" slate shows after the socket has been down this long. */
export const DISCONNECTED_SLATE_MS = 3000;
/** The stalled mark shows when the socket is open but no event has arrived for this long. */
export const STALLED_MS = 20_000;
/** Malakor's latest lines kept with their seqs, for the captions that follow the voice. */
export const SAID_MAX = 32;

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

  /** The absolute index (since the last clear) of the oldest entry still held. */
  get first(): number {
    return this.pushed - this.size;
  }

  /** Entries ever pushed since the last clear: one past the newest absolute index. */
  get end(): number {
    return this.pushed;
  }

  /** The entry at absolute index `i`, or undefined once it has left the ring (or not yet come). */
  at(i: number): T | undefined {
    return i >= this.first && i < this.pushed ? this.buf[i % this.capacity] : undefined;
  }

  /** Oldest first. */
  toArray(): T[] {
    const out: T[] = [];
    for (let i = this.pushed - this.size; i < this.pushed; i++) out.push(this.buf[i % this.capacity]!);
    return out;
  }
}

/** The scene after the event at ring index `index` (absolute, since the reset); `ts` is the latest
 * timestamp folded into it. A retained reference: `reduce` shares structure, it copies nothing. */
interface Keyframe {
  index: number;
  ts: number;
  scene: SceneState;
}

/** What `LiveStore.sceneAt` found: the scene at `t` (the time it actually shows), and whether the
 * asked time lay before the held window and was clamped to its earliest reachable scene. */
export interface SceneAt {
  scene: SceneState;
  t: number;
  clamped: boolean;
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
  /** Malakor's latest lines with their `say` seqs, oldest first (at most SAID_MAX). */
  said: SaidLine[] = [];
  /** Waiting for `lastSeq + 1` after a resubscribe: later seqs are held back, not counted again. */
  private resyncing = false;
  private lastArrival: number | null = null;
  private openSince: number | null = null;
  private downSince: number | null;
  private cache: { version: number; events: StreamEvent[] } | null = null;
  /** Oldest first; evicted with the ring, so at most RING_MAX / KEYFRAME_EVERY + 1 are held. */
  private keyframes: Keyframe[] = [];
  /** Events the last `sceneAt` folded (0 at the head): the bounded-seek check reads it. */
  lastSeekFolds = 0;

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
    const caption = this.scene.caption;
    this.scene = reduce(this.scene, f.event);
    const c = this.scene.caption;
    if (f.event.type === "say" && f.seq !== undefined && c && c !== caption) {
      const line: SaidLine = { id: c.id, seq: f.seq, text: c.text, at: c.at, replyingTo: c.replyingTo };
      if (c.foil !== undefined) line.foil = c.foil;
      this.said = [...this.said, line].slice(-SAID_MAX);
    }
    this.ring.push(f.event);
    this.keyframe();
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

  /** The earliest time the DVR window can show, or null before any event. */
  windowStart(): number | null {
    const k = this.reachable();
    return k ? k.ts : null;
  }

  /** The scene a preview `t` ms into the stream shows, for the DVR window only: never the source of
   * the head (`scene`), the acks or the seq tracking. At or past the head it is `scene` itself;
   * inside the window it restores the newest keyframe at or before `t` and folds the ring tail
   * (at most KEYFRAME_EVERY events); before the window it clamps to the earliest reachable scene. */
  sceneAt(t: number): SceneAt {
    this.lastSeekFolds = 0;
    const head = this.scene.lastTs;
    if (head === null || t >= head) return { scene: this.scene, t, clamped: false };
    const first = this.reachable()!;
    const clamped = t < first.ts;
    if (clamped) t = first.ts;
    let from = first;
    // newest usable keyframe whose folded events all sit at or before t
    for (let i = this.keyframes.length - 1; i >= 0; i--) {
      const k = this.keyframes[i]!;
      if (k.index + 1 < this.ring.first) break;
      if (k.ts <= t) {
        if (k.index > from.index) from = k;
        break;
      }
    }
    // Timeline's rule: apply events in order while their ts is <= t
    let scene = from.scene;
    for (let i = from.index + 1; i < this.ring.end; i++) {
      const e = this.ring.at(i)!;
      if (e.ts_ms > t) break;
      scene = reduce(scene, e);
      this.lastSeekFolds++;
    }
    return { scene, t, clamped };
  }

  /** The earliest scene the window can rebuild: the initial fold of the first event while nothing has
   * left the ring, else the oldest keyframe whose following events are all still held. */
  private reachable(): Keyframe | null {
    const first = this.ring.first;
    if (this.ring.size === 0) return null;
    if (first === 0) {
      const e = this.ring.at(0)!;
      return { index: 0, ts: e.ts_ms, scene: reduce(initialScene(), e) };
    }
    return this.keyframes.find((k) => k.index + 1 >= first) ?? null;
  }

  /** Keep the scene after every KEYFRAME_EVERY-th event, and drop keyframes the ring has left behind. */
  private keyframe(): void {
    const index = this.ring.end - 1;
    if ((index + 1) % KEYFRAME_EVERY === 0) this.keyframes.push({ index, ts: this.scene.lastTs ?? 0, scene: this.scene });
    while (this.keyframes.length > 0 && this.keyframes[0]!.index + 1 < this.ring.first) this.keyframes.shift();
  }

  private reset(): void {
    this.scene = initialScene();
    this.keyframes = [];
    this.ring.clear();
    this.said = [];
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
