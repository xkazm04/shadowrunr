// The evidence board (appendix (screens and board)): its regions, where each card sits and how a
// focus zooms it. Pure. A card's place comes from the case alone (its act and its index in it),
// never from what has been revealed, so it keeps its place as the board fills in, and every
// screen that draws the board (the board, the desk's window, a clip) uses the same layout.

import type { CaseAct, Hex } from "@shadowrunr/contract";
import { KIND_REGION, LOOSE_REGION } from "./boardNames";
import { overlaps, subAreas, type Box, type MapFit } from "./hexgrid";

export interface BoardRegion {
  name: string;
  hexes: string[];
}

/**
 * The board's regions, left to right. The case's acts when it has them (a hex named twice stays
 * in its first act); without acts, its hexes grouped by kind. Hexes no act names, and any the
 * case does not have (placed by `buildMap`), go to a last "Loose ends" region. Empty regions go.
 */
export function boardRegions(acts: readonly CaseAct[] | undefined, hexes: readonly Hex[]): BoardRegion[] {
  const known = new Set(hexes.map((h) => h.id));
  const placed = new Set<string>();
  const out: BoardRegion[] = [];
  const take = (ids: readonly string[]): string[] => {
    const fresh = ids.filter((id) => known.has(id) && !placed.has(id));
    for (const id of fresh) placed.add(id);
    return [...new Set(fresh)];
  };
  if (acts && acts.length > 0) {
    for (const a of acts) out.push({ name: a.name, hexes: take(a.hexes) });
  } else {
    for (const [kind, name] of KIND_REGION) out.push({ name, hexes: take(hexes.filter((h) => h.kind === kind).map((h) => h.id)) });
  }
  out.push({ name: LOOSE_REGION, hexes: take(hexes.map((h) => h.id)) });
  return out.filter((r) => r.hexes.length > 0);
}

/** A window the board is drawn in, in its own pixels, and what stands over it (`avoid`: Malakor's
 * corner, a window in front; no card is drawn under these), should stay clear when a card is
 * focused (`prefer`), or is the overlay lane (`lane`: the observer badge, decrees and explainer
 * cards; the acts end left of it, so no act, card or string label is ever under an overlay, and
 * the thesis band ends where it starts). */
export interface BoardFrame {
  w: number;
  h: number;
  avoid?: Box[];
  prefer?: Box[];
  lane?: Box;
}

/** A card at full size, and the gaps, in board pixels. */
export const CARD_W = 212;
export const CARD_H = 112;
export const CARD_GAP = 14;
/** The thesis band at the top and the timeline strip at the bottom of a board window. */
export const THESIS_H = 64;
export const STRIP_H = 62;
export const BOARD_PAD = 16;
/** A region's act name above its cards, its inner padding and the gap between regions. */
export const ACT_HEAD = 34;
const REGION_PAD = 10;
const REGION_GAP = 10;
/** How far a focused board may scale up. */
export const BOARD_ZOOM_MAX = 1.8;

/** Where the regions and cards go in a window: between the thesis band and the timeline strip,
 * and left of the overlay lane where one stands over the window. */
export function boardArea(frame: Pick<BoardFrame, "w" | "h" | "lane">): Box {
  const area = { x: BOARD_PAD, y: THESIS_H, w: frame.w - 2 * BOARD_PAD, h: frame.h - THESIS_H - STRIP_H - BOARD_PAD / 2 };
  const lane = frame.lane;
  if (lane && lane.y < area.y + area.h && lane.y + lane.h > area.y && lane.x < area.x + area.w) {
    area.w = Math.max(0, lane.x - BOARD_PAD - area.x);
  }
  return area;
}

/** The part of a window the cards and strings are drawn in: all of it, or left of the overlay lane
 * (a zoom moves the board, and what it moves toward the lane is cut off at the lane's edge rather
 * than drawn under an overlay). */
export function boardClip(frame: Pick<BoardFrame, "w" | "h" | "lane">): Box {
  const lane = frame.lane;
  return { x: 0, y: 0, w: lane ? Math.min(frame.w, Math.max(0, lane.x - BOARD_PAD / 2)) : frame.w, h: frame.h };
}

/** Where the thesis band ends: the window's right edge, or left of an overlay (the lane, a badge)
 * standing over the band's right half. */
export function thesisEnd(frame: BoardFrame): number {
  const over = [...(frame.avoid ?? []), ...(frame.lane ? [frame.lane] : [])];
  return Math.min(frame.w, ...over.filter((a) => a.y < THESIS_H && a.y + a.h > 0 && a.x > frame.w / 2).map((a) => a.x - 12));
}

/** The timeline strip: along the window's bottom, right of anything that stands over that band. */
export function stripBox(frame: BoardFrame): Box {
  const y = frame.h - STRIP_H - 4;
  let x = BOARD_PAD;
  for (const a of frame.avoid ?? []) if (a.y + a.h > y && a.y < frame.h && a.x < frame.w / 2) x = Math.max(x, a.x + a.w + BOARD_PAD);
  return { x, y, w: Math.max(0, frame.w - BOARD_PAD - x), h: STRIP_H };
}

