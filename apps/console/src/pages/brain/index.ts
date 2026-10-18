// P3's write set. The shell reads this module through app/pages.ts. Both layers are
// code-split, so each page is its own route chunk under the bundle budget; the mixer-bus mini
// is a few bytes and loads with the shell.
import { lazy } from "react";
import { BrainMini } from "./Mini";

const Layer1 = lazy(() => import("./Layer1").then((m) => ({ default: m.Layer1 })));
const Layer2 = lazy(() => import("./Layer2").then((m) => ({ default: m.Layer2 })));

const LABELS: Readonly<Record<string, string>> = { engine: "The engine", transcripts: "Transcripts" };

/** Breadcrumbs: the two nested pages by name, a run id verbatim, a line as `Line n`. */
export function brainLabel(segment: string, depth: number, parts: readonly string[]): string | undefined {
  if (depth === 1) return LABELS[segment];
  if (parts[1] === "transcripts" && depth === 2) return segment;
  if (parts[1] === "transcripts" && depth === 3) return `Line ${segment}`;
  return undefined;
}

export const page = {
  Layer1,
  Layer2,
  Mini: BrainMini,
  label: brainLabel,
};
