// Evidence refs (`<run_id>#<line>`) as links into the Brain's transcript viewer, by route:
// `#/brain/transcripts/<run>/<line>`. A ref that does not parse is shown as plain text.
import { pageRoute, RouteLink } from "../../router";
import styles from "./Layer2.module.css";
import { parseRef, refLabel } from "../../kit/events";

/** Refs shown per item before the rest fold into a count. */
export const MAX_REFS = 8;

export function RefLinks({ refs }: { refs: readonly string[] | undefined }) {
  if (!refs || refs.length === 0) return null;
  const shown = refs.slice(0, MAX_REFS);
  return (
    <span className={styles.refs}>
      {shown.map((r, i) => {
        const p = parseRef(r);
        return p ? (
          <RouteLink key={`${r}-${i}`} to={pageRoute("brain", "transcripts", p.runId, String(p.line))} className={styles.ref} title={`Open ${r} in the transcript`} data-role="reconcile-ref">
            {refLabel(r)}
          </RouteLink>
        ) : (
          <span key={`${r}-${i}`} className={styles.refRaw}>
            {r}
          </span>
        );
      })}
      {refs.length > shown.length && <span className={styles.refMore}>+{refs.length - shown.length} more</span>}
    </span>
  );
}
