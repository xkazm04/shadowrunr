// One way to show a query: loading, offline, degraded or empty states, otherwise the content.
// Pages use it so every surface has its offline, empty and degraded states by construction.
import type { ReactNode } from "react";
import { useConnection, type Query } from "../data";
import { OfflineState, StatePanel } from "../ui";

export function QueryView<T>(props: {
  query: Query<T>;
  /** True when the reply holds nothing to show. */
  isEmpty?: (data: T) => boolean;
  emptyTitle?: string;
  emptyHint?: ReactNode;
  children: (data: T) => ReactNode;
}) {
  const { query } = props;
  if (query.data !== undefined) {
    if (props.isEmpty?.(query.data)) {
      return <StatePanel state="empty" title={props.emptyTitle ?? "Nothing here yet"} {...(props.emptyHint === undefined ? {} : { hint: props.emptyHint })} />;
    }
    return (
      <>
        {query.errorView && (
          <StatePanel
            state="degraded"
            title={`Showing the last good reply · ${query.errorView.title}`}
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
      <OfflineState onRetry={query.reload} detail={v.message} />
    ) : (
      <StatePanel state={v.state} title={v.title} hint={v.hint} detail={`${v.code}: ${v.message}`} action={{ label: "Retry", onClick: query.reload }} />
    );
  }
  return <StatePanel state="loading" title="Loading" />;
}

/** The connection as a page state, for surfaces that have no query yet. */
export function ConnectionState({ children }: { children: ReactNode }) {
  const c = useConnection();
  if (c.summary === "offline") return <OfflineState />;
  if (c.summary === "degraded") {
    return <StatePanel state="degraded" title="The daemon answers, the live feed does not" hint="Live updates resume when the event stream reconnects." />;
  }
  return <>{children}</>;
}
