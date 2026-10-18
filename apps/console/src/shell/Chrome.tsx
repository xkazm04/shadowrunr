import { useConnection, type ConnectionSummary } from "../data";
import styles from "./Chrome.module.css";

const LABEL: Readonly<Record<ConnectionSummary, string>> = {
  online: "Daemon online",
  degraded: "Daemon degraded",
  offline: "Daemon offline",
  connecting: "Connecting",
};

function ConnectionChip() {
  const c = useConnection();
  const retry = c.summary !== "online" && c.retryInMs !== undefined ? ` · retry in ${Math.ceil(c.retryInMs / 1000)} s` : "";
  return (
    <span className={styles.conn} data-summary={c.summary} data-role="connection" role="status" title={c.issue?.message}>
      <i className={styles.dot} aria-hidden="true" />
      {LABEL[c.summary]}
      {retry}
    </span>
  );
}

export function Chrome(props: { onWall: boolean; narrow: boolean; onToggleWall: () => void }) {
  return (
    <header className={styles.chrome} data-narrow={props.narrow}>
      <ConnectionChip />
      <button type="button" className={styles.wallBtn} onClick={props.onToggleWall} aria-label={props.onWall ? "Back to the page (M)" : "Open the season wall (M)"}>
        <i className={styles.wallIcon} aria-hidden="true" />
        <span>Season</span>
      </button>
    </header>
  );
}
