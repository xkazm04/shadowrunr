// Shared bits of the simulator's nested pages: links (verdicts, cells, Brain transcripts)
// and the health swatch. Number formats and refs come from kit/events.
import type { CSSProperties, ReactNode } from "react";
import type { Query } from "../../data";
import { QueryView } from "../../kit";
import { pageRoute, RouteLink } from "../../router";
import { EmptyState } from "../../ui/States";
import { parseRef, pct } from "../../kit/events";
import { transcriptRoute } from "../../router/brain";
import { healthColor } from "./ramp";
import styles from "./l2.module.css";

export const shortHash = (h: string) => (h.length > 12 ? `${h.slice(0, 12)}…` : h);

export function RunLink({ runId, children }: { runId: string; children?: ReactNode }) {
  return (
    <RouteLink to={pageRoute("simulator", "verdicts", runId)} className={styles.ref} title={`Verdict of ${runId}`}>
      {children ?? runId}
    </RouteLink>
  );
}

/** A `<run_id>#<line>` ref as a link into Brain's transcript viewer, at that line. */
export function RefLink({ refStr }: { refStr: string }) {
  const r = parseRef(refStr);
  if (!r) return <code className={styles.code}>{refStr}</code>;
  return (
    <RouteLink to={transcriptRoute(r, r.line)} className={styles.ref} title={refStr}>
      L{r.line}
    </RouteLink>
  );
}

export function RefList({ refs }: { refs: readonly string[] }) {
  if (refs.length === 0) return null;
  return (
    <span className={styles.refs}>
      {refs.map((r, i) => (
        <RefLink key={`${r}-${i}`} refStr={r} />
      ))}
    </span>
  );
}

/** The share of rule checks held, as A/1's ramp swatch plus the figure. */
export function Health({ value }: { value: number | undefined }) {
  if (value === undefined) return <span className={styles.dim}>not judged</span>;
  const style: CSSProperties = { background: healthColor(value) };
  return (
    <span className={styles.health}>
      <i className={styles.swatch} style={style} aria-hidden="true" />
      {pct(value)}
    </span>
  );
}

export function Note({ children }: { children: ReactNode }) {
  return <p className={styles.note}>{children}</p>;
}

/**
 * `QueryView`, except that the daemon's `not_found` reads as this page's own empty state
 * (a run without a verdict, a tournament not summarised yet) rather than the generic one.
 */
export function Found<T>(props: { query: Query<T>; emptyTitle: string; emptyHint?: string; isEmpty?: (data: T) => boolean; children: (data: T) => ReactNode }) {
  const { query, emptyTitle, emptyHint } = props;
  if (query.data === undefined && query.error?.code === "not_found") {
    return <EmptyState title={emptyTitle} {...(emptyHint === undefined ? {} : { hint: emptyHint })} />;
  }
  return (
    <QueryView query={query} emptyTitle={emptyTitle} {...(emptyHint === undefined ? {} : { emptyHint })} {...(props.isEmpty ? { isEmpty: props.isEmpty } : {})}>
      {props.children}
    </QueryView>
  );
}
