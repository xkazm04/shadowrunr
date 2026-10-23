// The relit face's pure parts: the meshes' normals (a Sobel over the depth field), where its point
// lights sit, the jaw's hinge, the brow band, and how the look, the pose and the performance add
// up to one head turn. No WebGL here; renderer.ts reads these and sets its uniforms.

import type { Pose } from "../driver";
import type { LoadedForm, RigSpec } from "../layers";
import { sampleDepth, STRIDE, type DepthField, type Mesh } from "../depth/geom";
import type { LookPose } from "../depth/look";
import { HEAD_PITCH_MAX, HEAD_YAW_MAX } from "../depth/look";
import type { PerformancePose } from "../performance";

/**
 * The widest turn the relit head takes, degrees. The pitch goes past the depth face's 12; the yaw
 * stays at its 22, because the M8 art does not hold further: from about 20 degrees the leading
 * cheek and temple streak, and by 28 the far socket collapses and the brow tears (WP-A's
 * screenshots, runs/avatar-3d/wp-a/yaw-break-sheet.png).
 */
export const YAW_LIMIT = 22;
export const PITCH_LIMIT = 14;
/** The depth face's look follower turns the head 20 x 10 at a full look; the relit head turns to its limits. */
export const LOOK_GAIN: readonly [number, number] = [YAW_LIMIT / HEAD_YAW_MAX, PITCH_LIMIT / HEAD_PITCH_MAX];
/** A depth slope steeper than this is a silhouette, not a surface: its normal is held at ~72 degrees. */
export const MAX_SLOPE = 3;

const DEG = Math.PI / 180;
const clamp = (x: number, a: number, b: number) => (x < a ? a : x > b ? b : x);

/**
 * Rewrite each vertex's depth slope (`a_g`, floats 3 and 4 of `STRIDE`) with a 3 x 3 Sobel over
 * the field at `h` source px, clamped to `MAX_SLOPE`. The grid's own slope is a central difference
 * at its cell; the Sobel smooths across the cell, so the light does not show the grid.
 */
export function sobelSlopes(mesh: Mesh, field: DepthField, h: number): Mesh {
  const d = mesh.data;
  const at = (x: number, y: number) => sampleDepth(field, x, y);
  for (let k = 0; k < d.length; k += STRIDE) {
    const x = d[k]!;
    const y = d[k + 1]!;
    const a = at(x - h, y - h);
    const b = at(x, y - h);
    const c = at(x + h, y - h);
    const l = at(x - h, y);
    const r = at(x + h, y);
    const e = at(x - h, y + h);
    const f = at(x, y + h);
    const g = at(x + h, y + h);
    d[k + 3] = clamp((c + 2 * r + g - (a + 2 * l + e)) / (8 * h), -MAX_SLOPE, MAX_SLOPE);
    d[k + 4] = clamp((e + 2 * f + g - (a + 2 * b + c)) / (8 * h), -MAX_SLOPE, MAX_SLOPE);
  }
  return mesh;
}

/** A point light in the picture's rest frame: source px across and down, `z` toward the viewer. */
export interface PointLight {
  x: number;
  y: number;
  z: number;
  /** Inverse-square falloff: half strength at `r0` px, gone by `r1`. */
  r0: number;
  r1: number;
}

/** How far in front of its socket's rim the eye fire's light sits, and the monocle's ring in front of its eye, source px. */
const EYE_FORWARD = 12;
const RING_FORWARD = 46;

/** The nearest depth on a ring of radius `r` about (x, y): a socket's rim. */
function rimDepth(field: DepthField, x: number, y: number, r: number): number {
  let z = -Infinity;
  for (let i = 0; i < 16; i++) {
    const a = (i / 16) * 2 * Math.PI;
    z = Math.max(z, sampleDepth(field, x + r * Math.cos(a), y + r * Math.sin(a)));
  }
  return z;
}

