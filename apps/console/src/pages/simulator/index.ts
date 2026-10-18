// P5's write set. The shell reads this module through app/pages.ts. Both layers are
// code-split, so each page is its own route chunk under the bundle budget.
import { lazy } from "react";
import { cellLabel, parseCellRouteKey } from "./routeKey";

const Layer1 = lazy(() => import("./Layer1").then((m) => ({ default: m.Layer1 })));
const Layer2 = lazy(() => import("./Layer2").then((m) => ({ default: m.Layer2 })));

const LABELS: Readonly<Record<string, string>> = {
  tournaments: "Tournaments",
  runs: "Runs",
  verdicts: "Verdicts",
  heatmap: "Heatmap",
};

/** Breadcrumbs: tab names, then ids as they are (run and tournament ids are not prose). */
function label(segment: string, depth: number, parts: readonly string[]): string | undefined {
  if (depth === 1) return LABELS[segment];
  if (depth === 2 && parts[1] === "heatmap") {
    const k = parseCellRouteKey(segment);
    return k ? cellLabel(k.cell) : segment;
  }
  return depth >= 2 ? segment : undefined;
}

export const page = {
  Layer1,
  Layer2,
  label,
};
