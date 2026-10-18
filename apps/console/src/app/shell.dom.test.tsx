// The shell in jsdom: wall, six posters, layer-1 and nested layer-2 placeholders, the mixer bus,
// keys, and the DOM-count budget (budgets/dom.json). Counts are gated; timings are not.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createDataLayer, type DataLayer } from "../data";
import { contractDoor } from "../data/contractDoor";
import { fakeSockets, manualTimers, scriptedFetch } from "../data/testing";
import { createHashRouter, encodeRoute, pageRoute, type RouterStore } from "../router";
import DOM_BUDGETS from "../../budgets/dom.json";
import { App } from "./App";

const BUDGETS: { readonly routes: Readonly<Record<string, number | undefined>> } = DOM_BUDGETS;

let router: RouterStore;
let data: DataLayer;

function mount(hash: string) {
  window.location.hash = hash;
  router = createHashRouter(window);
  data = createDataLayer({
    baseUrl: "http://127.0.0.1:7717",
    door: contractDoor,
    fetch: scriptedFetch(() => ({ network: true })),
    socket: fakeSockets().factory,
    timers: manualTimers(),
  });
  return render(<App router={router} data={data} playIntro={false} />);
}

async function go(hash: string) {
  await act(async () => {
    window.location.hash = hash;
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  });
}

const nodes = () => document.body.querySelectorAll("*").length;
/** Presses a key, then lets jsdom deliver the hashchange it queued. */
const key = (k: string, mods: { ctrlKey?: boolean } = {}) =>
  act(async () => {
    fireEvent.keyDown(window, { key: k, ...mods });
    await new Promise((r) => setTimeout(r, 0));
  });

beforeEach(() => {
  window.location.hash = "";
});
afterEach(() => {
  cleanup();
  router.dispose();
});

describe("the shell", () => {
  it("renders the wall with all six posters and no bus", () => {
    mount("#/wall");
    const posters = document.querySelectorAll("[data-role='poster']");
    expect([...posters].map((p) => p.getAttribute("data-page"))).toEqual(["onair", "characters", "brain", "segments", "simulator", "reconcile"]);
    expect(document.querySelector("[data-role='mixer-bus']")).toBeNull();
    expect(screen.getByRole("status").textContent).toContain("Connecting");
  });

  it("renders every page's layer-1 placeholder and the mixer bus with the tally on it", async () => {
    mount("#/wall");
    for (const id of ["onair", "characters", "brain", "segments", "simulator", "reconcile"] as const) {
      await go(encodeRoute(pageRoute(id)));
      await screen.findByText(/Layer 1 ·/);
      const active = document.querySelector(`[data-role='poster'][data-page='${id}']`);
      expect(active?.getAttribute("data-active")).toBe("true");
      const channels = document.querySelectorAll("[data-role='bus-channel']");
      expect(channels).toHaveLength(6);
      expect([...channels].filter((c) => c.getAttribute("aria-current") === "page").map((c) => c.getAttribute("data-page"))).toEqual([id]);
    }
  });

  it("renders a nested layer-2 placeholder per page, with breadcrumbs from ids", async () => {
    mount("#/wall");
    const nested = { onair: "rundown", characters: "malakor", brain: "transcripts", segments: "tables", simulator: "tournaments", reconcile: "plan" };
    for (const [id, seg] of Object.entries(nested)) {
      await go(`#/${id}/${seg}`);
      await screen.findByText(/Layer 2 ·/);
      expect(document.querySelector("[data-role='spread']")?.getAttribute("data-open")).toBe("true");
      expect(screen.getByLabelText("Breadcrumb").textContent).toContain("Season");
    }
    expect(screen.getByLabelText("Breadcrumb").textContent).toContain("Plan options");
  });

  it("goes up one level on Esc and stops at the wall", async () => {
    mount("#/brain/transcripts");
    await key("Escape");
    expect(router.getRoute()).toEqual(pageRoute("brain"));
    await key("Escape");
    expect(router.getRoute()).toEqual({ view: "wall" });
    await key("Escape");
    expect(router.getRoute()).toEqual({ view: "wall" });
  });

  it("jumps pages with bare 1-6 and toggles the wall with M", async () => {
    mount("#/wall");
    await key("5");
    expect(router.getRoute()).toEqual(pageRoute("simulator"));
    await key("m");
    expect(router.getRoute()).toEqual({ view: "wall" });
    await key("m");
    expect(router.getRoute()).toEqual(pageRoute("simulator"));
  });

  it("shows the offline state once the daemon fails to answer", async () => {
    mount("#/brain/transcripts");
    await screen.findByText(/Layer 2 ·/);
    await act(async () => {
      await data.client.call("tournament.list", {});
    });
    expect(screen.getByText("The daemon is not reachable")).toBeTruthy();
  });
});

describe("the legacy desk (until cutover C2)", () => {
  it("stays reachable at #/legacy/<view>, on fixtures, with Ctrl+1..5", async () => {
    window.history.replaceState(null, "", "/?mock=1#/legacy/directives");
    mount("#/legacy/directives");
    expect(await screen.findByText("Shadowrunr · legacy desk")).toBeTruthy();
    await key("2", { ctrlKey: true });
    expect(router.getRoute()).toEqual({ view: "legacy", sub: ["simulations"] });
    await key("1"); // bare 1 still jumps to On Air
    expect(router.getRoute()).toEqual(pageRoute("onair"));
    window.history.replaceState(null, "", "/");
  });
});

describe("DOM-count budget", () => {
  it("keeps the wall plus one page under budget at every layer", async () => {
    const counts: Record<string, number> = {};
    mount("#/wall");
    counts["wall"] = nodes();
    await go("#/simulator");
    await screen.findByText(/Layer 1 ·/);
    counts["wall+l1"] = nodes();
    await go("#/simulator/tournaments");
    await screen.findByText(/Layer 2 ·/);
    counts["wall+l2"] = nodes();
    for (const [route, n] of Object.entries(counts)) {
      const budget = BUDGETS.routes[route];
      expect(budget, `budget for ${route}`).toBeDefined();
      expect(n, `${route}: ${n} nodes`).toBeLessThanOrEqual(budget ?? 0);
    }
    console.info(`dom-count: ${JSON.stringify(counts)}`);
  });
});
