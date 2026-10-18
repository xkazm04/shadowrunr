// The bands around a scene held at a fixed aspect (Brain's and Simulator's A/1 scenes are
// letterboxed at 1600:810). A page fills them with poster art: decorative, aria-hidden, never
// over the scene and never shifting it. The bands are absolutely placed siblings of the scene, so
// the scene keeps its exact box; they render only when there is a band worth filling.
import { useLayoutEffect, useState, type ReactNode, type RefObject } from "react";
import { cx } from "../ui/cx";
import { remPx } from "../ui/remPx";
import styles from "./LetterboxBands.module.css";

export interface BoxSize {
  readonly w: number;
  readonly h: number;
}

/** The element's size, re-read on resize only (ResizeObserver; jsdom has none and keeps 0). */
export function useBoxSize(ref: RefObject<HTMLElement | null>): BoxSize {
  const [size, setSize] = useState<BoxSize>({ w: 0, h: 0 });
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

/** Under this many rem a band holds no art (a sliver reads as a rendering fault). */
export const BAND_MIN_REM = 1.5;

/** The height of each band in px: half of what the host has left above and below the scene. */
export function bandOf(host: BoxSize, scene: BoxSize): number {
  return host.w > 0 && scene.w > 0 ? Math.max(0, (host.h - scene.h) / 2) : 0;
}

export function LetterboxBands(props: {
  /** Each band's height in px (`bandOf`); no art below `BAND_MIN_REM`. */
  readonly band: number;
  readonly top: ReactNode;
  readonly bottom: ReactNode;
  /** `data-role` prefix: `<role>-top` and `<role>-bottom`. */
  readonly role: string;
  /** The page's band surface (its face colour). */
  readonly className?: string | undefined;
}) {
  const rem = remPx();
  if (props.band < BAND_MIN_REM * rem) return null;
  const height = `${(props.band / rem).toFixed(3)}rem`;
  return (
    <>
      <div className={cx(styles.band, styles.top, props.className)} style={{ height }} aria-hidden="true" data-role={`${props.role}-top`}>
        {props.top}
      </div>
      <div className={cx(styles.band, styles.bottom, props.className)} style={{ height }} aria-hidden="true" data-role={`${props.role}-bottom`}>
        {props.bottom}
      </div>
    </>
  );
}
