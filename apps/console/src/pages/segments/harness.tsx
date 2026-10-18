// Test-only harness (imported by *.test.tsx only): the page on the real data layer, door and
// router, with the data layer's own doubles (scripted fetch, fake sockets, manual timers).
// Proposed for kit/ (every page builder wrote one): the Director may hoist it.
import { act, render } from "@testing-library/react";
import type { ReactNode } from "react";
import { createDataLayer, DataProvider } from "../../data";
import { contractDoor } from "../../data/contractDoor";
import { fakeSockets, manualTimers, scriptedFetch, type FetchReply } from "../../data/testing";
import { createHashRouter, RouterProvider } from "../../router";

export type Script = (method: string, sent: string) => FetchReply;

export const ok = (result: unknown): FetchReply => ({ body: { result } });

export function mountPage(ui: ReactNode, script: Script, hash = "#/segments") {
  window.location.hash = hash;
  const router = createHashRouter(window);
  const fetch = scriptedFetch((method, sent) => script(method, typeof sent === "string" ? sent : ""));
  const layer = createDataLayer({ baseUrl: "http://127.0.0.1:7717", door: contractDoor, fetch, socket: fakeSockets().factory, timers: manualTimers() });
  const view = render(
    <DataProvider layer={layer}>
      <RouterProvider router={router}>{ui}</RouterProvider>
    </DataProvider>,
  );
  return { router, fetch, layer, view };
}

/** Lets queued fetches, door validation and re-renders settle. */
export async function settle(rounds = 4) {
  for (let i = 0; i < rounds; i++) {
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
  }
}

export const nodes = (root: ParentNode = document.body) => root.querySelectorAll("*").length;
