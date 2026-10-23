import { describe, expect, it } from "vitest";

import { displacedGrid, STRIDE } from "../depth/geom";
import type { FormSpec, LoadedForm } from "../layers";
import { PERFORMANCE_REST } from "../performance";
import { browBand, eyeFire, headTurn, jawHinge, LOOK_GAIN, MAX_SLOPE, PITCH_LIMIT, rigLights, sobelSlopes, YAW_LIMIT } from "./geom";

const spec: FormSpec = {
  version: 1,
  width: 256,
  height: 256,
  rig_type: "glow",
  rig: { head: { center: [128, 100], radius: [48, 58], pivot_y: 150, chin_y: 155 }, aperture: { seam_y: 130, top: 120, bottom: 250, x0: 108, x1: 148 } },
  layers: [
    { name: "head", kind: "head", file: "head.png", bbox: [80, 40, 96, 104] },
    { name: "eye_r", kind: "eye", file: "eye_r.png", bbox: [132, 80, 30, 26] },
    { name: "eye_l", kind: "eye", file: "eye_l.png", bbox: [96, 80, 30, 26] },
    { name: "monocle", kind: "pulse", file: "monocle.png", bbox: [128, 72, 40, 40], meta: { center: [147, 93], radius: 12 } },
  ],
};
const form: LoadedForm = { spec, images: new Map(), form: "rig" };
const plane = (f: (x: number, y: number) => number) => {
  const z = new Float32Array(256 * 256);
  for (let y = 0; y < 256; y++) for (let x = 0; x < 256; x++) z[y * 256 + x] = f(x + 0.5, y + 0.5);
  return { width: 256, height: 256, z };
};

describe("the Sobel slopes", () => {
  it("recover a plane's slope, and hold a cliff at MAX_SLOPE", () => {
    const m = sobelSlopes(displacedGrid([40, 40, 100, 100], 4, 4, () => 0), plane((x, y) => 0.5 * x - 0.25 * y), 3);
    for (let k = 0; k < m.data.length; k += STRIDE) {
      expect(m.data[k + 3]).toBeCloseTo(0.5, 3);
      expect(m.data[k + 4]).toBeCloseTo(-0.25, 3);
    }
    const c = sobelSlopes(displacedGrid([100, 100, 40, 40], 4, 4, () => 0), plane((x) => (x < 120 ? 0 : 500)), 3);
    expect(Math.max(...Array.from({ length: c.data.length / STRIDE }, (_, i) => c.data[i * STRIDE + 3]!))).toBe(MAX_SLOPE);
  });
});

describe("the rig's lights", () => {
  it("puts an eye light in each eye in front of its socket's rim, and the ring in front of its eye", () => {
    const { eyes, ring } = rigLights(form, plane(() => 50));
    expect(eyes.map((e) => [e.x, e.y])).toEqual([[147, 93], [111, 93]]);
    for (const e of eyes) expect(e.z).toBeGreaterThan(50);
    expect(ring).not.toBeNull();
    expect(ring!.z).toBeGreaterThan(eyes[0]!.z);
    expect(ring!.r1).toBeGreaterThan(ring!.r0);
  });
});

describe("the jaw's hinge", () => {
  it("sits behind the teeth and opens the front by the lift", () => {
    const h = jawHinge(spec.rig, plane(() => 40))!;
    expect(h.y).toBe(130);
    expect(h.z).toBeLessThan(40);
    expect(h.angle(0)).toBe(0);
    expect(h.back * Math.sin(h.angle(6))).toBeCloseTo(6);
    expect(jawHinge({ head: spec.rig.head }, plane(() => 0))).toBeNull();
  });
});

describe("the brow band", () => {
  it("sits above each eye, left first", () => {
    const b = browBand(form)!;
    expect(b.centres[0]).toBeLessThan(b.centres[2]);
    expect(b.centres[1]).toBeLessThan(93);
    expect(browBand({ ...form, spec: { ...spec, layers: spec.layers.slice(0, 2) } })).toBeNull();
  });
});

describe("the head's turn", () => {
  const out = { yaw: 0, pitch: 0, roll: 0, lean: 0 };
  const pose = { yaw: 2, pitch: -1, roll: 0.5 };

  it("adds the widened look, the pose and the gestures", () => {
    headTurn(pose, { yaw: 10, pitch: 5 }, { ...PERFORMANCE_REST, gestureYaw: (3 * Math.PI) / 180 }, out);
    expect(out.yaw).toBeCloseTo(10 * LOOK_GAIN[0] + 2 + 3);
    expect(out.pitch).toBeCloseTo(5 * LOOK_GAIN[1] - 1);
  });

  it("holds the turn inside the limits the art takes", () => {
    headTurn(pose, { yaw: 20, pitch: 10 }, { ...PERFORMANCE_REST, gestureYaw: 1, gesturePitch: 1, lean: 4 }, out);
    expect(out).toMatchObject({ yaw: YAW_LIMIT, pitch: PITCH_LIMIT, lean: 1 });
  });
});

describe("the eye fire", () => {
  it("is the rig's own at rest, out at 0, and blazes in a flare", () => {
    expect(eyeFire(PERFORMANCE_REST)).toEqual({ gain: 1, bloom: 0, light: 1 });
    expect(eyeFire({ ...PERFORMANCE_REST, eyeLight: 0 }).gain).toBe(0);
    const f = eyeFire({ ...PERFORMANCE_REST, flare: 1, eyeLight: 9 });
    expect(f.gain).toBeCloseTo(2.5 * 1.8);
    expect(f.bloom).toBeGreaterThan(0);
    expect(eyeFire({ ...PERFORMANCE_REST, eyeLight: Number.NaN }).gain).toBe(1);
  });
});
