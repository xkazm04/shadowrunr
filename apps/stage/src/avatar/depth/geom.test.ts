import { describe, expect, it } from "vitest";

import { cameraFor, checkDepthMeta, depthField, displacedGrid, fitView, project, rotate, sampleDepth, STRIDE, tearFade, unionBox } from "./geom";

const head = { center: [511, 392] as [number, number], radius: [190, 235] as [number, number], pivot_y: 612, chin_y: 622 };

describe("depth.json", () => {
  const ok = { version: 1, method: "analytic-v1", width: 64, height: 32, near: 120, far: -40, scale: 0.0024, planes: { shard_c: 90, bad: "x" } };

  it("reads what the prep writes, dropping malformed planes", () => {
    const m = checkDepthMeta(ok, 64, 32);
    expect(m).toMatchObject({ method: "analytic-v1", near: 120, far: -40, planes: { shard_c: 90 } });
    expect(m.planes).not.toHaveProperty("bad");
  });

  it("refuses a map for another rig, another version or a flipped range", () => {
    expect(() => checkDepthMeta(ok, 128, 32)).toThrow(/64x32/);
    expect(() => checkDepthMeta({ ...ok, version: 2 }, 64, 32)).toThrow(/version/);
    expect(() => checkDepthMeta({ ...ok, near: -50 }, 64, 32)).toThrow(/near/);
    expect(() => checkDepthMeta(null, 64, 32)).toThrow();
  });
});

describe("the depth field", () => {
  it("maps level 65535 to near and 0 to far", () => {
    const f = depthField({ width: 2, height: 1, data: new Uint16Array([0, 65535]) }, { near: 100, far: -20 });
    expect(f.z[0]).toBeCloseTo(-20);
    expect(f.z[1]).toBeCloseTo(100);
  });

  it("samples bilinearly between pixel centres and clamps at the edges", () => {
    const f = { width: 2, height: 2, z: new Float32Array([0, 10, 20, 30]) };
    expect(sampleDepth(f, 0.5, 0.5)).toBeCloseTo(0);
    expect(sampleDepth(f, 1, 0.5)).toBeCloseTo(5);
    expect(sampleDepth(f, 1, 1)).toBeCloseTo(15);
    expect(sampleDepth(f, -50, 99)).toBeCloseTo(20);
  });
});

describe("the displaced grid", () => {
  it("carries each vertex's depth, its slope and its largest step", () => {
    // a step of 40 px between x < 10 and x >= 10
    const m = displacedGrid([0, 0, 20, 10], 4, 2, (x) => (x >= 10 ? 40 : 0));
    expect(m.data.length).toBe(5 * 3 * STRIDE);
    expect(m.idx.length).toBe(4 * 2 * 6);
    expect(m.cell).toBe(5);
    const v = (i: number, j: number) => m.data.subarray((j * 5 + i) * STRIDE, (j * 5 + i + 1) * STRIDE);
    expect(Array.from(v(0, 0).subarray(0, 3))).toEqual([0, 0, 0]);
    expect(v(2, 1)[2]).toBe(40);
    // the vertices beside the step see it, those away from it do not
    expect(v(1, 1)[5]).toBe(40);
    expect(v(2, 1)[5]).toBe(40);
    expect(v(4, 1)[5]).toBe(0);
    expect(v(1, 1)[6]).toBe(0);
    expect(v(1, 1)[3]).toBeCloseTo(40 / 10);
  });

  it("joins boxes", () => {
    expect(unionBox([[10, 20, 5, 5], [0, 22, 4, 30]])).toEqual([0, 20, 15, 32]);
  });
});

describe("the camera", () => {
  const cam = cameraFor(1024, 1024, head, 20);

  it("draws the art unchanged when nothing is turned, at any depth", () => {
    for (const [x, y, z] of [[100, 200, 0], [511, 392, 150], [900, 1000, -60]] as const) {
      const [px, py] = project(cam, x, y, z, 0, 0, 0);
      expect(px).toBeCloseTo(x, 6);
      expect(py).toBeCloseTo(y, 6);
    }
  });

  it("turns near points further than far ones (parallax)", () => {
    const yaw = (20 * Math.PI) / 180;
    const near = project(cam, 511, 392, 150, yaw, 0, 0)[0] - 511;
    const far = project(cam, 511, 392, 0, yaw, 0, 0)[0] - 511;
    expect(near).toBeGreaterThan(30);
    expect(near).toBeGreaterThan(far + 30);
  });

  it("turns right for a positive yaw and down for a positive pitch", () => {
    const a = (10 * Math.PI) / 180;
    expect(project(cam, 511, 392, 150, a, 0, 0)[0]).toBeGreaterThan(511);
    expect(project(cam, 511, 392, 150, 0, a, 0)[1]).toBeGreaterThan(392);
  });

  it("takes only its share of the turn", () => {
    const a = (20 * Math.PI) / 180;
    const full = project(cam, 511, 392, 150, a, 0, 0, 1)[0] - 511;
    const half = project(cam, 511, 392, 150, a, 0, 0, 0.5)[0] - 511;
    expect(half).toBeLessThan(full * 0.6);
    expect(half).toBeGreaterThan(full * 0.4);
  });

  it("rotates without changing length", () => {
    const [x, y, z] = rotate(3, -4, 12, 0.4, -0.3, 0.2);
    expect(Math.hypot(x, y, z)).toBeCloseTo(13);
  });
});

describe("the tearing fade", () => {
  const yaw = (20 * Math.PI) / 180;

  it("keeps a smooth surface and anything unturned", () => {
    expect(tearFade(2, 2, 4, yaw, 0)).toBe(1);
    expect(tearFade(200, 200, 4, 0, 0)).toBe(1);
  });

  it("fades a triangle across a depth step by how far the turn stretches it", () => {
    const some = tearFade(25, 0, 4, yaw, 0); // ~2.1 cells
    expect(some).toBeGreaterThan(0);
    expect(some).toBeLessThan(1);
    expect(tearFade(60, 0, 4, yaw, 0)).toBe(0);
    // a vertical step only tears under a pitch
    expect(tearFade(0, 60, 4, yaw, 0)).toBe(1);
    expect(tearFade(0, 60, 4, 0, yaw)).toBe(0);
  });
});

describe("the fit", () => {
  it("stands the art on the canvas's bottom edge, centred across", () => {
    const v = fitView(440, 440, 1024, 1024, 0.94);
    const clip = (x: number, y: number) => [x * v[0]! + v[2]!, y * v[1]! + v[3]!];
    const [bx, by] = clip(512, 1024);
    expect(bx).toBeCloseTo(0);
    expect(by).toBeCloseTo(-1);
    const [, top] = clip(512, 0);
    expect(top).toBeLessThan(1);
    expect(top).toBeCloseTo(-1 + 2 * 0.94);
  });
});
