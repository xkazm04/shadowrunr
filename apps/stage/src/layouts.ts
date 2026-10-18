// Named scene layouts. Every on-canvas element lives in a named region; a layout is a rect per
// region in 1920x1080 canvas pixels. Switching layouts moves the same React elements to new
// rects with a CSS transition, so a handover visibly re-arranges the stage instead of cutting.

import type { LayoutId } from "./scene";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
  /** Hidden regions keep a rect (to animate from) but fade out. */
  hidden?: boolean;
}

export const REGIONS = [
  "topbar",
  "avatar",
  "caption",
  "card",
  "decrees",
  "foil",
  "observer",
  "vote",
  "chat",
  "hexmap",
  "inspector",
  "tally",
  "ticker",
] as const;
export type Region = (typeof REGIONS)[number];

export interface LayoutFlags {
  poll: boolean;
  foil: boolean;
}

export const GUTTER = 32;
const TOP = 104;
const TICKER: Rect = { x: GUTTER, y: 1004, w: 1920 - 2 * GUTTER, h: 52 };
const CONTENT_BOTTOM = TICKER.y - 20;
const RIGHT_X = 1312;
const RIGHT_W = 1920 - GUTTER - RIGHT_X;
const STACK_GAP = 16;
/** Width of the overlay lane on the right of the hexmap panel (see styles.css .hex-svg). */
export const HEX_SIDE_W = 560;

function hide(r: Rect): Rect {
  return { ...r, hidden: true };
}

/** Stack panels in the right column top-down; the last one takes what is left. */
function column(
  entries: { region: Region; h: number; show: boolean }[],
  top: number,
): Partial<Record<Region, Rect>> {
  const out: Partial<Record<Region, Rect>> = {};
  let y = top;
  entries.forEach((e, i) => {
    const last = i === entries.length - 1;
    const h = last ? CONTENT_BOTTOM - y : e.h;
    if (!e.show) {
      out[e.region] = hide({ x: RIGHT_X, y, w: RIGHT_W, h: Math.max(e.h, 1) });
      return;
    }
    out[e.region] = { x: RIGHT_X, y, w: RIGHT_W, h };
    y += h + STACK_GAP;
  });
  return out;
}

export function layoutRects(layout: LayoutId, flags: LayoutFlags): Record<Region, Rect> {
  const topbar: Rect = { x: GUTTER, y: 24, w: 1920 - 2 * GUTTER, h: 60 };
  switch (layout) {
    case "hexmap": {
      const map: Rect = { x: GUTTER, y: TOP, w: RIGHT_X - GUTTER - 24, h: 744 };
      // The hexes fill the left part of the map panel; the right part (SIDE_W) is a lane for
      // the observer badge, decree cards and explainer cards, so nothing covers a hex.
      const sideX = map.x + map.w - HEX_SIDE_W;
      const sideW = HEX_SIDE_W - 24;
      const observer: Rect = { x: sideX, y: map.y + 20, w: sideW, h: 96 };
      return {
        topbar,
        hexmap: map,
        observer: flags.foil ? observer : hide(observer),
        decrees: { x: sideX, y: map.y + 128, w: sideW, h: 262 },
        card: { x: sideX, y: map.y + map.h - 52 - 300, w: sideW, h: 300 },
        avatar: { x: GUTTER + 8, y: 864, w: 124, h: 124 },
        caption: { x: GUTTER + 148, y: 864, w: map.w - 148, h: 124 },
        foil: hide(observer),
        ticker: TICKER,
        ...(column(
          [
            { region: "inspector", h: 206, show: true },
            { region: "tally", h: 206, show: true },
            { region: "vote", h: 262, show: flags.poll },
            { region: "chat", h: 0, show: true },
          ],
          TOP,
        ) as Record<"inspector" | "tally" | "vote" | "chat", Rect>),
      };
    }
    case "improv":
    case "descoped":
    case "boot": {
      const off = layout === "boot";
      const r: Record<Region, Rect> = {
        topbar,
        avatar: { x: 64, y: TOP + 8, w: 560, h: 612 },
        card: { x: 652, y: TOP + 16, w: 632, h: 404 },
        decrees: { x: 652, y: TOP + 440, w: 632, h: 248 },
        caption: { x: 64, y: 756, w: 1220, h: 214 },
        hexmap: hide({ x: 360, y: 200, w: 900, h: 600 }),
        inspector: hide({ x: RIGHT_X, y: TOP, w: RIGHT_W, h: 214 }),
        tally: hide({ x: RIGHT_X, y: TOP + 230, w: RIGHT_W, h: 214 }),
        observer: hide({ x: RIGHT_X, y: TOP, w: 340, h: 96 }),
        ticker: TICKER,
        ...(column(
          [
            { region: "foil", h: 300, show: flags.foil },
            { region: "vote", h: 300, show: flags.poll },
            { region: "chat", h: 0, show: true },
          ],
          TOP,
        ) as Record<"foil" | "vote" | "chat", Rect>),
      };
      if (off) {
        for (const k of REGIONS) if (k !== "topbar") r[k] = hide(r[k]);
      }
      return r;
    }
  }
}
