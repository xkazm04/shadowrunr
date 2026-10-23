// How Malakor performs beyond his mouth (spark avatar-3d): eye-light intensity, brow and head
// gestures, set from the delivery of the line he is speaking. The stage keeps each `say` event's
// `delivery` by its seq and sets a `PerformanceCue` when that line's audio starts, so the face
// changes on the audio clock, not when the text arrives. Every face reads the same cue through a
// `PerformanceSource` and turns it into a `PerformancePose` with its own `PerformanceDriver`.
//
// **The emotion table** (`EMOTION_TABLE`) is the one place an emotion means something: its eye
// light, its brow and, for a strong line (intensity >= GESTURE_MIN), its gesture. Intensity scales
// the distance from rest, so intensity 0 is the rest pose for every emotion.
//
// **The driver** eases eye light and brow on critically damped springs (eye light flares up faster
// than it dies down) and plays each gesture once as an envelope, started by a new cue id. It is
// pure and deterministic given the dt sequence, and it settles: at rest, or fully eased with no
// envelope playing, it reports `settled` and the face loop may sleep. Under reduced motion the head
// gestures shrink to REDUCED_GESTURE; the light still changes.
//
// The wire-level types (cue, pose, source) were fixed at P0 (Director); WP-P filled the bodies.

import type { Delivery, Emotion } from "@shadowrunr/contract";

import { springStep, type Spring } from "./cut/look";
import type { Driver, Pose } from "./driver";

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

/** A box holding `NEUTRAL_CUE` until the stage sets one; every `cue` is news (a new id). */
export function performanceBox(): PerformanceBox {
  let current: PerformanceCue = NEUTRAL_CUE;
  const subs = new Set<(cue: PerformanceCue) => void>();
  return {
    get: () => current,
    cue(emotion, intensity, extra) {
      const next: PerformanceCue = { id: current.id + 1, emotion, intensity };
      if (extra?.gesture !== undefined) next.gesture = extra.gesture;
      if (extra?.eyeLight !== undefined) next.eyeLight = extra.eyeLight;
      current = next;
      for (const fn of [...subs]) fn(current);
    },
    subscribe(fn) {
      subs.add(fn);
      return () => void subs.delete(fn);
    },
  };
}

/** A source that forwards whichever source it was last `set` to (none: the neutral cue). */
export interface PerformanceRelay extends PerformanceSource {
  /** Read `source` from now on; subscribers hear its cue at once, and its cues after. */
  set(source: PerformanceSource | undefined): void;
  subscribe(fn: (cue: PerformanceCue) => void): () => void;
}

/** The relay `<Avatar>` hands its face: the face keeps one source while the stage's may change. */
export function performanceRelay(): PerformanceRelay {
  let source: PerformanceSource | undefined;
  let off: (() => void) | undefined;
  const subs = new Set<(cue: PerformanceCue) => void>();
  const get = () => source?.get() ?? NEUTRAL_CUE;
  const tell = (cue: PerformanceCue) => {
    for (const fn of [...subs]) fn(cue);
  };
  return {
    get,
    set(next) {
      if (next === source) return;
      off?.();
      source = next;
      off = next?.subscribe?.(tell);
      tell(get());
    },
    subscribe(fn) {
      subs.add(fn);
      return () => void subs.delete(fn);
    },
  };
}

/** What one emotion does at full intensity. `then` is a second gesture that follows the first. */
export interface EmotionLook {
  /** Eye light at intensity 1 (1 = the rig's own). */
  eyeLight: number;
  /** Brow at intensity 1, -1 scowl .. 1 raised. */
  brow: number;
  /** The gesture of a strong line (intensity >= GESTURE_MIN). */
  gesture?: Gesture;
  then?: Gesture;
}

/**
 * The emotions, once. Read across: how bright the eye fire burns, where the brow sits, and what
 * the head does on a strong line. The rig has one brow channel, so sarcasm's single raised brow is
 * both brows half raised here; a face that can raise one may read `emotion` itself.
 */
