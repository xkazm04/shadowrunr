// Where the depth face looks: the eyes lead, the head follows. Pure, stepped by the face loop.
//
// A `Look` (variants.ts) names a direction. The eyes turn toward it fast (about 95 % there in
// 120 ms, an exponential approach); the head follows on a critically damped spring (about 95 %
// in 400 ms), and turns only part of the way, so at rest the eyes still sit a little toward the
// target in their sockets. While the head catches up the eyes re-centre, as eyes do.

import type { Look } from "../variants";

/** Head turn at the edge of the look box, degrees (yaw right-positive, pitch down-positive). */
export const HEAD_YAW_MAX = 20;
export const HEAD_PITCH_MAX = 10;
/** The eyes' whole turn at the edge, degrees: the head's share plus what the eyes do alone. */
const EYE_YAW_MAX = 35;
const EYE_PITCH_MAX = 18;
/** Source px the eye light slides per degree the eyes are turned in the head, and its limits. */
const EYE_PX_PER_DEG = 0.42;
const EYE_PX_MAX: [number, number] = [10, 6];

const EYE_TAU = 0.04; // 1 - e^-3 = 95 % at 120 ms
const HEAD_OMEGA = 11.9; // (1 + wt) e^-wt = 5 % at 400 ms

const clamp = (x: number, a: number, b: number) => (x < a ? a : x > b ? b : x);

export interface LookPose {
  /** Degrees. */
  yaw: number;
  pitch: number;
  /** The eye light's slide in its socket, source px. */
  eyeX: number;
  eyeY: number;
}

export interface LookFollower {
  /** Ease toward `look` by `dt` seconds. */
  step(dt: number, look: Look): LookPose;
  /** Jump to `look` (a still frame). */
  snap(look: Look): LookPose;
  /** True when the eyes and the head have arrived at `look` and stopped. */
  settledAt(look: Look): boolean;
  readonly pose: LookPose;
}

export function createLookFollower(): LookFollower {
  let eyeYaw = 0;
  let eyePitch = 0;
  let yaw = 0;
  let pitch = 0;
  let vYaw = 0;
  let vPitch = 0;
  const pose: LookPose = { yaw: 0, pitch: 0, eyeX: 0, eyeY: 0 };

  const target = (look: Look) => {
    const x = clamp(Number.isFinite(look.x) ? look.x : 0, -1, 1);
    const y = clamp(Number.isFinite(look.y) ? look.y : 0, -1, 1);
    return { eyeYaw: x * EYE_YAW_MAX, eyePitch: y * EYE_PITCH_MAX, yaw: x * HEAD_YAW_MAX, pitch: y * HEAD_PITCH_MAX };
  };

  const write = () => {
    pose.yaw = yaw;
    pose.pitch = pitch;
    pose.eyeX = clamp((eyeYaw - yaw) * EYE_PX_PER_DEG, -EYE_PX_MAX[0], EYE_PX_MAX[0]);
    pose.eyeY = clamp((eyePitch - pitch) * EYE_PX_PER_DEG, -EYE_PX_MAX[1], EYE_PX_MAX[1]);
    return pose;
  };

  /** One axis of the critically damped spring, exact for a constant target over `dt`. */
  const spring = (x: number, v: number, to: number, dt: number): [number, number] => {
    const w = HEAD_OMEGA;
    const d = x - to;
    const e = Math.exp(-w * dt);
    const c = v + w * d;
    return [to + (d + c * dt) * e, (v - w * c * dt) * e];
  };

  return {
    step(dtIn, look) {
      const dt = clamp(Number.isFinite(dtIn) ? dtIn : 0, 0, 0.1);
      const t = target(look);
      const k = 1 - Math.exp(-dt / EYE_TAU);
      eyeYaw += (t.eyeYaw - eyeYaw) * k;
      eyePitch += (t.eyePitch - eyePitch) * k;
      [yaw, vYaw] = spring(yaw, vYaw, t.yaw, dt);
      [pitch, vPitch] = spring(pitch, vPitch, t.pitch, dt);
      return write();
    },
    snap(look) {
      const t = target(look);
      eyeYaw = t.eyeYaw;
      eyePitch = t.eyePitch;
      yaw = t.yaw;
      pitch = t.pitch;
      vYaw = 0;
      vPitch = 0;
      return write();
    },
    settledAt(look) {
      const t = target(look);
      return (
        Math.abs(eyeYaw - t.eyeYaw) < 0.05 &&
        Math.abs(eyePitch - t.eyePitch) < 0.05 &&
        Math.abs(yaw - t.yaw) < 0.05 &&
        Math.abs(pitch - t.pitch) < 0.05 &&
        Math.abs(vYaw) < 0.05 &&
        Math.abs(vPitch) < 0.05
      );
    },
    get pose() {
      return pose;
    },
  };
}

/** Whether two looks differ enough to wake a still face. */
export function lookMoved(a: Look, b: Look): boolean {
  return Math.abs(a.x - b.x) > 1e-3 || Math.abs(a.y - b.y) > 1e-3;
}
