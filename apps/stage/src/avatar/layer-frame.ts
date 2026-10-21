// The rig's per-layer animation rules, once, for every face renderer (glow, cut, depth): what one
// layer looks like in one pose. How the seam brightens with the voice, how a pulse throbs, how an
// eye blinks as an iris or dims as a facet and warms while he speaks, where its light slides with
// the gaze, how a floating part drifts, how far the hinged aperture opens and whether the light in
// its slit is drawn at all. Pure numbers, no GPU: each renderer reads the frame and sets its own
// uniforms, and keeps only what is genuinely its own (its passes, its meshes, its composite).
//
// Where the renderers differ in a rule, the difference is data - a `FaceMapping` - not drifted
// code. The table, and why each row is what it is:
//
//   channel        glow                       cut                          depth
//   gaze (px)      3.5 x 2 (micro-saccades)   EYE_PX 9 x 6 (saccades and   3.5 x 2, plus the look
//                                             the seam's look, cut/look)   follower's eye slide
//   u_follow       layer's follow, else 0     the cut plan's follow        floats: layer's follow
//                  for static/body, 1         (cut/plan.ts)                (else 1); the rest 1 (one
//                                                                          head mesh, it all turns)
//   ring (halo)    breathes and warms         not drawn as a ring          not drawn (no halo mesh)
//                  with the voice
//
// Everything that moves by time is scaled by the pose's `activity`, so a settled face draws
// exactly its rest frame. No allocation per frame: the caller hands in the frame to fill.

import type { Pose } from "./driver";
import { EYE_PX } from "./cut/look";
import { gapCentre, gapHalf } from "./glow-geom";
import type { LayerSpec, RigSpec } from "./layers";

type Aperture = NonNullable<RigSpec["aperture"]>;
type Box = [number, number, number, number];

/** A floating part's drift: its sway in x, bob in y and rock, each on its own phase. */
export interface FloatMotion {
  amp: [number, number];
  freq: number;
  /** Radians. */
  rot: number;
  pivot: [number, number];
  phase: number;
  phase2: number;
}

/** What a renderer keeps per layer for the rules: made once at build time by `describeLayer`. */
export interface LayerDraw {
  spec: LayerSpec;
  /** The shaders' `u_kind` (glow-shaders.ts KIND). */
  kind: number;
  float?: FloatMotion;
  /** The eye light's centre, source px. */
  eyeC?: [number, number];
  /** A right eye (blinks with blinkR); any name not ending `_l`. */
  right?: boolean;
  /** The follow a planner chose for this layer (the cut plan), when the mapping reads it. */
  follow?: number;
}

/** How one renderer maps the channels the rules share (see the table above). */
export interface FaceMapping {
  /** Source px the eye light slides at a full gaze (x, y). */
  gazePx: readonly [number, number];
  /** Where `u_follow` comes from: the layer (glow), the draw's planned follow (cut), or only a float's (depth). */
  follow: "layer" | "planned" | "floats";
  /** Whether a halo layer (kind 9) breathes and warms with the voice. */
  ringBreath: boolean;
}

export const GLOW_MAPPING: FaceMapping = Object.freeze({ gazePx: [3.5, 2.0] as const, follow: "layer", ringBreath: true });
export const CUT_MAPPING: FaceMapping = Object.freeze({ gazePx: EYE_PX, follow: "planned", ringBreath: false });
export const DEPTH_MAPPING: FaceMapping = Object.freeze({ gazePx: [3.5, 2.0] as const, follow: "floats", ringBreath: false });

/** The rig's state this frame, shared by every layer: the aperture and how far it is open. */
export interface FaceRig {
  aperture: Aperture | undefined;
  /** Whether the rig has an upper lip that lifts too (the gap is then shared). */
  upperLip: boolean;
  /** This frame's `apertureLift`. */
  lift: number;
  /** An eye slide added to the gaze (the depth face's look follower), source px. */
  look?: { eyeX: number; eyeY: number };
}

/** One layer in one pose. Filled in place by `layerFrame`; read it, do not keep it. */
export interface LayerFrame {
  /** Draw nothing for this layer this frame (the mouth light while the mouth is shut). */
  skip: boolean;
  gain: number;
  follow: number;
  /** `u_aper` (top, centre, bottom, half gap or lift), when `hasAper`. */
  hasAper: boolean;
  aper: [number, number, number, number];
  /** `u_eye` (centre x, y, squash, 0), `u_eyeP` (iris amount, centre x, y, core radius) and
   *  `u_gaze` (slide x, y), when `hasEye`. */
  hasEye: boolean;
  eye: [number, number, number, number];
  eyeP: [number, number, number, number];
  gaze: [number, number];
  /** `u_float` (dx, dy, rock, 0), when `hasFloat`. */
  hasFloat: boolean;
  float: [number, number, number, number];
}

export const newLayerFrame = (): LayerFrame => ({
  skip: false,
  gain: 1,
  follow: 1,
  hasAper: false,
  aper: [0, 0, 0, 0],
  hasEye: false,
  eye: [0, 0, 0, 0],
  eyeP: [0, 0, 0, 0],
  gaze: [0, 0],
  hasFloat: false,
  float: [0, 0, 0, 0],
});

const smooth01 = (x: number) => {
  const y = Math.min(1, Math.max(0, x));
  return y * y * (3 - 2 * y);
};

