// One way to show a query: loading, offline, degraded or empty states, otherwise the content.
// Pages use it so every surface has its offline, empty and degraded states by construction,
// and its age stated whenever the live feed cannot vouch for it (freshness.ts).
import type { ReactNode } from "react";
import { currentDaemonUrl, unavailableHint, useConnection, useDataLayer, type Query } from "../data";
import { OfflineState, StatePanel } from "../ui/States";
import { FreshnessLine } from "./FreshnessLine";
import { freshnessOf, timeOf } from "./freshness";

export function QueryView<T>(props: {
  query: Query<T>;
  /** True when the reply holds nothing to show. */
  isEmpty?: (data: T) => boolean;
  emptyTitle?: string;
  emptyHint?: ReactNode;
  children: (data: T) => ReactNode;
}) {
  const { query } = props;
  const connection = useConnection();
  const layer = useDataLayer();
  if (query.data !== undefined) {
    if (props.isEmpty?.(query.data)) {
      return <StatePanel state="empty" title={props.emptyTitle ?? "Nothing here yet"} {...(props.emptyHint === undefined ? {} : { hint: props.emptyHint })} />;
    }
    const freshness = freshnessOf(query, connection, layer.reopenedAt());
    const held = query.updatedAt === undefined ? "Showing the last good reply" : `Showing the reply from ${timeOf(query.updatedAt)}`;
    return (
      <>
        <FreshnessLine freshness={freshness} updatedAt={query.updatedAt} refreshing={query.refreshing} connection={connection} onRefresh={query.reload} />
        {query.errorView && (
          <StatePanel
            state="degraded"
            title={`${held} · ${query.errorView.title}`}
            hint={query.errorView.hint}
            action={{ label: "Retry", onClick: query.reload }}
          />
        )}
        {props.children(query.data)}
      </>
    );
  }
  if (query.errorView) {
    const v = query.errorView;
    return v.state === "offline" ? (
      <OfflineState hint={v.hint} onRetry={query.reload} detail={v.message} />
    ) : (
      <StatePanel state={v.state} title={v.title} hint={v.hint} detail={`${v.code}: ${v.message}`} action={{ label: "Retry", onClick: query.reload }} />
    );
  }
  return <StatePanel state="loading" title="Loading" />;
}

/** The connection as a page state, for surfaces that have no query yet. */
export function ConnectionState({ children }: { children: ReactNode }) {
  const c = useConnection();
  if (c.summary === "offline") return <OfflineState hint={unavailableHint(currentDaemonUrl())} />;
  if (c.summary === "degraded") {
    return <StatePanel state="degraded" title="The daemon answers, the live feed does not" hint="Live updates resume when the event stream reconnects." />;
  }
  return <>{children}</>;
}
