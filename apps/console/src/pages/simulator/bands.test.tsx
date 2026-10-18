// Simulator's letterbox bands in jsdom (no layout, so the boxes are stubbed): the winner's motif
// fills the bands when the hero is taller than A/1's 1600:810 scene, nothing shows without a band
// or when the screen stacks, and the scene keeps its box. The real boxes are proven by the
// fidelity gate (simulator-a1 roles, simulator-art roles).
import { cleanup, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import PERF from "../../../budgets/perf.json";
import { mountPage, nodes, role } from "../../kit/testing";
import { Layer1 } from "./Layer1";
import { daemon, world } from "./testgen";

/** Stubs the frame (the screen's parent) and the screen boxes; everything else is 0 x 0. */
function boxes(frame: { w: number; h: number }, scene: { w: number; h: number }) {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const isFrame = this.querySelector(":scope > [data-role='simulator-screen']") !== null;
    const b = isFrame ? frame : this.getAttribute("data-role") === "simulator-screen" ? scene : { w: 0, h: 0 };
    return { x: 0, y: 0, left: 0, top: 0, right: b.w, bottom: b.h, width: b.w, height: b.h, toJSON: () => ({}) };
  });
}

const SCENE = { w: 1066, h: (1066 * 810) / 1600 };
let dispose: (() => void) | undefined;

async function mount() {
  const m = mountPage(<Layer1 active />, daemon(world(4, 24)), "#/simulator");
  dispose = m.dispose;
  await waitFor(() => expect(screen.getByLabelText("Simulator summary").textContent).toContain("96runs"), { timeout: 4000, interval: 5 });
}

afterEach(() => {
  cleanup();
  dispose?.();
  vi.restoreAllMocks();
});

describe("simulator letterbox bands", () => {
  it("fills both bands with the winner's motif, beside the scene, never in it", async () => {
    boxes({ w: 1066, h: 850 }, SCENE);
    await mount();
    const top = role("simulator-band-top");
    expect(top?.getAttribute("aria-hidden")).toBe("true");
    expect(role("simulator-band-bottom")?.getAttribute("aria-hidden")).toBe("true");
    expect(role("simulator-band-dots")?.querySelectorAll("circle")).toHaveLength(15);
    expect(role("simulator-band-core")?.getAttribute("r")).toBe("20");
    const scene = role("simulator-screen");
    expect(scene?.contains(top)).toBe(false);
    expect(top?.parentElement).toBe(scene?.parentElement);
    expect(scene?.getAttribute("style")).toBeNull();
    const art = [top, role("simulator-band-bottom")].reduce((n, b) => n + (b ? b.querySelectorAll("*").length + 1 : 0), 0);
    expect(art).toBe(22);
    expect(nodes()).toBeLessThanOrEqual(PERF.dom["simulator.l1"].max);
  });

  it("shows no art without a band", async () => {
    boxes({ w: 1066, h: SCENE.h }, SCENE);
    await mount();
    expect(role("simulator-band-top")).toBeNull();
  });

  it("shows no art when the screen stacks and fills a narrow hero", async () => {
    boxes({ w: 480, h: 900 }, { w: 480, h: 900 * 0.5 });
    await mount();
    expect(role("simulator-band-top")).toBeNull();
  });
});
