// Layer 1 in jsdom over a scripted daemon: the grid's data at 1x and 10x, its HUD, and its
// offline and empty states. The canvas holds the runs, so the DOM count does not grow with
// scale (counts are gated, timings are not).
import { act, cleanup, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { mountPage, nodes } from "../../kit/testing";
import { Layer1 } from "./Layer1";
import { daemon, world, type World } from "./testgen";

let dispose: (() => void) | undefined;

function mount(w: World | "offline", node: ReactNode = <Layer1 active />) {
  const m = mountPage(node, daemon(w), "#/simulator");
  dispose = m.dispose;
  return m;
}

const settle = async (until: () => void, timeout = 4000) => waitFor(until, { timeout, interval: 5 });

afterEach(() => {
  cleanup();
  dispose?.();
});

describe("simulator layer 1", () => {
  it("aggregates the tournaments and their runs into the HUD", async () => {
    const s = mount(world(4, 24));
    await settle(() => expect(screen.getByLabelText("Simulator summary").textContent).toContain("96runs"));
    const hud = screen.getByLabelText("Simulator summary").textContent ?? "";
    expect(hud).toContain("4 tournaments");
    expect(hud).toMatch(/\d+ red-line fails/);
    expect(hud).not.toContain("loading");
    expect(document.querySelector("[data-role='simulator-grid']")?.getAttribute("aria-label")).toContain("4 tournaments");
    for (const role of ["simulator-l1", "simulator-screen", "simulator-grid", "simulator-marquee", "simulator-hud", "simulator-legend", "simulator-heading"]) {
      expect(document.querySelector(`[data-role='${role}']`), role).not.toBeNull();
    }
    // tournaments.page once, then runs.page per tournament: each run row carries its own
    // health, so the grid reads no tournament.results.
    expect(s.calls.filter((m) => m === "tournaments.page")).toHaveLength(1);
    expect(s.calls.filter((m) => m === "runs.page")).toHaveLength(4);
    expect(s.calls.filter((m) => m === "tournament.results")).toHaveLength(0);
    expect(screen.getByText("Open the trials").closest("a")?.getAttribute("href")).toBe("#/simulator/tournaments");
  });

  it("holds at 10x (40 tournaments x 1,200 runs, paged at 500) with a constant DOM", async () => {
    mount(world(4, 120));
    await settle(() => expect(screen.getByLabelText("Simulator summary").textContent).toContain("480runs"));
    const small = nodes();
    cleanup();
    dispose?.();
    const s = mount(world(40, 1200));
    await settle(() => expect(screen.getByLabelText("Simulator summary").textContent).toContain("48,000runs"), 15_000);
    expect(screen.getByLabelText("Simulator summary").textContent).toContain("40 tournaments");
    // Each tournament's 1,200 runs arrive in three pages of 500.
    expect(s.calls.filter((m) => m === "runs.page")).toHaveLength(120);
    const big = nodes();
    expect(big).toBe(small);
    expect(big).toBeLessThan(120);
    console.info(`dom-count simulator.l1: ${big} nodes at 1x and 10x`);
  }, 20_000);

  it("shows the latest 40 of more tournaments and says so", async () => {
    mount(world(45, 2));
    await settle(() => expect(screen.getByText(/Latest 40 of 45/)).toBeTruthy());
    expect(screen.getByLabelText("Simulator summary").textContent).toContain("40 tournaments");
  });

  it("has an empty state and an offline state", async () => {
    mount(world(0, 0));
    await settle(() => expect(screen.getByText("No tournaments yet")).toBeTruthy());
    cleanup();
    dispose?.();
    const s = mount("offline");
    await act(async () => {
      await s.layer.client.call("tournaments.page", { limit: 1 });
    });
    await settle(() => expect(screen.getByText("The daemon is not reachable")).toBeTruthy());
    expect(document.querySelector("[data-role='simulator-grid']")).not.toBeNull();
  });

  it("renders nothing while hidden behind layer 2", () => {
    mount(world(1, 1), <Layer1 active={false} />);
    expect(document.querySelector("[data-role='simulator-l1']")).toBeNull();
  });
});
