// The Session review poster face: a strip of tape with type-coloured ticks (motif), the session
// count and how many lines carry the operator's marks (glance), and a mini tape on the mixer bus.
import styles from "./Face.module.css";
import { useSessions } from "./hooks";
import { KIND_INK } from "./ink";

const TICKS = [12, 30, 44, 61, 78, 96, 113, 129, 150, 168, 181] as const;
const KIND = ["roast", "joke", "show", "tease", "callout", "roast", "title", "show", "reply", "roast", "care"] as const;

export function Motif() {
  return (
    <svg className={styles.motif} viewBox="0 0 200 400" preserveAspectRatio="xMidYMid slice" aria-hidden="true" data-role="review-motif">
      <rect className={styles.tape} x="18" y="150" width="164" height="100" />
      {TICKS.map((x, i) => (
        <rect key={x} x={x + 8} y={162 + (i % 3) * 6} width="5" height={64 - (i % 3) * 12} style={{ fill: KIND_INK[KIND[i] ?? "other"] }} />
      ))}
      <circle className={styles.reel} cx="52" cy="310" r="26" />
      <circle className={styles.reel} cx="148" cy="310" r="26" />
    </svg>
  );
}

export function Glance() {
  const q = useSessions();
  if (!q.data) return <b>The tape</b>;
  const marked = q.data.reduce((n, s) => n + s.marked, 0);
  return (
    <>
      <b>{q.data.length} sessions</b>
      <b>{marked} lines marked</b>
    </>
  );
}

export function Mini() {
  return (
    <svg viewBox="0 0 80 50" width="100%" height="100%" data-role="review-mini" aria-hidden="true">
      <rect width="80" height="50" style={{ fill: "var(--sr-color-surface-void)" }} />
      {TICKS.slice(0, 8).map((x, i) => (
        <rect key={x} x={6 + i * 9} y="14" width="4" height="22" style={{ fill: KIND_INK[KIND[i] ?? "other"] }} />
      ))}
    </svg>
  );
}
