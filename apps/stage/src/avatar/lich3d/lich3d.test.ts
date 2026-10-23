// The lich3d face's logic, without a GPU. three.js does not suit the fake-WebGL call-stream golden
// (renderers.golden.test.ts): its draw order, program cache and uniform uploads are three.js's own
// and would pin the library's internals, not this face's. What is this face's own is tested here:
// the asset contract, the fallback decisions, the channel mapping, the crown's determinism and the
// placeholder skull's nodes.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";
import * as THREE from "three";

import { createDriver, type Driver, type Pose } from "../driver";
import { createLookFollower } from "../depth/look";
import { PERFORMANCE_REST, type PerformanceCue, type PerformanceDriver, type PerformancePose } from "../performance";
import { loadLichAsset } from "./asset";
import { BROW_PITCH_DEG, COLLAR_PITCH, COLLAR_YAW, collarMotion, emptyFrame, leanShift, lichFrame, PITCH_MAX_DEG, YAW_MAX_DEG } from "./channels";
import { checkLichMeta, chooseRenderer, chooseSkull, LICH_NODES, LICH_OPTIONAL_NODES, missingNodes, PLACEHOLDER_META } from "./contract";
import { CROWN, crystalAt, crystalCrown, FLOAT_GAIN, PORTRAIT_PX } from "./crystals";
import { lichDriver } from "./index";
import { paintRamp } from "./renderer";
import { buildPlaceholderSkull } from "./skull";
import { readJson } from "../../test-json";

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

describe("the collar", () => {
  it("is optional: the required nodes do not name it", () => {
    expect(LICH_OPTIONAL_NODES).toEqual(["collar"]);
    expect((LICH_NODES as readonly string[]).includes("collar")).toBe(false);
  });

  it("turns yaw x0.25 and pitch x0.15 and does not move at rest", () => {
    const rest = collarMotion(emptyFrame(), 1);
    expect(rest).toEqual({ yaw: 0, pitch: 0, y: 0, z: 0, scale: 1 });
    const m = collarMotion({ yaw: 0.6, pitch: -0.3, lean: 0 }, 1);
    expect(m.yaw).toBeCloseTo(0.6 * COLLAR_YAW, 9);
    expect(m.pitch).toBeCloseTo(-0.3 * COLLAR_PITCH, 9);
    expect(COLLAR_YAW).toBe(0.25);
    expect(COLLAR_PITCH).toBe(0.15);
  });

  it("follows the head's lean in full, scaled by the head's height", () => {
    const m = collarMotion({ yaw: 0, pitch: 0, lean: 1 }, 1.4);
    expect(m).toMatchObject({ yaw: 0, pitch: 0 });
    expect(m.y).toBeCloseTo(0.02 * 1.4, 9);
    expect(m.z).toBeCloseTo(0.12 * 1.4, 9);
    expect(m.scale).toBeCloseTo(1.05, 9);
    expect({ y: m.y, z: m.z, scale: m.scale }).toEqual(leanShift(1, 1.4));
    expect(collarMotion({ yaw: 0, pitch: 0, lean: -1 }, 1).z).toBeLessThan(0);
  });

  it("stays inside a fraction of the head's own limits", () => {
    const f = lichFrame(pose({ yaw: 90, pitch: 90 }), PERFORMANCE_REST, null, 0.3);
    const m = collarMotion(f, 1);
    expect(m.yaw).toBeCloseTo(YAW_MAX_DEG * DEG * COLLAR_YAW, 9);
    expect(m.pitch).toBeCloseTo(PITCH_MAX_DEG * DEG * COLLAR_PITCH, 9);
  });
});

describe("the paint ramp", () => {
  it("rises monotonically in three soft steps from deep shadow to lit", () => {
    const t = paintRamp();
    const px = Array.from((t.image.data as Uint8Array).filter((_, i) => i % 4 === 0));
    for (let i = 1; i < px.length; i++) expect(px[i]!).toBeGreaterThanOrEqual(px[i - 1]!);
    expect(px[0]!).toBeLessThan(40);
    expect(px.at(-1)!).toBe(255);
    // plateaus: shadow, mid and lit each hold for several texels
    const holds = (lo: number, hi: number) => px.filter((v) => v >= lo && v <= hi).length;
    expect(holds(0, 40)).toBeGreaterThan(20);
    expect(holds(100, 140)).toBeGreaterThan(8);
    expect(holds(240, 255)).toBeGreaterThan(20);
    // soft: no jump between neighbours larger than a third of the range
    for (let i = 1; i < px.length; i++) expect(px[i]! - px[i - 1]!).toBeLessThan(85);
  });
});

