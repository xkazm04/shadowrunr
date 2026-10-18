// The one filterable table (the winner had four copies): text filter, optional facet pills,
// sortable columns, and a virtualised body (spacer rows keep real <table> semantics).
import { observeElementRect, useVirtualizer } from "@tanstack/react-virtual";
import { useDeferredValue, useMemo, useRef, useState, type ReactNode } from "react";
import { cx } from "./cx";
import { filterRows, nextSort, type SortState } from "./filterRows";
import styles from "./FilterTable.module.css";
import { PillGroup, type PillOption } from "./PillGroup";
import { remPx } from "./remPx";
import { StatePanel } from "./States";
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
}

function alignClass(a: "start" | "end" | "center" | undefined): string | undefined {
  return a === "end" ? styles.end : a === "center" ? styles.center : undefined;
}

export function FilterTable<R, F extends string = string>(props: FilterTableProps<R, F>) {
  const { rows, columns, haystack, facets, getKey, onActivate, selectedKey } = props;
  const [query, setQuery] = useState("");
  const [facet, setFacet] = useState<F | undefined>(facets?.initial);
  const [sort, setSort] = useState<SortState | null>(props.initialSort ?? null);
  const deferred = useDeferredValue(query);

  const shown = useMemo(() => {
    const col = sort ? columns.find((c) => c.id === sort.column) : undefined;
    return filterRows(rows, {
      haystack,
      query: deferred,
      ...(facets && facet !== undefined ? { predicate: (r: R) => facets.predicate(r, facet) } : {}),
      ...(col?.sortValue ? { sortValue: col.sortValue, dir: sort?.dir ?? "asc" } : {}),
    });
  }, [rows, columns, haystack, facets, facet, deferred, sort]);

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
  const filtered = deferred.trim() !== "" || (facets !== undefined && facet !== facets.initial);
  const clear = () => {
    setQuery("");
    setFacet(facets?.initial);
  };
  const top = items[0]?.start ?? 0;
  const bottom = v.getTotalSize() - (items[items.length - 1]?.end ?? 0);

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
        {facets && facet !== undefined && (
          <PillGroup label={facets.label} options={facets.options} value={facet} onChange={setFacet} />
        )}
        <span className={styles.count} aria-live="polite">
          {shown.length.toLocaleString("en-US")} of {rows.length.toLocaleString("en-US")}
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
            {top > 0 && (
              <tr className={styles.spacer} aria-hidden="true">
                <td colSpan={columns.length} style={{ height: top }} />
              </tr>
            )}
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
                  tabIndex={onActivate ? 0 : undefined}
                  className={cx(styles.tr, onActivate && styles.clickable, selectedKey === key && styles.selected)}
                  onClick={onActivate ? () => onActivate(row) : undefined}
                  onKeyDown={
                    onActivate
                      ? (e) => {
                          if (e.key === "Enter") onActivate(row);
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
            {bottom > 0 && (
              <tr className={styles.spacer} aria-hidden="true">
                <td colSpan={columns.length} style={{ height: bottom }} />
              </tr>
            )}
            {shown.length === 0 && rows.length > 0 && (
              <tr data-role="filter-table-empty">
                <td colSpan={columns.length} className={styles.emptyCell}>
                  <StatePanel
                    state="empty"
                    title="No matches"
                    hint={
                      deferred.trim() === ""
                        ? `None of the ${rows.length.toLocaleString("en-US")} rows is in this view.`
                        : `None of the ${rows.length.toLocaleString("en-US")} rows matches “${deferred.trim()}”${facets && facet !== facets.initial ? " in this view" : ""}.`
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
