// The one URL contract: `#/<page>/<segment>/...`.
//
// Every segment is encoded exactly once (encodeURIComponent, so "/" inside a segment becomes
// %2F) by `encodeRoute`, and decoded exactly once by `decodeRoute`. Nothing else in the app
// builds or parses a hash. This replaces the winner's `~` substitution and its double decode.

export const PAGE_IDS = ["onair", "characters", "brain", "segments", "simulator", "reconcile"] as const;
export type PageId = (typeof PAGE_IDS)[number];

/** The wall is `#/wall`; an empty hash or `#/` also lands there. */
export const WALL_SEGMENT = "wall";

export type Route =
  | { readonly view: "wall" }
  /** `sub` empty: the page's layer 1. `sub` non-empty: a nested layer-2 page. */
  | { readonly view: "page"; readonly page: PageId; readonly sub: readonly string[] };

export type Layer = "wall" | "l1" | "l2";

export const WALL: Route = { view: "wall" };

export function isPageId(s: string | undefined): s is PageId {
  return s !== undefined && (PAGE_IDS as readonly string[]).includes(s);
}

export function pageRoute(page: PageId, ...sub: string[]): Route {
  return { view: "page", page, sub };
}

export function layerOf(route: Route): Layer {
  if (route.view === "wall") return "wall";
  return route.sub.length === 0 ? "l1" : "l2";
}

export function toParts(route: Route): string[] {
  switch (route.view) {
    case "wall":
      return [WALL_SEGMENT];
    case "page":
      return [route.page, ...route.sub];
  }
}

/** The single encoder. */
export function encodeRoute(route: Route): string {
  return `#/${toParts(route).map(encodeURIComponent).join("/")}`;
}

export type Decoded =
  | { readonly ok: true; readonly route: Route }
  /** The hash named no known route; `route` is where to go instead (the wall). */
  | { readonly ok: false; readonly route: Route; readonly reason: string };

/** The single decoder. Accepts `#/a/b`, `#a/b` and `""`. */
export function decodeRoute(hash: string): Decoded {
  const body = hash.replace(/^#\/?/, "");
  const raw = body.split("/").filter((s) => s.length > 0);
  const parts: string[] = [];
  for (const r of raw) {
    const d = safeDecode(r);
    if (d === undefined) return { ok: false, route: WALL, reason: `malformed segment "${r}"` };
    parts.push(d);
  }
  const [head, ...sub] = parts;
  if (head === undefined || head === WALL_SEGMENT) {
    return sub.length === 0 ? { ok: true, route: WALL } : { ok: false, route: WALL, reason: "the wall has no sub-pages" };
  }
  if (isPageId(head)) return { ok: true, route: { view: "page", page: head, sub } };
  return { ok: false, route: WALL, reason: `unknown page "${head}"` };
}

function safeDecode(segment: string): string | undefined {
  try {
    return decodeURIComponent(segment);
  } catch (e) {
    if (e instanceof URIError) return undefined;
    throw e;
  }
}

/** One level up (A/1's router). The wall has no parent: Esc there does nothing. */
export function up(route: Route): Route | null {
  switch (route.view) {
    case "wall":
      return null;
    case "page":
      return route.sub.length === 0 ? WALL : { ...route, sub: route.sub.slice(0, -1) };
  }
}

export function sameRoute(a: Route, b: Route): boolean {
  return encodeRoute(a) === encodeRoute(b);
}

export interface Crumb {
  readonly label: string;
  readonly route: Route;
  readonly current: boolean;
}

/** Resolves a segment id to a label; `undefined` falls back to `humanize`. */
export type LabelOf = (segment: string, depth: number, parts: readonly string[]) => string | undefined;

/** Breadcrumbs from ids: Season, then one crumb per segment of the route. */
export function crumbs(route: Route, labelOf: LabelOf): Crumb[] {
  const out: Crumb[] = [{ label: "Season", route: WALL, current: route.view === "wall" }];
  if (route.view === "wall") return out;
  const parts = toParts(route);
  parts.forEach((segment, depth) => {
    const prefix = parts.slice(0, depth + 1);
    const decoded = decodeRoute(`#/${prefix.map(encodeURIComponent).join("/")}`);
    out.push({
      label: labelOf(segment, depth, parts) ?? humanize(segment),
      route: decoded.route,
      current: depth === parts.length - 1,
    });
  });
  return out;
}

/** `rule-of-three` → `Rule of three`. */
export function humanize(id: string): string {
  const s = id.replace(/[-_]+/g, " ").trim();
  return s.length === 0 ? id : s.charAt(0).toUpperCase() + s.slice(1);
}
