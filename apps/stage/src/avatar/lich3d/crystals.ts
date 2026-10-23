// The crown: the portrait's eleven obsidian shards, fanned above the skull. Pure: the specs and the
// triangle soup are plain numbers, so the same seed gives the same crown byte for byte (the tests
// hold it) and three.js only wraps the arrays.
//
// The layout is the M8 rig's (`characters/malakor/rig/m8.spec.json`, the `shard_*` elements): each
// shard's polygon centroid, its extent along and across its axis, how much of the head's turn it
// follows and its float, in portrait px. The cut polygons carry a margin around the stone, so the
// stone is a share of them (STONE_LEN, STONE_GIRTH). The renderer projects each shard through the
// lich camera onto its depth plane (`depth`, skull heights in front of the skull's centre), so at
// rest it covers the portrait's shard, and when the head turns the planes parallax.
//
// Each stone is a faceted bipyramid the portrait's shape: widest a little above its middle, a short
// point above and a long taper below, its rings jittered so no two share a silhouette.

import { mulberry32 } from "../driver";

export interface ShardLayout {
  name: string;
  /** Polygon centroid, portrait px (y down). */
  cx: number;
  cy: number;
  /** The polygon's extent along its (vertical) axis and across it, portrait px. */
  polyLen: number;
  polyWidth: number;
  /** Share of the head's turn the shard follows (the spec's `follow`). */
  follow: number;
  /** Float: amplitude [x, y] px, frequency Hz, rotation amplitude degrees (the spec's `float`). */
  amp: readonly [number, number];
  freq: number;
  rotDeg: number;
  /** Depth plane, skull heights in front of the skull's centre (+ toward the camera). */
  depth: number;
  /** Lean off vertical as the portrait paints it, degrees (+: the point leans to the viewer's left). */
  lean: number;
}

/** The rig's shards, the centre stone first (spec numbers; `depth` and `lean` read off the portrait). */
export const CROWN: readonly ShardLayout[] = [
  { name: "shard_c", cx: 512.3, cy: 111.6, polyLen: 230, polyWidth: 102, follow: 1.0, amp: [0.6, 2.5], freq: 0.12, rotDeg: 0.6, depth: 0.42, lean: 0 },
  { name: "shard_l1", cx: 416.2, cy: 125.1, polyLen: 96, polyWidth: 50, follow: 0.8, amp: [1.2, 3.5], freq: 0.17, rotDeg: 2.0, depth: 0.04, lean: 3 },
  { name: "shard_r1", cx: 608.1, cy: 123.8, polyLen: 94, polyWidth: 50, follow: 0.8, amp: [1.2, 3.5], freq: 0.16, rotDeg: 2.0, depth: 0.04, lean: 1 },
  { name: "shard_l2", cx: 337.8, cy: 152.9, polyLen: 188, polyWidth: 77, follow: 0.7, amp: [1.5, 4.0], freq: 0.14, rotDeg: 2.5, depth: -0.04, lean: 5 },
  { name: "shard_r2", cx: 684.1, cy: 151.4, polyLen: 188, polyWidth: 74, follow: 0.7, amp: [1.5, 4.0], freq: 0.15, rotDeg: 2.5, depth: -0.04, lean: 4 },
  { name: "shard_l3", cx: 259.0, cy: 200.0, polyLen: 144, polyWidth: 54, follow: 0.6, amp: [1.5, 4.5], freq: 0.19, rotDeg: 3.0, depth: -0.16, lean: 5 },
  { name: "shard_r3", cx: 765.0, cy: 200.0, polyLen: 144, polyWidth: 54, follow: 0.6, amp: [1.5, 4.5], freq: 0.18, rotDeg: 3.0, depth: -0.16, lean: 3 },
  { name: "shard_l4", cx: 310.0, cy: 266.0, polyLen: 100, polyWidth: 46, follow: 0.6, amp: [1.5, 4.0], freq: 0.165, rotDeg: 2.5, depth: -0.1, lean: 4 },
  { name: "shard_r4", cx: 715.0, cy: 265.5, polyLen: 101, polyWidth: 46, follow: 0.6, amp: [1.5, 4.0], freq: 0.155, rotDeg: 2.5, depth: -0.1, lean: -1 },
  { name: "shard_l5", cx: 264.5, cy: 300.0, polyLen: 56, polyWidth: 43, follow: 0.5, amp: [1.5, 4.5], freq: 0.21, rotDeg: 3.5, depth: -0.2, lean: 6 },
  { name: "shard_r5", cx: 761.0, cy: 300.0, polyLen: 60, polyWidth: 42, follow: 0.5, amp: [1.5, 4.5], freq: 0.2, rotDeg: 3.5, depth: -0.2, lean: 2 },
];

/** The stone's share of its cut polygon: the centre stone is cut tight, the others with a margin. */
/** The portrait the layout is measured on (the spec's width and height), px. */
export const PORTRAIT_PX = 1024;

