// Brain's letterbox bands in jsdom (no layout, so the boxes are stubbed): the winner's motif fills
// the bands when the hero is taller than A/1's 1600:810 scene, nothing shows without a band, and
// the scene keeps its box (it stays the hero's only grid item; the bands are absolutely placed
// siblings). The real boxes are proven by the fidelity gate (brain-a1 roles, brain-art roles).
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PERF from "../../../budgets/perf.json";
import { mountPage as mount, nodes, role, roles, settle } from "../../kit/testing";
import { Layer1 } from "./Layer1";
import { daemon, Recorder, season } from "./testgen";

/** The boxes a 1440 x 1000 poster lays out: a 1066 px hero, 850 px tall, around a 1600:810 scene. */
function boxes(hero: { w: number; h: number }, scene: { w: number; h: number }) {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const r = this.getAttribute("data-role");
    const b = r === "brain-hero" ? hero : r === "brain-scene" ? scene : { w: 0, h: 0 };
    return { x: 0, y: 0, left: 0, top: 0, right: b.w, bottom: b.h, width: b.w, height: b.h, toJSON: () => ({}) };
  });
}

const SCENE = { w: 1066, h: (1066 * 810) / 1600 };

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(() => new Recorder() as never);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("brain letterbox bands", () => {
  it("fills both bands with the winner's motif when the hero is taller than the scene", async () => {
    boxes({ w: 1066, h: 850 }, SCENE);
    mount(<Layer1 active />, daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 20, transcriptLines: 50 })), "#/brain");
    await settle(8);
    const top = role("brain-band-top");
    const bottom = role("brain-band-bottom");
    expect(top?.getAttribute("aria-hidden")).toBe("true");
    expect(bottom?.getAttribute("aria-hidden")).toBe("true");
    // Each band is half of what the scene leaves: (850 - 539.66) / 2 = 155.17 px = 9.698 rem at 16 px.
    expect((top as HTMLElement | null)?.style.height).toBe("9.698rem");
    expect(top?.querySelector("svg")?.getAttribute("preserveAspectRatio")).toBe("xMidYMax slice");
    expect(bottom?.querySelector("svg")?.getAttribute("preserveAspectRatio")).toBe("xMidYMin slice");
    expect(role("brain-band-rings")?.querySelectorAll("circle")).toHaveLength(4);
    expect(role("brain-band-pulse")).not.toBeNull();
    expect(roles("brain-band-art")).toHaveLength(1);
  });

  it("leaves the scene's box alone: the scene stays the hero's only in-flow child", async () => {
    boxes({ w: 1066, h: 850 }, SCENE);
    mount(<Layer1 active />, daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 20, transcriptLines: 50 })), "#/brain");
    await settle(8);
    const hero = role("brain-hero");
    const scene = role("brain-scene") as HTMLElement | null;
    expect(scene?.parentElement).toBe(hero);
    expect(scene?.getAttribute("style")).toBeNull();
    expect(scene?.contains(role("brain-band-top"))).toBe(false);
    expect([...(hero?.children ?? [])].map((c) => c.getAttribute("data-role"))).toEqual(["brain-band-top", "brain-band-bottom", "brain-scene"]);
  });

  it("shows no art without a band (the scene fills the hero) or with only a sliver", async () => {
    boxes({ w: 1066, h: 540 }, { w: 1066, h: 540 });
    mount(<Layer1 active />, daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 20, transcriptLines: 50 })), "#/brain");
    await settle(8);
    expect(role("brain-band-top")).toBeNull();
    expect(role("brain-scene")).not.toBeNull();
    cleanup();
    vi.restoreAllMocks();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(() => new Recorder() as never);
    boxes({ w: 1066, h: SCENE.h + 40 }, SCENE);
    mount(<Layer1 active />, daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 20, transcriptLines: 50 })), "#/brain");
    await settle(8);
    expect(role("brain-band-top")).toBeNull();
  });

  it("stays inside the brain.l1 DOM line with the art on", async () => {
    boxes({ w: 1066, h: 850 }, SCENE);
    mount(<Layer1 active />, daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 20, transcriptLines: 50 })), "#/brain");
    await settle(8);
    const art = [role("brain-band-top"), role("brain-band-bottom")].reduce((n, b) => n + (b ? b.querySelectorAll("*").length + 1 : 0), 0);
    expect(art).toBe(16);
    expect(nodes()).toBeLessThanOrEqual(PERF.dom["brain.l1"].max);
  });
});
