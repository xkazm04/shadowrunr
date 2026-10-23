// The lich3d face's logic, without a GPU. three.js does not suit the fake-WebGL call-stream golden
// (renderers.golden.test.ts): its draw order, program cache and uniform uploads are three.js's own
// and would pin the library's internals, not this face's. What is this face's own is tested here:
// the asset contract, the fallback decisions, the channel mapping, the crown's determinism and the
// placeholder skull's nodes.

import { describe, expect, it } from "vitest";
import * as THREE from "three";

import { createDriver, type Driver, type Pose } from "../driver";
import { createLookFollower } from "../depth/look";
import { PERFORMANCE_REST, type PerformanceCue, type PerformanceDriver, type PerformancePose } from "../performance";
import { loadLichAsset } from "./asset";
import { BROW_PITCH_DEG, emptyFrame, lichFrame, PITCH_MAX_DEG, YAW_MAX_DEG } from "./channels";
import { checkLichMeta, chooseRenderer, chooseSkull, LICH_NODES, missingNodes, PLACEHOLDER_META } from "./contract";
import { CROWN, crystalAt, crystalCrown } from "./crystals";
import { lichDriver } from "./index";
import { buildPlaceholderSkull } from "./skull";

const DEG = Math.PI / 180;

const META = {
  version: 1,
  source_model: "trellis-image-large",
  portrait_sha256: "a".repeat(64),
  tris: 28000,
  jaw_open_rad: 0.35,
  eye_radius: 0.11,
  camera: { fov: 30, pos: [0, 0, 4], target: [0, 0, 0] },
};

function pose(over: Partial<Pose> = {}): Pose {
  return { t: 0, yaw: 0, pitch: 0, roll: 0, blinkL: 0, blinkR: 0, gazeX: 0, gazeY: 0, open: 0, voice: 0, activity: 0, history: new Float32Array(48), ...over };
}
const perf = (over: Partial<PerformancePose> = {}): PerformancePose => ({ ...PERFORMANCE_REST, ...over });

describe("lich.json", () => {
  it("passes a good file through", () => {
    expect(checkLichMeta(META)).toEqual(META);
  });

  it.each([
    ["not an object", null],
    ["a wrong version", { ...META, version: 2 }],
    ["no source model", { ...META, source_model: "" }],
    ["a bad sha", { ...META, portrait_sha256: "xyz" }],
    ["fractional tris", { ...META, tris: 1.5 }],
    ["a jaw that does not open", { ...META, jaw_open_rad: 0 }],
    ["a jaw past 1.2 rad", { ...META, jaw_open_rad: 2 }],
    ["a zero eye", { ...META, eye_radius: 0 }],
    ["no camera", { ...META, camera: undefined }],
    ["a fisheye", { ...META, camera: { ...META.camera, fov: 170 } }],
    ["a two-number position", { ...META, camera: { ...META.camera, pos: [0, 1] } }],
    ["a camera on its target", { ...META, camera: { ...META.camera, pos: [0, 0, 0] } }],
  ])("rejects %s", (_why, raw) => {
    expect(() => checkLichMeta(raw)).toThrow();
  });

  it("the placeholder's own numbers check", () => {
    expect(() => checkLichMeta({ ...PLACEHOLDER_META, tris: 1 })).not.toThrow();
  });
});

describe("which skull, which renderer", () => {
  it("names the nodes a model lacks", () => {
    expect(missingNodes((n) => n !== "jaw" && n !== "neck")).toEqual(["jaw", "neck"]);
    expect(missingNodes(() => true)).toEqual([]);
  });

  it("takes the GLB only when the json checks and every node is there", () => {
    expect(chooseSkull({ rigBase: "http://x/", meta: META, missing: [] })).toEqual({ use: "glb" });
    expect(chooseSkull({ rigBase: null, meta: META })).toMatchObject({ use: "placeholder", why: "no rig base" });
    expect(chooseSkull({ rigBase: "http://x/", metaError: "HTTP 404" })).toMatchObject({ use: "placeholder", why: "lich.json: HTTP 404" });
    expect(chooseSkull({ rigBase: "http://x/", meta: META, glbError: "HTTP 404" })).toMatchObject({ use: "placeholder", why: "lich.glb: HTTP 404" });
    expect(chooseSkull({ rigBase: "http://x/", meta: { ...META, version: 3 } })).toMatchObject({ use: "placeholder" });
    expect(chooseSkull({ rigBase: "http://x/", meta: META, missing: ["monocle"] })).toEqual({ use: "placeholder", why: "lich.glb lacks monocle" });
  });

  it("hands the canvas to the glow face with no WebGL2 or a renderer that will not start", () => {
    expect(chooseRenderer({ webgl2: true })).toEqual({ use: "lich3d" });
    expect(chooseRenderer({ webgl2: false })).toMatchObject({ use: "glow" });
    expect(chooseRenderer({ webgl2: true, rendererError: "no context" })).toMatchObject({ use: "glow", why: "the 3D head did not start: no context" });
  });

  it("falls back to the placeholder when the rig has no lich files, saying why", async () => {
    const get = (url: string) => Promise.resolve(new Response("nope", { status: url.endsWith(".json") ? 404 : 404 }));
    const asset = await loadLichAsset("http://rig/", get);
    expect(asset.kind).toBe("placeholder");
    expect(asset.why).toBe("lich.json: HTTP 404");
    expect(asset.meta).toBe(PLACEHOLDER_META);
  });

  it("falls back when the GLB is not a GLB", async () => {
    const get = (url: string) => Promise.resolve(url.endsWith(".json") ? Response.json(META) : new Response(new Uint8Array([1, 2, 3, 4])));
    const asset = await loadLichAsset("http://rig/", get);
    expect(asset.kind).toBe("placeholder");
    expect(asset.why).toMatch(/^lich\.glb: /);
  });
});

