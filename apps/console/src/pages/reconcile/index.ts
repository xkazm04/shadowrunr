// P6's write set. The shell reads this module through app/pages.ts. Both layers are
// code-split, so each page is its own route chunk under the bundle budget. The poster-face
// motif and glance are small and render on the wall, so they ship with the entry.
import { lazy } from "react";
import { Glance, Motif } from "./Face";

const Layer1 = lazy(() => import("./Layer1").then((m) => ({ default: m.Layer1 })));
const Layer2 = lazy(() => import("./Layer2").then((m) => ({ default: m.Layer2 })));

const LABELS: Readonly<Record<string, string>> = {
  report: "Report",
  held: "Held",
  failed: "Failed",
  plan: "Plan options",
  directives: "Directives",
  chat: "The Lord’s chat",
};

export const page = {
  Layer1,
  Layer2,
  Motif,
  Glance,
  label: (segment: string) => LABELS[segment],
};