export const STONE_LEN = { centre: 1.0, other: 0.9 } as const;
export const STONE_GIRTH = { centre: 0.95, other: 0.8 } as const;
/** The float, scaled up from the 2D rig's px (a 3D stone's bob reads smaller at the same px). */
export const FLOAT_GAIN = 2.5;

export interface CrystalSpec {
  layout: ShardLayout;
  /** Portrait px: the stone's centre, its length and girth. */
  cx: number;
  cy: number;
  length: number;
  width: number;
  /** Lean in radians (three's z: + counter-clockwise as the viewer sees it). */
  tilt: number;
  /** A fixed turn about its long axis, so each shows a different facet to the camera (rad). */
  turn0: number;
  /** Float phases (rad). */
  phaseX: number;
  phaseY: number;
  phaseR: number;
  /** Faces around the girth. */
  sides: number;
  /** A unit stone (y -0.5..0.5, girth 1 across): non-indexed triangles, xyz per vertex. */
  positions: Float32Array;
}

/**
 * One stone: a lower point, a waist ring (the widest), a shoulder ring and an upper point, `sides`
 * faces around. The lower taper is long, the upper point short, as the portrait's shards.
 */
export function crystalPositions(rnd: () => number, sides: number): Float32Array {
  const twist = rnd() * 0.4;
  const ring = (y: number, r: number, spin: number) =>
    Array.from({ length: sides }, (_, i) => {
      const a = ((i + spin + (rnd() - 0.5) * 0.35) / sides) * Math.PI * 2;
      const rr = r * (0.88 + rnd() * 0.24);
      return [Math.cos(a) * rr, y + (rnd() - 0.5) * 0.04, Math.sin(a) * rr] as const;
    });
  const low = [(rnd() - 0.5) * 0.05, -0.5, (rnd() - 0.5) * 0.05] as const;
  const high = [(rnd() - 0.5) * 0.08, 0.5, (rnd() - 0.5) * 0.08] as const;
  const r0 = ring(-0.2 + (rnd() - 0.5) * 0.06, 0.34, 0);
  const r1 = ring(0.08 + (rnd() - 0.5) * 0.06, 0.5, twist);
  const r2 = ring(0.24 + (rnd() - 0.5) * 0.05, 0.4, twist * 2);
  const out: number[] = [];
  const tri = (a: readonly number[], b: readonly number[], c: readonly number[]) => out.push(...a, ...b, ...c);
  const band = (lo: (readonly number[])[], hi: (readonly number[])[], i: number, j: number) => {
    // counter-clockwise from outside
    tri(lo[i]!, lo[j]!, hi[j]!);
    tri(lo[i]!, hi[j]!, hi[i]!);
  };
  for (let i = 0; i < sides; i++) {
    const j = (i + 1) % sides;
    tri(low, r0[j]!, r0[i]!);
    band(r0, r1, i, j);
    band(r1, r2, i, j);
    tri(r2[i]!, r2[j]!, high);
  }
  return new Float32Array(out);
}

/** The crown for `seed`: one spec per CROWN shard, in its order. */
export function crystalCrown(seed: number): CrystalSpec[] {
  const rnd = mulberry32(seed);
  return CROWN.map((s, i) => {
    const centre = i === 0;
    const sides = centre ? 6 : 5 + Math.floor(rnd() * 2);
    return {
      layout: s,
      cx: s.cx,
      cy: s.cy,
      length: s.polyLen * (centre ? STONE_LEN.centre : STONE_LEN.other),
      width: s.polyWidth * (centre ? STONE_GIRTH.centre : STONE_GIRTH.other),
      tilt: ((s.lean + (rnd() - 0.5) * 2) * Math.PI) / 180,
      turn0: rnd() * Math.PI * 2,
      phaseX: rnd() * Math.PI * 2,
      phaseY: rnd() * Math.PI * 2,
      phaseR: rnd() * Math.PI * 2,
      sides,
      positions: crystalPositions(rnd, sides),
    };
  });
}

/** A stone's float at `t`: an offset in portrait px (y down), a roll and a turn about its axis (rad). */
export interface CrystalFloat {
  dx: number;
  dy: number;
  roll: number;
  turn: number;
}

/**
 * Where a stone floats at `t` with the float scaled by `alive` (0: at its rest place). `t` is the
 * crown's own clock, which advances only as fast as the face is alive, so a settling face slows
 * its stones to a stop instead of snapping them back.
 */
export function crystalAt(c: CrystalSpec, t: number, alive: number): CrystalFloat {
  const { amp, freq, rotDeg } = c.layout;
  const w = t * freq * Math.PI * 2;
  return {
    dx: Math.sin(w * 0.8 + c.phaseX) * amp[0] * FLOAT_GAIN * alive,
    dy: Math.sin(w + c.phaseY) * amp[1] * FLOAT_GAIN * alive,
    roll: c.tilt + (Math.sin(w * 0.9 + c.phaseR) * rotDeg * Math.PI * alive) / 180,
    turn: c.turn0 + Math.sin(w * 0.6 + c.phaseR) * 0.3 * alive,
  };
}
