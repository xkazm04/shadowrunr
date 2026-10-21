// The On Air replay player: event-driven, never per-frame. It sleeps until the next event is
// due (scaled by the replay speed), applies everything due through stage-kit's Timeline, and
// notifies once. Timed effects on screen (a card that leaves after 7 s, the vote panel that
// clears after a close) are CSS animations keyed by the event, so nothing ticks between events.
import type { StreamEvent } from "@shadowrunr/contract";
import { Timeline, initialScene, reduce, type SceneState, type TsMs } from "@shadowrunr/stage-kit";
import type { Timers } from "../../data";

/** Replays run this many times faster than simulated time (A/1 ran 5×, the winner about 1.5×). */
export const REPLAY_SPEED = 4;
/** The last frame (the sign-off slate) holds this long before the replay loops. */
export const LOOP_HOLD_MS = 4000;
/** Events due within this many wall-clock ms are applied together: one render, not several. */
export const COALESCE_MS = 80;

export interface FeedSnapshot {
  readonly scene: SceneState;
  readonly playing: boolean;
  /** Transcript time applied so far (ms). */
  readonly position: TsMs;
  readonly start: TsMs;
  readonly end: TsMs;
  readonly speed: number;
  /** Bumped on play, pause and restart: the progress bar re-keys its CSS animation on it. */
  readonly epoch: number;
  readonly live: boolean;
  /** Live only: the stream reported missed events and no contiguous frame has followed yet. */
  readonly gap?: boolean;
}

export interface Feed {
  subscribe(listener: () => void): () => void;
  getSnapshot(): FeedSnapshot;
  play(): void;
  pause(): void;
  restart(): void;
  dispose(): void;
}

const browserTimers: Timers = {
  setTimeout: (fn, ms) => window.setTimeout(fn, ms),
  clearTimeout: (h) => window.clearTimeout(h),
};

export interface PlayerOptions {
  readonly timers?: Timers;
  readonly now?: () => number;
  readonly speed?: number;
  readonly loop?: boolean;
}

export class ReplayPlayer implements Feed {
  private readonly timeline: Timeline;
  private readonly timers: Timers;
  private readonly now: () => number;
  private readonly speed: number;
  private readonly loop: boolean;
  private readonly listeners = new Set<() => void>();
  private timer: number | null = null;
  private anchor: { wall: number; pos: TsMs } | null = null;
  private snapshot: FeedSnapshot;

  constructor(events: readonly StreamEvent[], opts: PlayerOptions = {}) {
    this.timeline = new Timeline(events);
    this.timers = opts.timers ?? browserTimers;
    this.now = opts.now ?? (() => performance.now());
    this.speed = opts.speed ?? REPLAY_SPEED;
    this.loop = opts.loop ?? true;
    // The first event lands at once, so the screen is never blank on load.
    const scene = this.timeline.advanceTo(this.timeline.start);
    this.snapshot = {
      scene,
      playing: false,
      position: this.timeline.start,
      start: this.timeline.start,
      end: this.timeline.end,
      speed: this.speed,
      epoch: 0,
      live: false,
    };
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): FeedSnapshot => this.snapshot;

  play(): void {
    if (this.snapshot.playing || this.timeline.events.length === 0) return;
    if (this.timeline.finished) this.rewind();
    this.anchor = { wall: this.now(), pos: this.snapshot.position };
    this.emit({ playing: true, epoch: this.snapshot.epoch + 1 });
    this.schedule();
  }

  pause(): void {
    if (!this.snapshot.playing) return;
    this.clear();
    const pos = this.clock();
    this.anchor = null;
    this.emit({ playing: false, position: Math.min(this.timeline.end, Math.max(pos, this.snapshot.position)), epoch: this.snapshot.epoch + 1 });
  }

  restart(): void {
    const wasPlaying = this.snapshot.playing;
    this.clear();
    this.rewind();
    this.anchor = wasPlaying ? { wall: this.now(), pos: this.timeline.start } : null;
    this.emit({ epoch: this.snapshot.epoch + 1 });
    if (wasPlaying) this.schedule();
  }

  dispose(): void {
    this.clear();
    this.listeners.clear();
  }

