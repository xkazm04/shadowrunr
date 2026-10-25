// Gated: DOM nodes per layer 1 and per main layer-2 list at the 10x fixture (budgets/perf.json
// `dom`). Counts, never timings. Each layer is mounted alone on the data-layer test double. The
// layers are PERF_PAGES (pages.tsx), so a registered page cannot go unmeasured here.
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nodes } from "../kit/testing";
import { Recorder } from "../pages/brain/testgen";
import { session } from "../pages/reconcile/session";
import { Layer1 as ReviewL1 } from "../pages/review/Layer1";
import { reviewRows10x, reviewScript } from "./fixture";
import { BUDGETS, line, mountPerf, quiet, type MountOptions, type Mounted } from "./harness";
import { LAYER1, LAYER2, type Named } from "./pages";

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
async function gate({ name, probe }: Named, opts: MountOptions = {}, budget = line(BUDGETS.dom, name).max): Promise<number> {
  probe.before?.();
  mounted = mountPerf(probe.ui(), probe.script(), probe.hash, opts);
  await quiet(mounted, 400);
  if (probe.ready) expect(probe.ready(), `${name}: the 10x content did not arrive`).toBe(true);
  expect(mounted.pending(), `${name}: requests still unanswered`).toBe(0);
  const n = await (probe.count ?? (async () => nodes()))();
  console.info(`perf dom ${name}: ${n} nodes (max ${budget})`);
  expect(n, `${name}: DOM nodes at 10x`).toBeLessThanOrEqual(budget);
  return n;
}

describe("DOM nodes per layer 1 at 10x", () => {
  for (const l of LAYER1) it(`${l.name}: ${l.probe.what}`, () => gate(l).then(() => undefined), l.probe.timeout);
});

describe("DOM nodes per main layer-2 list at 10x", () => {
  for (const l of LAYER2) it(`${l.name}: ${l.probe.what}`, () => gate(l).then(() => undefined), l.probe.timeout);
});

describe("the count does not depend on the machine's speed", () => {
  // Every reply is held 40 ms of real time, so each layer settles on a slower clock than the
  // plain run; the settled count must be the same node for node. On Air also plays its replay on
  // the wall clock, which is what this guards.
  for (const name of ["onair.l1", "segments.l1", "reconcile.l1"]) {
    it(`${name}: counts the same with every reply delayed`, async () => {
      const l = LAYER1.find((x) => x.name === name);
      if (!l) throw new Error(`no layer-1 probe ${name}`);
      const plain = await gate(l);
      mounted?.dispose();
      mounted = undefined;
      cleanup();
      const slow = await gate(l, { latencyMs: 40 });
      expect(slow, `${name}: the loaded count differs`).toBe(plain);
    }, 120_000);
  }

  it("still fails a layer that really exceeds its line", async () => {
    const l = LAYER1.find((x) => x.name === "onair.l1");
    if (!l) throw new Error("no onair.l1 probe");
    await expect(gate(l, {}, 100)).rejects.toThrow(/DOM nodes at 10x/);
  }, 120_000);
});

describe("review's session wall is virtualised", () => {
  it("the node count does not grow between 200 and 2,000 sessions", async () => {
    const at = async (n: number) => {
      mounted = mountPerf(<ReviewL1 active />, reviewScript(reviewRows10x().slice(0, n)), "#/review");
      await quiet(mounted, 400);
      const count = nodes();
      mounted.dispose();
      mounted = undefined;
      cleanup();
      return count;
    };
    const small = await at(200);
    const big = await at(2_000);
    console.info(`perf dom review.l1: ${small} nodes at 200 sessions, ${big} at 2,000`);
    expect(big, "review.l1: DOM nodes grow with the session count").toBeLessThanOrEqual(small);
  }, 60_000);
});