export const EMOTION_TABLE: Readonly<Record<Emotion, EmotionLook>> = Object.freeze({
  //          eye light   brow    gesture (strong lines)
  neutral: { eyeLight: 1.0, brow: 0 },
  menace: { eyeLight: 1.6, brow: -0.7, gesture: "glare" },
  glee: { eyeLight: 1.5, brow: 0.4, gesture: "tilt" },
  deadpan: { eyeLight: 0.8, brow: 0 },
  hushed: { eyeLight: 0.6, brow: 0.2, gesture: "lean_in" },
  outrage: { eyeLight: 2.2, brow: -1, gesture: "recoil", then: "flare" },
  sarcasm: { eyeLight: 1.1, brow: 0.5, gesture: "tilt" },
  weary: { eyeLight: 0.5, brow: -0.2, gesture: "nod" },
  triumph: { eyeLight: 2.0, brow: 0.3, gesture: "flare" },
  tender: { eyeLight: 0.9, brow: 0.6, gesture: "lean_in" },
});

/** A line at least this intense earns its emotion's gesture. */
export const GESTURE_MIN = 0.6;

const clamp = (x: number, a: number, b: number) => (x < a ? a : x > b ? b : x);
const clamp01 = (x: number) => clamp(Number.isFinite(x) ? x : 0, 0, 1);

/** The cue fields for a line's delivery (absent: neutral, 0). A strong line gets its gesture. */
export function cueFor(delivery: Delivery | undefined): Omit<PerformanceCue, "id"> {
  if (!delivery) return { emotion: "neutral", intensity: 0 };
  const intensity = clamp01(delivery.intensity);
  const row = EMOTION_TABLE[delivery.emotion] ?? EMOTION_TABLE.neutral;
  const cue: Omit<PerformanceCue, "id"> = { emotion: delivery.emotion, intensity };
  if (row.gesture && intensity >= GESTURE_MIN) cue.gesture = row.gesture;
  return cue;
}

