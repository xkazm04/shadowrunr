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
