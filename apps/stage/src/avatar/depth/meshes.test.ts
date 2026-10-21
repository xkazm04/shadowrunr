import { describe, expect, it } from "vitest";

import type { FormSpec, LoadedForm } from "../layers";
import { HEAD_CELLS, rigMeshes, STRIDE } from "./geom";

const spec: FormSpec = {
  version: 1,
  width: 256,
  height: 256,
  rig_type: "glow",
  rig: { head: { center: [128, 100], radius: [48, 58], pivot_y: 150, chin_y: 155 }, aperture: { seam_y: 130, top: 120, bottom: 250, x0: 108, x1: 148 } },
  layers: [
    { name: "background", kind: "static", file: "bg.png", bbox: [0, 0, 256, 256] },
    { name: "torso", kind: "body", file: "torso.png", bbox: [0, 110, 256, 146] },
    { name: "head", kind: "head", file: "head.png", bbox: [80, 40, 96, 104] },
    { name: "jaw", kind: "aperture_lower", file: "jaw.png", bbox: [92, 128, 72, 30] },
    { name: "eye_l", kind: "eye", file: "eye_l.png", bbox: [96, 80, 30, 26] },
    { name: "shard_c", kind: "float", file: "shard_c.png", bbox: [118, 0, 20, 50], follow: 1 },
    { name: "shard_l1", kind: "float", file: "shard_l1.png", bbox: [70, 20, 12, 30], follow: 0.6 },
  ],
};
const form: LoadedForm = { spec, images: new Map(), form: "rig" };
// a depth field rising toward the middle
const z = new Float32Array(256 * 256);
for (let y = 0; y < 256; y++) for (let x = 0; x < 256; x++) z[y * 256 + x] = 100 - Math.abs(x - 128) * 0.5;
const field = { width: 256, height: 256, z };

describe("the rig's meshes", () => {
  const { meshes, which, headBox } = rigMeshes(form, field, { version: 1, method: "t", width: 256, height: 256, near: 100, far: 0, planes: { shard_c: 77 } });

  it("puts the head's layers on one fine grid over all of them, and leaves the backplate out", () => {
    expect(which.get("head")).toBe(0);
    expect(which.get("jaw")).toBe(0);
    expect(which.get("eye_l")).toBe(0);
    expect(which.get("torso")).toBe(1);
    expect(which.has("background")).toBe(false);
    expect(meshes[0]!.data.length).toBe((HEAD_CELLS + 1) ** 2 * STRIDE);
    // the head's box takes in the aperture, clipped to the art
    expect(headBox[0]).toBeLessThanOrEqual(80);
    expect(headBox[1] + headBox[3]).toBeLessThanOrEqual(256);
  });

  it("stands each shard on a flat card at its plane, or at the map's depth with none", () => {
    const c = meshes[which.get("shard_c")!]!;
    const l = meshes[which.get("shard_l1")!]!;
    const depths = (m: typeof c) => Array.from({ length: m.data.length / STRIDE }, (_, i) => m.data[i * STRIDE + 2]);
    expect(new Set(depths(c))).toEqual(new Set([77]));
    const ls = new Set(depths(l));
    expect(ls.size).toBe(1);
    // sampled at the card's centre (x 76), between pixel centres 75 and 76
    expect([...ls][0]).toBeCloseTo(100 - (128 - 75.5) * 0.5);
  });
});
