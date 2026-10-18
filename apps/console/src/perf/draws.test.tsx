// Gated: canvas draw passes per input change for Brain and Simulator at the 10x fixture
// (budgets/perf.json `draws`): exactly one pass per input change, none while idle over
// `idleTicks` ticks of the data layer's timers (the backstop included) and real time.
import { cleanup, fireEvent } from "@testing-library/react";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Layer1 as BrainL1 } from "../pages/brain/Layer1";
import { season } from "../pages/brain/testgen";
import { gridLayout, gridRect, tileOrigin } from "../pages/simulator/grid";
import { Layer1 as SimulatorL1 } from "../pages/simulator/Layer1";
import { world } from "../pages/simulator/testgen";
import { brain10x, brainScript, SCALE, simGrid, simScript } from "./fixture";
import { BUDGETS, frame, idle, line, mountPerf, quiet, type Mounted } from "./harness";

/** A 2D context that records every call by name (canvas passes are counted on it). */
function recordingContext(calls: Map<string, number>): CanvasRenderingContext2D {
  const gradient = { addColorStop: () => undefined };
  const store = new Map<PropertyKey, unknown>();
  const target = {};
  const ctx = new Proxy(target, {
    get(_t, prop) {
      if (store.has(prop)) return store.get(prop);
      if (prop === "createRadialGradient" || prop === "createLinearGradient") return () => gradient;
      return (..._args: unknown[]) => {
        calls.set(String(prop), (calls.get(String(prop)) ?? 0) + 1);
      };
    },
    set(_t, prop, value) {
      store.set(prop, value);
      return true;
    },
  });
  return ctx as CanvasRenderingContext2D;
}

let mounted: Mounted | undefined;
let calls = new Map<string, number>();
const W = 1200;
const H = 700;

beforeEach(() => {
  calls = new Map();
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(() => recordingContext(calls) as never);
});
afterEach(() => {
  mounted?.dispose();
  mounted = undefined;
  cleanup();
  vi.restoreAllMocks();
});

const draws = (role: string) => Number(document.querySelector(`[data-role='${role}']`)?.getAttribute("data-draws") ?? "0");
const passes = () => calls.get("clearRect") ?? 0;

/** Pushes one live event through the fake `/events` socket. */
async function push(m: Mounted, seq: number, event: unknown): Promise<void> {
  await act(async () => {
    m.socket().push(frame(seq, event));
  });
}

