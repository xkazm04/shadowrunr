// Orientation: a route read back as the place a person sees, with the ids in view and the
// tools that fit it. The ids come from the same sub-route grammars the pages decode (their
// Layer2 readers, characters/routes.ts, router/brain.ts), never from an ambient selection.
import { crumbs, decodeRoute, encodeRoute, layerOf, type Decoded, type PageId, type Route } from "../../router";
import { matchSub } from "../../router/schema";
import { readSub } from "../../pages/characters/routes";
import { parseCellRouteKey } from "../../pages/simulator/routeKey";
import { pageLabeller } from "../../shell/rise";
import type { PageRegistry } from "../../shell/types";
import { PAGES } from "../pages";

const REGISTRY: PageRegistry = PAGES;
const labelOf = pageLabeller(REGISTRY);

export interface Ids {
  character?: string;
  session?: string;
  line?: number;
  tab?: string;
  run?: string;
  tournament?: string;
}

export interface Place {
  readonly view: string;
  readonly page: PageId | "wall";
  readonly layer: "wall" | "l1" | "l2";
  readonly title: string;
  readonly breadcrumbs: readonly string[];
  readonly ids: Ids;
  readonly tools_here: readonly string[];
}

const lineOf = (s: string | undefined): number | undefined => (s !== undefined && /^[1-9]\d{0,8}$/.test(s) ? Number(s) : undefined);

/** Sets only the keys that have a value (optional fields are omitted, never null). */
function put(ids: Ids, key: keyof Ids, value: string | number | undefined) {
  if (value === undefined || value === "") return;
  if (key === "line") {
    if (typeof value === "number") ids.line = value;
  } else if (typeof value === "string") ids[key] = value;
}

export function idsOf(route: Route): Ids {
  const ids: Ids = {};
  if (route.view === "wall") return ids;
  const [a, b, c] = route.sub;
  switch (route.page) {
    case "characters": {
      const r = readSub(route.sub);
      if (r.kind === "package") {
        put(ids, "character", r.id);
        put(ids, "tab", r.tab);
        put(ids, "session", r.from?.session);
        put(ids, "line", r.from?.line);
      } else if (r.kind === "all") put(ids, "tab", "all");
      break;
    }
    case "review":
      put(ids, "session", a);
      put(ids, "tab", a === undefined ? undefined : (b ?? "timeline"));
      if (b === "timeline") put(ids, "line", lineOf(c));
      break;
    case "simulator":
      put(ids, "tab", a);
      if (a === "tournaments") put(ids, "tournament", b);
      if (a === "verdicts") put(ids, "run", b);
      if (a === "heatmap" && b !== undefined) put(ids, "tournament", parseCellRouteKey(b)?.tournamentId);
      break;
    case "brain":
      put(ids, "tab", a);
      if (a === "transcripts") {
        put(ids, "run", b);
        put(ids, "line", lineOf(c));
      }
      break;
    case "onair":
      put(ids, "tab", a);
      if (a === "rundown" && b !== undefined && c !== undefined) {
        put(ids, "tournament", b);
        put(ids, "run", c);
      }
      break;
    case "segments":
    case "reconcile":
      put(ids, "tab", a);
      break;
  }
  return ids;
}

/** The 3-6 tools that fit a place, most useful first. */
export function toolsHere(route: Route): readonly string[] {
  if (route.view === "wall") return ["app_map", "list_sessions", "list_characters", "list_tournaments", "go_to"];
  const deep = route.sub.length > 0;
  switch (route.page) {
    case "characters":
      return deep && route.sub[0] !== "all"
        ? ["get_character", "read_character_doc", "edit_character_doc", "set_character_doc", "validate_character", "set_tone"]
        : ["list_characters", "get_character", "read_character_doc"];
    case "review":
      return deep ? ["get_session", "open_moment", "leave_feedback", "list_sessions"] : ["list_sessions", "get_session", "start_sandbox"];
    case "simulator":
      return ["list_tournaments", "get_tournament", "list_runs", "get_verdict", "start_sim"];
    case "brain":
      return ["get_transcript", "ledger_summary", "list_runs", "query"];
    case "segments":
      return ["ranks_table", "query", "go_to"];
    case "reconcile":
      return ["query", "list_tournaments", "go_to"];
    case "onair":
      return ["list_runs", "get_transcript", "start_sandbox", "stop_sandbox"];
  }
}

/**
 * The shell accepts a decoded route only when the page's declared grammar does (Brain declares
 * one); other pages show their own "no such page" state, so the route stands.
 */
export function checkRoute(hash: string): Decoded {
  const d = decodeRoute(hash);
  if (!d.ok || d.route.view === "wall") return d;
  const routes = REGISTRY[d.route.page].routes;
  if (routes === undefined) return d;
  const m = matchSub(routes, d.route.sub);
  if (m.ok) return d;
  const fallback: Route = { view: "page", page: d.route.page, sub: m.fallback };
  return { ok: false, route: fallback, reason: `no page at "${d.route.sub.join("/")}" under ${d.route.page}; the nearest is ${encodeRoute(fallback)}` };
}

export function describe(route: Route): Place {
  const trail = crumbs(route, labelOf).map((c) => c.label);
  return {
    view: encodeRoute(route),
    page: route.view === "wall" ? "wall" : route.page,
    layer: layerOf(route),
    title: trail[trail.length - 1] ?? "Season",
    breadcrumbs: trail,
    ids: idsOf(route),
    tools_here: toolsHere(route),
  };
}