/** The eye light and brow a cue settles to, with no gesture playing. */
export function cueTarget(cue: PerformanceCue): { eyeLight: number; brow: number } {
  const row = EMOTION_TABLE[cue.emotion] ?? EMOTION_TABLE.neutral;
  const i = clamp01(cue.intensity);
  const eye = cue.eyeLight !== undefined && Number.isFinite(cue.eyeLight) ? cue.eyeLight : 1 + (row.eyeLight - 1) * i;
  return { eyeLight: clamp(eye, 0, 2.5), brow: clamp(row.brow * i, -1, 1) || 0 };
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

/** Seconds to 90% of a step: the brow, the eye light flaring up and dying down. */
export const BROW_T90 = 0.25;
export const EYE_UP_T90 = 0.12;
export const EYE_DOWN_T90 = 0.4;
/** Head gestures under reduced motion are scaled to this; the light is not. */
export const REDUCED_GESTURE = 0.3;
/** A follow-up gesture (`then`) starts this long after the first. */
const THEN_S = 0.16;

/**
 * One gesture's shape: rise over `a` s, hold `h`, fall `r`; at full envelope the channels move by
 * these amounts (head radians, pitch down-positive, lean toward the viewer, eye light added, brow
 * pulled toward `brow`, flare). A gesture's amplitude is 0.5 + 0.5 x the cue's intensity.
 */
interface GestureShape {
  a: number;
  h: number;
  r: number;
  pitch?: number;
  yaw?: number;
  roll?: number;
  lean?: number;
  eye?: number;
  brow?: number;
  flare?: number;
}

export const GESTURE_SHAPES: Readonly<Record<Gesture, GestureShape>> = Object.freeze({
  // a dip of the chin and back
  nod: { a: 0.15, h: 0, r: 0.45, pitch: 0.09 },
  // the head cocked, held, then straight again
  tilt: { a: 0.25, h: 0.45, r: 0.45, roll: 0.12, yaw: 0.02 },
  // toward the viewer, confiding
  lean_in: { a: 0.3, h: 0.5, r: 0.4, lean: 1, pitch: 0.03 },
  // pulled back and up, fast
  recoil: { a: 0.08, h: 0.25, r: 0.5, lean: -1, pitch: -0.1 },
  // chin down, brow down hard, the fire up
  glare: { a: 0.2, h: 0.5, r: 0.5, pitch: 0.05, brow: -1, eye: 0.6 },
  // a burst of eye fire that dies away
  flare: { a: 0.05, h: 0, r: 0.8, flare: 1, eye: 0.8 },
});

const smooth = (x: number) => {
  const y = clamp(x, 0, 1);
  return y * y * (3 - 2 * y);
};

/** A gesture's envelope `t` s after it started; past its end it is done (-1). */
function envelope(s: GestureShape, t: number): number {
  if (t < 0) return 0;
  if (t < s.a) return smooth(t / s.a);
  if (t < s.a + s.h) return 1;
  if (t < s.a + s.h + s.r) return 1 - smooth((t - s.a - s.h) / s.r);
  return -1;
}

const SETTLE = 1e-3;

/**
 * The driver. `reduced` (read on each gesture start) shrinks the head gestures to REDUCED_GESTURE,
 * as the face's reduced-motion setting asks; the light still changes in full.
 */
export function createPerformanceDriver(opts: { reduced?: () => boolean } = {}): PerformanceDriver & { rest(cue: PerformanceCue): PerformancePose } {
  const pose: PerformancePose = { ...PERFORMANCE_REST };
  const eye: Spring = { x: 1, v: 0 };
  const brow: Spring = { x: 0, v: 0 };
  let target = { eyeLight: 1, brow: 0 };
  let cueId = NEUTRAL_CUE.id;
  let t = 0;
  // the gestures playing: started at `at` (face time), `amp` for the head, `light` for the light
  const playing: { g: Gesture; at: number; amp: number; light: number }[] = [];

  const springsAt = () => Math.abs(eye.x - target.eyeLight) < SETTLE && Math.abs(eye.v) < SETTLE && Math.abs(brow.x - target.brow) < SETTLE && Math.abs(brow.v) < SETTLE;

  function setCue(cue: PerformanceCue) {
    if (cue.id === cueId) return;
    cueId = cue.id;
    target = cueTarget(cue);
    if (!cue.gesture) return;
    const light = 0.5 + 0.5 * clamp01(cue.intensity);
    const amp = light * (opts.reduced?.() ? REDUCED_GESTURE : 1);
    playing.push({ g: cue.gesture, at: t, amp, light });
    const then = EMOTION_TABLE[cue.emotion]?.then;
    if (then && EMOTION_TABLE[cue.emotion]?.gesture === cue.gesture) playing.push({ g: then, at: t + THEN_S, amp, light });
  }

  function step(dtIn: number): PerformancePose {
    const dt = clamp(Number.isFinite(dtIn) ? dtIn : 0, 0, 0.1);
    t += dt;
    springStep(eye, target.eyeLight, dt, target.eyeLight > eye.x ? EYE_UP_T90 : EYE_DOWN_T90);
    springStep(brow, target.brow, dt, BROW_T90);
    if (springsAt()) {
      // snap the last crumbs so a settled pose is exactly its target (exactly rest at rest)
      eye.x = target.eyeLight;
      eye.v = 0;
      brow.x = target.brow;
      brow.v = 0;
    }

    let yaw = 0;
    let pitch = 0;
    let roll = 0;
    let lean = 0;
    let eyeAdd = 0;
    let flare = 0;
    let b = brow.x;
    for (let i = playing.length - 1; i >= 0; i--) {
      const p = playing[i]!;
      const s = GESTURE_SHAPES[p.g];
      const e = envelope(s, t - p.at);
      if (e < 0) {
        playing.splice(i, 1);
        continue;
      }
      const k = e * p.amp;
      const l = e * p.light;
      yaw += (s.yaw ?? 0) * k;
      pitch += (s.pitch ?? 0) * k;
      roll += (s.roll ?? 0) * k;
      lean += (s.lean ?? 0) * k;
      eyeAdd += (s.eye ?? 0) * l;
      flare = Math.max(flare, (s.flare ?? 0) * l);
      if (s.brow !== undefined) b += (s.brow - b) * l;
    }

    pose.eyeLight = clamp(eye.x + eyeAdd, 0, 2.5);
    pose.brow = clamp(b, -1, 1);
    // 0, never -0, so a settled pose compares equal to the rest pose
    pose.gestureYaw = yaw || 0;
    pose.gesturePitch = pitch || 0;
    pose.gestureRoll = roll || 0;
    pose.lean = clamp(lean, -1, 1) || 0;
    pose.flare = clamp(flare, 0, 1);
    return pose;
  }

  return {
    setCue,
    step,
    /** Jump to `cue`'s settled pose, dropping any gesture: what a still frame draws. */
    rest(cue) {
      cueId = cue.id;
      target = cueTarget(cue);
      playing.length = 0;
      eye.x = target.eyeLight;
      eye.v = 0;
      brow.x = target.brow;
      brow.v = 0;
      return performanceAt(cue, pose);
    },
    get pose() {
      return pose;
    },
    get settled() {
      return playing.length === 0 && springsAt();
    },
  };
}

/** The pose a cue settles to with nothing playing: what a still (reduced-motion) frame draws. */
export function performanceAt(cue: PerformanceCue, out: PerformancePose = { ...PERFORMANCE_REST }): PerformancePose {
  const target = cueTarget(cue);
  out.eyeLight = target.eyeLight;
  out.brow = target.brow;
  out.gestureYaw = out.gesturePitch = out.gestureRoll = out.lean = out.flare = 0;
  return out;
}

/** A pose that may carry the performance: the face drivers hang it on the pose they draw. */
export type PerformedPose = Pose & { perf?: PerformancePose };

const DEG = 180 / Math.PI;

/** Wake `wake` on each new cue from `source`; the returned function stops listening. */
export function onPerformanceChange(source: PerformanceSource | undefined, wake: () => void): () => void {
  return source?.subscribe?.(() => wake()) ?? (() => {});
}

/**
 * Wrap a face driver so it performs `source`'s cue: each step reads the cue, steps a performance
 * driver, adds the head gestures (radians, to the pose's degrees) and hangs the performance on the
 * pose as `perf`, which `layerFrame` reads for the eye lights. `settled` waits for both drivers;
 * `rest` draws the cue's settled light with no gesture. Without a source the driver is returned
 * untouched, so the face draws exactly as it did.
 */
export function withPerformance<D extends Driver>(inner: D, source: PerformanceSource | undefined, opts: { reduced?: () => boolean } = {}): D {
  if (!source) return inner;
  const perf = createPerformanceDriver(opts);
  const hang = (pose: Pose, p: PerformancePose): Pose => {
    (pose as PerformedPose).perf = p;
    if (p.gestureYaw) pose.yaw += p.gestureYaw * DEG;
    if (p.gesturePitch) pose.pitch += p.gesturePitch * DEG;
    if (p.gestureRoll) pose.roll += p.gestureRoll * DEG;
    return pose;
  };
  // the inner driver's other members (its pose, a look) read through unchanged
  const wrapped = Object.create(inner) as D;
  Object.defineProperties(wrapped, {
    step: {
      value(dt: number) {
        const pose = inner.step(dt);
        perf.setCue(source.get());
        return hang(pose, perf.step(dt));
      },
    },
    rest: {
      value() {
        return hang(inner.rest(), perf.rest(source.get()));
      },
    },
    settled: {
      get() {
        return inner.settled && perf.settled;
      },
    },
  });
  return wrapped;
}
