// Pure filter and sort for FilterTable (testable without a DOM).

export type SortDir = "asc" | "desc";

export interface SortState {
  readonly column: string;
  readonly dir: SortDir;
}

export interface FilterSpec<R> {
  /** Text the query is matched against (case-insensitive). */
  readonly haystack: (row: R) => string;
  readonly query: string;
  /** Extra predicate, e.g. the active facet pill. */
  readonly predicate?: (row: R) => boolean;
  readonly sortValue?: (row: R) => string | number;
  readonly dir?: SortDir;
}

/** Every whitespace-separated term must occur in the haystack. */
export function filterRows<R>(rows: readonly R[], spec: FilterSpec<R>): R[] {
  const terms = spec.query
    .toLowerCase()
    .split(/\s+/)
    .filter((t) => t.length > 0);
  const out = rows.filter(
    (r) => (spec.predicate ? spec.predicate(r) : true) && (terms.length === 0 || matchesAll(spec.haystack(r), terms)),
  );
  const key = spec.sortValue;
  if (key) {
    const sign = spec.dir === "desc" ? -1 : 1;
    out.sort((a, b) => sign * compare(key(a), key(b)));
  }
  return out;
}

export interface FacetCountSpec<R, F extends string> {
  readonly haystack: (row: R) => string;
  readonly query: string;
  readonly predicate: (row: R, facet: F) => boolean;
  readonly options: readonly { readonly id: F }[];
}

/**
 * How many rows each facet option would show under the query: the text filter runs once (the
 * haystack is read once per row, and only when there is a query), then each option's own predicate
 * over what it left. The count carries the applied predicate, so it agrees with the rows on screen.
 */
export function facetCounts<R, F extends string>(rows: readonly R[], spec: FacetCountSpec<R, F>): Record<F, number> {
  const matched = filterRows(rows, { haystack: spec.haystack, query: spec.query });
  const out = {} as Record<F, number>;
  for (const o of spec.options) out[o.id] = matched.filter((r) => spec.predicate(r, o.id)).length;
  return out;
}

function matchesAll(hay: string, terms: readonly string[]): boolean {
  const h = hay.toLowerCase();
  return terms.every((t) => h.includes(t));
}

function compare(a: string | number, b: string | number): number {
  if (typeof a === "number" && typeof b === "number") return a - b;
  return String(a).localeCompare(String(b), "en", { numeric: true });
}

export function nextSort(current: SortState | null, column: string): SortState {
  if (current?.column !== column) return { column, dir: "asc" };
  return { column, dir: current.dir === "asc" ? "desc" : "asc" };
}