export interface LaidRegion extends BoardRegion {
  box: Box;
}

export interface BoardLayout {
  /** The frame it was laid out for. */
  w: number;
  h: number;
  /** The card scale it took to fit (1: full size). */
  scale: number;
  /** The area the regions were laid out in (`boardArea` of the frame), and what stood over it. */
  area: Box;
  avoid: Box[];
  regions: LaidRegion[];
  /** Each card's box, by hex id. */
  cards: Record<string, Box>;
}

/** Row pitches tried, as multiples of a card's height plus gap: a region with room spreads out. */
const PITCH = [1.45, 1.25, 1.1, 1] as const;
const MAX_COLS = 6;

/** A region's card slots at `x` with `cols` columns: rows top-down, left to right in each row,
 * skipping any slot that touches an avoid box. */
function slots(x: number, cols: number, area: Box, avoid: readonly Box[], s: number, pitch: number): Box[] {
  const cw = CARD_W * s;
  const ch = CARD_H * s;
  const gap = CARD_GAP * s;
  const top = area.y + ACT_HEAD * s;
  const bottom = area.y + area.h - REGION_PAD * s;
  const out: Box[] = [];
  for (let y = top; y + ch <= bottom + 0.01; y += (ch + gap) * pitch) {
    for (let c = 0; c < cols; c++) {
      const b = { x: x + REGION_PAD * s + c * (cw + gap), y, w: cw, h: ch };
      if (!avoid.some((a) => overlaps(b, a))) out.push(b);
    }
  }
  return out;
}

function regionWidth(cols: number, s: number): number {
  return 2 * REGION_PAD * s + cols * CARD_W * s + (cols - 1) * CARD_GAP * s;
}

/** Where a region `w` wide at `x` starts: the area's top, or below anything that stands over its
 * act name there (an overlay badge), so no act name and no card sits under it. */
function regionTop(x: number, w: number, area: Box, avoid: readonly Box[], s: number): number {
  let top = area.y;
  for (const a of avoid) {
    if (a.x < x + w && x < a.x + a.w && a.y < top + ACT_HEAD * s && a.y + a.h > top) top = a.y + a.h + REGION_GAP;
  }
  return top;
}

/** Lay the regions out at card scale `s`, each `extra` px wider than it needs; null when they do
 * not fit across the area or a region runs out of slots. */
function layAt(regions: readonly BoardRegion[], area: Box, avoid: readonly Box[], s: number, extra: number, colsIn?: number[]) {
  const cols: number[] = [];
  const laid: LaidRegion[] = [];
  const cards: Record<string, Box> = {};
  let x = area.x;
  for (const [i, r] of regions.entries()) {
    // Columns from one up (or exactly the ones given), the widest row pitch that holds the cards.
    let n = colsIn?.[i] ?? 1;
    const last = colsIn ? n : MAX_COLS;
    let found: { slots: Box[]; box: Box } | null = null;
    for (; n <= last && found === null; n++) {
      const w = regionWidth(n, s) + extra;
      const top = regionTop(x, w, area, avoid, s);
      const box = { x, y: top, w, h: area.y + area.h - top };
      for (const p of PITCH) {
        const sl = slots(x + extra / 2, n, box, avoid, s, p);
        if (sl.length >= r.hexes.length) {
          found = { slots: sl, box };
          break;
        }
      }
    }
    if (found === null) return null;
    cols.push(n - 1);
    laid.push({ ...r, box: found.box });
    const placed = found.slots;
    r.hexes.forEach((id, k) => (cards[id] = placed[k]!));
    x += found.box.w + REGION_GAP;
  }
  if (x - REGION_GAP > area.x + area.w + 0.01) return null;
  return { cols, laid, cards, used: x - REGION_GAP - area.x };
}

/**
 * Lay the board out in `frame`: regions as columns left to right across the board area, each as
 * wide as its cards need (more columns where a region has more cards than rows), the cards as
 * large as fits (at most full size) with none touching an avoid box. What width is left is shared
 * out between the regions, so the board fills its window.
 */
export function layoutBoard(regions: readonly BoardRegion[], frame: BoardFrame): BoardLayout {
  const area = boardArea(frame);
  const avoid = frame.avoid ?? [];
  for (let s = 1; s >= 0.3; s *= 0.96) {
    const first = layAt(regions, area, avoid, s, 0);
    if (!first) continue;
    const extra = regions.length > 0 ? (area.w - first.used) / regions.length : 0;
    const spread = extra > 1 ? layAt(regions, area, avoid, s, extra, first.cols) : null;
    const pick = spread ?? first;
    return { w: frame.w, h: frame.h, scale: s, area, avoid: [...avoid], regions: pick.laid, cards: pick.cards };
  }
  // Nothing fits (a case far larger than a board): every card in one row, small.
  const cards: Record<string, Box> = {};
  const all = regions.flatMap((r) => r.hexes);
  const w = area.w / Math.max(1, all.length);
  all.forEach((id, i) => (cards[id] = { x: area.x + i * w, y: area.y + ACT_HEAD, w: w * 0.9, h: w * 0.45 }));
  return { w: frame.w, h: frame.h, scale: w / CARD_W, area, avoid: [...avoid], regions: regions.map((r) => ({ ...r, box: { ...area } })), cards };
}

