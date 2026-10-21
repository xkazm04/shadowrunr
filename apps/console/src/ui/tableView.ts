// What a FilterTable remembers between mounts, per explicit `viewKey`, in memory only (never
// localStorage): the query, facet and sort the operator applied and the row they last opened.
// A drill-down navigates away and the shell remounts the page on the way back; this store is
// how the table comes back as it was left. It is read once at mount (no subscription) and
// validated by the table on read - a column, facet or row that has since gone falls back.
import type { SortState } from "./filterRows";

export interface TableView {
  readonly query: string;
  readonly facet: string | undefined;
  readonly sort: SortState | null;
  /** `getKey` of the row last activated. */
  readonly opened: string | undefined;
}

const views = new Map<string, TableView>();

const EMPTY: TableView = { query: "", facet: undefined, sort: null, opened: undefined };

export const tableView = {
  get(viewKey: string): TableView | undefined {
    return views.get(viewKey);
  },
  set(viewKey: string, patch: Partial<TableView>): void {
    views.set(viewKey, { ...(views.get(viewKey) ?? EMPTY), ...patch });
  },
  clear(viewKey: string): void {
    views.delete(viewKey);
  },
  /** Tests only: forget every view. */
  reset(): void {
    views.clear();
  },
};
