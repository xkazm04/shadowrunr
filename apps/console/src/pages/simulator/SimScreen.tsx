// A/1's scaled simulator on canvas: one tile per tournament, one cell per run, hover
// hit-testing and a marquee, a click opens the tournament (push-in from the grid). The canvas
// redraws only when its tiles, its size or the hovered tile change: no animation loop.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { pageRoute, useRouter } from "../../router";
import { remPx } from "../../ui/remPx";
import { drawBackdrop, drawGrid, tileLabel } from "./draw";
import { gridLayout, gridRect, hitTest, type Tile } from "./grid";
import styles from "./SimScreen.module.css";

/** Under this many rem the screen stacks: grid on top, HUD below. */
export const NARROW_REM = 40;

interface Size {
  readonly w: number;
  readonly h: number;
}

/** The element's size, re-read on resize only (ResizeObserver; jsdom has none and keeps 0). */
function useSize(ref: RefObject<HTMLElement | null>): Size {
  const [size, setSize] = useState<Size>({ w: 0, h: 0 });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const read = () => {
      const r = el.getBoundingClientRect();
      setSize((s) => (s.w === r.width && s.h === r.height ? s : { w: r.width, h: r.height }));
    };
    read();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(read);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return size;
}

const pct = (h: number | undefined) => (h === undefined ? "not judged yet" : `${Math.round(h * 100)}% held`);

export function SimScreen(props: { tiles: readonly Tile[]; children?: ReactNode }) {
  const { tiles } = props;
  const router = useRouter();
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const size = useSize(wrap);
  const [hover, setHover] = useState(-1);
  const narrow = size.w > 0 && size.w < NARROW_REM * remPx();
  const area = gridRect(size.w, size.h, narrow);
  const layout = gridLayout(area.w, area.h);
  const hovered = hover >= 0 ? tiles[hover] : undefined;

  // One draw per change of tiles, size or hover (A/1 drew every frame; this draws on input).
  useEffect(() => {
    const cv = canvas.current;
    if (!cv || size.w === 0 || size.h === 0) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const W = Math.round(size.w * dpr);
    const H = Math.round(size.h * dpr);
    if (cv.width !== W || cv.height !== H) {
      cv.width = W;
      cv.height = H;
    }
    const c = cv.getContext("2d");
    if (!c) return;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    drawBackdrop(c, size.w, size.h);
    drawGrid(c, tiles, gridLayout(area.w, area.h), area, hover);
    // `area` is derived from `size` and `narrow`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tiles, size, narrow, hover]);

  const at = (clientX: number, clientY: number) => {
    const el = canvas.current;
    if (!el) return -1;
    const r = el.getBoundingClientRect();
    return hitTest(layout, clientX - r.left - area.x, clientY - r.top - area.y, tiles.length);
  };

  return (
    <div ref={wrap} className={styles.screen} data-role="simulator-screen" data-stack={narrow} data-hot={hover >= 0}>
      <canvas
        ref={canvas}
        className={styles.canvas}
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
            origin: { left: r.left + area.x, top: r.top + area.y, width: area.w, height: area.h },
          });
        }}
      />
      <div className={styles.marquee} data-role="simulator-marquee" data-on={hovered !== undefined} aria-hidden="true">
        {hovered && (
          <>
            <b>{tileLabel(hovered)}</b>
            <span>
              {pct(hovered.health)} · {hovered.red} red-line fails · {hovered.runs.length.toLocaleString("en-US")} runs
            </span>
          </>
        )}
      </div>
      {props.children}
    </div>
  );
}
