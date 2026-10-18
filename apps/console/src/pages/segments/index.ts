// P4's write set. The shell reads this module through app/pages.ts. Both layers are
// code-split, so each page is its own route chunk under the bundle budget. The poster-face
// motif, glance and the mixer-bus mini are small and render on the wall, so they ship with the
// entry.
import { lazy } from "react";
import { Glance, Mini, Motif } from "./Face";

const Layer1 = lazy(() => import("./Layer1").then((m) => ({ default: m.Layer1 })));
const Layer2 = lazy(() => import("./Layer2").then((m) => ({ default: m.Layer2 })));

const LABELS: Readonly<Record<string, string>> = {
  tables: "Segment tables",
  commands: "Chat commands",
  ranks: "Ranks and weights",
};

/** Breadcrumbs: the three nested pages by name, a segment id verbatim, a report as `Report <id>`. */
export function segmentsLabel(segment: string, depth: number, parts: readonly string[]): string | undefined {
  if (depth === 1) return LABELS[segment];
  if (depth === 2 && parts[1] === "tables") return segment;
  if (depth === 2 && parts[1] === "ranks") return `Report ${segment}`;
  return undefined;
}

export const page = {
  Layer1,
  Layer2,
  Motif,
  Glance,
  Mini,
  label: segmentsLabel,
};
