// P2's write set. The shell reads this module through app/pages.ts. Both layers are
// code-split, so each page is its own route chunk under the bundle budget. The poster-face
// motif and glance are small and render on the wall, so they ship with the entry.
import { lazy } from "react";
import { Glance, Motif } from "./Poster";
import { crumbLabel } from "./routes";

const Layer1 = lazy(() => import("./Layer1").then((m) => ({ default: m.Layer1 })));
const Layer2 = lazy(() => import("./Layer2").then((m) => ({ default: m.Layer2 })));

export const page = {
  Layer1,
  Layer2,
  Motif,
  Glance,
  label: crumbLabel,
};
