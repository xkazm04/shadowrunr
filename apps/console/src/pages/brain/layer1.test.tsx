// Layer 1 in jsdom on the real data layer: fidelity hooks, the read window at 10x, draws only
// on input change, and the offline and empty states. Counts are gated; timings are not.
import { cleanup, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Layer1 } from "./Layer1";
import { WINDOW } from "./useBrain";
import { daemon, Recorder, season } from "./testgen";
import { mountPage as mount, nodes, ok, settle } from "../../kit/testing";

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
    await settle(8);
    for (const role of ROLES) expect(document.querySelector(`[data-role='${role}']`), role).not.toBeNull();
    expect(document.querySelectorAll("[data-role='brain-lamp']").length).toBe(4);
    expect(screen.getByText(/Picker funnel · 4 runs/)).toBeTruthy();
    expect(screen.getByText("Layer 1 · The Brain")).toBeTruthy();
  });

  it("reads the first-word gauge from the newest Reconcile report when there is one", async () => {
    const base = daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 20, transcriptLines: 50 }));
    const report = {
      report_id: "rec-2",
      created_ms: 2,
      source: { tournament_id: "gen-t0000" },
      segments: [],
      tone: [],
      ranks: {},
      red_lines: [],
      latency: { ledger_rows: 10, errors: 0, fallbacks: 0, say_n: 12, say_first_text_p50_ms: 1500, say_first_text_p95_ms: 4000 },
      summary: "",
    };
    const { calls } = mount(
      <Layer1 active />,
      (method, sent) =>
        method === "reconcile.list"
          ? ok([{ report_id: "rec-2", created_ms: 2, source: { tournament_id: "gen-t0000" }, proposals: 0 }])
          : method === "reconcile.report"
            ? ok(report)
            : base(method, sent),
      "#/brain",
    );
    await settle(12);
    const gauge = document.querySelector("[data-role='brain-latency']")?.textContent ?? "";
    expect(gauge).toContain("1.5s p50");
    expect(gauge).toContain("p95 4.0 s · n=12 · newest report");
    expect(calls).toContain("reconcile.report");
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
    expect(count("ledger.summary")).toBe(1);
    expect(screen.getByText(/lanes drawn from the first pages/)).toBeTruthy();
    expect(n).toBeLessThan(400);
    console.info(`brain l1 dom-count at 10x: ${n}`);
  });

  it("draws each canvas on input change only, never per tick", async () => {
    const raf = vi.spyOn(window, "requestAnimationFrame");
    mount(<Layer1 active />, daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 20, transcriptLines: 60 })), "#/brain");
    await settle(8);
    const before = [draws("brain-scope-canvas"), draws("brain-memory-canvas")];
    expect(before[0]).toBeGreaterThan(0);
    await new Promise((r) => setTimeout(r, 60));
    await settle(8);
    expect([draws("brain-scope-canvas"), draws("brain-memory-canvas")]).toEqual(before);
    expect(raf).not.toHaveBeenCalled();
    // one lane per run here: 4 trace labels on the scope canvas
    expect(recs.flatMap((r) => r.texts).filter((t) => t.includes(" · A")).length % 4).toBe(0);
  });

  it("renders nothing and reads nothing while hidden under a nested page", async () => {
    const { calls } = mount(<Layer1 active={false} />, daemon(season({ tournaments: 1, runs: 4, rowsPerRun: 5, transcriptLines: 10 })), "#/brain/engine");
    await settle(8);
    expect(document.querySelector("[data-role='brain-layer1']")).toBeNull();
    expect(calls).toEqual([]);
  });

  // 3 runs x 3,000 rows: 9,000 ledger rows, past the 6,000-row window; the tournament row says
  // 2,400 runs while the window holds 3, so the HUD's runs can only come from the row.
  const whole = () => {
    const s = season({ tournaments: 1, runs: 3, rowsPerRun: 3_000, transcriptLines: 10 });
    return { ...s, tournaments: s.tournaments.map((t) => ({ ...t, runs: 2_400 })) };
  };
  const hud = () => document.querySelector("[data-role='brain-scope']")?.textContent ?? "";

  it("quotes the tournament's whole ledger from ledger.summary, not the page window", async () => {
    const s = whole();
    const { calls } = mount(<Layer1 active />, daemon(s), "#/brain");
    await settle(20);
    expect(calls.filter((c) => c === "ledger.summary")).toHaveLength(1);
    expect(hud()).toContain("9,000calls · 2,400 runs");
    const perRole = new Map<string, number>();
    for (const l of s.ledger) perRole.set(l.role, (perRole.get(l.role) ?? 0) + 1);
    const lamps = [...document.querySelectorAll("[data-role='brain-lamp']")];
    expect(lamps).toHaveLength(perRole.size);
    for (const lamp of lamps) {
      const role = lamp.getAttribute("data-lamp") ?? "";
      expect(lamp.textContent, role).toContain(String(perRole.get(role)));
    }
    expect(screen.getByText(/lanes drawn from the first pages/)).toBeTruthy();
    expect(screen.queryByText("Newest tournament · first pages only")).toBeNull();
  });

  it("falls back to the window's fold, labelled, when ledger.summary fails", async () => {
    const base = daemon(whole());
    mount(<Layer1 active />, (m, sent) => (m === "ledger.summary" ? { status: 500, body: { error: { code: "internal", message: "boom" } } } : base(m, sent)), "#/brain");
    await settle(20);
    expect(hud()).toContain(`${(WINDOW.ledgerPages * WINDOW.pageSize).toLocaleString("en-US")}calls`);
    expect(screen.getByText("Newest tournament · first pages only")).toBeTruthy();
    expect(document.querySelector("[data-role='brain-scope-canvas']")).not.toBeNull();
  });

  it("shows the offline state when the daemon is down", async () => {
    mount(<Layer1 active />, daemon(null), "#/brain");
    await settle(8);
    expect(screen.getByText("The daemon is not reachable")).toBeTruthy();
  });

  it("shows the designed empty state before any tournament", async () => {
    mount(<Layer1 active />, daemon(season({ tournaments: 0, runs: 0, rowsPerRun: 0, transcriptLines: 0 })), "#/brain");
    await settle(8);
    expect(screen.getByText("No simulation runs yet")).toBeTruthy();
  });
});
