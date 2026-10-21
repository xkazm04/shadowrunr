// A guard on the three WebGL face renderers (glow, cut, depth): a recording fake WebGL2 context
// drives each over one fixture form and three fixed poses (plus a resize and a lost-and-restored
// context), and the whole call stream - every uniform, every draw, every bind and upload - must
// hash to what the renderers produced before their per-layer rules moved to layer-frame.ts and
// their plumbing to gl-kit.ts. A change to what a face DRAWS shows here as a new hash; set
// GOLDEN_DUMP=<dir> to write the streams out and diff them.
//
// Left out of the stream: the status queries (getExtension, isContextLost, the info logs), which
// change no GPU state and which the refactor may ask in another order.

import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { createCutRenderer } from "./cut/renderer";
import { createDepthRenderer } from "./depth/renderer";
import type { DepthField, DepthMeta } from "./depth/geom";
import type { LookPose } from "./depth/look";
import type { Pose } from "./driver";
import { createGlowRenderer } from "./glow";
import type { FormSpec, LayerImage, LoadedForm } from "./layers";

const UNRECORDED = new Set(["getExtension", "isContextLost", "getShaderInfoLog", "getProgramInfoLog"]);

function fnv(bytes: Uint8Array): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i++) {
    h ^= bytes[i]!;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16);
}

function ser(v: unknown): unknown {
  if (v === null || v === undefined) return v ?? null;
  if (typeof v === "number" || typeof v === "string" || typeof v === "boolean") return v;
  if (ArrayBuffer.isView(v)) {
    const b = new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
    // short arrays (uniform vectors) are kept readable; meshes are hashed
    if (v instanceof Float32Array && v.length <= 64) return { f32: Array.from(v) };
    return { [v.constructor.name]: v.byteLength, h: fnv(b) };
  }
  if (typeof v === "object" && "__id" in (v as object)) return (v as { __id: string }).__id;
  if (Array.isArray(v)) return v.map(ser);
  return String(v);
}

/** A WebGL2 context that does nothing but write down what it was asked. */
function recordingGl() {
  const log: unknown[][] = [];
  let n = 0;
  const target = {} as WebGL2RenderingContext;
  const gl = new Proxy(target, {
    get(_t, prop) {
      if (typeof prop !== "string") return undefined;
      if (/^[A-Z0-9_]+$/.test(prop)) return prop; // constants read as their names
      return (...args: unknown[]) => {
        if (!UNRECORDED.has(prop)) log.push([prop, ...args.map(ser)]);
        if (prop.startsWith("create")) return { __id: `${prop.slice(6)}#${++n}` };
        if (prop === "getUniformLocation") return { __id: `u:${String(args[1])}` };
        if (prop === "getAttribLocation") return ["a_src", "a_z", "a_g", "a_dz"].indexOf(String(args[1]));
        if (prop === "getProgramParameter") return true;
        if (prop === "getExtension") return null;
        if (prop === "isContextLost") return false;
        if (prop.endsWith("InfoLog")) return "";
        return undefined;
      };
    },
  });
  return { gl, log };
}

/** A canvas double: an EventTarget (for the context-lost events) with the size and getContext. */
function fakeCanvas(gl: WebGL2RenderingContext, log: unknown[][]) {
  return Object.assign(new EventTarget(), {
    width: 320,
    height: 240,
    getContext(type: string, attrs: unknown): WebGL2RenderingContext | null {
      log.push(["getContext", type, structuredClone(attrs ?? null)]);
      return gl;
    },
  });
}