  /** Transcript time now, from the wall clock since the last anchor. */
  private clock(): TsMs {
    if (!this.anchor) return this.snapshot.position;
    return this.anchor.pos + (this.now() - this.anchor.wall) * this.speed;
  }

  /** Back to the first event (Timeline rebuilds from the start on a backward seek). */
  private rewind(): void {
    this.snapshot = { ...this.snapshot, scene: this.timeline.advanceTo(this.timeline.start), position: this.timeline.start };
  }

  private schedule(): void {
    const next = this.timeline.nextAt;
    if (next === null) {
      if (!this.loop) {
        this.anchor = null;
        this.emit({ playing: false, position: this.timeline.end, epoch: this.snapshot.epoch + 1 });
        return;
      }
      this.timer = this.timers.setTimeout(() => {
        this.timer = null;
        this.rewind();
        this.anchor = { wall: this.now(), pos: this.timeline.start };
        this.emit({ epoch: this.snapshot.epoch + 1 });
        this.schedule();
      }, LOOP_HOLD_MS);
      return;
    }
    const delay = Math.max(0, (next - this.clock()) / this.speed);
    this.timer = this.timers.setTimeout(() => {
      this.timer = null;
      const upTo = Math.max(next, this.clock()) + COALESCE_MS * this.speed;
      const scene = this.timeline.advanceTo(Math.min(upTo, this.timeline.end));
      this.emit({ scene, position: Math.min(this.timeline.end, Math.max(next, this.clock())) });
      this.schedule();
    }, delay);
  }

  private clear(): void {
    if (this.timer !== null) this.timers.clearTimeout(this.timer);
    this.timer = null;
  }

  private emit(patch: Partial<FeedSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch };
    for (const l of [...this.listeners]) l();
  }
}

const LIVE_START: FeedSnapshot = {
  scene: initialScene(),
  playing: true,
  position: 0,
  start: 0,
  end: 0,
  speed: 1,
  epoch: 0,
  live: true,
  gap: false,
};

/**
 * The live feed: every event of the current live run folded into one scene. A new run id (or a
 * fresh `stream_start`) starts a new scene. Its owner (On Air's page-owned subscription, which
 * backfills the run in flight from seq 1) calls `reset()` before each new subscription, so the
 * scene is always the fold of the run so far, not of what arrived while the page was open.
 */
export class LiveFeed implements Feed {
  private readonly listeners = new Set<() => void>();
  private runId: string | undefined;
  private lastSeq: number | undefined;
  private snapshot: FeedSnapshot = LIVE_START;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): FeedSnapshot => this.snapshot;

  push(event: StreamEvent, runId?: string, seq?: number): void {
    const fresh = (runId !== undefined && runId !== this.runId) || event.type === "stream_start";
    // The stream reports a hole before handing over the frame past it; the next contiguous frame
    // (or a fresh run) means the scene is whole again from here on.
    const contiguous = seq !== undefined && this.lastSeq !== undefined && seq === this.lastSeq + 1;
    const gap = (this.snapshot.gap ?? false) && !(fresh || contiguous);
    if (runId !== undefined) this.runId = runId;
    if (seq !== undefined) this.lastSeq = seq;
    const base = fresh ? initialScene() : this.snapshot.scene;
    const scene = reduce(base, event);
    const start = fresh ? event.ts_ms : this.snapshot.start;
    const end = fresh ? event.ts_ms : Math.max(event.ts_ms, this.snapshot.end);
    this.emit({ ...this.snapshot, scene, start, position: event.ts_ms, end, gap });
  }

  /** The stream missed events: say so until the next contiguous frame. */
  markGap(): void {
    if (this.snapshot.gap) return;
    this.emit({ ...this.snapshot, gap: true });
  }

  /** Back to nothing on air (a new subscription is about to backfill the run from seq 1). */
  reset(): void {
    this.runId = undefined;
    this.lastSeq = undefined;
    this.emit(LIVE_START);
  }

  private emit(next: FeedSnapshot): void {
    this.snapshot = next;
    for (const l of [...this.listeners]) l();
  }

  play(): void {}
  pause(): void {}
  restart(): void {}
  dispose(): void {
    this.listeners.clear();
  }
}
