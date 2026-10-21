// The one-line freshness strip QueryView shows over held data when the live feed cannot vouch
// for it, and the quiet busy mark while a refresh runs over it. The live path renders neither.
import { Button } from "../ui/Button";
import { feedDown, timeOf, type Freshness as Kind } from "./freshness";
import type { ConnectionSnapshot } from "../data";
import styles from "./FreshnessLine.module.css";

export function FreshnessLine(props: {
  readonly freshness: Kind;
  readonly updatedAt: number | undefined;
  readonly refreshing: boolean;
  readonly connection: ConnectionSnapshot;
  readonly onRefresh: () => void;
}) {
  const { updatedAt, refreshing } = props;
  if (props.freshness === "unconfirmed" && updatedAt !== undefined) {
    const why = feedDown(props.connection) ? "live updates paused" : "rechecking after the live feed dropped";
    return (
      <p className={styles.strip} role="status" aria-busy={refreshing} data-role="query-freshness" data-freshness="unconfirmed">
        <span>
          As of {timeOf(updatedAt)} · {why}
        </span>
        <Button variant="ink" size="compact" onClick={props.onRefresh} disabled={refreshing}>
          {refreshing ? "Refreshing…" : "Refresh"}
        </Button>
      </p>
    );
  }
  // A refresh over held data: content stays, the region is marked busy for assistive tech only.
  if (refreshing && updatedAt !== undefined) {
    return (
      <span className={styles.busy} aria-busy="true" data-role="query-freshness" data-freshness={props.freshness}>
        Refreshing
      </span>
    );
  }
  return null;
}
