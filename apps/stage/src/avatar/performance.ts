// How Malakor performs beyond his mouth (spark avatar-3d): eye-light intensity, brow and head
// gestures, set from the delivery of the line he is speaking. The stage keeps each `say` event's
// `delivery` by its seq and sets a `PerformanceCue` when that line's audio starts, so the face
// changes on the audio clock, not when the text arrives. Every face reads the same cue through a
// `PerformanceSource` and turns it into a `PerformancePose` with its own `PerformanceDriver`.
//
// P0 (Director): the wire-level types are final; the function bodies are stubs that WP-P fills.

import type { Delivery, Emotion } from "@shadowrunr/contract";

export type { Emotion };

/** The head and eye gestures a cue can trigger. Each plays once, as an envelope over ~0.4-1.2 s. */
export const GESTURES = ["nod", "tilt", "lean_in", "recoil", "glare", "flare"] as const;
export type Gesture = (typeof GESTURES)[number];

/**
 * What the face is asked to perform. `id` grows with every cue, so the same gesture set twice
 * plays twice. `eyeLight` overrides the emotion's eye light when present (1 = rest).
 */
export interface PerformanceCue {
  id: number;
  emotion: Emotion;
  /** 0..1, clamped when read. */
  intensity: number;
  gesture?: Gesture;
  eyeLight?: number;
}

/** The cue before any line: neutral, still. */
export const NEUTRAL_CUE: PerformanceCue = Object.freeze({ id: 0, emotion: "neutral", intensity: 0 });

/**
 * What a renderer applies on top of the driver's `Pose` this frame. All offsets are zero and
 * `eyeLight` is 1 at rest, so a face that ignores the channels draws as it always did.
 */
export interface PerformancePose {
  /** Eye-fire intensity, 1 = the rig's own, 0 (out) .. 2.5 (blazing). */
  eyeLight: number;
  /** -1 scowl (brow down and in) .. 1 raised. */
  brow: number;
  /** Head offsets in radians, added to the pose's yaw, pitch and roll. */
  gestureYaw: number;
  gesturePitch: number;
  gestureRoll: number;
  /** -1 pulled back .. 1 leaning toward the viewer (a renderer scales and nudges the head). */
  lean: number;
  /** 0..1, a burst of eye fire (sparks, bloom) that decays. */
  flare: number;
}

export const PERFORMANCE_REST: PerformancePose = Object.freeze({
  eyeLight: 1,
  brow: 0,
  gestureYaw: 0,
  gesturePitch: 0,
  gestureRoll: 0,
  lean: 0,
  flare: 0,
});

/** Where a face reads its cue: woken by `subscribe`, read with `get` on its own frames. */
export interface PerformanceSource {
  get(): PerformanceCue;
  subscribe?(fn: (cue: PerformanceCue) => void): () => void;
}

/** A source the stage sets. `cue` stamps the next id. */
export interface PerformanceBox extends PerformanceSource {
  /** Set the cue for a line (its delivery), optionally with a gesture. */
  cue(emotion: Emotion, intensity: number, extra?: { gesture?: Gesture; eyeLight?: number }): void;
  subscribe(fn: (cue: PerformanceCue) => void): () => void;
}

/** STUB (WP-P): a box holding `NEUTRAL_CUE`. */
export function performanceBox(): PerformanceBox {
  let current: PerformanceCue = NEUTRAL_CUE;
  const subs = new Set<(cue: PerformanceCue) => void>();
  return {
    get: () => current,
    cue(emotion, intensity, extra) {
      current = { id: current.id + 1, emotion, intensity, ...extra };
      for (const fn of [...subs]) fn(current);
    },
    subscribe(fn) {
      subs.add(fn);
      return () => void subs.delete(fn);
    },
  };
}

/** The cue fields for a line's delivery (absent: neutral, 0). STUB (WP-P): no gesture. */
export function cueFor(delivery: Delivery | undefined): Omit<PerformanceCue, "id"> {
  if (!delivery) return { emotion: "neutral", intensity: 0 };
  return { emotion: delivery.emotion, intensity: delivery.intensity };
}

/** Turns cues into a pose over time: emotion eases in, gestures play as envelopes. */
export interface PerformanceDriver {
  /** Hand in the current cue; a new `id` starts its gesture. */
  setCue(cue: PerformanceCue): void;
  /** Advance by `dt` seconds and return the pose. */
  step(dt: number): PerformancePose;
  readonly pose: PerformancePose;
  /** True when no gesture plays and the emotion has eased in: the face loop may sleep. */
  readonly settled: boolean;
}

/** STUB (WP-P): always at rest. */
export function createPerformanceDriver(): PerformanceDriver {
  return {
    setCue() {},
    step: () => PERFORMANCE_REST,
    pose: PERFORMANCE_REST,
    settled: true,
  };
}
