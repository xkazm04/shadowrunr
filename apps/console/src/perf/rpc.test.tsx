// Gated: RPC calls on first paint per page at the 10x fixture (budgets/perf.json `rpc`). Every
// call a layer 1 makes until it holds still is tallied by method; a method over its line, or
// one the line does not list, fails. Counts, never timings.
import { cleanup } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Script } from "../kit/testing";
import { Layer1 as BrainL1 } from "../pages/brain/Layer1";
import { Recorder } from "../pages/brain/testgen";
import { Layer1 as CharactersL1 } from "../pages/characters/Layer1";
import { Layer1 as OnAirL1 } from "../pages/onair/Layer1";
import { Layer1 as ReconcileL1 } from "../pages/reconcile/Layer1";
import { session } from "../pages/reconcile/session";
import { Layer1 as SegmentsL1 } from "../pages/segments/Layer1";
import { Layer1 as SimulatorL1 } from "../pages/simulator/Layer1";
import { brain10x, brainScript, charactersScript, onairScript, reconcileScript, segmentsScript, simGrid, simScript } from "./fixture";
import { BUDGETS, line, mountPerf, overBudget, quiet, tally, type Mounted } from "./harness";

let mounted: Mounted | undefined;
beforeEach(() => {
  session.reset();
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(() => new Recorder() as never);
});
afterEach(() => {
  mounted?.dispose();
  mounted = undefined;
  cleanup();
  vi.restoreAllMocks();
});

async function gate(name: string, ui: ReactNode, script: Script, hash: string): Promise<void> {
  mounted = mountPerf(ui, script, hash);
  await quiet(mounted, 400);
  console.info(`perf rpc ${name}: ${JSON.stringify(tally(mounted.calls))}`);
  const budget = line(BUDGETS.rpc, name).calls;
  expect(overBudget(mounted.calls, budget), `${name}: RPC calls on first paint`).toEqual([]);
}

describe("RPC calls on first paint per page at 10x", () => {
  it("onair.l1", async () => {
    await gate("onair.l1", <OnAirL1 active />, onairScript(), "#/onair");
  }, 60_000);

  it("characters.l1", async () => {
    await gate("characters.l1", <CharactersL1 active />, charactersScript(), "#/characters");
  });

  it("brain.l1", async () => {
    await gate("brain.l1", <BrainL1 active />, brainScript(brain10x()), "#/brain");
  }, 60_000);

  it("segments.l1", async () => {
    await gate("segments.l1", <SegmentsL1 active />, segmentsScript(), "#/segments");
  });

  it("simulator.l1: the grid reads runs.page per tournament and no tournament.results", async () => {
    await gate("simulator.l1", <SimulatorL1 active />, simScript(simGrid()), "#/simulator");
  }, 60_000);

  it("reconcile.l1", async () => {
    await gate("reconcile.l1", <ReconcileL1 active />, reconcileScript(), "#/reconcile");
  });
});
