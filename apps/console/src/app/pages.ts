// The page registry: the only place that knows every page folder.
import type { PageRegistry } from "../shell";
import { page as brain } from "../pages/brain";
import { page as characters } from "../pages/characters";
import { page as onair } from "../pages/onair";
import { page as reconcile } from "../pages/reconcile";
import { page as review } from "../pages/review";
import { page as segments } from "../pages/segments";
import { page as simulator } from "../pages/simulator";

export const PAGES = { onair, characters, brain, segments, simulator, review, reconcile } satisfies PageRegistry;
