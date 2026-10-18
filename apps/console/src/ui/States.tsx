// Offline, empty and degraded states: every page shows one of these instead of a blank or a
// spinner forever. Pages build the copy with the data layer's `viewOfError`.
import type { ReactNode } from "react";
import { cx } from "./cx";
import styles from "./States.module.css";

export type PanelState = "offline" | "degraded" | "empty" | "blocked" | "invalid" | "loading";

const KICKER: Readonly<Record<PanelState, string>> = {
  offline: "Offline",
  degraded: "Degraded",
  empty: "Empty",
  blocked: "Blocked",
  invalid: "Rejected",
  loading: "Loading",
};

export interface StatePanelProps {
  readonly state: PanelState;
  readonly title: string;
  readonly hint?: ReactNode;
  readonly detail?: string;
  readonly action?: { readonly label: string; readonly onClick: () => void };
}

export function StatePanel(props: StatePanelProps) {
  const live = props.state === "loading" ? "polite" : props.state === "empty" ? undefined : "assertive";
  return (
    <div className={cx(styles.panel, styles[props.state])} role={live ? "status" : undefined} aria-live={live} data-state={props.state}>
      <span className={styles.kicker}>{KICKER[props.state]}</span>
      <p className={styles.title}>{props.title}</p>
      {props.hint !== undefined && <p className={styles.hint}>{props.hint}</p>}
      {props.detail !== undefined && <p className={styles.detail}>{props.detail}</p>}
      {props.action && (
        <button type="button" className={styles.action} onClick={props.action.onClick}>
          {props.action.label}
        </button>
      )}
    </div>
  );
}

export function OfflineState(props: { onRetry?: () => void; detail?: string }) {
  return (
    <StatePanel
      state="offline"
      title="The daemon is not reachable"
      hint="Start it with `shadowrunr daemon --port 7717`; the console reconnects on its own."
      {...(props.detail === undefined ? {} : { detail: props.detail })}
      {...(props.onRetry ? { action: { label: "Retry now", onClick: props.onRetry } } : {})}
    />
  );
}

export function EmptyState(props: { title: string; hint?: ReactNode }) {
  return <StatePanel state="empty" title={props.title} {...(props.hint === undefined ? {} : { hint: props.hint })} />;
}

export function DegradedState(props: { title: string; hint?: ReactNode; onRetry?: () => void }) {
  return (
    <StatePanel
      state="degraded"
      title={props.title}
      {...(props.hint === undefined ? {} : { hint: props.hint })}
      {...(props.onRetry ? { action: { label: "Retry", onClick: props.onRetry } } : {})}
    />
  );
}
