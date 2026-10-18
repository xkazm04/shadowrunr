// The canvas scene at A/1's scale (40 x 120) and at 10x (40 x 1,200 = 48k runs), on a
// recording context: counts are asserted (one rect per run, one dot per red line), not timings.
import { describe, expect, it } from "vitest";
import { drawGrid, tileLabel, type SceneContext } from "./draw";
import { gridLayout, type Tile } from "./grid";
import { TOKENS_SIM } from "@shadowrunr/tokens";
import { healthColor } from "./ramp";

const SIM_INK = TOKENS_SIM;

interface Op {
  readonly op: string;
  readonly fill: unknown;
  readonly args: readonly unknown[];
}

function recorder(): SceneContext & { ops: Op[] } {
  const ops: Op[] = [];
  let fill: unknown = "";
  const rec =
    (op: string) =>
    (...args: unknown[]) => {
      ops.push({ op, fill, args });
    };
  const gradient = { addColorStop: rec("stop") };
  return {
    ops,
    get fillStyle() {
      return fill as string;
    },
    set fillStyle(v: string | CanvasGradient | CanvasPattern) {
      fill = v;
    },
    font: "",
    clearRect: rec("clearRect"),
    fillRect: rec("fillRect"),
    beginPath: rec("beginPath"),
    arc: rec("arc"),
    fill: rec("fill"),
    fillText: rec("fillText"),
    save: rec("save"),
    restore: rec("restore"),
    translate: rec("translate"),
    scale: rec("scale"),
    rect: rec("rect"),
    clip: rec("clip"),
    createRadialGradient: (...args: unknown[]) => {
      ops.push({ op: "radial", fill, args });
      return gradient as CanvasGradient;
    },
  };
}

function tiles(t: number, perT: number): Tile[] {
  return Array.from({ length: t }, (_, i) => ({
    id: `scaled-t${String(i + 1).padStart(2, "0")}`,
    health: (i % 10) / 10,
    red: 1,
    declared: perT,
    runs: Array.from({ length: perT }, (_, k) => ({ runId: `r${k}`, health: k % 7 === 0 ? undefined : (k % 11) / 10, red: k === 3 })),
  }));
}

const area = { x: 32, y: 105, w: 1184, h: 680 };

describe("drawGrid", () => {
  it("draws A/1's 40 x 120: one tile rect, one rect per run, one dot per red line, one label", () => {
    const c = recorder();
    const ts = tiles(40, 120);
    drawGrid(c, ts, gridLayout(area.w, area.h), area, 5);
    expect(c.ops.filter((o) => o.op === "fillRect")).toHaveLength(40 + 40 * 120);
    expect(c.ops.filter((o) => o.op === "arc")).toHaveLength(40);
    const labels = c.ops.filter((o) => o.op === "fillText");
    expect(labels).toHaveLength(40);
    expect(labels[0]?.args[0]).toBe("T01 · 0%");
    expect(labels[5]?.fill).toBe(SIM_INK.labelHover);
    expect(labels[6]?.fill).toBe(SIM_INK.label);
    const tileRects = c.ops.filter((o) => o.op === "fillRect" && (o.fill === SIM_INK.tile || o.fill === SIM_INK.tileHover));
    expect(tileRects.filter((o) => o.fill === SIM_INK.tileHover)).toHaveLength(1);
    // A run's colour is A/1's ramp at the still alpha 0.35 + 0.6h; unjudged runs are unlit.
    const runOps = c.ops.filter((o) => o.op === "fillRect").slice(1, 121);
    expect(runOps[1]?.fill).toBe(healthColor(0.1, 0.35 + 0.6 * 0.1));
    expect(runOps[0]?.fill).toBe(SIM_INK.unjudged);
    // A/1 geometry: a 12-wide grid under a 16 px label band, cells inset by half a pixel.
    const l = gridLayout(area.w, area.h);
    expect(runOps[13]?.args[0]).toBeCloseTo(area.x + (l.tw / 12) * 1 + 0.5);
    expect(runOps[13]?.args[1]).toBeCloseTo(area.y + 16 + ((l.th - 16) / 10) * 1 + 0.5);
  });

  it("holds at 10x: 40 tournaments x 1,200 runs is still one rect per run", () => {
    const c = recorder();
    drawGrid(c, tiles(40, 1200), gridLayout(area.w, area.h), area, -1);
    expect(c.ops.filter((o) => o.op === "fillRect")).toHaveLength(40 + 48_000);
    expect(c.ops.filter((o) => o.op === "fillText")).toHaveLength(40);
    // Runs share their cell colours: a handful of distinct fills over 48k rects.
    const fills = new Set(c.ops.filter((o) => o.op === "fillRect").map((o) => o.fill));
    expect(fills.size).toBeLessThan(20);
  });

  it("labels like A/1 and prints a dash before any verdict", () => {
    expect(tileLabel({ id: "tones-2026-10-17", health: 0.874, red: 0, declared: 0, runs: [] })).toBe("TONES-2026-10-17 · 87%");
    expect(tileLabel({ id: "pilot", health: undefined, red: 0, declared: 0, runs: [] })).toBe("PILOT · –");
  });
});
