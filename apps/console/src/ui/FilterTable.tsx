// The one filterable table (the winner had four copies): text filter, optional facet pills,
// sortable columns, and a virtualised body (spacer rows keep real <table> semantics).
import { observeElementRect, useVirtualizer } from "@tanstack/react-virtual";
import { useDeferredValue, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { cx } from "./cx";
import { facetCounts, filterRows, nextSort, type SortState } from "./filterRows";
import styles from "./FilterTable.module.css";
import { PillGroup, type PillOption } from "./PillGroup";
import { remPx } from "./remPx";
import { StatePanel } from "./States";
import { tableView } from "./tableView";
import { DEFAULT_INITIAL_RECT, measuredHeight } from "./VirtualList";

export interface Column<R> {
  readonly id: string;
  readonly header: string;
  readonly cell: (row: R) => ReactNode;
  readonly sortValue?: (row: R) => string | number;
  readonly align?: "start" | "end" | "center";
}

export interface Facets<R, F extends string> {
  readonly label: string;
  readonly options: readonly PillOption<F>[];
  readonly initial: F;
  readonly predicate: (row: R, facet: F) => boolean;
  /**
   * `derived` (default): FilterTable counts each option with `predicate` over the rows the query
   * leaves, so a pill's number is what pressing it would show. `given`: the options' own counts are
   * printed verbatim - only for totals over rows the client does not hold (a list the daemon cut).
   */
  readonly counts?: "derived" | "given";
}

export interface FilterTableProps<R, F extends string> {
  readonly rows: readonly R[];
  readonly columns: readonly Column<R>[];
  readonly getKey: (row: R) => string;
  readonly haystack: (row: R) => string;
  readonly label: string;
  readonly placeholder?: string;
  readonly facets?: Facets<R, F>;
  readonly onActivate?: (row: R) => void;
  readonly selectedKey?: string;
  readonly estimateRem?: number;
  readonly initialSort?: SortState;
  /**
   * Remember this table's view (query, facet, sort, the row last opened) in memory under this key,
   * so a drill-down and back restores it and lands focus on the row you came from. Unique per table
   * identity - two tables share a label, never a key. Absent: nothing is remembered.
   */
  readonly viewKey?: string;
}

const int = (n: number) => n.toLocaleString("en-US");

function alignClass(a: "start" | "end" | "center" | undefined): string | undefined {
  return a === "end" ? styles.end : a === "center" ? styles.center : undefined;
}

export function FilterTable<R, F extends string = string>(props: FilterTableProps<R, F>) {
  const { rows, columns, haystack, facets, getKey, onActivate, selectedKey, viewKey } = props;
  // The remembered view is read once, before the first paint, and validated: a column, facet
  // option or row that has gone since falls back to the table's own initial state.
  const [stored] = useState(() => (viewKey === undefined ? undefined : tableView.get(viewKey)));
  const [query, setQuery] = useState(stored?.query ?? "");
  const [facet, setFacet] = useState<F | undefined>(() => {
    const f = stored?.facet;
    return facets?.options.find((o) => o.id === f)?.id ?? facets?.initial;
  });
  const [sort, setSort] = useState<SortState | null>(() => {
    const s = stored?.sort;
    return s && columns.some((c) => c.id === s.column && c.sortValue) ? s : (props.initialSort ?? null);
  });
  const deferred = useDeferredValue(query);
  useEffect(() => {
    if (viewKey !== undefined) tableView.set(viewKey, { query, facet, sort });
  }, [viewKey, query, facet, sort]);

  // The text filter runs once per deferred query; the facet pills and the sort read its result,
  // so a pill's count and the "N of M" line answer the same predicate.
  const matched = useMemo(() => filterRows(rows, { haystack, query: deferred }), [rows, haystack, deferred]);
  const options = useMemo(() => {
    if (!facets || facets.counts === "given") return facets?.options;
    const n = facetCounts(matched, { ...facets, haystack, query: "" });
    return facets.options.map((o) => ({ ...o, count: n[o.id] }));
  }, [facets, matched, haystack]);

  const shown = useMemo(() => {
    const col = columns.find((c) => c.id === sort?.column);
    return filterRows(matched, {
      haystack,
      query: "",
      ...(facets && facet !== undefined ? { predicate: (r: R) => facets.predicate(r, facet) } : {}),
      ...(col?.sortValue ? { sortValue: col.sortValue, dir: sort?.dir ?? "asc" } : {}),
    });
  }, [matched, columns, haystack, facets, facet, sort]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const rowPx = (props.estimateRem ?? 2.4) * remPx();
  const v = useVirtualizer({
    count: shown.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowPx,
    measureElement: (el) => measuredHeight(el, rowPx),
    overscan: 10,
    initialRect: DEFAULT_INITIAL_RECT,
    getItemKey: (i) => {
      const r = shown[i];
      return r === undefined ? i : getKey(r);
    },
    observeElementRect: (instance, cb) =>
      observeElementRect(instance, (rect) => {
        if (rect.height > 0) cb(rect);
      }),
  });
  const items = v.getVirtualItems();

  // Back from a drill-down: bring the row that was opened into view once, then focus it as soon
  // as the virtualiser has rendered it. A row no longer in the view is not looked for again.
  const returned = stored?.opened;
  const landing = useRef<"scroll" | "focus" | "done">(returned === undefined ? "done" : "scroll");
  useEffect(() => {
    if (landing.current !== "scroll" || returned === undefined) return;
    const i = shown.findIndex((r) => getKey(r) === returned);
    landing.current = i < 0 ? "done" : "focus";
    if (i >= 0) v.scrollToIndex(i, { align: "center" });
  }, [returned, shown, getKey, v]);
  useEffect(() => {
    if (landing.current !== "focus") return;
    const el = scrollRef.current?.querySelector<HTMLElement>("tr[data-returned='true']");
    if (el) {
      el.focus({ preventScroll: true });
      landing.current = "done";
    }
  });
  const activate = onActivate
    ? (row: R) => {
        if (viewKey !== undefined) tableView.set(viewKey, { opened: getKey(row) });
        onActivate(row);
      }
    : undefined;
  const q = deferred.trim();
  const faceted = facets !== undefined && facet !== facets.initial;
  const filtered = q !== "" || faceted;
  const clear = () => {
    setQuery("");
    setFacet(facets?.initial);
  };
  const top = items[0]?.start ?? 0;
  const bottom = v.getTotalSize() - (items[items.length - 1]?.end ?? 0);
  const total = int(rows.length);
  const spacer = (height: number) =>
    height > 0 && (
      <tr className={styles.spacer} aria-hidden="true">
        <td colSpan={columns.length} style={{ height }} />
      </tr>
    );

  return (
    <div className={styles.root}>
      <div className={styles.tools}>
        <input
          type="search"
          className={styles.search}
          value={query}
          placeholder={props.placeholder ?? "Filter"}
          aria-label={`Filter ${props.label}`}
          onChange={(e) => setQuery(e.target.value)}
        />
        {facets && options && facet !== undefined && (
          <PillGroup label={facets.label} options={options} value={facet} onChange={setFacet} />
        )}
        <span className={styles.count} aria-live="polite">
          {int(shown.length)} of {total}
        </span>
      </div>
      <div ref={scrollRef} className={styles.scroll}>
        <table className={styles.table} aria-label={props.label} aria-rowcount={shown.length + 1}>
          <thead>
            <tr>
              {columns.map((c) => (
                <th
                  key={c.id}
                  className={cx(styles.th, alignClass(c.align))}
                  aria-sort={sort?.column === c.id ? (sort.dir === "asc" ? "ascending" : "descending") : undefined}
                >
                  {c.sortValue ? (
                    <button type="button" className={styles.sortBtn} onClick={() => setSort((s) => nextSort(s, c.id))}>
                      {c.header}
                    </button>
                  ) : (
                    c.header
                  )}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {spacer(top)}
            {items.map((item) => {
              const row = shown[item.index];
              if (row === undefined) return null;
              const key = getKey(row);
              return (
                <tr
                  key={item.key}
                  ref={v.measureElement}
                  data-index={item.index}
                  aria-rowindex={item.index + 2}
                  aria-selected={selectedKey === undefined ? undefined : selectedKey === key}
                  tabIndex={activate ? 0 : undefined}
                  data-returned={returned === key ? "true" : undefined}
                  className={cx(
                    styles.tr,
                    activate && styles.clickable,
                    selectedKey === key && styles.selected,
                    returned === key && styles.returned,
                  )}
                  onClick={activate ? () => activate(row) : undefined}
                  onKeyDown={
                    activate
                      ? (e) => {
                          if (e.key === "Enter") activate(row);
                        }
                      : undefined
                  }
                >
                  {columns.map((c) => (
                    <td key={c.id} className={cx(styles.td, alignClass(c.align))}>
                      {c.cell(row)}
                    </td>
                  ))}
                </tr>
              );
            })}
            {spacer(bottom)}
            {shown.length === 0 && rows.length > 0 && (
              <tr data-role="filter-table-empty">
                <td colSpan={columns.length} className={styles.emptyCell}>
                  <StatePanel
                    state="empty"
                    title="No matches"
                    hint={
                      `None of the ${total} rows ${q === "" ? "is in this view" : `matches “${q}”${faceted ? " in this view" : ""}`}.`
                    }
                    {...(filtered ? { action: { label: "Clear filter", onClick: clear } } : {})}
                  />
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