describe("canvas draw passes per input change at 10x", () => {
  it("brain: one pass per canvas per data change, none while idle", async () => {
    const script = brainScript(brain10x());
    const m = (mounted = mountPerf(<BrainL1 active />, script, "#/brain"));
    await quiet(m, 400);
    await act(async () => m.socket().open());
    const scope = line(BUDGETS.draws, "brain.scope");
    const memory = line(BUDGETS.draws, "brain.memory");
    const before = [draws("brain-scope-canvas"), draws("brain-memory-canvas")];
    expect(before[0], "the scope drew its first pass").toBeGreaterThan(0);

    await idle(m, BUDGETS.idleTicks);
    const idleDelta = [draws("brain-scope-canvas") - (before[0] ?? 0), draws("brain-memory-canvas") - (before[1] ?? 0)];
    console.info(`perf draws brain idle over ${BUDGETS.idleTicks} ticks: scope ${idleDelta[0]}, memory ${idleDelta[1]}`);
    expect(idleDelta[0], "brain.scope passes while idle").toBeLessThanOrEqual(scope.idle);
    expect(idleDelta[1], "brain.memory passes while idle").toBeLessThanOrEqual(memory.idle);

    // One input change: the newest tournament grows by one run; a stream_end invalidates its reads.
    const at = [draws("brain-scope-canvas"), draws("brain-memory-canvas")];
    script.serve(season({ tournaments: SCALE.tournaments, runs: SCALE.runsPerTournament10x + 1, rowsPerRun: SCALE.ledgerRowsPerRun, transcriptLines: SCALE.brainSampleLines }));
    await push(m, 1, { type: "stream_end", ts_ms: 1, reason: "sim_complete" });
    await quiet(m, 400);
    const change = [draws("brain-scope-canvas") - (at[0] ?? 0), draws("brain-memory-canvas") - (at[1] ?? 0)];
    console.info(`perf draws brain per data change: scope ${change[0]}, memory ${change[1]}`);
    expect(change[0], "brain.scope passes per data change").toBeLessThanOrEqual(scope.perChange);
    expect(change[1], "brain.memory passes per data change").toBeLessThanOrEqual(memory.perChange);
    expect(Math.min(change[0] ?? 0, change[1] ?? 0), "brain: both canvases redrew the change").toBeGreaterThan(0);

    // The same event again over unchanged data: what a refetch of identical replies costs.
    const was = [draws("brain-scope-canvas"), draws("brain-memory-canvas")];
    await push(m, 2, { type: "stream_end", ts_ms: 2, reason: "sim_complete" });
    await quiet(m, 400);
    const same = [draws("brain-scope-canvas") - (was[0] ?? 0), draws("brain-memory-canvas") - (was[1] ?? 0)];
    console.info(`perf draws brain per refetch of identical data: scope ${same[0]}, memory ${same[1]}`);
    expect(same[0], "brain.scope passes per identical refetch").toBeLessThanOrEqual(scope.sameData ?? 0);
    expect(same[1], "brain.memory passes per identical refetch").toBeLessThanOrEqual(memory.sameData ?? 0);
  }, 120_000);

  it("simulator: one pass per hover change and per data change, none for the same tile or while idle", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(DOMRect.fromRect({ x: 0, y: 0, width: W, height: H }));
    let w = simGrid();
    const m = (mounted = mountPerf(<SimulatorL1 active />, (method, sent) => simScript(w)(method, sent), "#/simulator"));
    await quiet(m, 400);
    await act(async () => m.socket().open());
    const hover = line(BUDGETS.draws, "simulator.hover");
    const data = line(BUDGETS.draws, "simulator.data");
    const canvas = document.querySelector("[data-role='simulator-grid']");
    if (!canvas) throw new Error("no simulator canvas");
    expect(passes(), "the grid drew its first pass").toBeGreaterThan(0);

    const area = gridRect(W, H, false);
    const l = gridLayout(area.w, area.h);
    // The canvas covers the grid's box (A/1's structure), and every rect is mocked at the origin,
    // so a tile's centre is in canvas-local coordinates.
    const centre = (i: number) => {
      const o = tileOrigin(l, i);
      return { clientX: o.ox + l.tw / 2, clientY: o.oy + l.th / 2 };
    };

    let p = passes();
    await idle(m, BUDGETS.idleTicks);
    const idleDelta = passes() - p;
    expect(idleDelta, "simulator passes while idle").toBeLessThanOrEqual(hover.idle);

    p = passes();
    fireEvent.mouseMove(canvas, centre(0));
    const onTile = passes() - p;
    p = passes();
    fireEvent.mouseMove(canvas, { ...centre(0), clientX: centre(0).clientX + 1 });
    const sameTile = passes() - p;
    p = passes();
    fireEvent.mouseMove(canvas, centre(1));
    const nextTile = passes() - p;
    console.info(`perf draws simulator idle ${idleDelta}, hover tile 0 ${onTile}, same tile ${sameTile}, tile 1 ${nextTile}`);
    expect(onTile, "simulator passes per hover change").toBe(hover.perChange);
    expect(nextTile, "simulator passes per hover change").toBe(hover.perChange);
    expect(sameTile, "simulator passes for a move inside the same tile").toBe(0);

    // One input change in the data: a stream_end invalidates the pages; one more tournament arrives.
    fireEvent.mouseLeave(canvas);
    await quiet(m, 50);
    w = world(SCALE.tournaments + 1, SCALE.runsPerTournament);
    p = passes();
    await push(m, 1, { type: "stream_end", ts_ms: 1, reason: "sim_complete" });
    await quiet(m, 400);
    const change = passes() - p;
    console.info(`perf draws simulator per data change: ${change}`);
    expect(change, "simulator passes per data change").toBeLessThanOrEqual(data.perChange);
    expect(change, "simulator: the grid redrew the change").toBeGreaterThan(0);

    p = passes();
    await push(m, 2, { type: "stream_end", ts_ms: 2, reason: "sim_complete" });
    await quiet(m, 400);
    const same = passes() - p;
    console.info(`perf draws simulator per refetch of identical data: ${same}`);
    expect(same, "simulator passes per identical refetch").toBeLessThanOrEqual(data.sameData ?? 0);
  }, 120_000);
});
