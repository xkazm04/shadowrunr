// Layer 2 in jsdom over a scripted daemon: tabs, the virtualised lists at 10x, the verdict's
// quote verification, and the heatmap worst first with rate, n and bound.
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { mountPage, nodes } from "../../kit/testing";
import { pageRoute, type RouterStore } from "../../router";
import { page } from "./index";
import { Layer2 } from "./Layer2";
import { daemon, VERDICT_RUN, world, type World } from "./testgen";

let router: RouterStore | undefined;

function mount(w: World | "offline", sub: readonly string[]) {
  const m = mountPage(<Layer2 sub={sub} />, daemon(w), `#/simulator/${sub.join("/")}`);
  router = m.router;
  return m;
}

const settle = async (until: () => void) => waitFor(until, { timeout: 4000, interval: 5 });

afterEach(() => {
  cleanup();
  router?.dispose();
});

describe("simulator layer 2", () => {
  it("lists tournaments and opens one on activation", async () => {
    mount(world(3, 24), ["tournaments"]);
    const table = await screen.findByRole("table", { name: "Tournaments" });
    await settle(() => expect(within(table).getByText("t003")).toBeTruthy());
    fireEvent.click(within(table).getByText("t002"));
    await settle(() => expect(router?.getRoute()).toEqual(pageRoute("simulator", "tournaments", "t002")));
  });

  it("shows a tournament's figures and its runs worst first", async () => {
    mount(world(1, 24), ["tournaments", "t001"]);
    await settle(() => expect(screen.getByText("Assumptions the calibration carries")).toBeTruthy());
    const runs = await screen.findByRole("table", { name: "Runs of t001" });
    await settle(() => expect(within(runs).getAllByText(/^t001-r/).length).toBeGreaterThan(0));
    const first = within(runs).getAllByRole("row")[1];
    expect(first?.textContent).toContain("fail");
  });

  it("virtualises 48,000 runs: the DOM holds a screenful", async () => {
    mount(world(40, 1200), ["runs"]);
    await waitFor(() => expect(screen.getByText(/48,000 of 48,000/)).toBeTruthy(), { timeout: 25_000, interval: 20 });
    const rows = within(screen.getByRole("table", { name: "Runs" })).getAllByRole("row");
    expect(rows.length).toBeLessThan(80);
    expect(nodes()).toBeLessThan(1500);
    console.info(`dom-count simulator.l2 runs @48k: ${nodes()} nodes, ${rows.length} rows`);
    fireEvent.click(screen.getByRole("button", { name: /^Not judged/ }));
    await settle(() => expect(screen.getByText(/4,800 of 48,000/)).toBeTruthy());
  }, 30_000);

  it("verifies every judge quote against its cited line ±2", async () => {
    mount(world(1, 4), ["verdicts", VERDICT_RUN]);
    await settle(() => expect(document.querySelectorAll("[data-check]")).toHaveLength(3));
    const states = [...document.querySelectorAll("[data-check]")].map((e) => e.getAttribute("data-check"));
    expect(states).toEqual(["verified", "nearby", "not_found"]);
    expect(screen.getByText("(L4, +2)", { exact: false })).toBeTruthy();
    const line = screen.getAllByRole("link", { name: "L2" })[0];
    expect(line?.getAttribute("href")).toBe(`#/brain/transcripts/${VERDICT_RUN}/2`);
    expect(screen.getByText("stays-in-character")).toBeTruthy();
    expect(screen.getByText("Held the room.")).toBeTruthy();
  });

  it("shows a run-level fact (no line to quote) as whole run, not as a bad ref", async () => {
    mount(world(1, 4), ["verdicts", VERDICT_RUN]);
    await settle(() => expect(screen.getByText("whole run")).toBeTruthy());
    expect(screen.queryByText("bad ref")).toBeNull();
  });

  it("shows a run with no verdict as empty, not as an error", async () => {
    mount(world(1, 4), ["verdicts", "t001-r0009"]);
    await settle(() => expect(screen.getByText("This run has no verdict yet")).toBeTruthy());
  });

  it("orders heatmap cells worst first with rate, n and bound, and opens a cell", async () => {
    mount(world(2, 24), ["heatmap"]);
    const table = await screen.findByRole("table", { name: /Cells of t002, worst first/ });
    await settle(() => expect(within(table).getAllByRole("row").length).toBeGreaterThan(2));
    const bars = within(table).getAllByRole("img");
    expect(bars[0]?.getAttribute("aria-label")).toMatch(/fail rate: \d+% \(\d+%–\d+%\), n=\d+/);
    const firstRate = Number(/: (\d+)%/.exec(bars[0]?.getAttribute("aria-label") ?? "")?.[1]);
    const lastRate = Number(/: (\d+)%/.exec(bars[bars.length - 1]?.getAttribute("aria-label") ?? "")?.[1]);
    expect(firstRate).toBeGreaterThanOrEqual(lastRate);
    fireEvent.click(within(table).getAllByRole("row")[1] as HTMLElement);
    await settle(() => {
      const r = router?.getRoute();
      expect(r?.view === "page" && r.sub[0] === "heatmap" && r.sub[1]?.startsWith("t002|")).toBe(true);
    });
  });

  it("opens a heatmap cell from its key with its failing rules and runs", async () => {
    const key = "t001|a3f9c2e17b44d0e5|theatrical|haiku|SEG-01|3";
    mount(world(1, 24), ["heatmap", key]);
    await settle(() => expect(screen.getByText("Failing rules")).toBeTruthy());
    expect(screen.getByText("red-line:hateful-conduct")).toBeTruthy();
    await settle(() => expect(screen.getByRole("link", { name: "t001-r0000" })).toBeTruthy());
    expect(page.label("heatmap", 1, ["simulator", "heatmap"])).toBe("Heatmap");
    expect(page.label(key, 2, ["simulator", "heatmap", key])).toBe("theatrical · haiku · SEG-01 · a3");
    expect(page.label(VERDICT_RUN, 2, ["simulator", "verdicts", VERDICT_RUN])).toBe(VERDICT_RUN);
  });

  it("shows the offline state and an unknown sub-page as empty", async () => {
    const s = mount("offline", ["runs"]);
    await s.layer.client.call("runs.page", { limit: 1 });
    await settle(() => expect(screen.getByText("The daemon is not reachable")).toBeTruthy());
    cleanup();
    router?.dispose();
    mount(world(1, 1), ["nope"]);
    expect(screen.getByText("There is no such simulator page")).toBeTruthy();
    expect(screen.getByText("Layer 2 · The Trials")).toBeTruthy();
  });
});
