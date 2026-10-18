// Reported, never gated: timings at the 10x fixture, printed with the machine and the date.
// Skipped unless vitest runs with `--mode perf` (the `perf` script) or SR_PERF_REPORT=1, so `test`
// stays fast.
import { cleanup, fireEvent, screen } from "@testing-library/react";
import { cpus, platform, release } from "node:os";
import { env } from "node:process";
import type { ReactNode } from "react";
import { afterAll, afterEach, beforeEach, describe, it, vi } from "vitest";
import type { RunRow } from "@shadowrunr/contract";
import type { Script } from "../kit/testing";
import { Layer1 as BrainL1 } from "../pages/brain/Layer1";
import { Recorder } from "../pages/brain/testgen";
import { Layer1 as CharactersL1 } from "../pages/characters/Layer1";
import { Layer1 as OnAirL1 } from "../pages/onair/Layer1";
import { Layer1 as ReconcileL1 } from "../pages/reconcile/Layer1";
import { session } from "../pages/reconcile/session";
import { Layer1 as SegmentsL1 } from "../pages/segments/Layer1";
import { Layer1 as SimulatorL1 } from "../pages/simulator/Layer1";
import { Layer2 as SimulatorL2 } from "../pages/simulator/Layer2";
import { filterRows } from "../ui/filterRows";
import { brain10x, brainScript, charactersScript, onairRuns, onairScript, reconcileScript, segmentsScript, sim10x, simScript } from "./fixture";
import { mountPerf, quiet, type Mounted } from "./harness";

const REPORT = import.meta.env.MODE === "perf" || env["SR_PERF_REPORT"] === "1";
const lines: string[] = [];

/** The p-th percentile (0..1) of `xs`, nearest rank. */
function percentile(xs: readonly number[], p: number): number {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.max(0, Math.ceil(p * s.length) - 1))] ?? 0;
}

const ms = (x: number) => `${x.toFixed(1)} ms`;

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
afterAll(() => {
  if (!REPORT) return;
  const cpu = cpus()[0]?.model ?? "unknown cpu";
  console.info(
    [`perf timings (reported, not gated) · ${new Date().toISOString()} · ${cpu} x${cpus().length} · ${platform()} ${release()} · jsdom`, ...lines.map((l) => `  ${l}`)].join("\n"),
  );
});

async function renderMs(name: string, ui: ReactNode, script: Script, hash: string): Promise<void> {
  const t0 = performance.now();
  mounted = mountPerf(ui, script, hash);
  await quiet(mounted, 400);
  lines.push(`render ${name}: ${ms(performance.now() - t0)} to a still DOM (fixture build excluded)`);
}

describe.skipIf(!REPORT)("timings at 10x (reported only)", () => {
  it("render: each layer 1 from mount to a still DOM", async () => {
    // Build the fixtures first so the render figure is the page's, not the generator's.
    brain10x();
    sim10x();
    onairRuns();
    await renderMs("onair.l1", <OnAirL1 active />, onairScript(), "#/onair");
    mounted?.dispose();
    cleanup();
    await renderMs("characters.l1", <CharactersL1 active />, charactersScript(), "#/characters");
    mounted?.dispose();
    cleanup();
    await renderMs("brain.l1", <BrainL1 active />, brainScript(brain10x()), "#/brain");
    mounted?.dispose();
    cleanup();
    await renderMs("segments.l1", <SegmentsL1 active />, segmentsScript(), "#/segments");
    mounted?.dispose();
    cleanup();
    await renderMs("simulator.l1", <SimulatorL1 active />, simScript(sim10x()), "#/simulator");
    mounted?.dispose();
    cleanup();
    await renderMs("reconcile.l1", <ReconcileL1 active />, reconcileScript(), "#/reconcile");
  }, 300_000);

  it("filter: p95 of a text filter over the 48k-run list (pure, and through the page)", async () => {
    const rows: RunRow[] = [...sim10x().runs.values()].flat();
    const hay = (r: RunRow) => `${r.run_id} ${r.tournament_id} ${r.spec.tone_variant} ${r.spec.core_tier} ${r.spec.segment} ${r.spec.audience_size}`;
    const queries = ["haiku", "SEG-02A 300", "t017", "weary sonnet", "r0119", "opus 3", "theatrical SEG-01", "t040-r1", "zzz", ""];
    const pure: number[] = [];
    for (let round = 0; round < 10; round++) {
      for (const query of queries) {
        const t0 = performance.now();
        filterRows(rows, { haystack: hay, query });
        pure.push(performance.now() - t0);
      }
    }
    lines.push(`filter 48k runs (filterRows): p50 ${ms(percentile(pure, 0.5))}, p95 ${ms(percentile(pure, 0.95))}, n=${pure.length}`);

    mounted = mountPerf(<SimulatorL2 sub={["runs"]} />, simScript(sim10x()), "#/simulator/runs");
    await screen.findByText(/48,000 of 48,000/, undefined, { timeout: 60_000 });
    const input = screen.getByRole("searchbox");
    const page: number[] = [];
    for (const query of queries) {
      const t0 = performance.now();
      fireEvent.change(input, { target: { value: query } });
      await quiet(mounted, 50);
      page.push(performance.now() - t0);
    }
    lines.push(`filter 48k runs (simulator/runs, input to still DOM incl. debounce and settle): p50 ${ms(percentile(page, 0.5))}, p95 ${ms(percentile(page, 0.95))}, n=${page.length}`);
  }, 300_000);
});
