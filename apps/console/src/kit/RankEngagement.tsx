// Engagement by rank from a Reconcile report (`report.ranks.by_rank`): who chatted, who got
// picked and who got answered. One table for Reconcile's report and Segments' ranks page
// (they held copies). Rates carry their n and bound.
import type { RankRow } from "@shadowrunr/contract";
import { RateBar } from "../ui/RateBar";
import styles from "./RankEngagement.module.css";

/** `dread_council` reads "dread council" when no title is given. */
export const rankLabel = (rank: string) => rank.replace(/_/g, " ");

export interface RankEngagementProps {
  readonly rows: readonly RankRow[];
  /** The rank's display title (a character's rank names); default: the rank id spelled out. */
  readonly title?: (rank: string) => string;
  /** `data-role` of the table; rows get `<role>-row` and `data-rank`. */
  readonly role?: string;
}

export function RankEngagement({ rows, title = rankLabel, role }: RankEngagementProps) {
  return (
    <div className={styles.wrap}>
      <table className={styles.tbl} data-role={role}>
        <thead>
          <tr>
            <th>Rank</th>
            <th className={styles.num}>Viewers</th>
            <th className={styles.num}>Messages</th>
            <th>Picked</th>
            <th>Answered</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((x) => (
            <tr key={x.rank} data-role={role === undefined ? undefined : `${role}-row`} data-rank={x.rank}>
              <td>
                <span className={styles.dot} data-rank={x.rank} aria-hidden="true" />
                {title(x.rank)}
              </td>
              <td className={styles.num}>{x.viewers.toLocaleString("en-US")}</td>
              <td className={styles.num}>{x.messages.toLocaleString("en-US")}</td>
              <td>{x.pick_rate ? <RateBar label="Picked" rate={x.pick_rate.rate} n={x.pick_rate.n} bound={x.pick_rate.bound} /> : x.picked.toLocaleString("en-US")}</td>
              <td>{x.answer_rate ? <RateBar label="Answered" rate={x.answer_rate.rate} n={x.answer_rate.n} bound={x.answer_rate.bound} /> : x.answered.toLocaleString("en-US")}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
