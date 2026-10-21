// Session review: the session wall (layer 1) and one session's tabs (layer 2). The shell reads
// this module through app/pages.ts; both layers are code-split. The poster face is small and
// ships with the entry.
import { lazy } from "react";
import { Glance, Mini, Motif } from "./Face";

const Layer1 = lazy(() => import("./Layer1").then((m) => ({ default: m.Layer1 })));
const Layer2 = lazy(() => import("./Layer2").then((m) => ({ default: m.Layer2 })));

/** Tab names that differ from the humanized id; Timeline, Player, Judge, Moderation and Compare are their id. */
const TAB_LABELS: Readonly<Record<string, string>> = { types: "Review by type", calls: "Engine calls" };

/** Breadcrumbs: a session id verbatim, then its tab by name, then a line or a kind; a compare's second session verbatim. */
export function reviewLabel(segment: string, depth: number, parts: readonly string[] = []): string | undefined {
  if (depth === 1 || (depth === 3 && parts[2] === "compare")) return segment;
  if (depth === 2) return TAB_LABELS[segment];
  return undefined;
}

export const page = {
  Layer1,
  Layer2,
  Motif,
  Glance,
  Mini,
  label: reviewLabel,
};
