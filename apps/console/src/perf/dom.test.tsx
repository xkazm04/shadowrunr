// Gated: DOM nodes per layer 1 and per main layer-2 list at the 10x fixture (budgets/perf.json
// `dom`). Counts, never timings. Each layer is mounted alone on the data-layer test double.
import { cleanup } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nodes } from "../kit/testing";
import { Layer1 as BrainL1 } from "../pages/brain/Layer1";
import { Layer2 as BrainL2 } from "../pages/brain/Layer2";
import { Recorder } from "../pages/brain/testgen";
import { Layer1 as CharactersL1 } from "../pages/characters/Layer1";
import { Layer2 as CharactersL2 } from "../pages/characters/Layer2";
import { Layer1 as OnAirL1 } from "../pages/onair/Layer1";
import { Layer2 as OnAirL2 } from "../pages/onair/Layer2";
import { Layer1 as ReconcileL1 } from "../pages/reconcile/Layer1";
import { Layer2 as ReconcileL2 } from "../pages/reconcile/Layer2";
import { session } from "../pages/reconcile/session";
import { Layer1 as SegmentsL1 } from "../pages/segments/Layer1";
import { Layer2 as SegmentsL2 } from "../pages/segments/Layer2";
import { Layer1 as SimulatorL1 } from "../pages/simulator/Layer1";
import { Layer2 as SimulatorL2 } from "../pages/simulator/Layer2";
import type { Script } from "../kit/testing";
import { genTables } from "../pages/segments/testgen";
import {
  BRAIN_BIG_RUN,
  brain10x,
  brainBigTranscript,
  brainRuns10x,
  brainScript,
  charactersScript,
  onairScript,
  reconcileScript,
  REPORT_10X,
  segmentsScript,
  sim10x,
  simScript,
} from "./fixture";
import { BUDGETS, line, mountPerf, quiet, type Mounted } from "./harness";

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

/** Mounts, settles, and checks the DOM count against its line; returns the count. */
async function gate(name: string, ui: ReactNode, script: Script, hash: string, ready?: () => boolean): Promise<number> {
  mounted = mountPerf(ui, script, hash);
  await quiet(mounted, 400);
  if (ready) expect(ready(), `${name}: the 10x content did not arrive`).toBe(true);
  const n = nodes();
  console.info(`perf dom ${name}: ${n} nodes (max ${line(BUDGETS.dom, name).max})`);
  expect(n, `${name}: DOM nodes at 10x`).toBeLessThanOrEqual(line(BUDGETS.dom, name).max);
  return n;
}

const has = (sel: string) => () => document.querySelector(sel) !== null;
const text = (re: RegExp) => () => re.test(document.body.textContent ?? "");

describe("DOM nodes per layer 1 at 10x", () => {
  it("onair.l1: the newest of 48k runs, a 100k-line transcript", async () => {
    await gate("onair.l1", <OnAirL1 active />, onairScript(), "#/onair", text(/Replay 4×/));
  }, 120_000);

  it("characters.l1: a 2,000-character roster", async () => {
    await gate("characters.l1", <CharactersL1 active />, charactersScript(), "#/characters", text(/1 of 2,000/));
  });

  it("brain.l1: the newest of 40 tournaments, 1,200 runs, 156k ledger rows", async () => {
    await gate("brain.l1", <BrainL1 active />, brainScript(brain10x()), "#/brain", text(/first pages only/));
  }, 60_000);

  it("segments.l1: 2,000 segments in the tables", async () => {
    await gate("segments.l1", <SegmentsL1 active />, segmentsScript(), "#/segments", has("[data-role='segments-tick-more']"));
  });

  it("simulator.l1: 40 tournaments x 1,200 runs (48k)", async () => {
    await gate("simulator.l1", <SimulatorL1 active />, simScript(sim10x()), "#/simulator", text(/48,000runs/));
  }, 60_000);

  it("reconcile.l1: 2,000 held, 2,000 failed, 4,170 incidents", async () => {
    await gate("reconcile.l1", <ReconcileL1 active />, reconcileScript(), "#/reconcile", () => document.querySelector("[data-role='reconcile-held'] [data-n]")?.getAttribute("data-n") === "2000");
  });
});

describe("DOM nodes per main layer-2 list at 10x", () => {
  it("onair.l2.rundown: a 500-run page of 48k runs", async () => {
    await gate("onair.l2.rundown", <OnAirL2 sub={["rundown"]} />, onairScript([]), "#/onair/rundown", text(/500 of 500/));
  });

  it("characters.l2.all: 2,000 characters", async () => {
    await gate("characters.l2.all", <CharactersL2 sub={["all"]} />, charactersScript(), "#/characters/all", text(/2,000 characters/));
  });

  it("brain.l2.runs: 48k runs paged in by cursor", async () => {
    await gate("brain.l2.runs", <BrainL2 sub={["transcripts"]} />, brainScript(brainRuns10x()), "#/brain/transcripts", text(/Runs · 2,000\+/));
  }, 60_000);

  it("brain.l2.transcript: a 100k-line transcript", async () => {
    const big = new Map([[BRAIN_BIG_RUN, brainBigTranscript()]]);
    const s = brainScript(brainRuns10x(), big);
    await gate("brain.l2.transcript", <BrainL2 sub={["transcripts", BRAIN_BIG_RUN, "50000"]} />, s, "#/brain/transcripts/gen-t0000%2Fr1/50000", text(/100,000 of 100,000 lines/));
  }, 120_000);

  it("segments.l2.tables: the compiled-in tables (SegmentId is a closed enum of four, so they do not scale)", async () => {
    await gate("segments.l2.tables", <SegmentsL2 sub={["tables"]} />, segmentsScript(genTables()), "#/segments/tables", has("[data-role='segments-card']"));
  });

  it("simulator.l2.runs: 48k runs", async () => {
    await gate("simulator.l2.runs", <SimulatorL2 sub={["runs"]} />, simScript(sim10x()), "#/simulator/runs", text(/48,000 of 48,000/));
  }, 60_000);

  it("reconcile.l2.report: 4,170 incidents, 2,000 + 2,000 findings", async () => {
    session.showReport(REPORT_10X);
    await gate("reconcile.l2.report", <ReconcileL2 sub={["report"]} />, reconcileScript(), "#/reconcile/report", text(/4,170 of 4,170/));
  });
});
