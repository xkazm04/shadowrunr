// Layer 1 in jsdom on the real data layer: fidelity hooks, the read window at 10x, draws only
// on input change, and the offline and empty states. Counts are gated; timings are not.
import { cleanup, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Layer1 } from "./Layer1";
import { WINDOW } from "./useBrain";
import { daemon, mount, nodes, Recorder, season, settle } from "./testgen";

const ROLES = [
  "brain-layer1",
  "brain-hero",
  "brain-backdrop",
  "brain-scope",
  "brain-scope-canvas",
  "brain-legend",
  "brain-latency",
  "brain-gauge",
  "brain-gauge-needle",
  "brain-memory",
  "brain-memory-canvas",
  "brain-pool",
  "brain-lamps",
  "brain-picker",
  "brain-funnel",
  "brain-door-engine",
  "brain-door-transcripts",
  "push-in",
];

let recs: Recorder[] = [];
beforeEach(() => {
  recs = [];
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(() => {
    const r = new Recorder();
    recs.push(r);
    return r as never;
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const draws = (role: string) => Number(document.querySelector(`[data-role='${role}']`)?.getAttribute("data-draws") ?? "0");

describe("brain layer 1", () => {
  it("renders every fidelity role on a pilot-sized season", async () => {
    mount(<Layer1 active />, daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 130, transcriptLines: 800 })), "#/brain");
    await settle();
    for (const role of ROLES) expect(document.querySelector(`[data-role='${role}']`), role).not.toBeNull();
    expect(document.querySelectorAll("[data-role='brain-lamp']").length).toBe(4);
    expect(screen.getByText(/Picker funnel · 4 runs/)).toBeTruthy();
    expect(screen.getByText("Layer 1 · The Brain")).toBeTruthy();
  });

  it("holds at 10x: a bounded read window, a constant DOM, and a flag that says so", async () => {
    const s = season({ tournaments: 400, runs: 120, rowsPerRun: 130, transcriptLines: 2000 });
    const { calls } = mount(<Layer1 active />, daemon(s), "#/brain");
    await settle(30);
    const n = nodes();
    const count = (m: string) => calls.filter((c) => c === m).length;
    expect(count("tournaments.page")).toBe(1);
    expect(count("runs.page")).toBe(1);
    expect(count("ledger.page")).toBe(WINDOW.ledgerPages);
    expect(count("run.transcript")).toBe(9);
    expect(screen.getByText(/first pages only/)).toBeTruthy();
    expect(n).toBeLessThan(400);
    console.info(`brain l1 dom-count at 10x: ${n}`);
  });

  it("draws each canvas on input change only, never per tick", async () => {
    const raf = vi.spyOn(window, "requestAnimationFrame");
    mount(<Layer1 active />, daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 20, transcriptLines: 60 })), "#/brain");
    await settle();
    const before = [draws("brain-scope-canvas"), draws("brain-memory-canvas")];
    expect(before[0]).toBeGreaterThan(0);
    await new Promise((r) => setTimeout(r, 60));
    await settle();
    expect([draws("brain-scope-canvas"), draws("brain-memory-canvas")]).toEqual(before);
    expect(raf).not.toHaveBeenCalled();
    // one lane per run here: 4 trace labels on the scope canvas
    expect(recs.flatMap((r) => r.texts).filter((t) => t.includes(" · A")).length % 4).toBe(0);
  });

  it("renders nothing and reads nothing while hidden under a nested page", async () => {
    const { calls } = mount(<Layer1 active={false} />, daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 5, transcriptLines: 10 })), "#/brain/engine");
    await settle();
    expect(document.querySelector("[data-role='brain-layer1']")).toBeNull();
    expect(calls).toEqual([]);
  });

  it("shows the offline state when the daemon is down", async () => {
    mount(<Layer1 active />, daemon(null), "#/brain");
    await settle();
    expect(screen.getByText("The daemon is not reachable")).toBeTruthy();
  });

  it("shows the designed empty state before any tournament", async () => {
    mount(<Layer1 active />, daemon(season({ tournaments: 0, runs: 0, rowsPerRun: 0, transcriptLines: 0 })), "#/brain");
    await settle();
    expect(screen.getByText("No simulation runs yet")).toBeTruthy();
  });
});
