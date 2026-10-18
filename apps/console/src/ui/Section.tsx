import type { ReactNode } from "react";
import styles from "./Section.module.css";

/** A spread section with its heavy-ruled heading. */
export function Section(props: { title: ReactNode; right?: ReactNode; children: ReactNode; id?: string }) {
  return (
    <section className={styles.section} id={props.id}>
      <h2 className={styles.sectionHead}>
        {props.title}
        {props.right !== undefined && <span className={styles.sectionRight}>{props.right}</span>}
      </h2>
      {props.children}
    </section>
  );
}

/** The layer-2 page head: kicker, display title, serif lede. */
export function Head(props: { kicker?: ReactNode; title: ReactNode; lede?: ReactNode }) {
  return (
    <header className={styles.head}>
      {props.kicker !== undefined && <div className={styles.kicker}>{props.kicker}</div>}
      <h1 className={styles.title}>{props.title}</h1>
      {props.lede !== undefined && <p className={styles.lede}>{props.lede}</p>}
    </header>
  );
}

export interface TabItem<T extends string> {
  readonly id: T;
  readonly label: string;
}

/** Sticky tabs. Selection is the caller's (usually a route segment). */
export function Tabs<T extends string>(props: {
  items: readonly TabItem<T>[];
  value: T;
  onSelect: (id: T) => void;
  label: string;
}) {
  return (
    <div className={styles.tabs} role="tablist" aria-label={props.label}>
      {props.items.map((t) => (
        <button
          key={t.id}
          type="button"
          role="tab"
          className={styles.tab}
          aria-selected={t.id === props.value}
          onClick={() => props.onSelect(t.id)}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}
