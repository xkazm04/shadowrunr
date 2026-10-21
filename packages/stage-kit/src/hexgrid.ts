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

/** An axis-aligned box in pixels. */
export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * A panel the map is fitted into, in its own pixels: its size, what stands over it (`avoid`:
 * Malakor's corner, a window in front; no hex goes under these) and what it should keep clear
 * when that is cheap (`prefer`: the overlay lane).
 */
export interface HexFrame {
  w: number;
  h: number;
  avoid?: Box[];
  prefer?: Box[];
}

/** Where a fitted map sits: a hex's panel position is `axialToPixel(q, r, size) * scale + (x, y)`. */
export interface MapFit {
  scale: number;
  x: number;
  y: number;
}

/** True when two boxes share any area (touching edges do not). */
export function overlaps(a: Box, b: Box): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** Each hex's box (centre ± its half extents) under `fit`. */
export function hexBoxes(hexes: readonly Hex[], size: number, fit: MapFit): Box[] {
  const hw = ((size * SQRT3) / 2) * fit.scale;
  const hh = size * fit.scale;
  return hexes.map((h) => {
    const p = axialToPixel(h.q, h.r, size);
    return { x: p.x * fit.scale + fit.x - hw, y: p.y * fit.scale + fit.y - hh, w: 2 * hw, h: 2 * hh };
  });
}

const ALIGN = [0.5, 1, 0] as const;

/** The parts of `area` beside each box (left, right, above, below), and `area` itself first. */
export function subAreas(area: Box, boxes: readonly Box[]): Box[] {
  const out: Box[] = [area];
  const right = area.x + area.w;
  const bottom = area.y + area.h;
  for (const a of boxes) {
    out.push({ ...area, w: Math.min(right, a.x) - area.x });
    out.push({ ...area, x: Math.max(area.x, a.x + a.w), w: right - Math.max(area.x, a.x + a.w) });
    out.push({ ...area, h: Math.min(bottom, a.y) - area.y });
    out.push({ ...area, y: Math.max(area.y, a.y + a.h), h: bottom - Math.max(area.y, a.y + a.h) });
  }
  return out.filter((b) => b.w > 0 && b.h > 0);
}

function fitIn(hexes: readonly Hex[], size: number, area: Box, avoid: readonly Box[], maxScale: number): MapFit | null {
  const b = mapBounds(hexes, size);
  const bw = b.maxX - b.minX;
  const bh = b.maxY - b.minY;
  const top = Math.min(maxScale, area.w / bw, area.h / bh);
  for (let scale = top; scale >= top * 0.35; scale *= 0.97) {
    for (const ay of ALIGN) {
      for (const ax of ALIGN) {
        const fit = { scale, x: area.x + ax * (area.w - bw * scale) - b.minX * scale, y: area.y + ay * (area.h - bh * scale) - b.minY * scale };
        if (hexBoxes(hexes, size, fit).every((h) => avoid.every((a) => !overlaps(h, a)))) return fit;
      }
    }
  }
  return null;
}

/** The largest fit over the whole area and the parts beside each box (centred in its part). */
function bestFit(hexes: readonly Hex[], size: number, area: Box, avoid: readonly Box[], maxScale: number): MapFit | null {
  let best: MapFit | null = null;
  for (const part of subAreas(area, avoid)) {
    const f = fitIn(hexes, size, part, avoid, maxScale);
    if (f && (!best || f.scale > best.scale * 1.001)) best = f;
  }
  return best;
}

/**
 * Fit the map into `area` as large as it goes (at most `maxScale`) with no hex's box touching any
 * `avoid` box (what stands over the map: Malakor's corner, a window in front). `prefer` boxes (an
 * overlay lane) are kept clear too while that costs less than a fifth of the scale. With nothing
 * that fits, the map is centred at the smallest scale tried.
 */
export function fitMap(
  hexes: readonly Hex[],
  size: number,
  area: Box,
  avoid: readonly Box[] = [],
  prefer: readonly Box[] = [],
  maxScale = 1,
): MapFit {
  const hard = bestFit(hexes, size, area, avoid, maxScale);
  if (prefer.length > 0) {
    const soft = bestFit(hexes, size, area, [...avoid, ...prefer], maxScale);
    if (soft && (!hard || soft.scale >= hard.scale * 0.8)) return soft;
  }
  if (hard) return hard;
  const b = mapBounds(hexes, size);
  const scale = Math.min(maxScale, area.w / (b.maxX - b.minX), area.h / (b.maxY - b.minY)) * 0.35;
  return {
    scale,
    x: area.x + (area.w - (b.maxX - b.minX) * scale) / 2 - b.minX * scale,
    y: area.y + (area.h - (b.maxY - b.minY) * scale) / 2 - b.minY * scale,
  };
}

/** Axial distance between two cells. */
export function hexDistance(a: { q: number; r: number }, b: { q: number; r: number }): number {
  const dq = a.q - b.q;
  const dr = a.r - b.r;
  return (Math.abs(dq) + Math.abs(dr) + Math.abs(dq + dr)) / 2;
}

/** How far a focused map may scale up: the focused hex's label and grade read large, not huge. */
export const MAP_ZOOM_MAX = 2.4;

/** A map zoomed to one hex (`show_screen` with a `focus`). */
export interface MapZoom {
  fit: MapFit;
  /** The focused hex and the hexes of its ring of neighbours, drawn in full. */
  ring: string[];
  /** Hexes outside the ring that the zoom puts under what stands over the panel: not drawn. */
  under: string[];
}

/**
 * Zoom the map to `focus` and its ring of neighbours: the fit is the one for the focused cell and
 * all six cells around it (whether the case has hexes there or not, so every focus zooms alike),
 * up to `maxScale`. The ring keeps clear of `avoid` and, unlike the overview, of `prefer` too (the
 * overlay lane): a zoom is about one hex, so no card stands over it. Only when no fit clears both
 * does `prefer` go back to being soft. The hexes beyond the ring stay on the map, dimmed; any that
 * would sit under an `avoid` box are listed in `under`, so no hex is ever drawn under Malakor.
 * Null when `focus` is not on the map.
 */
export function zoomMap(
  hexes: readonly Hex[],
  size: number,
  focus: string,
  area: Box,
  avoid: readonly Box[] = [],
  prefer: readonly Box[] = [],
  maxScale = MAP_ZOOM_MAX,
): MapZoom | null {
  const f = hexes.find((h) => h.id === focus);
  if (!f) return null;
  const cells: Hex[] = [f, ...DIRS.map(([dq, dr]) => ({ ...f, id: `${f.id}+${dq},${dr}`, q: f.q + dq, r: f.r + dr }))];
  const clear = [...avoid, ...prefer];
  const strict = fitMap(cells, size, area, clear, [], maxScale);
  const clean = hexBoxes(cells, size, strict).every((b) => clear.every((a) => !overlaps(b, a)));
  const fit = clean ? strict : fitMap(cells, size, area, avoid, prefer, maxScale);
  const ring = hexes.filter((h) => hexDistance(h, f) <= 1).map((h) => h.id);
  const boxes = hexBoxes(hexes, size, fit);
  const under = hexes
    .filter((h, i) => !ring.includes(h.id) && avoid.some((a) => overlaps(boxes[i]!, a)))
    .map((h) => h.id);
  return { fit, ring, under };
}
