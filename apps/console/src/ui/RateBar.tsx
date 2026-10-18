import styles from "./RateBar.module.css";

export interface RateBound {
  readonly lo: number;
  readonly hi: number;
}

const pct = (x: number) => `${Math.round(Math.min(1, Math.max(0, x)) * 100)}%`;

/** A rate with its sample size and interval: the bar shows the bound, the tick the point. */
export function RateBar(props: { rate: number | undefined; n: number; bound?: RateBound; label: string }) {
  const { rate, n, bound } = props;
  const text = rate === undefined ? "n/a" : pct(rate);
  const title = bound ? `${props.label}: ${text} (${pct(bound.lo)}–${pct(bound.hi)}), n=${n}` : `${props.label}: ${text}, n=${n}`;
  return (
    <span className={styles.wrap} title={title}>
      <span className={styles.bar} role="img" aria-label={title}>
        {bound && <i className={styles.bound} style={{ left: pct(bound.lo), width: pct(bound.hi - bound.lo) }} />}
        {rate !== undefined && <b className={styles.point} style={{ left: pct(rate) }} />}
      </span>
      <span className={styles.text}>
        {text}
        <small className={styles.n}>n={n}</small>
      </span>
    </span>
  );
}
