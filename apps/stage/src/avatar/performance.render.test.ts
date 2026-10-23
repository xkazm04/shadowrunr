// The performance reaches the face renderers through the pose (`pose.perf`, layer-frame.ts): a
// recording fake WebGL2 context (as in renderers.golden.test.ts) drives the glow and cut renderers
// and reads back the gain each layer was drawn with. At rest the call stream is the one drawn
// without a performance, call for call; with eyeLight 2 the eye lights, and only they, burn twice
// as bright.

import { describe, expect, it } from "vitest";

import { createCutRenderer } from "./cut/renderer";
import type { Pose } from "./driver";
import { createGlowRenderer } from "./glow";
import { BROW_SQUASH } from "./layer-frame";
import type { FormSpec, LayerImage, LoadedForm } from "./layers";
import { PERFORMANCE_REST, type PerformancePose, type PerformedPose } from "./performance";

function recordingGl() {
  const log: unknown[][] = [];
  let n = 0;
  const gl = new Proxy({} as WebGL2RenderingContext, {
    get(_t, prop) {
      if (typeof prop !== "string") return undefined;
      if (/^[A-Z0-9_]+$/.test(prop)) return prop;
      return (...args: unknown[]) => {
        log.push([prop, ...args.map((a) => (a && typeof a === "object" && "__id" in a ? (a as { __id: string }).__id : ArrayBuffer.isView(a) ? Array.from(a as Float32Array) : a))]);
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

const spec: FormSpec = {
  version: 1,
  width: 256,
  height: 256,
  rig_type: "glow",
  rig: { head: { center: [128, 100], radius: [48, 58], pivot_y: 150, chin_y: 155 }, aperture: { seam_y: 130, top: 120, bottom: 160, x0: 108, x1: 148 } },
  layers: [
    { name: "background", kind: "static", file: "bg.png", bbox: [0, 0, 256, 256] },
    { name: "head", kind: "head", file: "head.png", bbox: [80, 40, 96, 104] },
    { name: "jaw", kind: "aperture_lower", file: "jaw.png", bbox: [92, 128, 72, 30] },
    { name: "eye_l", kind: "eye", file: "eye_l.png", bbox: [96, 80, 30, 26], mode: "light", blink: "dim", bloom: 0.3 },
    { name: "eye_r", kind: "eye", file: "eye_r.png", bbox: [132, 80, 30, 26], mode: "light", blink: "dim", bloom: 0.3 },
    { name: "monocle", kind: "pulse", file: "monocle.png", bbox: [130, 70, 40, 40], mode: "light", bloom: 0.25 },
  ],
};

function form(): LoadedForm {
  const images = new Map<string, LayerImage>();
  for (const l of spec.layers) {
    if (!l.file) continue;
    const img: Partial<ImageBitmap> & { __id: string } = { __id: `img:${l.file}` };
    images.set(l.file, img as ImageBitmap);
  }
  return { spec, images, form: "rig" };
}

const pose = (perf?: PerformancePose): PerformedPose => ({
  t: 2,
  yaw: 1.5,
  pitch: -0.5,
  roll: 0.3,
  blinkL: 0,
  blinkR: 0,
  gazeX: 0.2,
  gazeY: -0.1,
  open: 0,
  voice: 0.3,
  activity: 1,
  history: new Float32Array(48),
  ...(perf ? { perf } : {}),
});

type Make = (canvas: HTMLCanvasElement) => Promise<{ render(p: Pose): void }>;
const FACES: [string, Make][] = [
  ["glow", (c) => createGlowRenderer(c, form(), () => {})],
  ["cut", (c) => createCutRenderer(c, form(), () => {})],
];

/** The frame's calls, and each layer's gain and eye squash by its `u_kind`. */
async function frame(make: Make, p: Pose) {
  const { gl, log } = recordingGl();
  const canvas = Object.assign(new EventTarget(), { width: 256, height: 256, getContext: (_type: string, _attrs?: unknown): WebGL2RenderingContext | null => gl });
  const face = await make(canvas as HTMLCanvasElement);
  log.length = 0;
  face.render(p);
  const gains: { kind: number; gain: number }[] = [];
  const squash: number[] = [];
  let kind = -1;
  for (const [fn, loc, ...v] of log) {
    if (fn === "uniform1i" && loc === "u:u_kind") kind = v[0] as number;
    if (fn === "uniform1f" && loc === "u:u_gain") gains.push({ kind, gain: v[0] as number });
    if (fn === "uniform4f" && loc === "u:u_eye") squash.push(v[2] as number);
  }
  return { calls: JSON.stringify(log), gains, squash };
}

describe.each(FACES)("the %s face honours the performance", (_name, make) => {
  it("at rest draws exactly what it draws with no performance", async () => {
    const none = await frame(make, pose());
    const rest = await frame(make, pose({ ...PERFORMANCE_REST }));
    expect(rest.calls).toBe(none.calls);
  });

  it("eyeLight 2 doubles the eye lights' gain (and with it their bloom) and nothing else", async () => {
    const rest = await frame(make, pose({ ...PERFORMANCE_REST }));
    const blazing = await frame(make, pose({ ...PERFORMANCE_REST, eyeLight: 2 }));
    expect(blazing.gains.map((g) => g.kind)).toEqual(rest.gains.map((g) => g.kind));
    const eyes = rest.gains.filter((g) => g.kind === 4);
    expect(eyes.length).toBeGreaterThanOrEqual(2);
    blazing.gains.forEach((g, i) => {
      const was = rest.gains[i]!.gain;
      if (g.kind === 4) expect(g.gain).toBeCloseTo(2 * was, 12);
      else expect(g.gain).toBe(was);
    });
  });

  it("a scowl squashes the eye light; a flare brightens it", async () => {
    const rest = await frame(make, pose({ ...PERFORMANCE_REST }));
    const scowl = await frame(make, pose({ ...PERFORMANCE_REST, brow: -1 }));
    scowl.squash.forEach((s, i) => expect(s).toBeCloseTo(rest.squash[i]! + BROW_SQUASH, 12));
    const flare = await frame(make, pose({ ...PERFORMANCE_REST, flare: 1 }));
    const eye = (f: typeof rest) => f.gains.find((g) => g.kind === 4)!.gain;
    expect(eye(flare)).toBeGreaterThan(eye(rest) * 1.5);
  });
});
