// Axial hex geometry (pointy-top) and the case → drawable map resolution.

import type { Case, Hex } from "@shadowrunr/contract";

export const SQRT3 = Math.sqrt(3);

/** Pixel centre of axial (q, r) for pointy-top hexes of circumradius `size`. */
export function axialToPixel(q: number, r: number, size: number): { x: number; y: number } {
  return { x: size * SQRT3 * (q + r / 2), y: size * 1.5 * r };
}

/** SVG `points` for a pointy-top hex centred on (0, 0). */
export function hexPoints(size: number): string {
  const pts: string[] = [];
  for (let i = 0; i < 6; i++) {
    const a = (Math.PI / 180) * (60 * i - 30);
    pts.push(`${(size * Math.cos(a)).toFixed(2)},${(size * Math.sin(a)).toFixed(2)}`);
  }
  return pts.join(" ");
}

const DIRS: [number, number][] = [
  [1, 0],
  [1, -1],
  [0, -1],
  [-1, 0],
  [-1, 1],
  [0, 1],
];

/** Axial cells in spiral order from the origin: ring 0, ring 1, ring 2... */
export function spiral(count: number): [number, number][] {
  const out: [number, number][] = [[0, 0]];
  for (let radius = 1; out.length < count; radius++) {
    let q = DIRS[4]![0] * radius;
    let r = DIRS[4]![1] * radius;
    for (let side = 0; side < 6; side++) {
      for (let step = 0; step < radius; step++) {
        out.push([q, r]);
        q += DIRS[side]![0];
        r += DIRS[side]![1];
      }
    }
  }
  return out.slice(0, count);
}

export interface MapModel {
  hexes: Hex[];
  links: [string, string][];
  /** Hex ids the transcript revealed that the case file did not contain (placed procedurally). */
  unplaced: string[];
}

/**
 * Hexes to draw. The case supplies coordinates and labels; any revealed hex id missing from it
 * is placed on the next free spiral cell with its id as the label, so a transcript is always
 * drawable, even without a case file.
 */
export function buildMap(c: Case | null, revealed: readonly string[]): MapModel {
  const hexes: Hex[] = c ? c.hexes.map((h) => ({ ...h })) : [];
  const links = c?.links ? [...c.links] : [];
  const known = new Set(hexes.map((h) => h.id));
  const used = new Set(hexes.map((h) => `${h.q},${h.r}`));
  const missing = revealed.filter((id) => !known.has(id));
  if (missing.length > 0) {
    const cells = spiral(hexes.length + missing.length + 64);
    let ci = 0;
    for (const id of missing) {
      while (ci < cells.length && used.has(`${cells[ci]![0]},${cells[ci]![1]}`)) ci++;
      const [q, r] = cells[ci] ?? [ci, 0];
      used.add(`${q},${r}`);
      hexes.push({ id, q, r, kind: "claim", label: id, fog: true });
      ci++;
    }
  }
  return { hexes, links, unplaced: missing };
}

/** Bounding box of the drawn map in pixels (centres ± one hex). */
export function mapBounds(hexes: readonly Hex[], size: number) {
  if (hexes.length === 0) return { minX: -size, minY: -size, maxX: size, maxY: size };
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const h of hexes) {
    const { x, y } = axialToPixel(h.q, h.r, size);
    minX = Math.min(minX, x - (size * SQRT3) / 2);
    maxX = Math.max(maxX, x + (size * SQRT3) / 2);
    minY = Math.min(minY, y - size);
    maxY = Math.max(maxY, y + size);
  }
  return { minX, minY, maxX, maxY };
}

/** SVG `points` for a pointy-top hex centred on (cx, cy), absolute and rounded to `digits`
 * (the Poster Wall's screen draws its case map this way: one decimal). */
export function hexPointsAt(cx: number, cy: number, size: number, digits = 1): string {
  const pts: string[] = [];
  for (let i = 0; i < 6; i++) {
    const a = (Math.PI / 180) * (60 * i - 30);
    pts.push(`${(cx + size * Math.cos(a)).toFixed(digits)},${(cy + size * Math.sin(a)).toFixed(digits)}`);
  }
  return pts.join(" ");
}
