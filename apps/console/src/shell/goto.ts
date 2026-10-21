// Go to anything (Ctrl+K), the pure half. One field resolves what the operator pastes or types
// through the vocabulary the console already has, never a parser of its own:
//   a console link or hash     decodeRoute, the one hash parser (route.ts)
//   a `<run>#<line>` ref       parseRef (kit/events.ts), landed by transcriptRoute (router/brain.ts)
//   a segment id               segments/tables/<id>
//   a page                     the poster copy (POSTERS), the wall's own names
//   a recent place             the routes visited this session, labelled by crumbs()
import { parseRef } from "../kit/events";
import { PAGE_IDS, crumbs, decodeRoute, pageRoute, type LabelOf, type Route } from "../router";
import { transcriptRoute } from "../router/brain";
import { POSTERS } from "./posters";

/** A place the palette can go: a route and how it reads. */
export interface Place {
  readonly route: Route;
  readonly label: string;
}

export type CandidateKind = "link" | "ref" | "segment" | "recent" | "page";

export interface Candidate extends Place {
  readonly kind: CandidateKind;
  /** The quiet second line: what kind of place, or where it came from. */
  readonly hint: string;
}

const NO_LABELS: LabelOf = () => undefined;

/** Recent routes as places, labelled by the breadcrumbs (Season dropped). */
export function placesOf(routes: readonly Route[], labelOf: LabelOf): Place[] {
  return routes.map((route) => ({ route, label: label(route, labelOf) }));
}

const label = (route: Route, labelOf: LabelOf): string =>
  crumbs(route, labelOf)
    .slice(route.view === "wall" ? 0 : 1)
    .map((c) => c.label)
    .join(" / ");

/** On Air's segment ids: SEG-01, SEG-02A. */
const SEGMENT_ID = /^SEG-\d+[A-Z]?$/i;
/** A pasted console URL (`http://127.0.0.1:1420/#/...`, `tauri://localhost/#/...`). */
const URL_WITH_HASH = /^[a-z][\w+.-]*:\/\/[^#\s]*#/i;

/** The route a pasted hash or console URL names; `null` when it is not one, `undefined` when it names nothing. */
function linkOf(q: string): Route | null | undefined {
  if (!q.startsWith("#") && !URL_WITH_HASH.test(q)) return null;
  const decoded = decodeRoute(q.slice(q.indexOf("#")));
  return decoded.ok ? decoded.route : undefined;
}

/** Word-prefix matches rank above substring matches; `undefined` is no match. */
function rank(needle: string, ...texts: string[]): number | undefined {
  const hay = texts.map((t) => t.toLowerCase());
  if (hay.some((t) => t.split(/[\s&/-]+/).some((w) => w.startsWith(needle)))) return 0;
  return hay.some((t) => t.includes(needle)) ? 1 : undefined;
}

/**
 * Everything a query can mean, best first: an exact link, ref or segment id; then the recent
 * places; then the pages. An empty query lists the recent places, then every page.
 */
export function resolveQuery(query: string, recents: readonly Place[] = [], labelOf: LabelOf = NO_LABELS): Candidate[] {
  const q = query.trim();
  const out: Candidate[] = [];

  const link = linkOf(q);
  if (link !== null) {
    // It is a link: offer it if it names a place, and nothing fuzzy either way.
    return link ? [{ kind: "link", route: link, label: label(link, labelOf), hint: "Console link" }] : [];
  }
  const ref = parseRef(q);
  if (ref) {
    out.push({ kind: "ref", route: transcriptRoute({ runId: ref.runId }, ref.line), label: `Transcript line ${ref.line}`, hint: ref.runId });
  }
  if (SEGMENT_ID.test(q)) {
    const id = q.toUpperCase();
    out.push({ kind: "segment", route: pageRoute("segments", "tables", id), label: `Segment ${id}`, hint: "Segment tables" });
  }

  const needle = q.toLowerCase();
  for (const place of recents) {
    if (needle === "" || rank(needle, place.label) !== undefined) out.push({ ...place, kind: "recent", hint: "Recent" });
  }
  const pages = PAGE_IDS.flatMap((id) => {
    const p = POSTERS[id];
    const r = needle === "" ? 0 : rank(needle, p.name, p.short, p.tag, id);
    return r === undefined ? [] : [{ r, c: { kind: "page" as const, route: pageRoute(id), label: p.name, hint: p.tag } }];
  });
  out.push(...pages.sort((a, b) => a.r - b.r).map((p) => p.c));
  return out;
}
