// Where the cut-out looks: the seam's `look` eased onto the driver's pose. Pure, no DOM.
//
// The eyes lead and the head follows: each eases toward the look on its own critically damped
// spring (no overshoot, no jump in speed), the eyes reaching 90% in about 120 ms and the head in
// about 400 ms. The driver's own motion (drift, saccades, nods, speech sway) is added on top, so
// with no look (or the look straight ahead) the pose is the driver's, unchanged.

import type { Driver, Pose } from "../driver";
import type { FaceSignal } from "../types";
import type { Look, LookSource } from "../variants";

/** Degrees of head turn at a full look (|x| or |y| = 1), before the driver's motion. */
export const LOOK_YAW = 10;
export const LOOK_PITCH = 4.5;
/** The most the head turns, look and driver together. */
export const YAW_MAX = 12;
export const PITCH_MAX = 6;
/** Seconds to 90% of a step, eyes and head. */
export const EYES_T90 = 0.12;
export const HEAD_T90 = 0.4;
/** Art px the eye lights shift at a full gaze (x, y). The glow renderer shifts 3.5 and 2 for the
 *  driver's micro-saccades; the cut-out keeps those as they were and gives the look the rest. */
export const EYE_PX: readonly [number, number] = [9, 6];
/** Art px the driver's own gaze (its micro-saccades) means at a full gaze: the glow renderer's scale. */
const DRIVER_PX: readonly [number, number] = [3.5, 2];

/**
 * How a face maps the look: the renderer's px per unit of `pose.gaze` (`eyePx`), the px a full look
 * slides the eye light (`lookPx`), the head turn at a full look and the most the head turns, look
 * and driver together. The driver's saccades keep their 3.5 x 2 px under every mapping.
 */
export interface LookUnits {
  eyePx: readonly [number, number];
  lookPx: readonly [number, number];
  lookYaw: number;
  lookPitch: number;
  yawMax: number;
  pitchMax: number;
}

/** The cut-out's mapping: its eye lights travel EYE_PX, its head turns LOOK_YAW x LOOK_PITCH. */
export const CUT_LOOK: LookUnits = Object.freeze({ eyePx: EYE_PX, lookPx: EYE_PX, lookYaw: LOOK_YAW, lookPitch: LOOK_PITCH, yawMax: YAW_MAX, pitchMax: PITCH_MAX });

/** A critically damped spring's step response reaches 90% at w*t = 3.8897. */
const W90 = 3.8897;

export interface Spring {
  x: number;
  v: number;
}

/**
 * Advance a critically damped spring toward `target` by `dt` s, tuned so a step reaches 90% in
 * `t90` s. Exact for any `dt`, so the ease does not depend on the frame rate.
 */
export function springStep(s: Spring, target: number, dt: number, t90: number): void {
  const w = W90 / Math.max(1e-4, t90);
  const y = s.x - target;
  const k = s.v + w * y;
  const e = Math.exp(-w * dt);
  s.x = target + (y + k * dt) * e;
  s.v = (s.v - w * k * dt) * e;
}

const clamp = (x: number, a: number, b: number) => (x < a ? a : x > b ? b : x);
const finite = (x: number) => (Number.isFinite(x) ? x : 0);

/** A look made safe: finite, each axis in -1..1. */
export function lookTarget(look: Look | null | undefined): Look {
  if (!look) return { x: 0, y: 0 };
  return { x: clamp(finite(look.x), -1, 1), y: clamp(finite(look.y), -1, 1) };
}

/**
 * The head turn a (head-eased) look asks for, plus the driver's own motion, held inside the
 * cut-out's range. `x` right is yaw right, `y` down is pitch down (the pose's signs).
 */
export function headAngles(look: Look, driverYaw: number, driverPitch: number, units: LookUnits = CUT_LOOK): { yaw: number; pitch: number } {
  return {
    yaw: clamp(look.x * units.lookYaw + driverYaw, -units.yawMax, units.yawMax),
    pitch: clamp(look.y * units.lookPitch + driverPitch, -units.pitchMax, units.pitchMax),
  };
}

