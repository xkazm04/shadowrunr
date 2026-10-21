// The shell in jsdom: wall, seven posters, every page's layer 1 and a nested layer 2, the mixer
// bus, keys, and the DOM-count budget (budgets/dom.json). Counts are gated; timings are not.
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createDataLayer, type DataLayer } from "../data";
import { contractDoor } from "../data/contractDoor";
import { fakeSockets, manualTimers, scriptedFetch } from "../data/testing";
import { createHashRouter, encodeRoute, pageRoute, type RouterStore } from "../router";
import DOM_BUDGETS from "../../budgets/dom.json";
import { App } from "./App";

const BUDGETS: { readonly allowances: { readonly shell: number; readonly motif: number }; readonly routes: Readonly<Record<string, number | undefined>> } = DOM_BUDGETS;

/** The active poster's layer-1 root once its page has rendered into it. */
const layer1Of = (id: string) => document.querySelector(`[data-role='poster'][data-page='${id}'] [data-role='layer1']`);
const waitLayer1 = (id: string) =>
  waitFor(
    () => {
      const root = layer1Of(id);
      expect(root?.childElementCount ?? 0, `${id} layer 1 rendered`).toBeGreaterThan(0);
    },
    { timeout: 4000 },
  );
/** The open spread once a page's layer 2 has rendered into it. */
const waitLayer2 = () =>
  waitFor(
    () => {
      const spread = document.querySelector("[data-role='spread'][data-open='true']");
      expect(spread?.querySelector("[data-role$='layer2'], [data-role$='-l2']"), "layer 2 rendered").not.toBeNull();
    },
    { timeout: 4000 },
  );

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
  it("renders the wall with all seven posters and no bus", () => {
    mount("#/wall");
    const posters = document.querySelectorAll("[data-role='poster']");
    expect([...posters].map((p) => p.getAttribute("data-page"))).toEqual(["onair", "characters", "brain", "segments", "simulator", "review", "reconcile"]);
    expect(document.querySelector("[data-role='mixer-bus']")).toBeNull();
    expect(screen.getByRole("status").textContent).toContain("Connecting");
  });

  it("renders every page's layer 1 and the mixer bus with the tally on it", async () => {
    mount("#/wall");
    for (const id of ["onair", "characters", "brain", "segments", "simulator", "review", "reconcile"] as const) {
      await go(encodeRoute(pageRoute(id)));
      await waitLayer1(id);
      const active = document.querySelector(`[data-role='poster'][data-page='${id}']`);
      expect(active?.getAttribute("data-active")).toBe("true");
      const channels = document.querySelectorAll("[data-role='bus-channel']");
      expect(channels).toHaveLength(7);
      expect([...channels].filter((c) => c.getAttribute("aria-current") === "page").map((c) => c.getAttribute("data-page"))).toEqual([id]);
    }
  });

  it("renders a nested layer 2 per page, with breadcrumbs from ids", async () => {
    mount("#/wall");
    const nested = { onair: "rundown", characters: "malakor", brain: "transcripts", segments: "tables", simulator: "tournaments", reconcile: "plan" };
    for (const [id, seg] of Object.entries(nested)) {
      await go(`#/${id}/${seg}`);
      await waitLayer2();
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

  it("jumps pages with bare 1-7 and toggles the wall with M", async () => {
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
    await waitLayer2();
    await act(async () => {
      await data.client.call("tournament.list", {});
    });
    expect(screen.getByText("The daemon is not reachable")).toBeTruthy();
  });
});

describe("retired routes (cutover C2)", () => {
  it("sends a hash naming no page to the wall, where Ctrl+digit is unbound and bare digits still work", async () => {
    mount("#/desk/directives");
    expect(router.getRoute()).toEqual({ view: "wall" });
    await key("2", { ctrlKey: true });
    expect(router.getRoute()).toEqual({ view: "wall" });
    await key("1");
    expect(router.getRoute()).toEqual(pageRoute("onair"));
  });
});

describe("DOM-count budget", () => {
  it("keeps the wall plus one page under budget at every layer", async () => {
    const counts: Record<string, number> = {};
    mount("#/wall");
    counts["wall"] = nodes();
    // The wall's line is the shell plus one motif allowance per poster; every motif keeps to it.
    expect(BUDGETS.routes["wall"]).toBe(BUDGETS.allowances.shell + 7 * BUDGETS.allowances.motif);
    for (const m of document.querySelectorAll("[data-role$='-motif']")) {
      const n = m.querySelectorAll("*").length + 2; // the art, its root and the face's motif box
      expect(n, `${m.getAttribute("data-role")}: ${n} nodes`).toBeLessThanOrEqual(BUDGETS.allowances.motif);
    }
    await go("#/simulator");
    await waitLayer1("simulator");
    counts["wall+l1"] = nodes();
    await go("#/simulator/tournaments");
    await waitLayer2();
    counts["wall+l2"] = nodes();
    // The freshness strip is never part of the counted path: QueryView adds no node unless the feed cannot vouch.
    expect(document.querySelectorAll("[data-role='query-freshness']")).toHaveLength(0);
    for (const [route, n] of Object.entries(counts)) {
      const budget = BUDGETS.routes[route];
      expect(budget, `budget for ${route}`).toBeDefined();
      expect(n, `${route}: ${n} nodes`).toBeLessThanOrEqual(budget ?? 0);
    }
    console.info(`dom-count: ${JSON.stringify(counts)}`);
  });
});