const spec: FormSpec = {
  version: 1,
  width: 256,
  height: 256,
  rig_type: "glow",
  rig: {
    head: { center: [128, 100], radius: [48, 58], pivot_y: 150, chin_y: 155 },
    aperture: { seam_y: 130, top: 120, bottom: 160, x0: 108, x1: 148 },
    shimmer: { center: [128, 90], speed_px_s: 40, wavelength_px: 30, gain: 0.4 },
    glint: { gain: 0.5, width_px: 24 },
    tip_pulse: 0.3,
    ground: { y: 240, half_width: 100, fade: 30, curve: 12 },
  },
  layers: [
    { name: "background", kind: "static", file: "bg.png", bbox: [0, 0, 256, 256] },
    { name: "halo", kind: "halo", meta: { center: [128, 96], radius: 70, color: [0.2, 0.7, 0.9] }, ring_w: 2.2, gain: 0.8 },
    { name: "torso", kind: "body", file: "torso.png", bbox: [0, 110, 256, 146], occl: 0.9 },
    { name: "head", kind: "head", file: "head.png", bbox: [80, 40, 96, 104], depth: 0.4, shimmer: true, glint: true },
    { name: "jaw_up", kind: "aperture_upper", file: "jaw_up.png", bbox: [92, 118, 72, 14] },
    { name: "jaw", kind: "aperture_lower", file: "jaw.png", bbox: [92, 128, 72, 30] },
    { name: "mouth", kind: "aperture_glow" },
    { name: "seam", kind: "seam", file: "seam.png", bbox: [100, 126, 56, 8], mode: "light" },
    { name: "seam2", kind: "seam", file: "seam2.png", bbox: [104, 127, 48, 6], mode: "light", voice_gain: 1.2 },
    { name: "eye_l", kind: "eye", file: "eye_l.png", bbox: [96, 80, 30, 26], mode: "light", blink: "iris", meta: { core_r: 9 } },
    { name: "eye_r", kind: "eye", file: "eye_r.png", bbox: [132, 80, 30, 26], mode: "light", blink: "dim", voice_gain: 0.3, meta: { center: [146, 92] } },
    { name: "pulse", kind: "pulse", file: "pulse.png", bbox: [110, 60, 36, 20], mode: "light", pulse: { amp: 0.25, freq: 0.3 }, voice_gain: 0.4 },
    { name: "shard_c", kind: "float", file: "shard_c.png", bbox: [118, 0, 20, 50], follow: 1, bloom: 0.5 },
    { name: "shard_l1", kind: "float", file: "shard_l1.png", bbox: [70, 20, 12, 30], follow: 0.6, float: { amp: [2, 4], freq: 0.2, rot_deg: 6, pivot: [76, 50] } },
  ],
};

/** An image double: the recording context logs it by its id. */
function image(id: string): LayerImage {
  const img: Partial<ImageBitmap> & { __id: string } = { __id: id };
  return img as ImageBitmap;
}

function form(): LoadedForm {
  const images = new Map<string, LayerImage>();
  for (const l of spec.layers) if (l.file) images.set(l.file, image(`img:${l.file}`));
  return { spec, images, form: "rig" };
}

const history = (k: number) => Float32Array.from({ length: 48 }, (_, i) => Math.abs(Math.sin(i * 0.3 + k)) * 0.8);

function pose(over: Partial<Pose>): Pose {
  return { t: 0, yaw: 0, pitch: 0, roll: 0, blinkL: 0, blinkR: 0, gazeX: 0, gazeY: 0, open: 0, voice: 0, activity: 0, history: history(0), ...over };
}

const POSES: Pose[] = [
  // at rest: a settled face
  pose({}),
  // idle, mid-blink (left), looking right and up
  pose({ t: 3.7, yaw: 4.2, pitch: -1.3, roll: 0.6, blinkL: 0.55, blinkR: 0.1, gazeX: 0.6, gazeY: -0.4, open: 0.05, voice: 0.02, activity: 1, history: history(1) }),
  // speaking: the mouth open, the right eye shut
  pose({ t: 11.25, yaw: -6.5, pitch: 2.1, roll: -1.2, blinkL: 0, blinkR: 1, gazeX: -0.3, gazeY: 0.2, open: 0.8, voice: 0.9, activity: 0.7, history: history(2) }),
];

