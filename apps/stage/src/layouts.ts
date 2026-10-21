// Named scene layouts. Every on-canvas element lives in a named region; a layout is a rect per
// region in 1920x1080 canvas pixels. Switching layouts moves the same React elements to new
// rects with a CSS transition, so a handover or a screen switch visibly re-arranges the stage
// instead of cutting.
//
// Two families: `host` (and `boot`, the same rects hidden) is the big-avatar room; the content
// screens (`hexmap`, `browser`, `desk`) are the streamer layout: the content fills everything left
// of the right column, Malakor stands cut out over its bottom-left corner, the caption is a lower
// third from his right edge to the content's, and decrees and cards float in the content's
// top-right.

import { isContentScreen, type HexFrame, type LayoutId } from "@shadowrunr/stage-kit";

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
  "browser",
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

/** The square Malakor's face is drawn in, at its full (host) size; smaller boxes scale it down. */
export const FACE_PX = 560;
/** The host room's avatar box. */
const HOST_AVATAR: Rect = { x: 64, y: TOP + 8, w: FACE_PX, h: 612 };
/** His corner on the content screens: bottom-left, the box's bottom on the canvas's bottom edge
 * (the rig's torso is cut off at its box's bottom, like a streamer's cut-out). */
export const CORNER_AVATAR: Rect = { x: 16, y: 1080 - 448, w: 448, h: 448 };
/** The content area: everything left of the right column, down to the lower third. */
const CONTENT: Rect = { x: GUTTER, y: TOP, w: RIGHT_X - GUTTER - 24, h: 804 };
const CONTENT_RIGHT = CONTENT.x + CONTENT.w;
/** The lower third: from his right edge to the content's right edge. */
const LOWER_THIRD: Rect = { x: CORNER_AVATAR.x + CORNER_AVATAR.w + 8, y: 924, w: 0, h: 132 };
LOWER_THIRD.w = CONTENT_RIGHT - LOWER_THIRD.x;
/** On the content screens the ticker closes the right column, never under him. */
const COLUMN_TICKER: Rect = { x: RIGHT_X, y: TICKER.y, w: RIGHT_W, h: TICKER.h };
/** The overlay lane in the content's top-right: the foil badge, decrees, explainer cards. */
const LANE_W = 440;
const LANE_X = CONTENT_RIGHT - 24 - LANE_W;
/** The desk: the hexmap window behind (top-left, smaller), the browser window in front. */
const DESK_MAP: Rect = { x: GUTTER, y: TOP, w: 800, h: 540 };
const DESK_BROWSER: Rect = { x: 520, y: 236, w: CONTENT_RIGHT - 520, h: CONTENT.y + CONTENT.h - 236 };

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

function contentRects(layout: "hexmap" | "browser" | "desk", flags: LayoutFlags, topbar: Rect): Record<Region, Rect> {
  const observer: Rect = { x: LANE_X, y: CONTENT.y + 20, w: LANE_W, h: 96 };
  // Inspector and tally belong to the map; the browser screen's column is chat (and the vote).
  const panels = layout !== "browser";
  return {
    topbar,
    hexmap: layout === "desk" ? DESK_MAP : layout === "hexmap" ? CONTENT : hide(CONTENT),
    browser: layout === "desk" ? DESK_BROWSER : layout === "browser" ? CONTENT : hide(DESK_BROWSER),
    observer: flags.foil ? observer : hide(observer),
    decrees: { x: LANE_X, y: CONTENT.y + 128, w: LANE_W, h: 262 },
    card: { x: LANE_X, y: CONTENT.y + 406, w: LANE_W, h: 300 },
    avatar: CORNER_AVATAR,
    caption: LOWER_THIRD,
    foil: hide(observer),
    ticker: COLUMN_TICKER,
    ...(column(
      [
        { region: "inspector", h: 206, show: panels },
        { region: "tally", h: 206, show: panels },
        { region: "vote", h: 262, show: flags.poll },
        { region: "chat", h: 0, show: true },
      ],
      TOP,
    ) as Record<"inspector" | "tally" | "vote" | "chat", Rect>),
  };
}

export function layoutRects(layout: LayoutId, flags: LayoutFlags): Record<Region, Rect> {
  const topbar: Rect = { x: GUTTER, y: 24, w: 1920 - 2 * GUTTER, h: 60 };
  if (isContentScreen(layout)) return contentRects(layout, flags, topbar);
  const off = layout === "boot";
  const r: Record<Region, Rect> = {
    topbar,
    avatar: HOST_AVATAR,
    card: { x: 652, y: TOP + 16, w: 632, h: 404 },
    decrees: { x: 652, y: TOP + 440, w: 632, h: 248 },
    caption: { x: 64, y: 756, w: 1220, h: 214 },
    hexmap: hide({ x: 360, y: 200, w: 900, h: 600 }),
    browser: hide(DESK_BROWSER),
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

/** `r` in the coordinates of the box `inside` (its top-left at 0, 0). */
function local(r: Rect, inside: Rect): { x: number; y: number; w: number; h: number } {
  return { x: r.x - inside.x, y: r.y - inside.y, w: r.w, h: r.h };
}

/**
 * How the hexmap panel fits its hexes on a content screen: around Malakor's corner and, on the
 * desk, the browser window in front (no hex goes under either); on the hexmap screen it also keeps
 * the overlay lane clear when that is cheap. Null where the map is not shown.
 */
export function hexFrame(layout: LayoutId, rects: Record<Region, Rect>): HexFrame | null {
  if (layout !== "hexmap" && layout !== "desk") return null;
  const map = rects.hexmap;
  const avoid = [local(rects.avatar, map)];
  if (layout === "desk") avoid.push(local(rects.browser, map));
  const frame: HexFrame = { w: map.w, h: map.h, avoid };
  if (layout === "hexmap") frame.prefer = [local({ x: LANE_X, y: CONTENT.y, w: LANE_W, h: rects.card.y + rects.card.h - CONTENT.y }, map)];
  return frame;
}