/** The region a card is in, or null. */
export function regionOf(layout: BoardLayout, id: string): LaidRegion | null {
  return layout.regions.find((r) => r.hexes.includes(id)) ?? null;
}

/** `b` under `fit`. */
export function fitBox(b: Box, fit: MapFit): Box {
  return { x: b.x * fit.scale + fit.x, y: b.y * fit.scale + fit.y, w: b.w * fit.scale, h: b.h * fit.scale };
}

function union(boxes: readonly Box[]): Box {
  const x = Math.min(...boxes.map((b) => b.x));
  const y = Math.min(...boxes.map((b) => b.y));
  return { x, y, w: Math.max(...boxes.map((b) => b.x + b.w)) - x, h: Math.max(...boxes.map((b) => b.y + b.h)) - y };
}

/** Where a target sits in the part it fits in: centred first, then against either edge. */
const ALIGN = [0.5, 0, 1] as const;

/** The largest fit of `target` into `area` (at most `maxScale`), in one of the parts beside the
 * avoid boxes (centred in it, else against an edge), with `target` touching none of them; null
 * when nothing fits. */
function fitTarget(target: Box, area: Box, avoid: readonly Box[], maxScale: number): MapFit | null {
  let best: MapFit | null = null;
  for (const part of subAreas(area, avoid)) {
    search: for (let scale = Math.min(maxScale, part.w / target.w, part.h / target.h); scale > 0.2; scale *= 0.97) {
      if (best && scale <= best.scale * 1.001) break;
      for (const ay of ALIGN) {
        for (const ax of ALIGN) {
          const fit = { scale, x: part.x + ax * (part.w - target.w * scale) - target.x * scale, y: part.y + ay * (part.h - target.h * scale) - target.y * scale };
          if (avoid.every((a) => !overlaps(fitBox(target, fit), a))) {
            best = fit;
            break search;
          }
        }
      }
    }
  }
  return best;
}

/** How a board window shows the layout now. */
export interface BoardView {
  fit: MapFit;
  /** The focused card and the cards strung to it, drawn in full (empty: the overview). */
  ring: string[];
  /** Cards this view would put under what stands over the window: not drawn. */
  under: string[];
}

/** Room left around a focused ring, in layout pixels. */
const RING_MARGIN = 24;

/**
 * The window's view of the layout. The overview fits the layout's area into the window's board
 * area (on the board screen, where the layout was made for the window, it is the identity). A
 * focus zooms to the card and the cards strung to it (and those strings' `labels`, by link), as
 * large as fits (at most `BOARD_ZOOM_MAX`) clear of `avoid` and, as a rule, of `prefer` too (only
 * when no fit clears
 * both at least at the overview's scale does `prefer` give way, and when neither does the view
 * stays the overview). Either way a card this view would put under an `avoid` box is listed in
 * `under`, so no card is ever drawn under Malakor.
 */
export function boardView(
  layout: BoardLayout,
  frame: BoardFrame,
  focus: string | null,
  links: readonly { from: string; to: string }[],
  labels: readonly (Box | null)[] = [],
): BoardView {
  const area = boardArea(frame);
  const avoid = frame.avoid ?? [];
  const home = layout.area;
  const s = Math.min(1, area.w / home.w, area.h / home.h);
  let fit: MapFit = { scale: s, x: area.x - home.x * s, y: area.y - home.y * s };
  let ring: string[] = [];
  const f = focus === null ? undefined : layout.cards[focus];
  if (focus !== null && f) {
    ring = [focus];
    const shown: Box[] = [];
    links.forEach((l, i) => {
      const other = l.from === focus ? l.to : l.to === focus ? l.from : null;
      if (other === null || !layout.cards[other]) return;
      if (!ring.includes(other)) ring.push(other);
      const label = labels[i];
      if (label) shown.push(label);
    });
    const box = union([...ring.map((id) => layout.cards[id]!), ...shown]);
    const target = { x: box.x - RING_MARGIN, y: box.y - RING_MARGIN, w: box.w + 2 * RING_MARGIN, h: box.h + 2 * RING_MARGIN };
    // A zoom only ever closes in: a ring spread wider than the window stays at the overview,
    // with the ring lit and the rest dimmed.
    const strict = fitTarget(target, area, [...avoid, ...(frame.prefer ?? [])], BOARD_ZOOM_MAX);
    const zoom = strict && strict.scale >= s ? strict : fitTarget(target, area, avoid, BOARD_ZOOM_MAX);
    if (zoom && zoom.scale >= s) fit = zoom;
  }
  const under = Object.entries(layout.cards)
    .filter(([id, b]) => !ring.includes(id) && avoid.some((a) => overlaps(fitBox(b, fit), a)))
    .map(([id]) => id);
  return { fit, ring, under };
}