describe("the channel mapping", () => {
  it("is still at rest: no turn, the jaw shut, the eye light the rig's own", () => {
    const f = lichFrame(pose(), PERFORMANCE_REST, null, 0.3);
    for (const k of ["yaw", "pitch", "roll", "jaw", "flameX", "flameY", "lean", "flare", "float"] as const) expect(Math.abs(f[k])).toBe(0);
    expect(f).toMatchObject({ eyeL: 1, eyeR: 1, flameH: 1, flameW: 1 });
  });

  it("turns the head by pose, look and gesture together, inside +-35 x +-18 degrees", () => {
    const f = lichFrame(pose({ yaw: 5 }), perf({ gestureYaw: 0.1 }), { yaw: 10, pitch: 0, eyeX: 0, eyeY: 0 }, 0.3);
    expect(f.yaw).toBeCloseTo((5 + 12.5) * DEG + 0.1, 6);
    const far = lichFrame(pose({ yaw: 30, pitch: -20 }), perf({ gestureYaw: 0.5, gesturePitch: -0.5 }), { yaw: 20, pitch: -10, eyeX: 0, eyeY: 0 }, 0.3);
    expect(far.yaw).toBeCloseTo(YAW_MAX_DEG * DEG, 9);
    expect(far.pitch).toBeCloseTo(-PITCH_MAX_DEG * DEG, 9);
  });

  it("turns the pose's clockwise roll into three's counter-clockwise one", () => {
    expect(lichFrame(pose({ roll: 4 }), PERFORMANCE_REST, null, 0.3).roll).toBeCloseTo(-4 * DEG, 9);
  });

  it("opens the jaw by the pose's opening up to the asset's angle", () => {
    expect(lichFrame(pose({ open: 0.5 }), PERFORMANCE_REST, null, 0.4).jaw).toBeCloseTo(0.2, 9);
    expect(lichFrame(pose({ open: 3 }), PERFORMANCE_REST, null, 0.4).jaw).toBeCloseTo(0.4, 9);
  });

  it("dips a blinking socket's light and scales both by eyeLight, held to 0..2.5", () => {
    const f = lichFrame(pose({ blinkL: 1 }), perf({ eyeLight: 2 }), null, 0.3);
    expect(f.eyeL).toBeCloseTo(0.3, 9);
    expect(f.eyeR).toBe(2);
    expect(lichFrame(pose(), perf({ eyeLight: 9 }), null, 0.3).eyeR).toBe(2.5);
    expect(lichFrame(pose(), perf({ eyeLight: -1 }), null, 0.3).eyeR).toBe(0);
  });

  it("shows a scowl as a lowered chin and a narrow, wide flame; a raised brow as a tall one", () => {
    const scowl = lichFrame(pose(), perf({ brow: -1 }), null, 0.3);
    expect(scowl.pitch).toBeCloseTo(BROW_PITCH_DEG * DEG, 9);
    expect(scowl.flameH).toBeCloseTo(0.55, 9);
    expect(scowl.flameW).toBeCloseTo(1.15, 9);
    const raised = lichFrame(pose(), perf({ brow: 1 }), null, 0.3);
    expect(raised.pitch).toBeCloseTo(-BROW_PITCH_DEG * DEG, 9);
    expect(raised.flameH).toBeCloseTo(1.2, 9);
  });

  it("slides the flame toward the look and up for an upward gaze", () => {
    const f = lichFrame(pose({ gazeY: -1 }), PERFORMANCE_REST, { yaw: 0, pitch: 0, eyeX: 10, eyeY: 0 }, 0.3);
    expect(f.flameX).toBeCloseTo(0.7, 9);
    expect(f.flameY).toBeCloseTo(0.5, 9);
  });

  it("survives NaN anywhere without a NaN out", () => {
    const f = lichFrame(pose({ yaw: NaN, open: NaN, blinkL: NaN, voice: NaN }), perf({ eyeLight: NaN, brow: NaN, flare: NaN }), { yaw: NaN, pitch: NaN, eyeX: NaN, eyeY: NaN }, 0.3, emptyFrame());
    for (const v of Object.values(f)) expect(Number.isFinite(v)).toBe(true);
  });
});