/** The rig's lights: the eye fire in each eye layer's centre, and the monocle's ring (a `pulse` with a centre). */
export function rigLights(form: LoadedForm, field: DepthField): { eyes: PointLight[]; ring: PointLight | null } {
  const eyes: PointLight[] = [];
  let ring: PointLight | null = null;
  for (const l of form.spec.layers) {
    const bb = l.bbox;
    if (!bb) continue;
    const c = l.meta?.center ?? [bb[0] + bb[2] / 2, bb[1] + bb[3] / 2];
    if (l.kind === "eye") {
      const r = Math.max(bb[2], bb[3]) / 2;
      // the fire burns in the socket, but its light reaches the bone from the socket's mouth: a
      // light at the floor would sit behind the rim it is meant to light
      eyes.push({ x: c[0], y: c[1], z: rimDepth(field, c[0], c[1], 0.6 * r) + EYE_FORWARD, r0: 0.75 * r, r1: 4 * r });
    } else if (l.kind === "pulse" && l.meta?.center && !ring) {
      const r = l.meta.radius ?? Math.max(bb[2], bb[3]) / 4;
      ring = { x: c[0], y: c[1] + 0.4 * r, z: sampleDepth(field, c[0], c[1]) + RING_FORWARD, r0: 0.9 * r, r1: 3.2 * r };
    }
  }
  return { eyes: eyes.slice(0, 2), ring };
}

/**
 * The jaw's hinge: a level axis through the seam's height, `back` px behind the jaw's front, so an
 * opening swings the chin down and back as a jaw does. `angle(lift)` is the swing that opens the
 * front teeth by `lift` source px (the aperture's gap the mouth light fills).
 */
export function jawHinge(rig: RigSpec, field: DepthField): { y: number; z: number; back: number; angle(lift: number): number } | null {
  const ap = rig.aperture;
  if (!ap) return null;
  const front = sampleDepth(field, (ap.x0 + ap.x1) / 2, ap.seam_y + 12);
  const back = 0.7 * rig.head.radius[0];
  return { y: ap.seam_y, z: front - back, back, angle: (lift) => Math.asin(clamp(lift / back, 0, 1)) };
}

/** The brow band: a soft patch above each socket that `brow` moves (vertices only; the texture rides along). */
export interface BrowBand {
  /** Left and right patch centres, source px. */
  centres: [number, number, number, number];
  /** Patch half size, source px. */
  size: [number, number];
}

/** The brow band from the eye layers: centred `0.42` of an eye box above each eye. */
export function browBand(form: LoadedForm): BrowBand | null {
  const eyes = form.spec.layers.filter((l) => l.kind === "eye" && l.bbox);
  if (eyes.length < 2) return null;
  const [a, b] = eyes.map((l) => l.bbox!).sort((p, q) => p[0] - q[0]) as [[number, number, number, number], [number, number, number, number]];
  const w = (a[2] + b[2]) / 4;
  const h = (a[3] + b[3]) / 2;
  return {
    centres: [a[0] + a[2] / 2, a[1] + a[3] / 2 - 0.42 * h, b[0] + b[2] / 2, b[1] + b[3] / 2 - 0.42 * h],
    size: [1.05 * w, 0.3 * h],
  };
}

/** The head's whole turn this frame, degrees and a lean: the look (widened), the driver's pose and the gestures. */
export interface Turn {
  yaw: number;
  pitch: number;
  roll: number;
  lean: number;
}

/** Adds into `out`, clamped to the limits. */
export function headTurn(pose: Pick<Pose, "yaw" | "pitch" | "roll">, look: Pick<LookPose, "yaw" | "pitch">, perf: PerformancePose, out: Turn): Turn {
  out.yaw = clamp(look.yaw * LOOK_GAIN[0] + pose.yaw + perf.gestureYaw / DEG, -YAW_LIMIT, YAW_LIMIT);
  out.pitch = clamp(look.pitch * LOOK_GAIN[1] + pose.pitch + perf.gesturePitch / DEG, -PITCH_LIMIT, PITCH_LIMIT);
  out.roll = clamp(pose.roll + perf.gestureRoll / DEG, -12, 12);
  out.lean = clamp(perf.lean, -1, 1);
  return out;
}

/** The eye fire this frame: the layers' gain factor, their extra bloom, and the point lights' strength. */
export function eyeFire(perf: PerformancePose): { gain: number; bloom: number; light: number } {
  const e = clamp(Number.isFinite(perf.eyeLight) ? perf.eyeLight : 1, 0, 2.5);
  const f = clamp(perf.flare, 0, 1);
  return { gain: e * (1 + 0.8 * f), bloom: 1.2 * f, light: e * (1 + 1.6 * f) };
}
