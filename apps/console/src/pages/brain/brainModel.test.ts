import { describe, expect, it } from "vitest";
import { buildBrainModel, buildLanes, LIMITS, memoryCurve, percentile, roleStats, transcriptStats } from "./brainModel";
import { ledgerRows, runRow, season, transcript } from "./testgen";

describe("lanes (A/1 SR.brain pulses, aggregated)", () => {
  it("draws one lane per run when every run is its own (tier, audience) cell, as A/1 did", () => {
    const runs = [0, 1, 3, 4].map((i) => runRow(i, "t"));
    const ledger = runs.flatMap((r) => ledgerRows(r, 10));
    const lanes = buildLanes(runs, ledger);
    expect(lanes.map((l) => l.label)).toEqual(["haiku · a3", "sonnet · a3", "haiku · a30", "sonnet · a30"]);
    expect(lanes.every((l) => l.runIds.length === 1 && l.pulses.length === 10)).toBe(true);
  });

  it("times pulses from the run's first ledger row and spans the on-axis calls by 1.06", () => {
    const r = runRow(0, "t");
    const rows = ledgerRows(r, 3);
    const [lane] = buildLanes([r], rows);
    expect(lane?.pulses.map((p) => p.t)).toEqual([0, 4000, 8000]);
    const end = Math.max(...rows.map((x, k) => k * 4000 + x.duration_ms));
    expect(lane?.span).toBeCloseTo(end * 1.06);
  });

  it("leaves judge and consolidator calls off the time axis", () => {
    const r = runRow(0, "t");
    const rows = ledgerRows(r, 2);
    const judge = { ...rows[0]!, role: "judge" as const, ts_ms: rows[0]!.ts_ms + 1_000_000, duration_ms: 300_000 };
    const [lane] = buildLanes([r], [...rows, judge]);
    expect(lane?.span).toBeLessThan(1_000_000);
    expect(lane?.pulses).toHaveLength(3);
  });

  it("holds at 48k runs: at most 12 lanes of at most 480 pulses, every call counted", () => {
    const s = season({ tournaments: 1, runs: 48_000, rowsPerRun: 3, transcriptLines: 10 });
    const lanes = buildLanes(s.runs, s.ledger);
    expect(lanes.length).toBeLessThanOrEqual(LIMITS.maxLanes);
    expect(lanes.length).toBe(9);
    for (const l of lanes) expect(l.pulses.length).toBeLessThanOrEqual(LIMITS.maxPulses);
    expect(lanes.reduce((n, l) => n + l.calls, 0)).toBe(144_000);
    // a binned lane keeps the error marks of the calls it folded
    expect(lanes.some((l) => l.pulses.some((p) => p.err))).toBe(true);
  });
});

describe("transcript statistics", () => {
  it("uses the reconcile crate's nearest-rank percentile", () => {
    const v = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    expect(percentile(v, 50)).toBe(5);
    expect(percentile(v, 95)).toBe(10);
    expect(percentile([], 50)).toBeUndefined();
  });

  it("counts the picker funnel: chat in, past moderation, picked, answered (from replying_to)", () => {
    const { funnel, latency } = transcriptStats([transcript(61)]);
    expect(funnel.chat).toBe(10);
    expect(funnel.dropped + funnel.collapsed).toBe(10);
    expect(funnel.picked).toBe(10);
    expect(funnel.answered).toBe(10);
    expect(latency.n).toBe(10);
    expect(latency.p50).toBeDefined();
  });

  it("keeps the memory curve cumulative and bounded, last moment included", () => {
    const tr = transcript(6000);
    const c = memoryCurve(tr);
    expect(c.points.length).toBeLessThanOrEqual(LIMITS.maxMemoryPoints);
    expect(c.points[c.points.length - 1]?.n).toBe(tr.filter((e) => (e.type === "op" && e.op.op === "mark_moment") || (e.type === "say" && e.text.startsWith("Memory candidates"))).length);
  });

  it("orders roles by calls with their errors", () => {
    const rows = ledgerRows(runRow(0, "t"), 100);
    const roles = roleStats(rows);
    expect(roles[0]?.role).toBe("core_thread");
    expect(roles.reduce((n, r) => n + r.errors, 0)).toBe(rows.filter((r) => r.error_kind).length);
  });

  it("builds the whole model with transcripts still loading", () => {
    const s = season({ tournaments: 1, runs: 6, rowsPerRun: 5, transcriptLines: 30 });
    const m = buildBrainModel(s.runs, s.ledger, [transcript(30), undefined]);
    expect(m.memory[1]).toBeUndefined();
    expect(m.funnel.runs).toBe(1);
    expect(m.calls).toBe(30);
  });
});
