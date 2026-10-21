// The cut-out's plan: depths for every head-bound layer, the layout in the box, the fade, the
// rim's balance and the whole matte.
import { describe, expect, it, vi } from "vitest";

import type { FormSpec, LayerSpec } from "../layers";
import { closeMatte, fillHoles, wholeMatte } from "./matte";
import { artBox, cutLayout, cutMask, HEADROOM, lightRig, newLightRig, planCut, rimGains, ringRadius, shardDepth } from "./plan";
import { startFace } from "./index";

const HEAD = { center: [511, 392] as [number, number], radius: [190, 235] as [number, number], pivot_y: 612, chin_y: 622 };
const shard = (name: string, x: number, follow: number, extra: Partial<LayerSpec> = {}): LayerSpec => ({
  name,
  kind: "float",
  file: `${name}.png`,
  bbox: [x - 25, 100, 50, 100],
  follow,
  ...extra,
});

/** A rig shaped like M8: backplate, body, head, jaw, shards (one with a recorded depth), eyes, monocle. */
const spec: FormSpec = {
  version: 1,
  width: 1024,
  height: 1024,
  rig_type: "glow",
  rig: { head: HEAD },
  layers: [
    { name: "background", kind: "static", file: "background.png", bbox: [0, 0, 1024, 1024] },
    { name: "torso", kind: "body", file: "torso.png", bbox: [0, 445, 1024, 579], mode: "cut", occl: 1 },
    { name: "head", kind: "head", file: "head.png", bbox: [317, 153, 389, 424], mode: "cut", occl: 1 },
    { name: "jaw", kind: "aperture_lower", file: "jaw.png", bbox: [370, 511, 283, 118], depth: 0.466, occl: 1 },
    shard("side", 760, 0.5),
    shard("mid", 690, 0.7),
    shard("front", 605, 0.8),
    shard("centre", 511, 1, { depth: 0.25 }),
    { name: "eye_l", kind: "eye", file: "eye_l.png", bbox: [324, 263, 196, 170], depth: 0.518, mode: "light" },
    { name: "orb", kind: "pulse", file: "orb.png", bbox: [480, 360, 60, 60], mode: "light" },
  ],
};

describe("planCut", () => {
  const plan = planCut(spec);
  const by = (n: string) => plan.layers.find((l) => l.spec.name === n);

  it("keeps the backplate out of the drawn layers, and the solids as the matte", () => {
    expect(plan.backplate?.name).toBe("background");
    expect(by("background")).toBeUndefined();
    expect(plan.matte.map((l) => l.name)).toEqual(["torso", "head", "jaw"]);
  });

  it("gives every head-bound layer a depth, the rig's where it records one", () => {
    expect(by("jaw")).toMatchObject({ depth: 0.466, recorded: true, follow: 1 });
    expect(by("eye_l")).toMatchObject({ depth: 0.518, recorded: true });
    expect(by("centre")).toMatchObject({ depth: 0.25, recorded: true });
    for (const n of ["side", "mid", "front", "orb"]) {
      expect(by(n)?.recorded).toBe(false);
      expect(by(n)?.follow).toBe(1); // one shared field: no layer turns by less than the head
    }
    // a part on the skull sits on its surface (the orb near the face's centre, nearly the front)
    expect(by("orb")?.depth).toBeGreaterThan(0.5);
    expect(by("orb")?.depth).toBeLessThanOrEqual(0.6);
    // the body blends into the head at the neck in the shader, not by `follow`
    expect(by("torso")).toMatchObject({ follow: 0, depth: 0 });
  });

  it("derives the shards' depth from where they stand on the crown and how closely they follow", () => {
    const side = by("side")?.depth ?? -1;
    const mid = by("mid")?.depth ?? -1;
    const front = by("front")?.depth ?? -1;
    expect(front).toBeGreaterThan(mid);
    expect(mid).toBeGreaterThan(side);
    expect(side).toBeGreaterThanOrEqual(0);
    expect(front).toBeLessThan(0.6); // never ahead of the face
  });

  it("fades out over the body's last 8%", () => {
    expect(plan.fade[1]).toBe(1024);
    expect(plan.fade[0]).toBeCloseTo(1024 - 0.08 * 579, 6);
  });
});

describe("shardDepth and ringRadius", () => {
  it("is deepest at the ring's front, zero at its side, scaled by follow", () => {
    expect(shardDepth(0, 1.4, 1)).toBeCloseTo(0.45, 9);
    expect(shardDepth(1.4, 1.4, 1)).toBe(0);
    expect(shardDepth(-2, 1.4, 1)).toBe(0);
    expect(shardDepth(0, 1.4, 0.5)).toBeCloseTo(0.225, 9);
    expect(shardDepth(0.7, 1.4, 1)).toBeCloseTo(0.45 * Math.sqrt(0.75), 9);
  });

  it("puts the ring past the widest shard and never inside the skull", () => {
    expect(ringRadius([0.2, -1.3, 0.9])).toBeCloseTo(1.43, 9);
    expect(ringRadius([0.1])).toBeCloseTo(1.1, 9);
    expect(ringRadius([])).toBeCloseTo(1.1, 9);
  });
});

