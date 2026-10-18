import { lazy, Suspense } from "react";
import { DataProvider, type DataLayer } from "../data";
import { RouterProvider, type RouterStore } from "../router";
import { Shell } from "../shell";
import { StatePanel } from "../ui";
import { PAGES } from "./pages";

// LEGACY (C2 deletes this and src/legacy, src/views, src/api, src/lib, src/fixtures, src/contract.ts).
const LegacyDesk = lazy(() => import("../legacy/LegacyDesk"));
export const LEGACY_VIEWS = ["preflight", "simulations", "reconcile", "lord", "directives"] as const;

export function App(props: { router: RouterStore; data: DataLayer; playIntro: boolean }) {
  return (
    <RouterProvider router={props.router}>
      <DataProvider layer={props.data}>
        <Shell
          pages={PAGES}
          playIntro={props.playIntro}
          legacyViews={LEGACY_VIEWS}
          renderLegacy={(sub, navigate) => (
            <Suspense fallback={<StatePanel state="loading" title="Loading the legacy desk" />}>
              <LegacyDesk view={sub[0]} onNavigate={navigate} />
            </Suspense>
          )}
        />
      </DataProvider>
    </RouterProvider>
  );
}
