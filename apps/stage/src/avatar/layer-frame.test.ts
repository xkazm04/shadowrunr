// The rig's per-layer animation rules, without a GPU: what one layer looks like in one pose (its
// gain, its eye light, its float, its aperture), for every face renderer from one module.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import type { Pose } from "./driver";
import { KIND } from "./glow-shaders";
import { apertureLift, describeLayer, GLOW_MAPPING, layerFrame, newLayerFrame, type FaceRig } from "./layer-frame";
import type { LayerSpec, RigSpec } from "./layers";

const pose = (over: Partial<Pose>): Pose => ({
  t: 2.5,
  yaw: 0,
  pitch: 0,
  roll: 0,
  blinkL: 0,
  blinkR: 0,
  gazeX: 0,
  gazeY: 0,
  open: 0,
  voice: 0,
  activity: 1,
  history: new Float32Array(48),
  ...over,
});

const noAperture: FaceRig = { aperture: undefined, upperLip: false, lift: 0 };

function frameOf(spec: LayerSpec, p: Pose, rig: FaceRig = noAperture, fi = 0) {
  const draw = describeLayer(spec, KIND[spec.kind] ?? 2, spec.bbox ?? [0, 0, 256, 256], fi);
  return layerFrame(draw, p, rig, GLOW_MAPPING, newLayerFrame());
}

describe("layerFrame", () => {
  it("brightens the seam with the voice or the open mouth, whichever is more (voice_gain 2 by default)", () => {
    const f = frameOf({ name: "seam", kind: "seam", bbox: [100, 126, 56, 8] }, pose({ voice: 0.5, open: 0.3 }));
    expect(f.gain).toBeCloseTo(0.9 + 2 * Math.max(0.4, 0.3) * 0.6, 12);
    expect(f.gain).toBeCloseTo(1.38, 12);
  });

  it("closes an iris eye toward its centre and dims it to 40 % when shut", () => {
    const f = frameOf({ name: "eye_r", kind: "eye", bbox: [130, 80, 30, 26], blink: "iris", meta: { center: [145, 92] } }, pose({ blinkR: 1, voice: 0 }));
    expect(f.hasEye).toBe(true);
    expect(f.gain).toBeCloseTo(0.4, 12);
    expect(f.eye).toEqual([145, 92, 0.92, 0]);
  });

  it("turns a dim (facet) eye from bright light to dark crystal at a full blink", () => {
    const f = frameOf({ name: "eye_l", kind: "eye", bbox: [96, 80, 30, 26], blink: "dim" }, pose({ blinkL: 1 }));
    expect(f.gain).toBe(0);
  });

  it("holds a float still when the face is settled: activity 0 draws exactly the rest frame", () => {
    const f = frameOf(
      { name: "shard", kind: "float", bbox: [118, 0, 20, 50], float: { amp: [2, 4], freq: 0.2, rot_deg: 6 } },
      pose({ activity: 0, t: 7.3 }),
    );
    expect(f.hasFloat).toBe(true);
    expect(f.float.map((v) => v + 0)).toEqual([0, 0, 0, 0]);
  });

  it("skips the mouth light while the aperture's lift is under 0.2 px", () => {
    const rig: RigSpec = { head: { center: [128, 100], radius: [48, 58], chin_y: 155 }, aperture: { seam_y: 130, top: 120, bottom: 160, x0: 108, x1: 148 } };
    const p = pose({ open: 0.02, voice: 0 });
    const lift = apertureLift(rig.aperture, p);
    expect(lift).toBeLessThan(0.2);
    const f = frameOf({ name: "mouth", kind: "aperture_glow" }, p, { aperture: rig.aperture, upperLip: false, lift });
    expect(f.skip).toBe(true);
    const wide = pose({ open: 0.8, voice: 0.9 });
    const open = frameOf({ name: "mouth", kind: "aperture_glow" }, wide, { aperture: rig.aperture, upperLip: false, lift: apertureLift(rig.aperture, wide) });
    expect(open.skip).toBe(false);
    expect(open.hasAper).toBe(true);
  });
});

// The source guards: each rule and each piece of plumbing is written once.
const here = dirname(fileURLToPath(import.meta.url));
function filesWith(needle: string): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.tsx?$/.test(name) && !name.endsWith(".test.ts") && readFileSync(p, "utf8").includes(needle)) out.push(p.slice(here.length + 1).replace(/\\/g, "/"));
    }
  };
  walk(here);
  return out;
}

describe("the rules and the plumbing live in one module each", () => {
  it("the aperture lift formula", () => {
    expect(filesWith("open_frac ?? 0.15")).toEqual(["layer-frame.ts"]);
  });

  it("the texture upload (straight alpha, no colour conversion)", () => {
    expect(filesWith("UNPACK_PREMULTIPLY_ALPHA_WEBGL")).toEqual(["gl-kit.ts"]);
  });

  it("the context-loss guard", () => {
    expect(filesWith("webglcontextlost")).toEqual(["gl-kit.ts"]);
  });
});