const SETTLE = 1e-3;

export interface LookDriver extends Driver {
  /** The eased look of the eyes and of the head, for tests and the harness. */
  readonly eyes: Look;
  readonly head: Look;
  /** Whether the look's target moved since the last call (and remember the new one). */
  poll(): boolean;
}

/**
 * Wrap `base` so every pose it returns also looks where `source` says. `step` reads the look each
 * frame; `rest` jumps straight to it (what reduced motion draws); `settled` waits for both springs
 * as well as the driver. Without a source the look is straight ahead and the pose is `base`'s,
 * with its gaze in `units.eyePx` (the cut-out's `EYE_PX` by default), so the eyes move exactly as
 * the glow's do. The look only says where to go: the face wakes on the source's event, not here.
 */
export function withLook(base: Driver, source?: LookSource, units: LookUnits = CUT_LOOK): LookDriver {
  const baseGaze = [DRIVER_PX[0] / units.eyePx[0], DRIVER_PX[1] / units.eyePx[1]] as const;
  const lookGaze = [units.lookPx[0] / units.eyePx[0], units.lookPx[1] / units.eyePx[1]] as const;
  const eyes = { x: { x: 0, v: 0 }, y: { x: 0, v: 0 } };
  const head = { x: { x: 0, v: 0 }, y: { x: 0, v: 0 } };
  let target: Look = { x: 0, y: 0 };
  const read = () => (source ? lookTarget(source.get()) : { x: 0, y: 0 });

  const apply = (pose: Pose): Pose => {
    const a = headAngles({ x: head.x.x, y: head.y.x }, pose.yaw, pose.pitch, units);
    pose.yaw = a.yaw;
    pose.pitch = a.pitch;
    pose.gazeX = clamp(pose.gazeX * baseGaze[0] + eyes.x.x * lookGaze[0], -1.2, 1.2);
    pose.gazeY = clamp(pose.gazeY * baseGaze[1] + eyes.y.x * lookGaze[1], -1.2, 1.2);
    return pose;
  };

  const springsSettled = () =>
    [eyes.x, eyes.y, head.x, head.y].every((s) => Math.abs(s.v) < SETTLE) &&
    Math.abs(eyes.x.x - target.x) < SETTLE &&
    Math.abs(eyes.y.x - target.y) < SETTLE &&
    Math.abs(head.x.x - target.x) < SETTLE &&
    Math.abs(head.y.x - target.y) < SETTLE;

  const snapAll = () => {
    for (const [s, t] of [
      [eyes.x, target.x],
      [eyes.y, target.y],
      [head.x, target.x],
      [head.y, target.y],
    ] as const) {
      s.x = t;
      s.v = 0;
    }
  };

  return {
    setSignal: (signal: FaceSignal) => base.setSignal(signal),
    step(dt) {
      const pose = base.step(dt);
      target = read();
      const d = clamp(Number.isFinite(dt) ? dt : 0, 0, 0.1);
      springStep(eyes.x, target.x, d, EYES_T90);
      springStep(eyes.y, target.y, d, EYES_T90);
      springStep(head.x, target.x, d, HEAD_T90);
      springStep(head.y, target.y, d, HEAD_T90);
      if (springsSettled()) snapAll();
      return apply(pose);
    },
    rest() {
      const pose = base.rest();
      target = read();
      snapAll();
      return apply(pose);
    },
    get pose() {
      return base.pose;
    },
    get settled() {
      return base.settled && (!source || springsSettled());
    },
    get eyes() {
      return { x: eyes.x.x, y: eyes.y.x };
    },
    get head() {
      return { x: head.x.x, y: head.y.x };
    },
    poll() {
      if (!source) return false;
      const next = read();
      const moved = next.x !== target.x || next.y !== target.y;
      target = next;
      return moved;
    },
  };
}
