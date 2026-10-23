// What the lich3d face does with each channel: the driver's `Pose`, the look and the
// `PerformancePose`, folded into one `LichFrame` the renderer applies. Pure, so the mapping is
// tested without a GPU.
//
// The mapping table (every channel, mapped or said to be ignored):
//
// | channel                      | lich3d                                                               |
// | ---------------------------- | -------------------------------------------------------------------- |
// | pose yaw / pitch / roll      | the head about `neck` (deg -> rad), with the look and the gestures   |
// | look (eyes lead, head after) | head turn up to LOOK_YAW x LOOK_PITCH; the eye fire slides in socket |
// | gestureYaw / Pitch / Roll    | added to the head, radians                                           |
// | (sum)                        | held inside +-35 deg yaw, +-18 deg pitch, +-20 deg roll              |
// | blinkL / blinkR              | a dip of that socket's light (a skull has no lids)                   |
// | gazeX / gazeY                | the eye fire slides within its socket                                |
// | open                         | the jaw about its hinge, up to `jaw_open_rad`                        |
// | voice                        | the eye fire flickers harder                                         |
// | activity                     | the crystals' float and orbit (0 at rest: the loop can sleep)        |
// | eyeLight                     | socket point lights and eye fire, x0..x2.5                           |
// | brow                         | a skull has no brow: a small head pitch (scowl = chin down) and the  |
// |                              | eye fire's shape (scowl = a narrow, wide slit; raised = tall)        |
// | lean                         | the head comes toward the camera and grows a little (or pulls back)  |
// | flare                        | bloom up, crystal veins pulse, eye fire spikes                       |
// | history                      | ignored                                                              |

import type { Pose } from "../driver";
import type { PerformancePose } from "../performance";

const DEG = Math.PI / 180;

/** The head's range, look, driver and gestures together. */
export const YAW_MAX_DEG = 35;
export const PITCH_MAX_DEG = 18;
export const ROLL_MAX_DEG = 20;
/** The look follower (`../depth/look`) turns the head up to 20 x 10 deg; the 3D head goes further. */
export const LOOK_GAIN = 1.25;
/** Head pitch per unit of brow, degrees: a scowl lowers the chin. */
export const BROW_PITCH_DEG = 4;
/** How far a blink dims the socket. */
const BLINK_DIP = 0.85;
/** The look follower's eye slide limits, px (`../depth/look` EYE_PX_MAX). */
const LOOK_EYE_PX: readonly [number, number] = [10, 6];

/** Where the depth look follower has the head and the eyes this frame. */
export interface LichLook {
  /** Degrees. */
  yaw: number;
  pitch: number;
  /** The eyes' slide in the head, the follower's px. */
  eyeX: number;
  eyeY: number;
}

export interface LichFrame {
  /** Face clock, seconds. */
  t: number;
  /** Radians in three.js's frame: +yaw turns the face to the viewer's right, +pitch lowers it,
   *  +roll turns it counter-clockwise as the viewer sees it. */
  yaw: number;
  pitch: number;
  roll: number;
  /** Jaw opening about the hinge, radians. */
  jaw: number;
  /** Socket light, 1 = the rig's own. */
  eyeL: number;
  eyeR: number;
  /** The eye fire's slide in its socket, -1..1 of its travel (+x viewer's right, +y up). */
  flameX: number;
  flameY: number;
  /** The eye fire's shape: height and width scale. */
  flameH: number;
  flameW: number;
  /** Flicker depth from the voice, 0..1. */
  flicker: number;
  /** -1..1. */
  lean: number;
  /** 0..1. */
  flare: number;
  /** How alive the crystals' float is, 0..1. */
  float: number;
}

const clamp = (x: number, a: number, b: number) => (x < a ? a : x > b ? b : x);
const num = (x: number, fallback = 0) => (Number.isFinite(x) ? x : fallback);

export function emptyFrame(): LichFrame {
  return { t: 0, yaw: 0, pitch: 0, roll: 0, jaw: 0, eyeL: 1, eyeR: 1, flameX: 0, flameY: 0, flameH: 1, flameW: 1, flicker: 0, lean: 0, flare: 0, float: 0 };
}

/** Fold the channels into `out` (reused between frames). `look` null: straight out. */
export function lichFrame(pose: Pose, perf: PerformancePose, look: LichLook | null, jawOpenRad: number, out: LichFrame = emptyFrame()): LichFrame {
  const brow = clamp(num(perf.brow), -1, 1);
  const yawDeg = num(pose.yaw) + (look ? num(look.yaw) * LOOK_GAIN : 0);
  const pitchDeg = num(pose.pitch) + (look ? num(look.pitch) * LOOK_GAIN : 0) - brow * BROW_PITCH_DEG;
  out.t = num(pose.t);
  out.yaw = clamp(yawDeg * DEG + num(perf.gestureYaw), -YAW_MAX_DEG * DEG, YAW_MAX_DEG * DEG);
  out.pitch = clamp(pitchDeg * DEG + num(perf.gesturePitch), -PITCH_MAX_DEG * DEG, PITCH_MAX_DEG * DEG);
  // the pose's roll is clockwise-positive as the viewer sees it; three's is counter-clockwise
  out.roll = clamp(-num(pose.roll) * DEG - num(perf.gestureRoll), -ROLL_MAX_DEG * DEG, ROLL_MAX_DEG * DEG);
  out.jaw = clamp(num(pose.open), 0, 1) * jawOpenRad;

  const light = clamp(num(perf.eyeLight, 1), 0, 2.5);
  out.eyeL = light * (1 - BLINK_DIP * clamp(num(pose.blinkL), 0, 1));
  out.eyeR = light * (1 - BLINK_DIP * clamp(num(pose.blinkR), 0, 1));

  const lx = look ? num(look.eyeX) / LOOK_EYE_PX[0] : 0;
  const ly = look ? num(look.eyeY) / LOOK_EYE_PX[1] : 0;
  // gaze y and the look's y are down-positive; the flame's is up-positive
  out.flameX = clamp(num(pose.gazeX) * 0.5 + lx * 0.7, -1, 1);
  out.flameY = clamp(-(num(pose.gazeY) * 0.5 + ly * 0.7), -1, 1);
  out.flameH = brow < 0 ? 1 + 0.45 * brow : 1 + 0.2 * brow;
  out.flameW = 1 - 0.15 * brow;

  out.flicker = clamp(num(pose.voice), 0, 1.2) / 1.2;
  out.lean = clamp(num(perf.lean), -1, 1);
  out.flare = clamp(num(perf.flare), 0, 1);
  out.float = clamp(num(pose.activity), 0, 1);
  return out;
}
