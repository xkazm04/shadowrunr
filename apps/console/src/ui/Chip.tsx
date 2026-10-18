import type { ReactNode } from "react";
import styles from "./Chip.module.css";
import { cx } from "./cx";

export type Tone = "neutral" | "ok" | "fail" | "caution" | "unevaluable" | "muted" | "signal";

export function Chip(props: { children: ReactNode; tone?: Tone; title?: string; onClick?: () => void }) {
  const cls = cx(styles.chip, props.tone && props.tone !== "neutral" && styles[props.tone]);
  return props.onClick ? (
    <button type="button" className={cls} title={props.title} onClick={props.onClick}>
      {props.children}
    </button>
  ) : (
    <span className={cls} title={props.title}>
      {props.children}
    </span>
  );
}

export function ChipRow({ children }: { children: ReactNode }) {
  return <div className={styles.chips}>{children}</div>;
}

/**
 * The one status → tone table (the winner mapped states to chip classes in four places).
 * Check states, verdicts and health share it.
 */
export const STATE_TONES: Readonly<Record<string, Tone>> = {
  pass: "ok",
  held: "ok",
  green: "ok",
  fail: "fail",
  failed: "fail",
  red: "fail",
  wobbled: "caution",
  amber: "caution",
  not_evaluable: "unevaluable",
  not_provoked: "muted",
};

export function toneOfState(state: string): Tone {
  return STATE_TONES[state] ?? "neutral";
}

/** A status chip: `<State value="not_provoked" />` reads "not provoked" in the muted tone. */
export function State({ value, label }: { value: string; label?: string }) {
  return <Chip tone={toneOfState(value)}>{label ?? value.replace(/_/g, " ")}</Chip>;
}
