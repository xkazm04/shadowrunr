import { BREAKPOINTS_PX } from "@shadowrunr/tokens";
import { useEffect, useState } from "react";

export interface Viewport {
  readonly width: number;
  readonly height: number;
  /** px per rem: the token root size is fluid (16px at 1280x800, up to 1.9x). */
  readonly rem: number;
  readonly narrow: boolean;
}

function read(): Viewport {
  const width = window.innerWidth;
  const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
  return {
    width,
    height: window.innerHeight,
    rem: Number.isFinite(rem) && rem > 0 ? rem : 16,
    narrow: width <= BREAKPOINTS_PX.narrow,
  };
}

function same(a: Viewport, b: Viewport): boolean {
  return a.width === b.width && a.height === b.height && a.rem === b.rem && a.narrow === b.narrow;
}

/** The viewport, re-read at most once per frame on resize (and the listener is removed). */
export function useViewport(): Viewport {
  const [vp, setVp] = useState(read);
  useEffect(() => {
    let frame = 0;
    const onResize = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => setVp((prev) => {
        const next = read();
        return same(prev, next) ? prev : next;
      }));
    };
    window.addEventListener("resize", onResize);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", onResize);
    };
  }, []);
  return vp;
}