describe("the box", () => {
  /** Where art point (x, y) lands in the canvas, in px. */
  const place = (v: number[], cw: number, ch: number, x: number, y: number): [number, number] => [
    ((x * (v[0] ?? 0) + (v[2] ?? 0) + 1) / 2) * cw,
    ((1 - (y * (v[1] ?? 0) + (v[3] ?? 0))) / 2) * ch,
  ];

  it("sits the art's bottom on the box's floor, centred, with headroom", () => {
    const v = cutLayout(440, 440, 1024, 1024);
    const [x0, yb] = place(v, 440, 440, 0, 1024);
    const [x1, yt] = place(v, 440, 440, 1024, 0);
    expect(yb).toBeCloseTo(440, 6);
    expect(yt).toBeCloseTo(440 - 440 / (1 + HEADROOM), 6);
    expect(x0 + x1).toBeCloseTo(440, 6);
  });

  it("gives the art's box in uv for the shadow", () => {
    const b = artBox(cutLayout(440, 440, 1024, 1024), 1024, 1024);
    expect(b[1]).toBeCloseTo(0, 6); // the floor
    expect(b[3]).toBeCloseTo(1 / (1 + HEADROOM), 6);
    expect((b[0] ?? 0) + (b[2] ?? 0)).toBeCloseTo(1, 6);
  });

  it("dissolves the bottom and the low sides, and leaves the rest whole", () => {
    const fade: [number, number] = [970, 1024];
    expect(cutMask(512, 500, 1024, fade)).toBe(1);
    expect(cutMask(512, 1024, 1024, fade)).toBe(0);
    expect(cutMask(512, 997, 1024, fade)).toBeCloseTo(0.5, 6);
    expect(cutMask(4, 960, 1024, fade)).toBeLessThan(0.05); // a shoulder at the box's side
    expect(cutMask(4, 300, 1024, fade)).toBe(1); // high up nothing is cut
  });
});

describe("the rim and the shadow", () => {
  it("lights the side he turns toward more, evenly when he looks straight out", () => {
    const ahead = rimGains(0);
    expect(ahead.right).toBeCloseTo(ahead.left, 9);
    const right = rimGains(12);
    expect(right.right).toBeCloseTo(1, 9);
    expect(right.left).toBeCloseTo(0.3, 9);
    expect(rimGains(-40)).toEqual(rimGains(-12));
  });

  it("uses the rig's teal on the right, its ember on the left, the shadow falling from the light", () => {
    const box = artBox(cutLayout(440, 440, 1024, 1024), 1024, 1024);
    const r = lightRig(newLightRig(), 12, 0, [0.2, 0.85, 0.78], [1, 0.62, 0.22], box);
    expect(r.rimR[1]).toBeGreaterThan(r.rimR[0] ?? 0); // teal
    expect(r.rimL[0]).toBeGreaterThan(r.rimL[2] ?? 0); // ember
    expect(r.rimR[1]).toBeGreaterThan(r.rimL[1] ?? 0);
    expect(r.shadow[0]).toBeLessThan(0); // lit from the right, cast to the left
    expect(lightRig(newLightRig(), -12, 0, [0.2, 0.85, 0.78], [1, 0.62, 0.22], box).shadow[0]).toBeGreaterThan(0);
    expect(r.pool[0]).toBeCloseTo(0.5, 6);
  });
});

describe("the whole matte", () => {
  const N = 16;
  /** A ring (a body with a hole in it), plus a notch open to the outside. */
  const ring = () => {
    const m = new Uint8Array(N * N);
    for (let y = 2; y < 14; y++) for (let x = 2; x < 14; x++) m[y * N + x] = 255;
    for (let y = 5; y < 9; y++) for (let x = 5; x < 9; x++) m[y * N + x] = 0; // the hole
    return m;
  };

  it("fills a hole that cannot reach the border and leaves the outside empty", () => {
    const f = fillHoles(ring(), N);
    expect(f[6 * N + 6]).toBe(255);
    expect(f[0]).toBe(0);
    expect(f[1 * N + 8]).toBe(0);
  });

  it("closing seals a narrow channel to the outside so the hole behind it is filled too", () => {
    const m = ring();
    for (let y = 2; y < 6; y++) m[y * N + 7] = 0; // a one-pixel channel from the hole to the top edge
    expect(fillHoles(m, N)[6 * N + 6]).toBe(0); // open: not a hole
    const w = wholeMatte(m, N, 1);
    expect(w[6 * N + 6]).toBe(255);
    expect(w[0]).toBe(0);
    // never less than the union
    for (let i = 0; i < m.length; i++) expect(w[i] ?? 0).toBeGreaterThanOrEqual(m[i] ?? 0);
  });

  it("closing keeps a solid block's outline", () => {
    const m = new Uint8Array(N * N);
    for (let y = 4; y < 12; y++) for (let x = 4; x < 12; x++) m[y * N + x] = 255;
    expect(Array.from(closeMatte(m, N, 2))).toEqual(Array.from(m));
  });
});

describe("startFace without WebGL2", () => {
  it("is unavailable, hides its canvas and keeps the silhouette, like the glow", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const attrs = new Map<string, string>();
    const root = { setAttribute: (k: string, v: string) => void attrs.set(k, v) } as unknown as HTMLElement;
    const canvas = { width: 0, height: 0, style: { visibility: "" } } as unknown as HTMLCanvasElement;
    const drawn: unknown[] = [];
    const handle = startFace({
      root,
      canvas,
      rigBase: null,
      source: { subscribe: () => () => {} },
      size: 440,
      look: { get: () => ({ x: 1, y: 0 }) },
      onDrawn: (f) => drawn.push(f),
    });
    expect(attrs.get("data-face")).toBe("unavailable");
    expect(canvas.style.visibility).toBe("hidden");
    expect(drawn).toEqual([null]);
    handle.resize(200);
    handle.dispose();
    expect(String(warn.mock.calls[0]?.[0])).toMatch(/WebGL2 is unavailable/);
    warn.mockRestore();
  });
});
