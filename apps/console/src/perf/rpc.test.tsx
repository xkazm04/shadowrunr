// Gated: RPC calls on first paint per page at the 10x fixture (budgets/perf.json `rpc`). Every
// call a layer 1 makes until it holds still is tallied by method; a method over its line, or
// one the line does not list, fails. Counts, never timings. The layers are PERF_PAGES (pages.tsx).
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Recorder } from "../pages/brain/testgen";
import { session } from "../pages/reconcile/session";
import { BUDGETS, line, mountPerf, overBudget, quiet, tally, type Mounted } from "./harness";
import { LAYER1, PERF_PAGES, type Named } from "./pages";

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

async function gate({ name, probe }: Named & { probe: (typeof LAYER1)[number]["probe"] }): Promise<readonly string[]> {
  probe.before?.();
  mounted = mountPerf(probe.ui(), (probe.rpcScript ?? probe.script)(), probe.hash);
  await quiet(mounted, 400);
  console.info(`perf rpc ${name}: ${JSON.stringify(tally(mounted.calls))}`);
  const budget = line(BUDGETS.rpc, name).calls;
  expect(overBudget(mounted.calls, budget), `${name}: RPC calls on first paint`).toEqual([]);
  return mounted.calls;
}

describe("RPC calls on first paint per page at 10x", () => {
  for (const l of LAYER1) it(`${l.name}: ${l.probe.what}`, () => gate(l).then(() => undefined), l.probe.timeout);
});

describe("review's wall reads the session list and the marks once each", () => {
  it("the tally is exactly its line, and a per-session read is unlisted", async () => {
    const calls = await gate({ page: "review", name: "review.l1", probe: PERF_PAGES.review.l1 });
    expect(tally(calls)).toEqual({ "review.sessions": 1, "review.feedback.list": 1 });
    expect(line(BUDGETS.rpc, "review.l1").calls).toEqual({ "review.sessions": 1, "review.feedback.list": 1 });
    expect(overBudget([...calls, "review.session"], line(BUDGETS.rpc, "review.l1").calls)).toEqual(["review.session: 1 > 0"]);
  }, 60_000);
});
