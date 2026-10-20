// The placeholder Malakor: a procedural glow rig the stage draws until the owner's portrait has
// been split into the real one (served by the daemon at `/avatar/layers.json`).
//
// Nothing here is a binary asset. `placeholderSpec()` is the rig (pure, tested), and
// `buildPlaceholder()` paints each of its layers on a canvas in the browser and decodes it to an
// ImageBitmap. The figure is original and simple, after the portrait brief's grammar (an
// arch-lich lit from inside on a near-black ground, parts separated by clear gaps): a dark
// crowned skull, a hinged jaw under an ember seam, two verdigris eye lights, five obsidian crown
// shards floating above, an ink collar edged in ochre, and a small ember phylactery orb. It
// exercises every channel the renderer has: blink (the eye lights dim), the mouth (the jaw drops,
// the seam brightens, light shows in the gap), head sway and breath, the floats, the eyes and the
// orb warming with the voice, and the ground line it dissolves into. Colours are the stage tokens.

import { TOKENS_COLOR } from "@shadowrunr/tokens";

import type { FormSpec, LayerSpec, LoadedForm } from "./layers";

/** The art's size, source pixels; the layers are painted at `PAINT_SCALE` times it. */
export const ART = 512;
const PAINT_SCALE = 2;
const CX = ART / 2;
/** Where the skull meets the jaw. */
const SEAM_Y = 300;

const SHARDS: readonly { dx: number; y: number; h: number; w: number }[] = [
  { dx: -96, y: 104, h: 34, w: 22 },
  { dx: -50, y: 82, h: 48, w: 26 },
  { dx: 0, y: 70, h: 62, w: 30 },
  { dx: 50, y: 82, h: 48, w: 26 },
  { dx: 96, y: 104, h: 34, w: 22 },
];

const EYES = { l: [CX - 44, 206], r: [CX + 44, 206] } as const;

/** The placeholder's rig. Pure: the same spec every call. */
export function placeholderSpec(): FormSpec {
  const shard = (s: (typeof SHARDS)[number], i: number): LayerSpec => ({
    name: `shard_${i}`,
    kind: "float",
    file: `shard_${i}.png`,
    bbox: [CX + s.dx - s.w / 2 - 6, s.y - s.h - 6, s.w + 12, s.h + 30],
    depth: -0.08,
    occl: 1,
    bloom: 0.25,
    float: { amp: [1.2, 3 + (i % 2)], freq: 0.11 + 0.025 * i, rot_deg: 3, pivot: [CX + s.dx, s.y] },
  });
  const eye = (side: "l" | "r"): LayerSpec => {
    const [x, y] = EYES[side];
    return {
      name: `eye_${side}`,
      kind: "eye",
      file: `eye_${side}.png`,
      bbox: [x - 28, y - 28, 56, 56],
      depth: 0.04,
      mode: "light",
      blink: "dim",
      bloom: 0.6,
      voice_gain: 0.45,
      meta: { center: [x, y] },
    };
  };
  return {
    version: 1,
    width: ART,
    height: ART,
    rig_type: "glow",
    layers: [
      { name: "aura", kind: "static", file: "aura.png", bbox: [0, 0, ART, ART], mode: "light" },
      { name: "collar", kind: "body", file: "collar.png", bbox: [88, 344, 336, 136], occl: 1 },
      { name: "orb", kind: "pulse", file: "orb.png", bbox: [CX - 30, 386, 60, 60], mode: "light", bloom: 0.5, voice_gain: 0.8, pulse: { amp: 0.18, freq: 0.19 } },
      { name: "mouth_light", kind: "aperture_glow", mode: "light", gain: 1.1 },
      { name: "skull", kind: "head", file: "skull.png", bbox: [CX - 128, 60, 256, SEAM_Y - 60 + 4], occl: 1 },
      { name: "jaw", kind: "aperture_lower", file: "jaw.png", bbox: [CX - 68, SEAM_Y, 136, 60], depth: 0.02, occl: 1 },
      { name: "seam", kind: "seam", file: "seam.png", bbox: [CX - 70, SEAM_Y - 10, 140, 20], depth: 0.03, mode: "light", bloom: 0.4, voice_gain: 1.6 },
      eye("l"),
      eye("r"),
      ...SHARDS.map(shard),
    ],
    rig: {
      head: { center: [CX, 210], radius: [124, 142], pivot_y: 330, chin_y: 356 },
      // `bottom` sits well under the chin, so the jaw drops nearly whole instead of squashing
      aperture: { seam_y: SEAM_Y, open_frac: 0.1, top: 250, bottom: 470, x0: CX - 56, x1: CX + 56 },
      ground: { y: 474, half_width: 168, fade: 36, curve: 30 },
    },
  };
}