describe("the crown", () => {
  // the committed rig spec: the shards' polygons, follow and float, in portrait px
  type SpecShard = { name: string; region: { poly: [number, number][] }; follow: number; float: { amp: [number, number]; freq: number; rot_deg: number } };
  const spec = readJson(readFileSync(fileURLToPath(new URL("../../../../../characters/malakor/rig/m8.spec.json", import.meta.url)), "utf8")) as {
    width: number;
    height: number;
    elements: SpecShard[];
  };
  const shards = spec.elements.filter((e) => e.name.startsWith("shard_"));
  const centroid = (poly: [number, number][]) => {
    let a = 0;
    let cx = 0;
    let cy = 0;
    poly.forEach(([x0, y0], i) => {
      const [x1, y1] = poly[(i + 1) % poly.length]!;
      const c = x0 * y1 - x1 * y0;
      a += c;
      cx += (x0 + x1) * c;
      cy += (y0 + y1) * c;
    });
    return [cx / (3 * a), cy / (3 * a)] as const;
  };

  it("is the rig spec's shards: their centroids, extents, follow and float", () => {
    expect(spec.width).toBe(PORTRAIT_PX);
    expect(spec.height).toBe(PORTRAIT_PX);
    expect(CROWN.map((s) => s.name).sort()).toEqual(shards.map((s) => s.name).sort());
    for (const s of shards) {
      const c = CROWN.find((x) => x.name === s.name)!;
      const [cx, cy] = centroid(s.region.poly);
      expect(Math.abs(c.cx - cx)).toBeLessThan(0.1);
      expect(Math.abs(c.cy - cy)).toBeLessThan(0.1);
      const xs = s.region.poly.map((p) => p[0]);
      const ys = s.region.poly.map((p) => p[1]);
      expect(c.polyLen).toBe(Math.max(...ys) - Math.min(...ys));
      expect(c.polyWidth).toBe(Math.max(...xs) - Math.min(...xs));
      expect(c.follow).toBe(s.follow);
      expect([...c.amp, c.freq, c.rotDeg]).toEqual([...s.float.amp, s.float.freq, s.float.rot_deg]);
    }
  });

  it("fans out: the centre stone tallest and on the axis, pairs stepping smaller, lower and back", () => {
    const [centre, ...rest] = CROWN;
    expect(centre!.name).toBe("shard_c");
    expect(Math.abs(centre!.cx - PORTRAIT_PX / 2)).toBeLessThan(2);
    for (const s of rest) {
      expect(s.polyLen).toBeLessThan(centre!.polyLen);
      expect(s.cy).toBeGreaterThan(centre!.cy);
      expect(s.depth).toBeLessThan(centre!.depth);
    }
  });

  it("is one faceted stone per shard, a unit tall, widest above its middle", () => {
    const crown = crystalCrown(11);
    expect(crown).toHaveLength(CROWN.length);
    crown.forEach((c, i) => {
      expect(c.layout).toBe(CROWN[i]);
      // four triangles a side: the lower point, two bands, the upper point
      expect(c.positions.length).toBe(c.sides * 6 * 9);
      expect(c.positions.every(Number.isFinite)).toBe(true);
      const ys = c.positions.filter((_, k) => k % 3 === 1);
      expect(Math.max(...ys)).toBeCloseTo(0.5, 9);
      expect(Math.min(...ys)).toBeCloseTo(-0.5, 9);
      // the widest ring stands above the middle: the long taper is below
      let widest = 0;
      let widestY = 0;
      for (let k = 0; k < c.positions.length; k += 3) {
        const r = Math.hypot(c.positions[k]!, c.positions[k + 2]!);
        if (r > widest) [widest, widestY] = [r, c.positions[k + 1]!];
      }
      expect(widestY).toBeGreaterThan(0);
      expect(c.length).toBeGreaterThan(c.width);
    });
  });

  it("is the same crown for the same seed, byte for byte, and another for another seed", () => {
    const a = crystalCrown(11);
    const b = crystalCrown(11);
    const c = crystalCrown(12);
    expect(a.map((x) => Array.from(x.positions))).toEqual(b.map((x) => Array.from(x.positions)));
    expect(a.map((x) => [x.tilt, x.turn0, x.phaseY])).toEqual(b.map((x) => [x.tilt, x.turn0, x.phaseY]));
    expect(Array.from(a[0]!.positions)).not.toEqual(Array.from(c[0]!.positions));
  });

  it("stands still at its rest place when nothing is alive, and floats within its amplitude when alive", () => {
    const [c] = crystalCrown(11);
    for (const t of [0, 3.7]) {
      const f = crystalAt(c!, t, 0);
      // (+0 or -0: a sine times nothing)
      expect([Math.abs(f.dx), Math.abs(f.dy), f.roll, f.turn]).toEqual([0, 0, c!.tilt, c!.turn0]);
    }
    for (let t = 0; t < 20; t += 0.37) {
      const f = crystalAt(c!, t, 1);
      expect(Math.abs(f.dy)).toBeLessThanOrEqual(c!.layout.amp[1] * FLOAT_GAIN + 1e-9);
      expect(Math.abs(f.roll - c!.tilt)).toBeLessThanOrEqual((c!.layout.rotDeg * Math.PI) / 180 + 1e-9);
    }
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
