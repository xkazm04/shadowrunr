// The shell canonicalises a page's unknown sub-route the way the router canonicalises an unknown
// page: replace (never push) to the deepest prefix the page's declared schema accepts.
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createDataLayer, DataProvider } from "../data";
import { contractDoor } from "../data/contractDoor";
import { fakeSockets, manualTimers, scriptedFetch } from "../data/testing";
import { createHashRouter, crumbs, decodeRoute, RouterProvider, type RouterStore } from "../router";
import { BRAIN_ROUTES } from "../router/brain";
import { pageLabeller, RISE_DEPTH, riseKey } from "./rise";
import { Shell } from "./Shell";
import type { PageModule, PageRegistry } from "./types";

const stand = (id: string, routes?: PageModule["routes"]): PageModule => ({
  Layer1: () => <p>{id}</p>,
  Layer2: ({ sub }) => <div data-role={`${id}-layer2`} data-sub={sub.join("/")} />,
  ...(routes ? { routes } : {}),
});
const PAGES: PageRegistry = {
  onair: stand("onair"),
  characters: stand("characters"),
  brain: stand("brain", BRAIN_ROUTES),
  segments: stand("segments"),
  simulator: stand("simulator"),
  review: stand("review"),
  reconcile: stand("reconcile"),
};

let router: RouterStore;

function mount(hash: string) {
  window.location.hash = hash;
  router = createHashRouter(window);
  const data = createDataLayer({
    baseUrl: "http://127.0.0.1:7717",
    door: contractDoor,
    fetch: scriptedFetch(() => ({ network: true })),
    socket: fakeSockets().factory,
    timers: manualTimers(),
  });
  return render(
    <RouterProvider router={router}>
      <DataProvider layer={data}>
        <Shell pages={PAGES} playIntro={false} />
      </DataProvider>
    </RouterProvider>,
  );
}

beforeEach(() => {
  window.location.hash = "";
});
afterEach(() => {
  cleanup();
  router.dispose();
});

describe("an unknown sub-route", () => {
  it("replaces #/brain/nonsense with #/brain, pushing no history entry and never mounting brain's layer 2", async () => {
    window.location.hash = "#/brain/nonsense";
    const before = window.history.length;
    let mounted = false;
    const spy = new MutationObserver(() => {
      if (document.querySelector("[data-role='brain-layer2']")) mounted = true;
    });
    spy.observe(document.body, { childList: true, subtree: true });
    mount("#/brain/nonsense");
    await waitFor(() => expect(window.location.hash).toBe("#/brain"));
    spy.disconnect();
    expect(window.history.length).toBe(before);
    expect(router.getRoute()).toEqual({ view: "page", page: "brain", sub: [] });
    expect(mounted).toBe(false);
  });

  it("keeps the deepest prefix the schema accepts", async () => {
    mount("#/brain/transcripts/t1%2Fr7/notaline");
    await waitFor(() => expect(window.location.hash).toBe("#/brain/transcripts/t1%2Fr7"));
    await waitFor(() => expect(document.querySelector("[data-role='brain-layer2']")?.getAttribute("data-sub")).toBe("transcripts/t1/r7"));
  });

  it("also canonicalises a sub reached by navigation, not only on first paint", async () => {
    mount("#/brain/engine");
    await act(async () => {
      window.location.hash = "#/brain/engine/nope";
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    });
    await waitFor(() => expect(window.location.hash).toBe("#/brain/engine"));
  });

  it("(guard) leaves a page that declares no schema to its own layer 2", async () => {
    mount("#/reconcile/anything/goes");
    await waitFor(() => expect(document.querySelector("[data-role='reconcile-layer2']")?.getAttribute("data-sub")).toBe("anything/goes"));
    expect(window.location.hash).toBe("#/reconcile/anything/goes");
  });
});

describe("what the shell derives from a declared schema", () => {
  it("keys brain's layer 2 from BRAIN_ROUTES, equal to the RISE_DEPTH hand-list for every brain sub the motion tests use", () => {
    expect(riseKey("brain", ["transcripts", "r1", "12"], BRAIN_ROUTES)).toBe("transcripts/r1");
    const subs = [["transcripts", "r1", "12"], ["transcripts", "r1"], ["transcripts", "r1", "2"], ["transcripts"], ["engine"], ["preflight"]];
    for (const sub of subs) expect(riseKey("brain", sub, BRAIN_ROUTES), sub.join("/")).toBe(riseKey("brain", sub));
    expect(RISE_DEPTH.brain).toBe(2);
  });

  it("labels brain's breadcrumbs from the schema", () => {
    const label = pageLabeller({ brain: { routes: BRAIN_ROUTES } });
    const trail = crumbs(decodeRoute("#/brain/transcripts/t1%2Fr7/12").route, label).map((c) => c.label);
    expect(trail).toEqual(["Season", "Brain", "Transcripts", "t1/r7", "Line 12"]);
    expect(crumbs(decodeRoute("#/brain/preflight").route, label).map((c) => c.label)).toEqual(["Season", "Brain", "Pre-flight"]);
  });

  it("(guard) still reads a page's hand-written label when it declares no schema", () => {
    const label = pageLabeller({ reconcile: { label: (seg) => (seg === "report" ? "The report" : undefined) } });
    expect(crumbs(decodeRoute("#/reconcile/report/x_y").route, label).map((c) => c.label)).toEqual(["Season", "Reconcile & plan", "The report", "X y"]);
  });
});
