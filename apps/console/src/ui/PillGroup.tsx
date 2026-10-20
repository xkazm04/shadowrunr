import type { ReactNode } from "react";
import styles from "./PillGroup.module.css";

export interface PillOption<T extends string> {
  readonly id: T;
  readonly label: string;
  readonly count?: number;
}

/** Single-select pills (filters, facets): exactly one is pressed. */
export function PillGroup<T extends string>(props: {
  options: readonly PillOption<T>[];
  value: T;
  onChange: (id: T) => void;
  label: string;
}) {
  return (
    <div className={styles.pills} role="group" aria-label={props.label}>
      {props.options.map((o) => (
        <button
          key={o.id}
          type="button"
          className={styles.pill}
          aria-pressed={o.id === props.value}
          onClick={() => props.onChange(o.id)}
        >
          {o.label}
          {o.count !== undefined && <span className={styles.count}>{o.count.toLocaleString("en-US")}</span>}
        </button>
      ))}
    </div>
  );
}

/**
 * One on/off pill, for a multi-select set (kind filters, feedback marks) where any number may be
 * pressed. It looks like a PillGroup pill; the caller lays the set out with `pillRow`.
 */
export function Pill(props: { pressed: boolean; onToggle: () => void; children: ReactNode; title?: string }) {
  return (
    <button type="button" className={styles.pill} aria-pressed={props.pressed} title={props.title} onClick={props.onToggle}>
      {props.children}
    </button>
  );
}

/** The row a set of `Pill`s sits in. */
export const pillRow = styles.pills;