// ── Painting ────────────────────────────────────────────────────────────────────────────────

type Ctx = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

/** A token colour (`#rgb` or `#rrggbb`) as 0..255 channels. */
function channels(hex: string): [number, number, number] {
  const h = hex.length === 4 ? `#${hex[1]}${hex[1]}${hex[2]}${hex[2]}${hex[3]}${hex[3]}` : hex;
  return [0, 1, 2].map((i) => parseInt(h.slice(1 + 2 * i, 3 + 2 * i), 16) || 0) as [number, number, number];
}

/** A token colour at an alpha, as a canvas colour string. */
function tint(hex: string, a: number): string {
  return `rgba(${channels(hex).join(",")},${a})`;
}

/** Two token colours mixed, `t` of the way from `a` to `b`, opaque. */
function mix(a: string, b: string, t: number): string {
  const x = channels(a);
  const y = channels(b);
  return `rgb(${x.map((v, i) => Math.round(v + (y[i]! - v) * t)).join(",")})`;
}

const BONE = TOKENS_COLOR.surfacePaper;
const EMBER = TOKENS_COLOR.signal;
const VERDIGRIS = TOKENS_COLOR.okBright;
const OCHRE = TOKENS_COLOR.foil;
const INK = TOKENS_COLOR.surfaceVoid;
const DEEP = TOKENS_COLOR.screenSetNear;
/** Pure black: no light at all (a light layer's ground). */
const BLACK = TOKENS_COLOR.maskInk;

function skullPath(ctx: Ctx) {
  ctx.beginPath();
  ctx.moveTo(CX - 62, SEAM_Y);
  ctx.bezierCurveTo(CX - 72, 286, CX - 98, 282, CX - 104, 258);
  ctx.bezierCurveTo(CX - 120, 228, CX - 122, 160, CX - 104, 126);
  ctx.bezierCurveTo(CX - 82, 84, CX - 42, 72, CX, 72);
  ctx.bezierCurveTo(CX + 42, 72, CX + 82, 84, CX + 104, 126);
  ctx.bezierCurveTo(CX + 122, 160, CX + 120, 228, CX + 104, 258);
  ctx.bezierCurveTo(CX + 98, 282, CX + 72, 286, CX + 62, SEAM_Y);
  ctx.closePath();
}

