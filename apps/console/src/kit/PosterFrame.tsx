// The frame every layer 1 shares (the winner's l1.js): the brand row above the page's hero,
// and the footer with the credit and the call to action that pushes into layer 2. The page
// owns its root (layout and palette) and its hero; the frame owns the repeated print.
import type { CSSProperties, ReactNode } from "react";
import { RouteLink, type Route } from "../router";
import { cx } from "../ui/cx";
import { visuallyHidden } from "../ui/VisuallyHidden";
import styles from "./PosterFrame.module.css";

/** The footer's call to action: a lit link into the page's layer 2. */
export interface PosterCta {
  readonly to: Route;
  readonly label: string;
  /** `data-role` of the link (`push-in` where the shell's tests look for it). */
  readonly role: string;
}

export const POSTER_NOTE = "v0.1 · media-less phase: voice, avatar and broadcast come next · stylised illustration";

export interface PosterFrameProps {
  /** The page's root class: its layout and palette. */
  readonly className: string | undefined;
  /** `data-role` of the root, e.g. `brain-layer1`. */
  readonly role: string;
  readonly style?: CSSProperties | undefined;
  /** A visually hidden heading for the layer. */
  readonly heading?: string | undefined;
  /** The brand row and the footer keep the poster body's measure (centred). */
  readonly measure?: boolean | undefined;
  /** Shown beside the brand (a flag, a note). */
  readonly topExtra?: ReactNode;
  /** The footer's credit line. */
  readonly credit: ReactNode;
  /** The small line under the credit. */
  readonly note?: string | undefined;
  readonly cta: PosterCta;
  /** The hero. */
  readonly children: ReactNode;
}

export function Brand() {
  return (
    <div className={styles.brand}>
      <svg viewBox="0 0 40 40" aria-hidden="true">
        <path d="M6 16L9 5l7 7 4-9 4 9 7-7 3 11z" fill="currentColor" />
        <ellipse cx="20" cy="26" rx="11" ry="10" fill="none" stroke="currentColor" strokeWidth="3" />
        <circle cx="16" cy="25" r="2.4" fill="currentColor" />
        <circle cx="24" cy="25" r="2.4" fill="currentColor" />
      </svg>
      <b>Shadowrunr OS</b>
      <span>Season 0 · v0.1</span>
    </div>
  );
}

export function PosterFrame(p: PosterFrameProps) {
  const measure = p.measure ? styles.measure : undefined;
  return (
    <div className={p.className} style={p.style} data-role={p.role}>
      {p.heading !== undefined && <h2 className={visuallyHidden}>{p.heading}</h2>}
      <div className={cx(styles.top, measure)}>
        <Brand />
        {p.topExtra}
      </div>
      {p.children}
      <div className={cx(styles.foot, measure)}>
        <p className={styles.credit}>
          {p.credit}
          <span>{p.note ?? POSTER_NOTE}</span>
        </p>
        <RouteLink to={p.cta.to} className={styles.cta} data-role={p.cta.role}>
          <span>{p.cta.label}</span>
          <i />
        </RouteLink>
      </div>
    </div>
  );
}
