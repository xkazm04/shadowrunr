import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { layoutPosters, POSTER_REM, ROOT_PX, rootPx, type LayoutInput } from "./layout";
import { BUS_REM } from "./MixerBus";
import { METRICS } from "./Wall";

const base: LayoutInput = {
  view: "wall",
  count: 6,
  active: 0,
  hover: null,
  rail: null,
  width: 1280,
  height: 800,
  rem: 16,
  narrow: false,
  busRem: 4,
};

describe("poster layout", () => {
  it("splits the wall evenly, and gives a hovered poster 60%", () => {
    expect(layoutPosters(base, METRICS).rects.map((r) => r.width)).toEqual(Array(6).fill(1280 / 6));
    const hovered = layoutPosters({ ...base, hover: 2 }, METRICS).rects;
    expect(hovered[2]?.width).toBeCloseTo(768);
    expect(hovered[0]?.width).toBeCloseTo(102.4);
    expect(hovered.reduce((s, r) => s + r.width, 0)).toBeCloseTo(1280);
  });

  it("packs the other posters into a left rail on layer 1, above the bus", () => {
    const l = layoutPosters({ ...base, view: "l1", active: 3 }, METRICS);
    const strip = METRICS.stripRem * 16;
    expect(l.rects[3]).toEqual({ left: 5 * strip, top: 0, width: 1280 - 5 * strip, height: 800 - 64 });
    expect(l.rects.filter((_, i) => i !== 3).map((r) => r.left)).toEqual([0, 1, 2, 3, 4].map((k) => k * strip));
    expect(l.bodyWidth).toBe(1280 - 5 * strip);
  });

  it("opens a rail strip and shifts the ones after it", () => {
    const l = layoutPosters({ ...base, view: "l1", active: 0, rail: 2 }, METRICS);
    const strip = METRICS.stripRem * 16;
    const peek = METRICS.railPeekRem * 16;
    expect(l.rects[2]?.width).toBe(strip + peek);
    expect(l.rects[3]?.left).toBe(2 * strip + peek);
  });

  it("compresses the active poster into the spine on layer 2", () => {
    const l = layoutPosters({ ...base, view: "l2", active: 1 }, METRICS);
    expect(l.rects[1]?.width).toBe(METRICS.spineRem * 16);
    expect(l.rects.filter((_, i) => i !== 1).every((r) => r.width === 0)).toBe(true);
  });

  it("stacks on narrow screens and docks tabs on layer 1", () => {
    const wall = layoutPosters({ ...base, narrow: true, width: 400 }, METRICS).rects;
    expect(wall.every((r) => r.width === 400)).toBe(true);
    const l1 = layoutPosters({ ...base, narrow: true, width: 400, view: "l1" }, METRICS).rects;
    expect(l1.slice(1).every((r) => r.width === 80)).toBe(true);
  });
});

describe("the root size (app/global.css)", () => {
  // Vitest runs from the console package root.
  const css = readFileSync(join(process.cwd(), "src/app/global.css"), "utf8");

  it("keeps the height above the full mixer bus at the winner's 50rem", () => {
    for (const [w, h] of [
      [1600, 1000],
      [1920, 1080],
      [1280, 800],
    ] as const) {
      const r = rootPx(w, h, false, BUS_REM.full);
      const l = layoutPosters({ ...base, view: "l1", width: w, height: h, rem: r, busRem: BUS_REM.full }, METRICS);
      expect(l.stageHeight / r, `${w}x${h}`).toBeCloseTo(POSTER_REM, 5);
    }
    // Width-bound and clamped screens keep the clamp.
    expect(rootPx(1000, 1000, false, BUS_REM.full)).toBe(14);
    expect(rootPx(3840, 2160, false, BUS_REM.full)).toBe(30.4);
    expect(rootPx(700, 900, true, BUS_REM.full)).toBe(16);
  });

  it("is the formula global.css writes", () => {
    expect(css).toContain(`--sr-root-size: clamp(${ROOT_PX.min}px, min(1.25vw, calc(100vh / ${POSTER_REM + BUS_REM.full})), ${ROOT_PX.max}px);`);
    expect(css).toContain(`--sr-root-size: ${ROOT_PX.narrow}px;`);
  });
});
