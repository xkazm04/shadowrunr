// Playback over a parsed transcript: an incremental cursor with cheap forward steps and a
// rebuild on backward seeks. No timers here; the React hook drives `advanceTo`.

import type { StreamEvent } from "@shadowrunr/contract";
import type { TsMs } from "./vocab";
import { reduce } from "./scene";
import { initialScene, type SceneState } from "./sceneModel";

export class Timeline {
  private cursor = 0;
  private state: SceneState = initialScene();
  private appliedTo: TsMs = Number.NEGATIVE_INFINITY;
  readonly events: readonly StreamEvent[];
  readonly start: TsMs;
  readonly end: TsMs;

  constructor(events: readonly StreamEvent[]) {
    this.events = events;
    this.start = events.length ? events.reduce((m, e) => Math.min(m, e.ts_ms), events[0]!.ts_ms) : 0;
    this.end = events.reduce((m, e) => Math.max(m, e.ts_ms), this.start);
  }

  get duration(): number {
    return this.end - this.start;
  }

  get scene(): SceneState {
    return this.state;
  }

  get position(): TsMs {
    return this.appliedTo;
  }

  /** Apply every event with `ts_ms <= t`. Seeking backwards rebuilds from the start. */
  advanceTo(t: TsMs): SceneState {
    if (t < this.appliedTo) {
      this.cursor = 0;
      this.state = initialScene();
    }
    this.appliedTo = t;
    while (this.cursor < this.events.length && this.events[this.cursor]!.ts_ms <= t) {
      this.state = reduce(this.state, this.events[this.cursor]!);
      this.cursor++;
    }
    return this.state;
  }

  /** The timestamp of the next event not yet applied, or null at the end: an event-driven
   * player sleeps until then instead of ticking every frame. */
  get nextAt(): TsMs | null {
    return this.events[this.cursor]?.ts_ms ?? null;
  }

  get finished(): boolean {
    return this.cursor >= this.events.length;
  }
}

/** Clip markers for the scrubber: `mark_moment` ops as fractions of the duration. */
export function clipMarkers(events: readonly StreamEvent[]): { at: TsMs; label: string; frac: number }[] {
  if (events.length === 0) return [];
  const start = events[0]!.ts_ms;
  const end = events.reduce((m, e) => Math.max(m, e.ts_ms), start);
  const span = Math.max(1, end - start);
  const out: { at: TsMs; label: string; frac: number }[] = [];
  for (const e of events) {
    if (e.type === "op" && e.op.op === "mark_moment") {
      out.push({ at: e.ts_ms, label: e.op.label, frac: (e.ts_ms - start) / span });
    }
  }
  return out;
}

/** Segment spans for the scrubber background. */
export function segmentSpans(events: readonly StreamEvent[]) {
  const spans: { segment: string; from: TsMs; to: TsMs }[] = [];
  if (events.length === 0) return spans;
  const end = events.reduce((m, e) => Math.max(m, e.ts_ms), events[0]!.ts_ms);
  for (const e of events) {
    const seg = e.type === "segment_start" ? e.segment : e.type === "handover" ? e.to : null;
    if (seg === null) continue;
    const last = spans[spans.length - 1];
    if (last && last.segment === seg) continue;
    if (last) last.to = e.ts_ms;
    spans.push({ segment: seg, from: e.ts_ms, to: end });
  }
  return spans;
}

/** Canvas scaling: fit 1920x1080 into the window with letterboxing. */
export const CANVAS_W = 1920;
export const CANVAS_H = 1080;

export function fitCanvas(winW: number, winH: number) {
  const scale = Math.min(winW / CANVAS_W, winH / CANVAS_H);
  const w = CANVAS_W * scale;
  const h = CANVAS_H * scale;
  return { scale, left: (winW - w) / 2, top: (winH - h) / 2 };
}
