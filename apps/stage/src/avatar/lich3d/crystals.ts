// The crown: eleven faceted crystals floating around the head, made from a seed. Pure: the specs and
// the triangle soup are plain numbers, so the same seed gives the same crown byte for byte (the
// tests hold it) and three.js only wraps the arrays.
//
// The places come from the M8 rig's shard layers (their anchors and heights in portrait px, the
// skull 470 px tall about (511, 392)), so the procedural crown stands where the painted one did:
// a tall centre stone over the brow, then pairs falling away and outward to either side.

import { mulberry32 } from "../driver";

/** The rig's shards: [x, y, length] in skull units (y up), the centre stone first. */
export const CROWN: readonly (readonly [number, number, number])[] = [
  [0, 0.583, 0.5],
  [-0.202, 0.564, 0.22],
  [0.198, 0.566, 0.22],
  [-0.374, 0.509, 0.4],
  [0.377, 0.509, 0.4],
  [-0.538, 0.402, 0.32],
  [0.534, 0.408, 0.32],
  [-0.428, 0.268, 0.23],
  [0.434, 0.268, 0.23],
  [-0.521, 0.196, 0.14],
  [0.528, 0.194, 0.14],
];

export interface CrystalSpec {
  /** Rest centre, skull units. */
  x: number;
  y: number;
  z: number;
  /** Long axis and girth (the unit geometry is scaled by these). */
  length: number;
  width: number;
  /** Tilt off vertical, radians: about z (outward) and about x (toward the viewer). */
  tilt: number;
  lean: number;
  /** Spin about the long axis: start (rad) and rate (rad/s). */
  spin0: number;
  spinRate: number;
  /** Float: amplitude (units), frequency (Hz) and phase (rad). */
  bobAmp: number;
  bobFreq: number;
  bobPhase: number;
  /** Faces around the girth. */
  sides: number;
  /** A unit crystal (y -0.5..0.5, girth about 0.5): non-indexed triangles, xyz per vertex. */
  positions: Float32Array;
}

/**
 * One faceted crystal: a lower point, two jittered rings and an upper point, `sides` faces around.
 * The rings' radii and angles wander, so no two stones share a silhouette.
 */
export function crystalPositions(rnd: () => number, sides: number): Float32Array {
  const ring = (y: number, r: number) =>
    Array.from({ length: sides }, (_, i) => {
      const a = ((i + (rnd() - 0.5) * 0.5) / sides) * Math.PI * 2;
      const rr = r * (0.8 + rnd() * 0.4);
      return [Math.cos(a) * rr, y + (rnd() - 0.5) * 0.06, Math.sin(a) * rr] as const;
    });
  const low = [(rnd() - 0.5) * 0.08, -0.5, (rnd() - 0.5) * 0.08] as const;
  const high = [(rnd() - 0.5) * 0.12, 0.5, (rnd() - 0.5) * 0.12] as const;
  const r1 = ring(-0.22 + (rnd() - 0.5) * 0.08, 0.42);
  const r2 = ring(0.18 + (rnd() - 0.5) * 0.1, 0.36 + rnd() * 0.08);
  const out: number[] = [];
  const tri = (a: readonly number[], b: readonly number[], c: readonly number[]) => out.push(...a, ...b, ...c);
  for (let i = 0; i < sides; i++) {
    const j = (i + 1) % sides;
    const a1 = r1[i]!;
    const b1 = r1[j]!;
    const a2 = r2[i]!;
    const b2 = r2[j]!;
    // counter-clockwise from outside
    tri(low, b1, a1);
    tri(a1, b1, b2);
    tri(a1, b2, a2);
    tri(a2, b2, high);
  }
  return new Float32Array(out);
}

/** The crown for `seed`: eleven specs in CROWN's order. */
export function crystalCrown(seed: number): CrystalSpec[] {
  const rnd = mulberry32(seed);
  return CROWN.map(([x, y, length], i) => {
    const sides = 5 + Math.floor(rnd() * 3);
    const side = Math.sign(x);
    // the outer stones stand back around the cranium, the centre stone just behind the brow
    const z = i === 0 ? 0.04 : -0.1 - Math.abs(x) * 0.25 + (rnd() - 0.5) * 0.06;
    return {
      x: x + (rnd() - 0.5) * 0.02,
      y: y + (rnd() - 0.5) * 0.02,
      z,
      length: length * (0.95 + rnd() * 0.1),
      width: length * (0.3 + rnd() * 0.08),
      tilt: -side * (0.12 + Math.abs(x) * 0.35) + (rnd() - 0.5) * 0.12,
      lean: (rnd() - 0.5) * 0.25,
      spin0: rnd() * Math.PI * 2,
      spinRate: (0.12 + rnd() * 0.18) * (rnd() < 0.5 ? -1 : 1),
      bobAmp: 0.012 + rnd() * 0.014,
      bobFreq: 0.18 + rnd() * 0.16,
      bobPhase: rnd() * Math.PI * 2,
      sides,
      positions: crystalPositions(rnd, sides),
    };
  });
}

/**
 * Where a crystal floats at `t` with the float scaled by `alive` (0: at its rest place). `t` is the
 * crown's own clock, which advances only as fast as the face is alive, so a settling face slows
 * its stones to a stop instead of snapping them back.
 */
export function crystalAt(c: CrystalSpec, t: number, alive: number): { y: number; spin: number } {
  return {
    y: c.y + Math.sin(t * c.bobFreq * Math.PI * 2 + c.bobPhase) * c.bobAmp * alive,
    spin: c.spin0 + t * c.spinRate * alive,
  };
}
