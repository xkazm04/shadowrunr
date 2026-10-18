// Draws the simulator scene on a 2D context: A/1's backdrop and its scaled grid
// (app.js:420 and 455-466) at the reduced-motion still, since rendering here is event-driven:
// a draw happens when the data, the size or the hovered tile changes, never per tick.
import { LABEL_BAND, runGrid, tileOrigin, type GridLayout, type Rect, type Tile } from "./grid";
import { SIM_FONT, SIM_INK, healthColor } from "./palette";

/** The slice of CanvasRenderingContext2D the scene uses (tests pass a recording fake). */
export type SceneContext = Pick<
  CanvasRenderingContext2D,
  | "fillStyle"
  | "font"
  | "clearRect"
  | "fillRect"
  | "beginPath"
  | "arc"
  | "fill"
  | "fillText"
  | "save"
  | "restore"
  | "translate"
  | "scale"
  | "rect"
  | "clip"
  | "createRadialGradient"
>;

/** A/1's backdrop: the dark ground and the verdigris glow, stretched to the scene (preserveAspectRatio none). */
export function drawBackdrop(c: SceneContext, w: number, h: number): void {
  c.clearRect(0, 0, w, h);
  c.fillStyle = SIM_INK.backdrop;
  c.fillRect(0, 0, w, h);
  c.save();
  c.translate(w / 2, h / 2);
  c.scale(w, h);
  const g = c.createRadialGradient(0, 0, 0, 0, 0, 0.6);
  g.addColorStop(0, SIM_INK.glow);
  g.addColorStop(1, SIM_INK.glowEnd);
  c.fillStyle = g;
  c.fillRect(-0.5, -0.5, 1, 1);
  c.restore();
}

const pct = (h: number | undefined) => (h === undefined ? "–" : `${Math.round(h * 100)}%`);

/** The tile label: A/1 prints the id upper-cased with its health. */
export function tileLabel(t: Tile): string {
  return `${t.id.replace("scaled-", "").toUpperCase()} · ${pct(t.health)}`;
}

/** Draws every tile and run cell inside `area`; `hover` is the hovered tile index or -1. */
export function drawGrid(c: SceneContext, tiles: readonly Tile[], l: GridLayout, area: Rect, hover: number): void {
  const colours = new Map<number, string>();
  const colourOf = (h: number) => {
    let s = colours.get(h);
    if (s === undefined) {
      // A/1 twinkle at rest (reduced motion): alpha 0.35 + 0.6 * h.
      s = healthColor(h, 0.35 + 0.6 * h);
      colours.set(h, s);
    }
    return s;
  };
  const font = `600 ${Math.max(11, Math.min(14, l.tw / 9))}px ${SIM_FONT}`;
  tiles.forEach((tile, i) => {
    const o = tileOrigin(l, i);
    const ox = area.x + o.ox;
    const oy = area.y + o.oy;
    const g = runGrid(tile.runs.length, l.tw, l.th);
    const cw = l.tw / g.cols;
    const ch = (l.th - LABEL_BAND) / g.rows;
    c.fillStyle = i === hover ? SIM_INK.tileHover : SIM_INK.tile;
    c.fillRect(ox, oy, l.tw, l.th);
    tile.runs.forEach((r, k) => {
      const row = Math.floor(k / g.cols);
      const col = k % g.cols;
      const x = ox + col * cw;
      const y = oy + LABEL_BAND + row * ch;
      c.fillStyle = r.health === undefined ? SIM_INK.unjudged : colourOf(r.health);
      c.fillRect(x + 0.5, y + 0.5, cw - 1, ch - 1);
      if (r.red) {
        c.fillStyle = SIM_INK.redLine;
        c.beginPath();
        c.arc(x + cw / 2, y + ch / 2, Math.min(cw, ch) * 0.3, 0, 7);
        c.fill();
      }
    });
    c.save();
    c.beginPath();
    c.rect(ox, oy, l.tw, LABEL_BAND);
    c.clip();
    c.fillStyle = i === hover ? SIM_INK.labelHover : SIM_INK.label;
    c.font = font;
    c.fillText(tileLabel(tile), ox + 2, oy + 12);
    c.restore();
  });
}