const LOOKS: LookPose[] = [
  { yaw: 0, pitch: 0, eyeX: 0, eyeY: 0 },
  { yaw: 7.5, pitch: -2, eyeX: 2.5, eyeY: -1 },
  { yaw: -12, pitch: 3, eyeX: -4, eyeY: 1.5 },
];

const field: DepthField = (() => {
  const z = new Float32Array(256 * 256);
  for (let y = 0; y < 256; y++) for (let x = 0; x < 256; x++) z[y * 256 + x] = 100 - Math.abs(x - 128) * 0.5 - Math.abs(y - 100) * 0.1;
  return { width: 256, height: 256, z };
})();
const meta: DepthMeta = { version: 1, method: "t", width: 256, height: 256, near: 100, far: 0, planes: { shard_c: 77 } };

interface Face {
  render(p: Pose, i: number): void;
  resize(): void;
  dispose(): void;
}

/** Drive one renderer through the script and return its call stream. */
async function drive(make: (canvas: HTMLCanvasElement, restored: () => void) => Promise<Face>): Promise<unknown[][]> {
  const { gl, log } = recordingGl();
  const canvas = fakeCanvas(gl, log);
  let restored = 0;
  const face = await make(canvas as HTMLCanvasElement, () => restored++);
  log.push(["--frames"]);
  POSES.forEach((p, i) => face.render(p, i));
  canvas.width = 200;
  canvas.height = 300;
  face.resize();
  log.push(["--resized"]);
  face.render(POSES[2]!, 2);
  log.push(["--lost"]);
  canvas.dispatchEvent(new Event("webglcontextlost", { cancelable: true }));
  face.render(POSES[1]!, 1);
  canvas.dispatchEvent(new Event("webglcontextrestored"));
  for (let i = 0; i < 200 && restored === 0; i++) await new Promise((r) => setTimeout(r, 1));
  log.push(["--restored", restored]);
  face.render(POSES[1]!, 1);
  log.push(["--disposed"]);
  face.dispose();
  face.render(POSES[0]!, 0);
  return log;
}

function digest(name: string, log: unknown[][]): { calls: number; sha: string } {
  const text = log.map((c) => JSON.stringify(c)).join("\n");
  const dir = process.env.GOLDEN_DUMP;
  if (dir) {
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, `${name}.jsonl`), text);
  }
  return { calls: log.length, sha: createHash("sha256").update(text).digest("hex").slice(0, 16) };
}

describe("the face renderers' call streams (guard: unchanged by the layer-frame refactor)", () => {
  it("glow", async () => {
    const log = await drive(async (c, r) => {
      const g = await createGlowRenderer(c, form(), r);
      return { render: (p) => g.render(p), resize: () => g.resize(), dispose: () => g.dispose() };
    });
    expect(digest("glow", log)).toEqual(GOLDEN.glow);
  });

  it("cut", async () => {
    const log = await drive(async (c, r) => {
      const g = await createCutRenderer(c, form(), r);
      return { render: (p) => g.render(p), resize: () => g.resize(), dispose: () => g.dispose() };
    });
    expect(digest("cut", log)).toEqual(GOLDEN.cut);
  });

  it("depth", async () => {
    const log = await drive(async (c, r) => {
      const g = await createDepthRenderer(c, form(), field, meta, r);
      return { render: (p, i) => g.render(p, LOOKS[i]!), resize: () => g.resize(), dispose: () => g.dispose() };
    });
    expect(digest("depth", log)).toEqual(GOLDEN.depth);
  });
});

/** Captured on 94e5998, before the refactor. */
const GOLDEN = {
  glow: { calls: 2055, sha: "defc283277a710cc" },
  cut: { calls: 2012, sha: "605b02004ade47b6" },
  depth: { calls: 1680, sha: "1dfa5fbfedd1ecc1" },
};
