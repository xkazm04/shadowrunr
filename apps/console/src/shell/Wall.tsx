// The season wall: six posters laid out by `layoutPosters`. Hover widens a poster on the wall
// and opens a rail strip on layer 1 (armed 1.1 s after arriving, so the strip the pointer
// happens to rest on does not pop open).
import { TOKENS } from "@shadowrunr/tokens";
import { useEffect, useMemo, useRef, useState, type PointerEvent } from "react";
import { PAGE_IDS, type Crumb, type PageId } from "../router";
import { layoutPosters, type LayoutMetrics, type View } from "./layout";
import { Poster } from "./Poster";
import { POSTERS } from "./posters";
import type { PageRegistry } from "./types";
import type { Viewport } from "./useViewport";
import styles from "./Wall.module.css";

const rem = (v: string): number => Number.parseFloat(v);

export const METRICS: LayoutMetrics = {
  stripRem: rem(TOKENS.layout.strip),
  spineRem: rem(TOKENS.layout.spine),
  railPeekRem: rem(TOKENS.layout.railPeek),
  topBarRem: 3,
  tabRem: 3.2,
  barRem: 3.4,
};

const RAIL_ARM_MS = 1100;
const LEAVE_MS = 160;

export interface WallProps {
  readonly pages: PageRegistry;
  readonly view: View;
  readonly active: PageId;
  readonly hover: number | null;
  readonly onHover: (i: number | null) => void;
  readonly viewport: Viewport;
  readonly busRem: number;
  readonly pre: boolean;
  readonly still: boolean;
  readonly crumbs: readonly Crumb[];
  readonly onOpen: (id: PageId) => void;
  readonly onBack: () => void;
  readonly registerHit: (index: number, el: HTMLButtonElement | null) => void;
}

export function Wall(p: WallProps) {
  const activeIndex = PAGE_IDS.indexOf(p.active);
  const [rail, setRail] = useState<number | null>(null);
  const [armed, setArmed] = useState(false);
  const [entered, setEntered] = useState(`${p.view}:${p.active}`);
  const leave = useRef(0);
  const { onHover, registerHit } = p;

  // Re-arm the rail each time a layer-1 page is entered (state adjusted during render).
  if (entered !== `${p.view}:${p.active}`) {
    setEntered(`${p.view}:${p.active}`);
    setRail(null);
    setArmed(false);
  }
  useEffect(() => {
    if (p.view !== "l1") return;
    const t = window.setTimeout(() => setArmed(true), RAIL_ARM_MS);
    return () => window.clearTimeout(t);
  }, [p.view, p.active]);

  useEffect(() => () => window.clearTimeout(leave.current), []);

  const layout = useMemo(
    () =>
      layoutPosters(
        {
          view: p.view,
          count: PAGE_IDS.length,
          active: activeIndex,
          hover: p.view === "wall" && !p.viewport.narrow ? p.hover : null,
          rail: p.view === "l1" && !p.viewport.narrow ? rail : null,
          width: p.viewport.width,
          height: p.viewport.height,
          rem: p.viewport.rem,
          narrow: p.viewport.narrow,
          busRem: p.busRem,
        },
        METRICS,
      ),
    [p.view, activeIndex, p.hover, rail, p.viewport, p.busRem],
  );

  const indexOf = (target: EventTarget): number => {
    const el = target instanceof Element ? target.closest("[data-role='poster']") : null;
    const id = el?.getAttribute("data-page");
    return PAGE_IDS.findIndex((x) => x === id);
  };

  const onPointerOver = (e: PointerEvent<HTMLElement>) => {
    if (p.viewport.narrow || e.pointerType === "touch") return;
    const i = indexOf(e.target);
    if (i < 0) return;
    window.clearTimeout(leave.current);
    if (p.view === "wall" && p.hover !== i) onHover(i);
    else if (p.view === "l1" && armed && i !== activeIndex && rail !== i) setRail(i);
  };

  const onPointerLeave = () => {
    window.clearTimeout(leave.current);
    leave.current = window.setTimeout(() => {
      onHover(null);
      setRail(null);
    }, LEAVE_MS);
  };

  return (
    <div
      className={styles.wall}
      role="navigation"
      aria-label="The season wall: six posters"
      onPointerOver={onPointerOver}
      onPointerLeave={onPointerLeave}
      onFocus={(e) => {
        const i = indexOf(e.target);
        if (p.view === "wall" && i >= 0 && i !== p.hover) onHover(i);
      }}
      data-role="wall"
    >
      {PAGE_IDS.map((id, i) => (
        <Poster
          key={id}
          id={id}
          index={i}
          copy={POSTERS[id]}
          module={p.pages[id]}
          view={p.view}
          active={i === activeIndex}
          big={p.view === "wall" && p.hover === i && !p.viewport.narrow}
          dim={p.view === "wall" && p.hover !== null && p.hover !== i}
          railOpen={p.view === "l1" && rail === i}
          pre={p.pre}
          still={p.still}
          narrow={p.viewport.narrow}
          rect={layout.rects[i]}
          bodyWidth={layout.bodyWidth}
          crumbs={p.crumbs}
          onOpen={() => p.onOpen(id)}
          onBack={p.onBack}
          registerHit={(el) => registerHit(i, el)}
        />
      ))}
    </div>
  );
}
