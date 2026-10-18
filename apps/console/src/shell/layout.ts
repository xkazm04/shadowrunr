// Where each poster sits for a given view: the winner's layout() as a pure function, so it is
// tested without a DOM and recomputed only when its inputs change (view, hover, rail, size).
//
// Desktop: the wall is an accordion (hover widens one poster to 60%); layer 1 packs the other
// posters into a rail of strips on the left; layer 2 compresses the active poster into the spine.
// Narrow: the wall stacks, layer 1 docks the others as tabs at the bottom, layer 2 keeps a bar.

export type View = "wall" | "l1" | "l2";

export interface Rect {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
}

export interface LayoutInput {
  readonly view: View;
  readonly count: number;
  /** Index of the active page (layers 1 and 2). */
  readonly active: number;
  /** Hovered poster on the wall. */
  readonly hover: number | null;
  /** Rail strip opened by hover on layer 1. */
  readonly rail: number | null;
  readonly width: number;
  readonly height: number;
  /** px per rem (the root size is fluid). */
  readonly rem: number;
  readonly narrow: boolean;
  /** Space the mixer bus takes at the bottom on layers 1 and 2, in rem. */
  readonly busRem: number;
}

export interface LayoutMetrics {
  readonly stripRem: number;
  readonly spineRem: number;
  readonly railPeekRem: number;
  /** Narrow screens: the chrome bar, the docked tabs and the layer-2 bar, in rem. */
  readonly topBarRem: number;
  readonly tabRem: number;
  readonly barRem: number;
}

export interface Layout {
  readonly rects: readonly Rect[];
  /** Width of the layer-1 body inside the active poster. */
  readonly bodyWidth: number;
  /** Height left for posters above the mixer bus. */
  readonly stageHeight: number;
}

export function layoutPosters(i: LayoutInput, m: LayoutMetrics): Layout {
  const { width: W, rem: r, count: n } = i;
  const bus = i.view === "wall" ? 0 : i.busRem * r;
  const H = Math.max(0, i.height - bus);
  return i.narrow ? narrow(i, m, W, H, r, n) : desktop(i, m, W, H, r, n);
}

function desktop(i: LayoutInput, m: LayoutMetrics, W: number, H: number, r: number, n: number): Layout {
  const strip = m.stripRem * r;
  const rail = (n - 1) * strip;
  const rects: Rect[] = [];
  if (i.view === "wall") {
    const big = 0.6 * W;
    const small = n > 1 ? (W - big) / (n - 1) : W;
    let x = 0;
    for (let k = 0; k < n; k++) {
      const w = i.hover === null ? W / n : k === i.hover ? big : small;
      rects.push({ left: x, top: 0, width: w, height: H });
      x += w;
    }
  } else if (i.view === "l1") {
    const peek = m.railPeekRem * r;
    let slot = 0;
    for (let k = 0; k < n; k++) {
      if (k === i.active) {
        rects.push({ left: rail, top: 0, width: W - rail, height: H });
        continue;
      }
      const extra = i.rail === k ? peek : 0;
      const shift = i.rail !== null && k > i.rail ? peek : 0;
      rects.push({ left: slot * strip + shift, top: 0, width: strip + extra, height: H });
      slot++;
    }
  } else {
    const spine = m.spineRem * r;
    for (let k = 0; k < n; k++) {
      rects.push(k === i.active ? { left: 0, top: 0, width: spine, height: H } : { left: spine, top: 0, width: 0, height: H });
    }
  }
  return { rects, bodyWidth: W - rail, stageHeight: H };
}

function narrow(i: LayoutInput, m: LayoutMetrics, W: number, H: number, r: number, n: number): Layout {
  const tb = m.topBarRem * r;
  const area = H - tb;
  const tab = m.tabRem * r;
  const rects: Rect[] = [];
  if (i.view === "wall") {
    for (let k = 0; k < n; k++) rects.push({ left: 0, top: tb + (k * area) / n, width: W, height: area / n });
  } else if (i.view === "l1") {
    let slot = 0;
    const tabW = n > 1 ? W / (n - 1) : W;
    for (let k = 0; k < n; k++) {
      if (k === i.active) {
        rects.push({ left: 0, top: tb, width: W, height: H - tb - tab });
      } else {
        rects.push({ left: slot * tabW, top: H - tab, width: tabW, height: tab });
        slot++;
      }
    }
  } else {
    const bar = m.barRem * r;
    for (let k = 0; k < n; k++) rects.push({ left: 0, top: tb, width: W, height: k === i.active ? bar : 0 });
  }
  return { rects, bodyWidth: W, stageHeight: H };
}

/** The winner's poster height in rem (its root was 2vh). */
export const POSTER_REM = 50;
/** The root size bounds in px (app/global.css). */
export const ROOT_PX = { min: 14, max: 30.4, narrow: 16 } as const;

/**
 * The root size app/global.css computes: `min(1.25vw, 100vh / (50 + bus))`, clamped, so the
 * height above the full mixer bus is exactly the winner's 50rem.
 */
export function rootPx(width: number, height: number, narrow: boolean, busRem: number): number {
  if (narrow) return ROOT_PX.narrow;
  const fluid = Math.min(width * 0.0125, height / (POSTER_REM + busRem));
  return Math.min(ROOT_PX.max, Math.max(ROOT_PX.min, fluid));
}