/** The light in the slit is not drawn under this lift, source px. */
const MIN_LIFT = 0.2;

/** How far the hinged aperture opens, source px: the mouth's opening, a little wider with the voice. */
export function apertureLift(ap: Aperture | undefined, pose: Pick<Pose, "open" | "voice">): number {
  return ap ? pose.open * (ap.open_frac ?? 0.15) * (ap.bottom - ap.top) * (0.75 + 0.35 * Math.min(1, pose.voice)) : 0;
}

/**
 * A layer's per-layer motion, made once at build time: the drift of the `fi`th floating part, an
 * eye's centre and side. `bb` is the box the renderer draws it in.
 */
export function describeLayer(spec: LayerSpec, kind: number, bb: Box, fi: number): LayerDraw {
  const d: LayerDraw = { spec, kind };
  if (spec.kind === "float") {
    const f = spec.float ?? {};
    d.float = {
      amp: f.amp ?? [1, 3],
      freq: f.freq ?? 0.15,
      rot: ((f.rot_deg ?? 0) * Math.PI) / 180,
      pivot: f.pivot ?? [bb[0] + bb[2] / 2, bb[1] + bb[3] / 2],
      phase: 1.7 * fi + 0.4,
      phase2: 2.3 * fi + 1.1,
    };
  }
  if (spec.kind === "eye") {
    d.eyeC = spec.meta?.center ?? [bb[0] + bb[2] / 2, bb[1] + bb[3] / 2];
    d.right = !spec.name.endsWith("_l");
  }
  return d;
}

/** Fill `out` with `draw` in `pose`, by the shared rules and `map`'s choices. Returns `out`. */
export function layerFrame(draw: LayerDraw, pose: Pose, rig: FaceRig, map: FaceMapping, out: LayerFrame): LayerFrame {
  const l = draw.spec;
  const kind = draw.kind;
  const t = pose.t;
  const act = pose.activity;
  const voice = pose.voice;
  const lift = rig.lift;
  out.hasAper = false;
  out.hasEye = false;
  out.hasFloat = false;
  out.skip = kind === 8 && lift < MIN_LIFT;
  if (out.skip) return out;

  const ap = rig.aperture;
  if (ap && kind >= 6 && kind <= 8) {
    // the hinged layers move by the lift; the light between them is centred in the gap they open
    const lit = kind === 8;
    out.hasAper = true;
    out.aper[0] = ap.top;
    out.aper[1] = lit ? gapCentre(ap.seam_y, lift, rig.upperLip) : ap.seam_y;
    out.aper[2] = ap.bottom;
    out.aper[3] = lit ? gapHalf(lift, rig.upperLip) : lift;
  }

  out.follow =
    map.follow === "layer"
      ? (l.follow ?? (kind === 0 || kind === 1 ? 0 : 1))
      : map.follow === "planned"
        ? (draw.follow ?? 1)
        : kind === 3
          ? (l.follow ?? 1)
          : 1;

  let gain = l.gain ?? 1;
  if (kind === 5) gain *= 0.9 + (l.voice_gain ?? 2) * Math.max(voice * 0.8, pose.open) * 0.6;
  if (kind === 10) gain *= 1 + act * (l.pulse?.amp ?? 0.15) * Math.sin(2 * Math.PI * (l.pulse?.freq ?? 0.2) * t) + (l.voice_gain ?? 0) * voice;
  if (kind === 9 && map.ringBreath) gain *= 0.92 + act * 0.08 * Math.sin((2 * Math.PI * t) / 5.2) + 0.12 * voice;
  if (kind === 4 && draw.eyeC) {
    const c = draw.eyeC;
    const blink = draw.right ? pose.blinkR : pose.blinkL;
    out.hasEye = true;
    out.eye[0] = c[0];
    out.eye[1] = c[1];
    out.eye[3] = 0;
    if (l.blink === "iris") {
      out.eye[2] = 0.92 * Math.pow(blink, 0.8);
      out.eyeP[0] = blink;
      out.eyeP[1] = c[0];
      out.eyeP[2] = c[1];
      out.eyeP[3] = l.meta?.core_r ?? 12;
      gain *= 1 - 0.6 * blink;
    } else {
      out.eye[2] = 0.35 * blink;
      out.eyeP[0] = 0;
      out.eyeP[1] = 0;
      out.eyeP[2] = 0;
      out.eyeP[3] = 1;
      // a facet: bright light to dark crystal in the middle of the blink
      gain *= 1 - smooth01((blink - 0.25) / 0.4);
    }
    const gx = pose.gazeX * map.gazePx[0];
    const gy = pose.gazeY * map.gazePx[1];
    out.gaze[0] = rig.look ? rig.look.eyeX + gx : gx;
    out.gaze[1] = rig.look ? rig.look.eyeY + gy : gy;
    gain *= 1 + (l.voice_gain ?? 0.12) * voice; // the eyes warm while he speaks
  }
  out.gain = gain;

  if (kind === 3 && draw.float) {
    const f = draw.float;
    const w = 2 * Math.PI * f.freq;
    out.hasFloat = true;
    out.float[0] = act * f.amp[0] * Math.sin(w * 0.77 * t + f.phase2);
    out.float[1] = act * -f.amp[1] * Math.sin(w * t + f.phase);
    out.float[2] = act * f.rot * Math.sin(w * 0.6 * t + f.phase + 0.8);
    out.float[3] = 0;
  }
  return out;
}
