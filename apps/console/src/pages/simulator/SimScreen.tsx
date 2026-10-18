// A/1's scaled simulator: its scene (app.js:420-475) letterboxed at A/1's 1600:810 aspect, with
// A/1's backdrop and the grid on a canvas that covers the grid area only, as A/1's does. One tile
// per tournament, one cell per run, hover hit-testing and a marquee; a click opens the tournament
// (push-in from the grid). The canvas redraws only when its tiles, its size or the hovered tile
// change: no animation loop. The letterbox bands carry the winner's Simulator face motif (BandArt).
import { TOKENS_SIM } from "@shadowrunr/tokens";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { LetterboxBands, bandOf, useBoxSize } from "../../kit/LetterboxBands";
import { pageRoute, useRouter } from "../../router";
import { remPx } from "../../ui/remPx";
import { SimBandBottom, SimBandTop } from "./BandArt";
import bandStyles from "./BandArt.module.css";
import { drawGrid, tileLabel } from "./draw";
import { gridLayout, gridRect, hitTest, type Tile } from "./grid";
import styles from "./SimScreen.module.css";

/** Under this many rem of hero width the screen stacks (grid on top, HUD below) and fills the hero. */
export const NARROW_REM = 40;

const heldText = (h: number | undefined) => (h === undefined ? "not judged yet" : `${Math.round(h * 100)}% held`);

/** A/1's simulator backdrop (`smG`): the dark ground and the verdigris glow, stretched to the scene. */
function Backdrop() {
  return (
    <svg className={styles.backdrop} viewBox="0 0 1600 810" preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <radialGradient id="smG" cx="50%" cy="50%" r="60%">
          <stop offset="0" stopColor={TOKENS_SIM.glow} />
          <stop offset="1" stopColor={TOKENS_SIM.glowEnd} />
        </radialGradient>
      </defs>
      <rect width="1600" height="810" fill={TOKENS_SIM.backdrop} />
      <rect width="1600" height="810" fill="url(#smG)" />
    </svg>
  );
}

export function SimScreen(props: { tiles: readonly Tile[]; children?: ReactNode }) {
  const { tiles } = props;
  const router = useRouter();
  const frame = useRef<HTMLDivElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const hero = useBoxSize(frame);
  const size = useBoxSize(wrap);
  const [hover, setHover] = useState(-1);
  const narrow = hero.w > 0 && hero.w < NARROW_REM * remPx();
  // The grid's box in the scene (A/1's pos 2, 13, 74, 84); the canvas is exactly this box.
  const area = gridRect(size.w, size.h, narrow);
  const layout = gridLayout(area.w, area.h);
  const hovered = hover >= 0 ? tiles[hover] : undefined;
  // Stacked, the screen fills the hero: no band.
  const band = narrow ? 0 : bandOf(hero, size);

  // One draw per change of tiles, size or hover (A/1 drew every frame; this draws on input).
  useEffect(() => {
    const cv = canvas.current;
    if (!cv || area.w === 0 || area.h === 0) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const W = Math.round(area.w * dpr);
    const H = Math.round(area.h * dpr);
    if (cv.width !== W || cv.height !== H) {
      cv.width = W;
      cv.height = H;
    }
    const c = cv.getContext("2d");
    if (!c) return;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    c.clearRect(0, 0, area.w, area.h);
    drawGrid(c, tiles, gridLayout(area.w, area.h), { x: 0, y: 0, w: area.w, h: area.h }, hover);
    // `area` is derived from `size` and `narrow`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tiles, size, narrow, hover]);

  const at = (clientX: number, clientY: number) => {
    const el = canvas.current;
    if (!el) return -1;
    const r = el.getBoundingClientRect();
    return hitTest(layout, clientX - r.left, clientY - r.top, tiles.length);
  };

  return (
    <div ref={frame} className={styles.frame} data-stack={narrow}>
      <LetterboxBands band={band} top={<SimBandTop />} bottom={<SimBandBottom />} role="simulator-band" className={bandStyles.band} />
      <div ref={wrap} className={styles.screen} data-role="simulator-screen" data-hot={hover >= 0}>
        <Backdrop />
        <canvas
          ref={canvas}
          className={styles.canvas}
          style={{ left: area.x, top: area.y, width: area.w, height: area.h }}
          data-role="simulator-grid"
          role="img"
          aria-label={`Tournament grid: ${tiles.length} tournaments, one tile each, one cell per run, coloured by the share of rule checks held. A red dot marks a red-line fail.`}
          onMouseMove={(e) => {
            const i = at(e.clientX, e.clientY);
            if (i !== hover) setHover(i);
          }}
          onMouseLeave={() => setHover(-1)}
          onClick={(e) => {
            const i = at(e.clientX, e.clientY);
            const t = i >= 0 ? tiles[i] : undefined;
            if (!t) return;
            const r = e.currentTarget.getBoundingClientRect();
            router.navigate(pageRoute("simulator", "tournaments", t.id), {
              origin: { left: r.left, top: r.top, width: r.width, height: r.height },
            });
          }}
        />
        <div className={styles.marquee} data-role="simulator-marquee" data-on={hovered !== undefined} aria-hidden="true">
          {hovered && (
            <>
              <b>{tileLabel(hovered)}</b>
              <span>
                {heldText(hovered.health)} · {hovered.red} red-line fails · {hovered.runs.length.toLocaleString("en-US")} runs
              </span>
            </>
          )}
        </div>
        {props.children}
      </div>
    </div>
  );
}