describe("the crown", () => {
  it("is eleven crystals where the rig's shards stood", () => {
    const crown = crystalCrown(11);
    expect(crown).toHaveLength(11);
    crown.forEach((c, i) => {
      expect(Math.abs(c.x - CROWN[i]![0])).toBeLessThan(0.011);
      expect(Math.abs(c.y - CROWN[i]![1])).toBeLessThan(0.011);
      expect(c.positions.length).toBe(c.sides * 4 * 9);
      expect(c.positions.every(Number.isFinite)).toBe(true);
      expect(Math.max(...c.positions.filter((_, k) => k % 3 === 1))).toBeCloseTo(0.5, 9);
    });
  });

  it("is the same crown for the same seed, byte for byte, and another for another seed", () => {
    const a = crystalCrown(11);
    const b = crystalCrown(11);
    const c = crystalCrown(12);
    expect(a.map((x) => Array.from(x.positions))).toEqual(b.map((x) => Array.from(x.positions)));
    expect(a.map((x) => [x.z, x.tilt, x.spin0])).toEqual(b.map((x) => [x.z, x.tilt, x.spin0]));
    expect(Array.from(a[0]!.positions)).not.toEqual(Array.from(c[0]!.positions));
  });

  it("stands still at its rest place when nothing is alive", () => {
    const [c] = crystalCrown(11);
    expect(crystalAt(c!, 0, 0)).toEqual({ y: c!.y, spin: c!.spin0 });
    expect(crystalAt(c!, 3.7, 0).y).toBe(c!.y);
  });
});

describe("the placeholder skull", () => {
  const group = buildPlaceholderSkull(new THREE.MeshStandardMaterial({ vertexColors: true }));

  it("names every node the GLB contract names", () => {
    expect(missingNodes((n) => !!group.getObjectByName(n))).toEqual([]);
    expect(LICH_NODES).toHaveLength(6);
  });

  it("hinges the jaw at its origin, behind and above the teeth, and keeps its geometry finite", () => {
    const jaw = group.getObjectByName("jaw") as THREE.Mesh;
    const skull = group.getObjectByName("skull") as THREE.Mesh;
    jaw.geometry.computeBoundingBox();
    const box = jaw.geometry.boundingBox!;
    // in its own space the jaw hangs below and in front of the hinge
    expect(box.max.y).toBeLessThan(0.05);
    expect(box.max.z).toBeGreaterThan(0.3);
    for (const m of [jaw, skull]) expect((m.geometry.getAttribute("position").array as Float32Array).every(Number.isFinite)).toBe(true);
  });

  it("is about one unit tall with the face toward +z", () => {
    const box = new THREE.Box3().setFromObject(group);
    expect(box.max.y - box.min.y).toBeGreaterThan(0.9);
    expect(box.max.y - box.min.y).toBeLessThan(1.15);
    expect(box.max.z).toBeGreaterThan(0.35);
    for (const n of ["eye_l", "eye_r", "monocle"]) expect(group.getObjectByName(n)!.position.z).toBeGreaterThan(0.15);
    expect(group.getObjectByName("eye_l")!.position.x).toBeLessThan(0);
  });
});

describe("the lich3d driver", () => {
  function fakePerf(): PerformanceDriver & { cues: PerformanceCue[]; busy: boolean } {
    const cues: PerformanceCue[] = [];
    const d = {
      cues,
      busy: false,
      setCue: (c: PerformanceCue) => void cues.push(c),
      step: () => perf({ eyeLight: 2 }),
      pose: PERFORMANCE_REST,
      get settled() {
        return !d.busy;
      },
    };
    return d;
  }

  it("reads the cue each frame and is settled only when the pose, look and performance are", () => {
    const p = fakePerf();
    const cue: PerformanceCue = { id: 3, emotion: "menace", intensity: 1 };
    const inner: Driver = createDriver({ seed: 7, idleLinger: 0 });
    const d = lichDriver(inner, createLookFollower(), () => ({ x: 0, y: 0 }), p, { get: () => cue });
    d.step(1 / 60);
    expect(p.cues.at(-1)).toBe(cue);
    expect(d.perf.eyeLight).toBe(2);
    for (let i = 0; i < 600; i++) d.step(1 / 60);
    expect(inner.settled).toBe(true);
    expect(d.settled).toBe(true);
    p.busy = true;
    expect(d.settled).toBe(false);
  });

  it("draws the rest performance on a still frame", () => {
    const d = lichDriver(createDriver({ seed: 7 }), createLookFollower(), () => ({ x: 1, y: 0 }), fakePerf());
    d.step(1 / 60);
    d.rest();
    expect(d.perf).toBe(PERFORMANCE_REST);
    expect(d.look.yaw).toBeGreaterThan(0);
  });
});
