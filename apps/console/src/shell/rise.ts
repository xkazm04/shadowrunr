import type { PageId } from "../router";

/**
 * How many segments below the page id make a new layer-2 view: a tab, or a list's detail. The
 * shell keys the page's layer 2 on them, so the entrance rise replays when they change and a
 * deeper segment (a transcript line, a timeline moment, a report filter, a picked situation)
 * re-renders in place and keeps its state.
 */
export const RISE_DEPTH: Readonly<Record<PageId, number>> = {
  // rundown / <tournament> / <run>, clips / <clip>
  onair: 3,
  // <id> / package / <tab>; a situation opens beside its list
  characters: 3,
  // <floor> / <run>; the line is a scroll target in the transcript
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

/** The key a page's layer 2 remounts on. */
export function riseKey(page: PageId, sub: readonly string[]): string {
  return sub.slice(0, RISE_DEPTH[page]).join("/");
}
