// P3's write set. The shell reads this module through app/pages.ts. Both layers are
// code-split, so each page is its own route chunk under the bundle budget; the mixer-bus mini
// is a few bytes and loads with the shell.
import { lazy } from "react";
import { BrainMini } from "./Mini";
import { BRAIN_ROUTES } from "../../router/brain";

const Layer1 = lazy(() => import("./Layer1").then((m) => ({ default: m.Layer1 })));
const Layer2 = lazy(() => import("./Layer2").then((m) => ({ default: m.Layer2 })));

export const page = {
  Layer1,
  Layer2,
  Mini: BrainMini,
  // breadcrumbs, the rise key and the unknown-floor fallback derive from the declared grammar
  routes: BRAIN_ROUTES,
};
