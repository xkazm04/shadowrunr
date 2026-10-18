// The Segments & ranks poster face: the winner's motif (art.js M.segments, a stepped ziggurat
// under a ticket), the glance chips on the hovered wall poster (app.js glance, with descoped
// counted as descoped), and A/1's vision-mixer mini-render (app.js MINI.segments).
import type { CSSProperties } from "react";
import styles from "./Face.module.css";
import { useTables } from "./hooks";
import { glance } from "./counts";

export function Motif() {
  return (
    <svg className={styles.motif} viewBox="0 0 200 400" preserveAspectRatio="xMidYMid slice" aria-hidden="true" data-role="segments-motif">
      <g className={styles.k}>
        <rect x="20" y="330" width="160" height="34" />
        <rect x="38" y="296" width="124" height="34" />
        <rect x="56" y="262" width="88" height="34" />
        <rect x="74" y="228" width="52" height="34" />
        <rect x="88" y="194" width="24" height="34" />
      </g>
      <g className={styles.o}>
        <rect x="26" y="336" width="148" height="10" />
        <rect x="44" y="302" width="112" height="10" />
        <rect x="62" y="268" width="76" height="10" />
      </g>
      <rect className={`${styles.t} ${styles.breathe}`} x="30" y="70" width="140" height="64" />
      <path className={styles.f} d="M30 102h14a8 8 0 0 0 0-16M170 102h-14a8 8 0 0 1 0-16" opacity=".9" />
      <path className={styles.lines} d="M62 84H138M62 102H138M62 120H110" strokeWidth="5" />
    </svg>
  );
}

export function Glance() {
  const tables = useTables();
  const chips = tables.data ? glance(tables.data.segments) : [];
  if (chips.length === 0) return <b>The programme</b>;
  return (
    <>
      {chips.map((c) => (
        <b key={c}>{c}</b>
      ))}
    </>
  );
}

/** A/1's mini ziggurat, apex blinking, inked from the rank tokens (one rank source). */
const MINI_STEPS = [
  { x: 18, y: 40, w: 44, h: 6, ink: "--sr-rank-thrall" },
  { x: 22, y: 33, w: 36, h: 7, ink: "--sr-rank-acolyte" },
  { x: 26, y: 26, w: 28, h: 7, ink: "--sr-rank-cultist" },
  { x: 30, y: 18, w: 20, h: 8, ink: "--sr-rank-warlock" },
  { x: 34, y: 10, w: 12, h: 8, ink: "--sr-rank-lich" },
] as const;

export function Mini() {
  return (
    <svg viewBox="0 0 80 50" width="100%" height="100%" data-role="segments-mini" aria-hidden="true">
      <rect width="80" height="50" style={{ fill: "var(--sr-color-surface-void)" }} />
      {MINI_STEPS.map((s) => (
        <rect key={s.ink} x={s.x} y={s.y} width={s.w} height={s.h} style={{ fill: `var(${s.ink})` } as CSSProperties} />
      ))}
      <rect x="37" y="3" width="6" height="7" style={{ fill: "var(--sr-rank-dread-council)" }}>
        <animate attributeName="opacity" values="1;.5;1" dur="2s" repeatCount="indefinite" />
      </rect>
    </svg>
  );
}
