// Pieces the layer 2 tabs share: a tick strip of when things happened (one SVG, however many
// ticks), a pager of fixed-size pages, and the Malakor line a moment opens on.
import type { Derived } from "./derive";
import { fmtClock } from "./names";
import styles from "./tabkit.module.css";

/** Rows per page wherever a tab lists occurrences or calls. */
export const PAGE = 20;

/** `id` as a sentence: capital first letter, a full stop when it has no end mark. */
export function sentence(s: string): string {
  const t = s.trim();
  if (!t) return t;
  const c = t.charAt(0).toUpperCase() + t.slice(1);
  return /[.?!…]$/.test(c) ? c : `${c}.`;
}

/** The Malakor line a moment opens on: the line itself, else the next one, else the last before it. */
export function sayNear(d: Derived, line: number): number | undefined {
  if (d.sayByLine.has(line)) return line;
  const s = d.says;
  let lo = 0;
  let hi = s.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((s[mid]?.line ?? 0) < line) lo = mid + 1;
    else hi = mid;
  }
  return (s[lo] ?? s[s.length - 1])?.line;
}

/** Nearest-rank percentile of `xs` (unsorted); undefined when empty. */
export function pct(xs: readonly number[], p: number): number | undefined {
  if (xs.length === 0) return undefined;
  const b = [...xs].sort((x, y) => x - y);
  return b[Math.min(b.length - 1, Math.floor(p * b.length))];
}

export const secs = (ms: number | undefined): string => (ms === undefined ? "–" : `${(ms / 1000).toFixed(1)} s`);

export interface Tick {
  /** Stream seconds. */
  readonly t: number;
  readonly tone?: "on" | "ok";
}

const W = 1000;

/** When things happened across the session, as one SVG: plain ticks, then marked (ok), then the current one (signal). */
export function TickStrip(props: { ticks: readonly Tick[]; lengthS: number; label: string; axis?: boolean }) {
  const L = Math.max(1, props.lengthS);
  const path = (tone: Tick["tone"]) =>
    props.ticks
      .filter((k) => k.tone === tone)
      .map((k) => `M${Math.round((W * Math.min(L, Math.max(0, k.t))) / L)} 0V10`)
      .join("");
  const plain = path(undefined);
  const ok = path("ok");
  const on = path("on");
  return (
    <div className={styles.strip}>
      <svg className={styles.ticks} viewBox={`0 0 ${W} 10`} preserveAspectRatio="none" role="img" aria-label={props.label}>
        {plain && <path d={plain} className={styles.tick} />}
        {ok && <path d={ok} className={styles.tickOk} />}
        {on && <path d={on} className={styles.tickOn} />}
      </svg>
      {props.axis && (
        <div className={styles.axis}>
          <span>00:00</span>
          <span>{fmtClock(L)}</span>
        </div>
      )}
    </div>
  );
}

/** "Showing a to b of n" with Previous / Next; `page` is 0-based. */
export function Pager(props: { page: number; total: number; onPage: (page: number) => void; label: string }) {
  const last = Math.max(0, Math.ceil(props.total / PAGE) - 1);
  const from = props.total === 0 ? 0 : props.page * PAGE + 1;
  const to = Math.min(props.total, (props.page + 1) * PAGE);
  return (
    <nav className={styles.pager} aria-label={props.label}>
      <button type="button" className={styles.btn} disabled={props.page <= 0} onClick={() => props.onPage(props.page - 1)}>
        Previous
      </button>
      <button type="button" className={styles.btn} disabled={props.page >= last} onClick={() => props.onPage(props.page + 1)}>
        Next
      </button>
      <span className={styles.showing}>
        Showing {from} to {to} of {props.total}
      </span>
    </nav>
  );
}