function paintSkull(ctx: Ctx) {
  skullPath(ctx);
  const g = ctx.createLinearGradient(0, 72, 0, SEAM_Y);
  g.addColorStop(0, mix(INK, BONE, 0.2));
  g.addColorStop(0.5, mix(INK, DEEP, 0.9));
  g.addColorStop(1, INK);
  ctx.fillStyle = g;
  ctx.fill();
  ctx.lineWidth = 3;
  ctx.strokeStyle = tint(BONE, 0.9);
  ctx.stroke();
  // a hairline crack lit from inside
  ctx.beginPath();
  ctx.moveTo(CX + 18, 74);
  ctx.lineTo(CX + 30, 104);
  ctx.lineTo(CX + 22, 126);
  ctx.lineTo(CX + 36, 150);
  ctx.lineWidth = 1.6;
  ctx.strokeStyle = tint(VERDIGRIS, 0.7);
  ctx.stroke();
  // the sockets and the nose are holes: no light, so the stage shows through them
  ctx.globalCompositeOperation = "destination-out";
  for (const [x, y] of [EYES.l, EYES.r]) {
    ctx.beginPath();
    ctx.ellipse(x, y, 31, 25, 0, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.beginPath();
  ctx.moveTo(CX, 238);
  ctx.lineTo(CX - 13, 268);
  ctx.lineTo(CX + 13, 268);
  ctx.closePath();
  ctx.fill();
  ctx.globalCompositeOperation = "source-over";
  for (const [x, y] of [EYES.l, EYES.r]) {
    ctx.beginPath();
    ctx.ellipse(x, y, 31, 25, 0, 0, Math.PI * 2);
    ctx.lineWidth = 2;
    ctx.strokeStyle = tint(BONE, 0.55);
    ctx.stroke();
  }
}

function paintJaw(ctx: Ctx) {
  ctx.beginPath();
  ctx.moveTo(CX - 60, SEAM_Y + 2);
  ctx.lineTo(CX + 60, SEAM_Y + 2);
  ctx.bezierCurveTo(CX + 62, 330, CX + 46, 352, CX + 26, 356);
  ctx.lineTo(CX - 26, 356);
  ctx.bezierCurveTo(CX - 46, 352, CX - 62, 330, CX - 60, SEAM_Y + 2);
  ctx.closePath();
  const g = ctx.createLinearGradient(0, SEAM_Y, 0, 356);
  g.addColorStop(0, mix(INK, DEEP, 0.9));
  g.addColorStop(1, INK);
  ctx.fillStyle = g;
  ctx.fill();
  ctx.lineWidth = 2.6;
  ctx.strokeStyle = tint(BONE, 0.8);
  ctx.stroke();
  // teeth: a row of dim bone ticks along the top edge
  ctx.fillStyle = tint(BONE, 0.5);
  for (let x = -46; x <= 46; x += 13) ctx.fillRect(CX + x - 4, SEAM_Y + 4, 8, 9);
}

function paintSeam(ctx: Ctx) {
  ctx.beginPath();
  ctx.moveTo(CX - 58, SEAM_Y);
  ctx.lineTo(CX + 58, SEAM_Y);
  ctx.lineCap = "round";
  ctx.shadowColor = tint(EMBER, 1);
  ctx.shadowBlur = 8;
  ctx.lineWidth = 3;
  ctx.strokeStyle = tint(EMBER, 0.9);
  ctx.stroke();
  ctx.shadowBlur = 0;
}

function glowDot(ctx: Ctx, x: number, y: number, r: number, hex: string) {
  const g = ctx.createRadialGradient(x, y, 0, x, y, r);
  g.addColorStop(0, mix(hex, BONE, 0.55));
  g.addColorStop(0.18, hex);
  g.addColorStop(0.45, mix(BLACK, hex, 0.4));
  g.addColorStop(1, BLACK);
  ctx.fillStyle = g;
  ctx.fillRect(x - r, y - r, 2 * r, 2 * r);
}

function paintShard(ctx: Ctx, i: number) {
  const s = SHARDS[i]!;
  const x = CX + s.dx;
  ctx.beginPath();
  ctx.moveTo(x, s.y - s.h);
  ctx.lineTo(x + s.w / 2, s.y - 8);
  ctx.lineTo(x + s.w * 0.15, s.y + 14);
  ctx.lineTo(x - s.w * 0.3, s.y + 5);
  ctx.lineTo(x - s.w / 2, s.y - 14);
  ctx.closePath();
  const g = ctx.createLinearGradient(x - s.w / 2, s.y - s.h, x + s.w / 2, s.y + 14);
  g.addColorStop(0, tint(DEEP, 1));
  g.addColorStop(1, tint(INK, 1));
  ctx.fillStyle = g;
  ctx.fill();
  ctx.lineWidth = 1.8;
  ctx.strokeStyle = tint(EMBER, 0.85);
  ctx.stroke();
  // an ember core through a crack
  ctx.beginPath();
  ctx.moveTo(x, s.y - s.h + 10);
  ctx.lineTo(x + 3, s.y - s.h / 2);
  ctx.lineTo(x - 2, s.y);
  ctx.lineWidth = 1.4;
  ctx.strokeStyle = tint(EMBER, 0.75);
  ctx.stroke();
}

function paintCollar(ctx: Ctx) {
  for (const side of [-1, 1]) {
    ctx.beginPath();
    ctx.moveTo(CX + side * 34, 352);
    ctx.lineTo(CX + side * 150, 392);
    ctx.lineTo(CX + side * 162, 476);
    ctx.lineTo(CX + side * 44, 476);
    ctx.lineTo(CX + side * 18, 420);
    ctx.closePath();
    const g = ctx.createLinearGradient(0, 352, 0, 476);
    g.addColorStop(0, tint(DEEP, 1));
    g.addColorStop(1, tint(INK, 1));
    ctx.fillStyle = g;
    ctx.fill();
    ctx.lineWidth = 2.4;
    ctx.strokeStyle = tint(OCHRE, 0.8);
    ctx.stroke();
  }
}

function paintAura(ctx: Ctx) {
  const g = ctx.createRadialGradient(CX, 214, 20, CX, 214, 230);
  g.addColorStop(0, mix(BLACK, VERDIGRIS, 0.22));
  g.addColorStop(0.5, mix(BLACK, VERDIGRIS, 0.08));
  g.addColorStop(1, BLACK);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, ART, ART);
}

/** Paint one layer in source coordinates (the caller has set up the bbox transform). */
export function paintLayer(ctx: Ctx, layer: LayerSpec): void {
  const name = layer.name;
  if (name === "aura") paintAura(ctx);
  else if (name === "collar") paintCollar(ctx);
  else if (name === "orb") glowDot(ctx, CX, 416, 26, EMBER);
  else if (name === "skull") paintSkull(ctx);
  else if (name === "jaw") paintJaw(ctx);
  else if (name === "seam") paintSeam(ctx);
  else if (name === "eye_l") glowDot(ctx, EYES.l[0], EYES.l[1], 24, VERDIGRIS);
  else if (name === "eye_r") glowDot(ctx, EYES.r[0], EYES.r[1], 24, VERDIGRIS);
  else if (name.startsWith("shard_")) paintShard(ctx, Number(name.slice(6)));
}

function surface(w: number, h: number): { canvas: OffscreenCanvas | HTMLCanvasElement; ctx: Ctx } {
  if (typeof OffscreenCanvas !== "undefined") {
    const canvas = new OffscreenCanvas(w, h);
    const ctx = canvas.getContext("2d");
    if (ctx) return { canvas, ctx };
  }
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("no 2D canvas to paint the placeholder");
  return { canvas, ctx };
}

/** Paint every layer of the placeholder and decode each to an ImageBitmap (straight alpha). */
export async function buildPlaceholder(): Promise<LoadedForm> {
  const spec = placeholderSpec();
  const images = new Map<string, ImageBitmap>();
  for (const layer of spec.layers) {
    if (!layer.file || !layer.bbox) continue;
    const [x, y, w, h] = layer.bbox;
    const { canvas, ctx } = surface(Math.ceil(w * PAINT_SCALE), Math.ceil(h * PAINT_SCALE));
    ctx.scale(PAINT_SCALE, PAINT_SCALE);
    ctx.translate(-x, -y);
    if (layer.mode === "light" || layer.kind === "static") {
      // a light layer is read as light (its rgb), never by its alpha: paint it on black
      ctx.fillStyle = BLACK;
      ctx.fillRect(x, y, w, h);
    }
    paintLayer(ctx, layer);
    images.set(layer.file, await createImageBitmap(canvas, { premultiplyAlpha: "none", colorSpaceConversion: "none" }));
  }
  return { spec, images, form: "placeholder" };
}
