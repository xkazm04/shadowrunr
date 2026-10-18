import type { CSSProperties, ReactNode } from "react";
import { cx } from "./cx";
import styles from "./StatBand.module.css";

export interface Stat {
  readonly label: string;
  readonly value: ReactNode;
  /** `fail` prints the numeral in the warning ink. */
  readonly tone?: "fail";
}

/** The heavy-ruled band of big numerals (the winner drew it three ways). */
export function StatBand({ stats }: { stats: readonly Stat[] }) {
  const style = { "--cols": String(Math.max(1, stats.length)) } as CSSProperties;
  return (
    <div className={styles.band} style={style}>
      {stats.map((s) => (
        <div key={s.label} className={cx(styles.cell, s.tone === "fail" && styles.fail)}>
          <b className={styles.value}>{s.value}</b>
          <span className={styles.label}>{s.label}</span>
        </div>
      ))}
    </div>
  );
}
