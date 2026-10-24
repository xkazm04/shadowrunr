// Named scene layouts. Every on-canvas element lives in a named region; a layout is a rect per
// region in 1920x1080 canvas pixels. Switching layouts moves the same React elements to new
// rects with a CSS transition, so a handover or a screen switch visibly re-arranges the stage
// instead of cutting.
//
// Two families: `host` (and `boot`, the same rects hidden) is the big-avatar room; the content
// screens (`board`, `hexmap`, `browser`, `desk`) are the streamer layout: the content fills
// everything left of the right column, Malakor stands cut out over its bottom-left corner, the
// caption is a lower third from his right edge to the content's, and decrees and cards float in
// the content's top-right (on the board, in a lane the acts leave free).
//
// SEG-03's Table Read draws the host room as a two-shot (`flags.twoShot`): Malakor on the left, the
// guest's portrait on the right where the card and decrees stood, the scene's title band under the
// two of them, and the explainer card moved into the right column above the chat.

import { isContentScreen, type BoardFrame, type HexFrame, type LayoutId, type MapScreen } from "@shadowrunr/stage-kit";

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
  "board",
  "browser",
  "inspector",
  "tally",
  "ticker",
  "guest",
  "scene",
] as const;
export type Region = (typeof REGIONS)[number];

export interface LayoutFlags {
  poll: boolean;
  foil: boolean;
  /** The map the desk shows behind the browser window (absent: the board). */
  back?: MapScreen;
  /** SEG-03 on the host screen: the two-shot (ignored on the content screens). */
  twoShot?: boolean;
  /** The two-shot's scene title band is up (a scene is playing). */
  scene?: boolean;
  /** The two-shot's card slot has something to show (an explainer, or the standing card). */
  card?: boolean;
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
/** The two-shot: his box, a little shorter so the scene band fits under both halves. */
export const TWO_SHOT_HOST: Rect = { x: 64, y: TOP + 8, w: FACE_PX, h: 552 };
/** The guest's half: where the host room's card and decrees stand, as tall as his. */
export const TWO_SHOT_GUEST: Rect = { x: 652, y: TOP + 8, w: 632, h: 552 };
/** The scene's title band under the two halves, above the caption. */
export const SCENE_BAND: Rect = { x: 64, y: TWO_SHOT_HOST.y + TWO_SHOT_HOST.h + 12, w: 1220, h: 68 };
/** His corner on the content screens: bottom-left, the box's bottom on the canvas's bottom edge
 * (the rig's torso is cut off at its box's bottom, like a streamer's cut-out). */
export const CORNER_AVATAR: Rect = { x: 16, y: 1080 - 448, w: 448, h: 448 };
/** The content area: everything left of the right column, down to the lower third. The board,
 * the hexmap and the browser fill it exactly, so the clip factory's 9:16 reframe crops each the
 * same way (`CONTENT_SCREEN` in crates/shadowrunr-clips footage.rs). */
export const CONTENT: Rect = { x: GUTTER, y: TOP, w: RIGHT_X - GUTTER - 24, h: 804 };
const CONTENT_RIGHT = CONTENT.x + CONTENT.w;
/** The lower third: from his right edge to the content's right edge. */
const LOWER_THIRD: Rect = { x: CORNER_AVATAR.x + CORNER_AVATAR.w + 8, y: 924, w: 0, h: 132 };
LOWER_THIRD.w = CONTENT_RIGHT - LOWER_THIRD.x;
/** On the content screens the ticker closes the right column, never under him. */
const COLUMN_TICKER: Rect = { x: RIGHT_X, y: TICKER.y, w: RIGHT_W, h: TICKER.h };
/** The overlay lane in the content's top-right: the foil badge, decrees, explainer cards. On the
 * board the acts end left of it, so no overlay ever stands over an act. */
const LANE_W = 440;
const LANE_X = CONTENT_RIGHT - 24 - LANE_W;
const LANE_GAP = 16;
/** The desk: the map window behind (top-left, smaller, ending left of the overlay lane), the
 * browser window in front. */
const DESK_MAP: Rect = { x: GUTTER, y: TOP, w: LANE_X - LANE_GAP - GUTTER, h: 540 };
const DESK_BROWSER: Rect = { x: 520, y: 236, w: CONTENT_RIGHT - 520, h: CONTENT.y + CONTENT.h - 236 };
/** The browser window's chrome (tab strip, URL bar and the FICTIONAL ribbon): no overlay covers it. */
export const BROWSER_CHROME = 130;

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

/**
 * The overlay lane top-down (the foil badge, decrees, the explainer card), stepping below the
 * browser window's chrome where a box would cover its tabs or URL bar. The card takes what is left
 * above the lower third, at most its full height.
 */
function lane(browser: Rect | null): { observer: Rect; decrees: Rect; card: Rect } {
  const band = browser ? { top: browser.y, bottom: browser.y + BROWSER_CHROME + LANE_GAP } : null;
  let y = CONTENT.y + 20;
  const place = (h: number, gap: number): Rect => {
    if (band && y < band.bottom && y + h > band.top) y = band.bottom;
    const r = { x: LANE_X, y, w: LANE_W, h: Math.min(h, CONTENT.y + CONTENT.h - y) };
    y += r.h + gap;
    return r;
  };
  const observer = place(96, 12);
  const decrees = place(262, LANE_GAP);
  return { observer, decrees, card: place(300, 0) };
}

function contentRects(layout: "board" | "hexmap" | "browser" | "desk", flags: LayoutFlags, topbar: Rect): Record<Region, Rect> {
  const browser = layout === "desk" ? DESK_BROWSER : layout === "browser" ? CONTENT : null;
  const { observer, decrees, card } = lane(browser);
  // Inspector and tally belong to the map; the browser screen's column is chat (and the vote).
  const panels = layout !== "browser";
  const back = layout === "desk" ? (flags.back ?? "board") : null;
  return {
    topbar,
    board: layout === "board" ? CONTENT : back === "board" ? DESK_MAP : hide(CONTENT),
    hexmap: layout === "hexmap" ? CONTENT : back === "hexmap" ? DESK_MAP : hide(CONTENT),
    browser: browser ?? hide(DESK_BROWSER),
    guest: hide(TWO_SHOT_GUEST),
    scene: hide(SCENE_BAND),
    observer: flags.foil ? observer : hide(observer),
    decrees,
    card,
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

/** SEG-03's host room: the two-shot. The foil card is the guest's half, so it never shows. */
function twoShotRects(r: Record<Region, Rect>, flags: LayoutFlags): Record<Region, Rect> {
  return {
    ...r,
    avatar: TWO_SHOT_HOST,
    guest: TWO_SHOT_GUEST,
    scene: flags.scene ? SCENE_BAND : hide(SCENE_BAND),
    decrees: hide(r.decrees),
    foil: hide(r.foil),
    ...(column(
      [
        { region: "vote", h: 300, show: flags.poll },
        { region: "card", h: 240, show: flags.card ?? false },
        { region: "chat", h: 0, show: true },
      ],
      TOP,
    ) as Record<"vote" | "card" | "chat", Rect>),
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
    board: hide({ x: 360, y: 200, w: 900, h: 600 }),
    browser: hide(DESK_BROWSER),
    guest: hide(TWO_SHOT_GUEST),
    scene: hide(SCENE_BAND),
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
    return r;
  }
  return flags.twoShot ? twoShotRects(r, flags) : r;
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
  if ((layout !== "hexmap" && layout !== "desk") || rects.hexmap.hidden) return null;
  const map = rects.hexmap;
  const avoid = [local(rects.avatar, map)];
  if (layout === "desk") avoid.push(local(rects.browser, map));
  const frame: HexFrame = { w: map.w, h: map.h, avoid };
  if (layout === "hexmap") frame.prefer = [local({ x: LANE_X, y: CONTENT.y, w: LANE_W, h: rects.card.y + rects.card.h - CONTENT.y }, map)];
  return frame;
}

/** The overlay lane over a content window, in its coordinates. */
function laneIn(rects: Record<Region, Rect>, win: Rect) {
  return local({ x: LANE_X, y: CONTENT.y, w: LANE_W, h: rects.card.y + rects.card.h - CONTENT.y }, win);
}

/**
 * What stands over the board's window on a content screen: Malakor's corner and, on the desk, the
 * browser window in front (no card is drawn under these), and the overlay lane where it crosses
 * the window (the board screen; the desk's map window ends left of it). The lane's place is kept
 * whether a badge, decree or card is up or not, so no card moves when one arrives: the acts are
 * laid out left of it, the overview and every zoom stay there, and the thesis band ends where it
 * starts. Null where the board is not shown.
 */
export function boardFrame(layout: LayoutId, rects: Record<Region, Rect>): BoardFrame | null {
  if ((layout !== "board" && layout !== "desk") || rects.board.hidden) return null;
  const win = rects.board;
  const avoid = [local(rects.avatar, win)];
  if (layout === "desk") avoid.push(local(rects.browser, win));
  const frame: BoardFrame = { w: win.w, h: win.h, avoid };
  const lane = laneIn(rects, win);
  if (lane.x < win.w && lane.x + lane.w > 0 && lane.y < win.h && lane.y + lane.h > 0) frame.lane = lane;
  return frame;
}

/** The frame every board window places its cards in: the board screen's (stable positions on the
 * board, behind the desk's browser and in a clip). */
export const BOARD_LAYOUT_FRAME: BoardFrame = boardFrame("board", layoutRects("board", { poll: false, foil: false }))!;
