// The one page test harness (imported by tests only; nothing here reaches the production
// bundle): a page on the real data layer, door and router, with the data layer's own doubles
// (scripted fetch, fake sockets, manual timers), plus the helpers every page test uses.
import { act, render } from "@testing-library/react";
import type { ReactNode } from "react";
import { contractDoor } from "../data/contractDoor";
import { createDataLayer, DataProvider } from "../data";
import { fakeSockets, manualTimers, scriptedFetch, type FakeSocket, type FetchReply } from "../data/testing";
import { createHashRouter, RouterProvider } from "../router";

/** Answers one RPC: `sent` is the raw request body (`{"method","params"}`). */
export type Script = (method: string, sent: string) => FetchReply;

export const ok = (result: unknown): FetchReply => ({ body: { result } });
export const fail = (code: string, message: string): FetchReply => ({ status: 400, body: { error: { code, message } } });
export const offline: Script = () => ({ network: true });

/** A string param of the request body, e.g. `param(sent, "run_id")`. */
export const param = (sent: string, name: string): string | undefined => new RegExp(`"${name}":"([^"]*)"`).exec(sent)?.[1];

/** One page of `items` by the request's `cursor` (an offset here) and `limit` (capped at 500). */
export function page<T>(items: readonly T[], sent: string): FetchReply {
  const start = Number(param(sent, "cursor") ?? "0");
  const end = start + Math.min(500, Number(/"limit":(\d+)/.exec(sent)?.[1] ?? "500"));
  return ok({ items: items.slice(start, end), ...(end < items.length ? { next_cursor: String(end) } : {}) });
}

/** Mounts `ui` at `hash` (the route the page reads; `#/` when it does not matter). */
export function mountPage(ui: ReactNode, script: Script, hash = "#/") {
  window.location.hash = hash;
  const router = createHashRouter(window);
  const fetch = scriptedFetch((method, sent) => script(method, typeof sent === "string" ? sent : ""));
  const sockets = fakeSockets();
  const layer = createDataLayer({ baseUrl: "http://127.0.0.1:7717", door: contractDoor, fetch, socket: sockets.factory, timers: manualTimers() });
  const view = render(
    <DataProvider layer={layer}>
      <RouterProvider router={router}>{ui}</RouterProvider>
    </DataProvider>,
  );
  return {
    router,
    fetch,
    /** RPC methods called, in order. */
    calls: fetch.calls,
    layer,
    view,
    /** The live `/events` socket, once the layer opened it. */
    socket: (): FakeSocket => sockets.last(),
    dispose: () => router.dispose(),
  };
}

/** Lets queued fetches, door validation and re-renders settle. */
export async function settle(rounds = 4): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
  }
}

export const nodes = (root: ParentNode = document.body): number => root.querySelectorAll("*").length;
export const role = (r: string): Element | null => document.querySelector(`[data-role='${r}']`);
export const roles = (r: string): Element[] => [...document.querySelectorAll(`[data-role='${r}']`)];
