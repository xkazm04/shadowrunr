import { PAGE_IDS, type LabelOf, type PageId } from "../router";
import { matchSub, schemaLabel, type RouteSchema } from "../router/schema";
import { POSTERS } from "./posters";
import type { PageModule } from "./types";

/**
 * How many segments below the page id make a new layer-2 view: a tab, or a list's detail. The
 * shell keys the page's layer 2 on them, so the entrance rise replays when they change and a
 * deeper segment (a transcript line, a timeline moment, a report filter, a picked situation)
 * re-renders in place and keeps its state.
 *
 * This hand-list is the fallback for pages that have not declared `routes`: a page with a
 * schema gets its depth from the schema (an `inPlace` segment does not count), and its entry
 * here is kept only as the parity check its tests assert. Migrate a page by declaring a schema.
 */
export const RISE_DEPTH: Readonly<Record<PageId, number>> = {
  // rundown / <tournament> / <run>, clips / <clip>
  onair: 3,
  // <id> / package / <tab>; a situation opens beside its list
  characters: 3,
  // declared in pages/brain/routes.ts (parity: <floor> / <run>; the line is in place)
  brain: 2,
  // tables / <segment>, ranks / <report>
  segments: 2,
  // <tab> / <tournament | run | cell>
  simulator: 2,
  // <session> / <tab>; the line opens the timeline's drawer
  review: 2,
  // <tab>; held and failed filter the report in place
  reconcile: 1,
};

/** The key a page's layer 2 remounts on: from its schema when it declares one. */
export function riseKey(page: PageId, sub: readonly string[], routes?: RouteSchema): string {
  const m = routes ? matchSub(routes, sub) : undefined;
  return sub.slice(0, m?.ok ? m.riseDepth : RISE_DEPTH[page]).join("/");
}

/** The parts of a page module the breadcrumbs read. */
export type LabelSource = Partial<Record<PageId, Pick<PageModule, "routes" | "label">>>;

/** The shell's breadcrumb labeller: poster names for pages, then each page's schema or `label`. */
export function pageLabeller(pages: LabelSource): LabelOf {
  const fromSchema = new Map<PageId, LabelOf>();
  for (const id of PAGE_IDS) {
    const routes = pages[id]?.routes;
    if (routes) fromSchema.set(id, schemaLabel(routes));
  }
  return (segment, depth, parts) => {
    const page = PAGE_IDS.find((id) => id === parts[0]);
    if (page === undefined) return undefined;
    if (depth === 0) return POSTERS[page].name;
    return (fromSchema.get(page) ?? pages[page]?.label)?.(segment, depth, parts);
  };
}
