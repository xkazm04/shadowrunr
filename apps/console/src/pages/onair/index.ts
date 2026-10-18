// P1's write set. The shell reads this module through app/pages.ts. Both layers are
// code-split, so each page is its own route chunk under the bundle budget.
import { lazy } from "react";

const Layer1 = lazy(() => import("./Layer1").then((m) => ({ default: m.Layer1 })));
const Layer2 = lazy(() => import("./Layer2").then((m) => ({ default: m.Layer2 })));

const LABELS: Readonly<Record<string, string>> = { "rundown": "The rundown" };

export const page = {
  Layer1,
  Layer2,
  label: (segment: string) => LABELS[segment],
};
