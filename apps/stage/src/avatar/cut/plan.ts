// The cut-out's pure parts: which layers it draws and at what depth, where the art sits in its
// box, where the figure fades out, and how its rim lights share the turn. No WebGL here, so the
// tests read it directly.

import type { FormSpec, LayerSpec } from "../layers";

/** How deep the skull's own sphere is, in head x-radii (the shader's `zs * 0.6`). */
export const SKULL_DEPTH = 0.6;
/** The crown shards stand on a ring this much wider than the widest of them. */
const RING_PAD = 1.1;
/** A shard right at the front of the ring sits this deep: behind the face's front, ahead of its sides. */
const RING_DEPTH = 0.45;

/** Where a layer is: its anchor (the bbox centre when it has none). */
function centreOf(l: LayerSpec, W: number, H: number): [number, number] {
  const b = l.bbox ?? [0, 0, W, H];
  const anchor = (l as { anchor?: unknown }).anchor;
  if (Array.isArray(anchor) && anchor.length === 2 && anchor.every((v) => typeof v === "number" && Number.isFinite(v))) {
    return [anchor[0] as number, anchor[1] as number];
  }
  return [b[0] + b[2] / 2, b[1] + b[3] / 2];
}

/**
 * A floating part's depth when the rig does not record one. The crown's shards stand on a ring
 * around the skull: a shard at the ring's front (straight above the face) sits `RING_DEPTH` deep,
 * one at its side sits at 0, so a turn slides the front shards across and only narrows the side
 * ones. A shard that follows the head less (`follow` < 1) is held further out on the ring, so it
 * parallaxes less: depth scales by `follow`. Head x-radii, like the shader's z.
 */
export function shardDepth(ux: number, ringR: number, follow: number): number {
  const r = Math.max(1, ringR);
  const across = Math.min(1, Math.abs(ux) / r);
  const f = Math.min(1, Math.max(0, follow));
  return RING_DEPTH * Math.sqrt(1 - across * across) * f;
}

/** The width of the crown's ring: past the widest shard, never narrower than the skull. */
export function ringRadius(uxs: readonly number[]): number {
  let m = 1;
  for (const u of uxs) m = Math.max(m, Math.abs(u));
  return m * RING_PAD;
}

export interface PlannedLayer {
  spec: LayerSpec;
  /** z in head x-radii the shader adds to the layer (on top of the skull's sphere for `head`). */
  depth: number;
  /** Whether it turns with the head: every head-bound layer turns through the same field (1). */
  follow: number;
  /** Whether the depth came from the rig (true) or was derived here (false). */
  recorded: boolean;
}

export interface CutPlan {
  layers: PlannedLayer[];
  /** The backplate the cut-out keeps only inside the figure's matte, if the form has one. */
  backplate: LayerSpec | null;
  /** The solids whose union is the figure's rest matte (body, head, jaw). */
  matte: LayerSpec[];
  /** Source y where the figure starts to fade out, and where it is gone. */
  fade: [number, number];
}

/** How much of the body's height the bottom fade takes. */
export const BOTTOM_FADE = 0.08;

/**
 * Plan the cut-out of `spec`. Every layer above the backplate is drawn; every head-bound layer
 * gets a depth (the rig's where it records one; else a shard's from the crown ring, any other
 * part's from the skull's surface under it) and
 * follows the head fully, so all of them warp through the one shared field and only separate by
 * their depth difference. The body keeps its own blend into the neck (`follow` 0, the shader's).
 */
export function planCut(spec: FormSpec): CutPlan {
  const W = spec.width;
  const H = spec.height;
  const head = spec.rig.head;
  const ux = (l: LayerSpec) => (centreOf(l, W, H)[0] - head.center[0]) / head.radius[0];
  /** A part with no recorded depth lies on the skull's surface where it sits (0 off the skull). */
  const skullDepth = (l: LayerSpec) => {
    const c = centreOf(l, W, H);
    const u = (c[0] - head.center[0]) / head.radius[0];
    const v = (c[1] - head.center[1]) / head.radius[1];
    return SKULL_DEPTH * Math.sqrt(Math.max(0, 1 - u * u - v * v));
  };
  const floats = spec.layers.filter((l) => l.kind === "float");
  const ringR = ringRadius(floats.map(ux));

  let backplate: LayerSpec | null = null;
  const layers: PlannedLayer[] = [];
  for (const l of spec.layers) {
    if (l.kind === "static") {
      backplate ??= l;
      continue;
    }
    if (l.kind === "halo" || l.kind === "ground") {
      // a halo ring is the glow's backdrop and the ground line its floor: a cut-out has neither
      continue;
    }
    const bodyLike = l.kind === "body";
    const recorded = typeof l.depth === "number" && Number.isFinite(l.depth);
    let depth = recorded ? (l.depth as number) : 0;
    if (!recorded && l.kind === "float") depth = shardDepth(ux(l), ringR, l.follow ?? 1);
    else if (!recorded && !bodyLike && l.kind !== "head") depth = skullDepth(l);
    layers.push({ spec: l, depth, follow: bodyLike ? 0 : 1, recorded });
  }

  const matte = spec.layers.filter((l) => l.file && (l.kind === "body" || l.kind === "head" || l.kind === "aperture_lower" || l.kind === "aperture_upper"));
  const body = spec.layers.find((l) => l.kind === "body" && l.bbox);
  const bottom = body?.bbox ? Math.min(H, body.bbox[1] + body.bbox[3]) : H;
  const height = body?.bbox ? body.bbox[3] : H;
  return { layers, backplate, matte, fade: [bottom - BOTTOM_FADE * height, bottom] };
}

