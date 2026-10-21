// The evidence board's strings and timeline strip, as geometry. Pure.
//
// A string runs from the edge of its `from` card to the edge of its `to` card and sags a little
// below the straight line, like string pinned on cork; more strings between the same two cards
// sag further, so none hides another. The arrowhead sits on the `to` card's edge along the
// curve's tangent, and the label rides the curve where it covers no card, or hangs beside it on a
// short leader where the free part of the string is too short to hold it.

import type { EvidenceGrade, Hex, ShownRecord } from "@shadowrunr/contract";
import type { Box } from "./hexgrid";

export interface Point {
  x: number;
  y: number;
}

export interface StringGeom {
  /** SVG path: a quadratic curve from the `from` card's edge to the `to` card's edge. */
  d: string;
  /** SVG `points` of the arrowhead, its tip on the `to` card's edge. */
  head: string;
  /** Where the label rides: the curve's midpoint. */
  mid: Point;
  start: Point;
  end: Point;
  control: Point;
}

const centre = (b: Box): Point => ({ x: b.x + b.w / 2, y: b.y + b.h / 2 });

/** Where the ray from `b`'s centre toward `toward` leaves `b` (plus `pad`). */
function exitPoint(b: Box, toward: Point, pad: number): Point {
  const c = centre(b);
  const dx = toward.x - c.x;
  const dy = toward.y - c.y;
  const tx = dx === 0 ? Infinity : (b.w / 2 + pad) / Math.abs(dx);
  const ty = dy === 0 ? Infinity : (b.h / 2 + pad) / Math.abs(dy);
  const t = Math.min(tx, ty, 1);
  return { x: c.x + dx * t, y: c.y + dy * t };
}

/** The sag of the `lane`-th string between one pair of cards, in px. */
export function stringSag(length: number, lane: number): number {
  return Math.min(48, length * 0.12) + lane * 30;
}

const ARROW_LEN = 15;
const ARROW_HALF = 7;
const round = (n: number) => Math.round(n * 10) / 10;

/** A string from card box `a` to card box `b`, the `lane`-th between them; null when the boxes
 * touch (a string needs room to run). */
export function stringGeom(a: Box, b: Box, lane = 0): StringGeom | null {
  if (a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h) return null;
  const ca = centre(a);
  const cb = centre(b);
  const dx = cb.x - ca.x;
  const dy = cb.y - ca.y;
  const len = Math.hypot(dx, dy);
  // The normal that points down the screen: strings sag, they never float.
  let nx = -dy / len;
  let ny = dx / len;
  if (ny < 0 || (ny === 0 && nx < 0)) {
    nx = -nx;
    ny = -ny;
  }
  const sag = stringSag(len, lane);
  const control = { x: (ca.x + cb.x) / 2 + nx * sag * 2, y: (ca.y + cb.y) / 2 + ny * sag * 2 };
  const start = exitPoint(a, control, 4);
  const end = exitPoint(b, control, 3);
  // The head along the tangent at the end (control -> end).
  const tx = end.x - control.x;
  const ty = end.y - control.y;
  const tl = Math.hypot(tx, ty) || 1;
  const ux = tx / tl;
  const uy = ty / tl;
  const base = { x: end.x - ux * ARROW_LEN, y: end.y - uy * ARROW_LEN };
  const head = [end, { x: base.x - uy * ARROW_HALF, y: base.y + ux * ARROW_HALF }, { x: base.x + uy * ARROW_HALF, y: base.y - ux * ARROW_HALF }]
    .map((p) => `${round(p.x)},${round(p.y)}`)
    .join(" ");
  // The curve stops at the head's base, so a dashed string does not run through the arrow.
  const d = `M ${round(start.x)} ${round(start.y)} Q ${round(control.x)} ${round(control.y)} ${round(base.x)} ${round(base.y)}`;
  const mid = { x: 0.25 * start.x + 0.5 * control.x + 0.25 * end.x, y: 0.25 * start.y + 0.5 * control.y + 0.25 * end.y };
  return { d, head, mid, start, end, control };
}

/** The point at `t` (0..1) along a string's curve. */
export function stringAt(g: Pick<StringGeom, "start" | "control" | "end">, t: number): Point {
  const u = 1 - t;
  return { x: u * u * g.start.x + 2 * u * t * g.control.x + t * t * g.end.x, y: u * u * g.start.y + 2 * u * t * g.control.y + t * t * g.end.y };
}

/** Where along the curve a label tries to sit, in order: the middle first. */
const LABEL_T = [0.5, 0.42, 0.58, 0.34, 0.66, 0.26, 0.74, 0.18, 0.82] as const;
/** Where along the curve an offset label hangs from, in order. */
const LEADER_T = [0.5, 0.34, 0.66, 0.2, 0.8] as const;
/** The eight ways an offset label may step off the curve, the ones below it first (strings sag). */
const STEP = [
  [0, 1],
  [0, -1],
  [1, 0],
  [-1, 0],
  [0.71, 0.71],
  [-0.71, 0.71],
  [0.71, -0.71],
  [-0.71, -0.71],
] as const;

function overlapArea(a: Box, b: Box): number {
  const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
  const h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
  return w > 0 && h > 0 ? w * h : 0;
}

/** A label's place: its centre, and the point on the string it belongs to (the same point when
 * it rides the curve; a short leader joins the two when it had to step off). */
export interface LabelSpot extends Point {
  anchor: Point;
}

/**
 * Where a `w` x `h` label goes: on the string's free part, from the middle out, where it covers
 * none of `obstacles` (the cards, the labels already placed) and stays inside `bounds`; else
 * stepped off the curve, nearest first, to the first such spot (with a leader back to the
 * string); else, when nothing is free, wherever it covers least.
 */
