// The glow renderer's pure parts: the mesh grid, the context it asks for, its palette, and the
// ground line's placement. Kept apart from glow.ts so tests and the placeholder can use them with
// no WebGL.

import { TOKENS_COLOR } from "@shadowrunr/tokens";

import type { Pose } from "./driver";
import type { Rgb, RigSpec } from "./layers";

/** A grid over a box, `step` px per cell (at most 160 a side): the mesh the vertex shader warps. */
export function gridMesh(x: number, y: number, w: number, h: number, step: number) {
  const nx = Math.max(1, Math.min(160, Math.ceil(w / step)));
  const ny = Math.max(1, Math.min(160, Math.ceil(h / step)));
  const pos = new Float32Array((nx + 1) * (ny + 1) * 2);
  let k = 0;
  for (let j = 0; j <= ny; j++)
    for (let i = 0; i <= nx; i++) {
      pos[k++] = x + (w * i) / nx;
      pos[k++] = y + (h * j) / ny;
    }
  const idx = new Uint32Array(nx * ny * 6);
  k = 0;
  for (let j = 0; j < ny; j++)
    for (let i = 0; i < nx; i++) {
      const a = j * (nx + 1) + i;
      const b = a + 1;
      const c = a + nx + 1;
      const d = c + 1;
      idx[k++] = a;
      idx[k++] = b;
      idx[k++] = c;
      idx[k++] = b;
      idx[k++] = d;
      idx[k++] = c;
    }
  return { pos, idx };
}

/**
 * The canvas is transparent and premultiplied end to end: the stage shows through wherever the
 * figure is dark, and its light composites over the stage as light, not as a black box.
 */
export const CONTEXT_ATTRIBUTES: WebGLContextAttributes = Object.freeze({
  alpha: true,
  premultipliedAlpha: true,
  antialias: true,
  preserveDrawingBuffer: false,
});

/** A token colour (`#rrggbb`) as 0..1 channels. */
export function tokenRgb(hex: string): Rgb {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  if (!m) return [1, 1, 1];
  return [parseInt(m[1]!, 16) / 255, parseInt(m[2]!, 16) / 255, parseInt(m[3]!, 16) / 255];
}

/** The stage palette's lights: ember in the open mouth, a verdigris ground line. */
export const MOUTH_LIGHT: Rgb = tokenRgb(TOKENS_COLOR.signal);
export const LINE_CORE: Rgb = tokenRgb(TOKENS_COLOR.okBright);
export const LINE_GLOW: Rgb = tokenRgb(TOKENS_COLOR.ok);

/** The renderer's lights for a rig: its own `colors`, else the stage palette. */
export function rigColors(rig: RigSpec): { mouth: Rgb; lineCore: Rgb; lineGlow: Rgb } {
  return {
    mouth: rig.colors?.mouth ?? MOUTH_LIGHT,
    lineCore: rig.colors?.line_core ?? LINE_CORE,
    lineGlow: rig.colors?.line_glow ?? LINE_GLOW,
  };
}

/**
 * How much of the stage a solid's darkest part still covers. At 1 the rig's dark solids read as
 * a cut-out, at 0 as light alone. A little keeps them a body.
 */
export const BODY_COVER = 0.06;

type Ground = NonNullable<RigSpec["ground"]>;

/**
 * The ground line this frame: where it lies, the box it is drawn in, and how bright it is. It
 * breathes with the idle breath and brightens a little with the voice; at rest (activity 0,
 * silent) it is the same every frame, which is what reduced motion draws.
 */
export function groundLine(ground: Ground, artWidth: number, pose: Pick<Pose, "t" | "activity" | "voice">) {
  const cx = ground.x ?? artWidth / 2;
  const reach = ground.half_width * 1.18; // the taper runs a little past the figure's base
  const breath = 0.5 - 0.5 * Math.cos((2 * Math.PI * pose.t) / 5.2);
  const gain = 0.85 * (1 + 0.1 * pose.activity * breath) * (1 + 0.35 * Math.min(1.2, Math.max(0, pose.voice)));
  return {
    cx,
    y: ground.y,
    halfWidth: ground.half_width,
    gain,
    box: [cx - reach, ground.y - 120, 2 * reach, 180] as [number, number, number, number],
  };
}

/** How much of the figure shows at source point (x, y): the shader's dissolve into the ground, in JS. */
export function groundMask(ground: Ground, artWidth: number, x: number, y: number): number {
  const cx = ground.x ?? artWidth / 2;
  const dx = (x - cx) / ground.half_width;
  const yy = y + ground.curve * dx * dx;
  const k = Math.min(1, Math.max(0, (yy - (ground.y - ground.fade)) / (1.08 * ground.fade)));
  return 1 - k * k * (3 - 2 * k);
}