/** Headroom above the art, as a share of its height, so the tallest shard never meets the box top. */
export const HEADROOM = 0.05;

/**
 * Where the art sits in a `cw` x `ch` canvas: scaled to fit with `HEADROOM` above it, centred
 * across, its bottom on the canvas bottom (the figure sits on the box's floor like a streamer's
 * cut-out). Returns the clip-space transform the shaders take: x' = x * a + c, y' = y * b + d.
 */
export function cutLayout(cw: number, ch: number, W: number, H: number): [number, number, number, number] {
  const w = Math.max(1, cw);
  const h = Math.max(1, ch);
  const s = Math.min(w / W, h / (H * (1 + HEADROOM)));
  const ox = w / 2 - (W / 2) * s;
  const oy = h - H * s;
  return [(2 * s) / w, (-2 * s) / h, (2 * ox) / w - 1, 1 - (2 * oy) / h];
}

/**
 * How much of the figure shows at source point (x, y): it fades out over the body's last
 * `BOTTOM_FADE` and, low down, toward the art's sides, so the shoulders never end on a hard edge
 * of the box. The shader's `cutMask`, in JS.
 */
export function cutMask(x: number, y: number, W: number, fade: readonly [number, number]): number {
  const s = (a: number, b: number, v: number) => {
    const t = Math.min(1, Math.max(0, (v - a) / (b - a)));
    return t * t * (3 - 2 * t);
  };
  const bottom = 1 - s(fade[0], fade[1], y);
  const low = s(fade[0] - 0.45 * W, fade[0], y);
  const side = s(0, 0.14 * W, Math.min(x, W - x));
  return bottom * (1 - low * (1 - side));
}

/** The rim lights' gains for a head turn: the side he turns toward is lit more. */
export function rimGains(yawDeg: number, yawMax = 12): { right: number; left: number } {
  const s = Math.min(1, Math.max(-1, yawDeg / yawMax));
  const k = (v: number) => 0.3 + 0.7 * v;
  return { right: k((1 + s) / 2), left: k((1 - s) / 2) };
}

/** The art's box in the canvas's uv (y up): x0 y0 x1 y1, from `cutLayout`'s transform. */
export function artBox(v: readonly number[], W: number, H: number): Float32Array {
  const [a = 1, b = -1, c = -1, d = 1] = v;
  return new Float32Array([(c + 1) / 2, (1 + H * b + d) / 2, (W * a + c + 1) / 2, (1 + d) / 2]);
}

/** The rim and shadow, tuned by eye on the M8 rig over the hexmap. */
export const RIM_GAIN = 0.85;
export const SHADOW_BEHIND = 0.5;
export const SHADOW_BENEATH = 0.55;

/** The composite pass's lights for one frame, reused frame to frame. */
export interface LightRig {
  rimR: Float32Array;
  rimL: Float32Array;
  /** Shadow offset x, y (uv), behind strength, beneath strength. */
  shadow: Float32Array;
  /** Beneath: centre x, half width, height (uv). */
  pool: Float32Array;
}

export const newLightRig = (): LightRig => ({
  rimR: new Float32Array(3),
  rimL: new Float32Array(3),
  shadow: new Float32Array(4),
  pool: new Float32Array(3),
});

/**
 * Fill `out` for a head turn of `yawDeg` and a voice level: the rig's teal (its mouth light) rims
 * the right-hand edge and its ember (the ground line's core) the left, each stronger as he turns
 * toward it (`rimGains`); the teal warms a little with the voice. The shadow behind him falls
 * away from the brighter side and a little down; the one beneath is centred under the art.
 */
export function lightRig(out: LightRig, yawDeg: number, voice: number, teal: readonly number[], ember: readonly number[], box: Float32Array): LightRig {
  const g = rimGains(yawDeg);
  const lift = 1 + 0.25 * Math.min(1, Math.max(0, voice));
  for (let i = 0; i < 3; i++) {
    out.rimR[i] = (teal[i] ?? 0) * g.right * RIM_GAIN * lift;
    out.rimL[i] = (ember[i] ?? 0) * g.left * RIM_GAIN;
  }
  const side = g.right - g.left;
  out.shadow[0] = -0.03 * side - 0.004;
  out.shadow[1] = -0.018;
  out.shadow[2] = SHADOW_BEHIND;
  out.shadow[3] = SHADOW_BENEATH;
  const x0 = box[0] ?? 0;
  const x1 = box[2] ?? 1;
  out.pool[0] = (x0 + x1) / 2;
  out.pool[1] = 0.55 * (x1 - x0);
  out.pool[2] = 0.07;
  return out;
}