export function labelSpot(g: Pick<StringGeom, "start" | "control" | "end">, w: number, h: number, obstacles: readonly Box[], bounds?: Box): LabelSpot {
  const cover = (c: Point) => {
    const box = { x: c.x - w / 2, y: c.y - h / 2, w, h };
    const out = bounds ? w * h - overlapArea(box, bounds) : 0;
    return obstacles.reduce((sum, o) => sum + overlapArea(box, o), out);
  };
  let best: LabelSpot = { ...stringAt(g, 0.5), anchor: stringAt(g, 0.5) };
  let bestCover = Infinity;
  const tryAt = (c: Point, anchor: Point): boolean => {
    const k = cover(c);
    if (k < bestCover - 0.5) {
      best = { x: c.x, y: c.y, anchor };
      bestCover = k;
    }
    return k === 0;
  };
  for (const t of LABEL_T) {
    const p = stringAt(g, t);
    if (tryAt(p, p)) return best;
  }
  const unit = Math.max(h, 24) * 0.6;
  for (let k = 1; k <= 8; k++) {
    for (const t of LEADER_T) {
      const p = stringAt(g, t);
      for (const [dx, dy] of STEP) {
        // The label's edge touches the point, then steps a unit further each round.
        const c = { x: p.x + dx * (w / 2 + (k - 1) * unit), y: p.y + dy * (h / 2 + (k - 1) * unit) };
        if (tryAt(c, p)) return best;
      }
    }
  }
  return best;
}

/** Each string's lane: its index among the strings between the same two cards (either way). */
export function stringLanes(links: readonly { from: string; to: string }[]): number[] {
  const seen = new Map<string, number>();
  return links.map((l) => {
    const pair = l.from < l.to ? `${l.from}|${l.to}` : `${l.to}|${l.from}`;
    const n = seen.get(pair) ?? 0;
    seen.set(pair, n + 1);
    return n;
  });
}

// ----- the timeline strip ------------------------------------------------------------------

export interface TimelineMark {
  /** The hex or record id. */
  id: string;
  kind: "card" | "record";
  date: string;
  /** Across the strip, in px from its left edge. */
  x: number;
  /** The label's row (0 above the axis, 1 below), or null when neither has room: a bare mark. */
  row: 0 | 1 | null;
  label: string;
  grade: EvidenceGrade;
}

export interface BoardTimeline {
  marks: TimelineMark[];
  /** The axis's ends (`YYYY-MM-DD`), or null when nothing on the board is dated. */
  from: string | null;
  to: string | null;
}

const DAY = 86_400_000;

/** Ms since the epoch for a `YYYY-MM-DD` date, or null when it is not one. */
export function dayMs(date: string | undefined): number | null {
  const m = date === undefined ? null : /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!m) return null;
  const t = Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return Number.isNaN(t) ? null : t;
}

/** A label's width on the strip, in px (the strip's 12 px condensed face). */
export const markLabelWidth = (label: string) => label.length * 6.4 + 14;

/** Room at each end of the strip for the axis's first and last date. */
export const STRIP_INSET = 84;

/**
 * The strip's marks: every face-up card with a date (its short label) and every record received
 * with one (its id). The axis spans every date in the case and the records received, face down
 * or not, so marks keep their place as cards flip; a face-down card's date is never drawn.
 * Labels take the first of two rows with room, cards before records, each in date order.
 */
export function timelineMarks(
  hexes: readonly Hex[],
  faceUp: Readonly<Record<string, EvidenceGrade>>,
  records: readonly ShownRecord[],
  width: number,
  shortLabel: (h: Hex) => string,
): BoardTimeline {
  const dated = [...hexes.map((h) => h.date), ...records.map((r) => r.date)].flatMap((d) => {
    const t = dayMs(d);
    return t === null ? [] : [{ t, d: d! }];
  });
  if (dated.length === 0) return { marks: [], from: null, to: null };
  dated.sort((a, b) => a.t - b.t);
  const lo = dated[0]!;
  const hi = dated[dated.length - 1]!;
  const t0 = lo.t - 20 * DAY;
  const span = hi.t + 20 * DAY - t0;
  const xOf = (t: number) => STRIP_INSET + ((t - t0) / span) * (width - 2 * STRIP_INSET);
  const cards = hexes.flatMap((h) => {
    const t = dayMs(h.date);
    const g = faceUp[h.id];
    return t === null || g === undefined ? [] : [{ id: h.id, kind: "card" as const, date: h.date!, t, label: shortLabel(h), grade: g }];
  });
  const recs = records.flatMap((r) => {
    const t = dayMs(r.date);
    return t === null ? [] : [{ id: r.id, kind: "record" as const, date: r.date!, t, label: r.id, grade: r.grade }];
  });
  const rows: [number, number][][] = [[], []];
  const marks: TimelineMark[] = [];
  for (const group of [cards, recs]) {
    for (const m of [...group].sort((a, b) => a.t - b.t || a.id.localeCompare(b.id))) {
      const x = xOf(m.t);
      const w = markLabelWidth(m.label);
      const span: [number, number] = [x - w / 2, x + w / 2];
      const row = ([0, 1] as const).find((r) => rows[r]!.every(([a, b]) => span[1] < a || span[0] > b)) ?? null;
      if (row !== null) rows[row]!.push(span);
      marks.push({ id: m.id, kind: m.kind, date: m.date, x, row, label: m.label, grade: m.grade });
    }
  }
  marks.sort((a, b) => a.x - b.x || a.id.localeCompare(b.id));
  return { marks, from: lo.d, to: hi.d };
}
