import type { TournamentResults } from "@shadowrunr/contract";
import { describe, expect, it } from "vitest";
import { buildTile, gridLayout, hitTest, runGrid, summarise, tilesInView, tileOrigin } from "./grid";
import { cellFailRate, cellRouteKey, parseCellRouteKey, rateBound, worstFirst } from "./cells";
import { healthColor } from "./palette";
import { eventText, parseRef, verifyQuote } from "./quote";
import { readResults, type CellResult } from "./results";
import { TRANSCRIPT, world } from "./testgen";

/** A/1 app.js:25, verbatim, as the reference for the port. */
function a1HealthColor(h: number, a?: number) {
  var hue = h < 0.5 ? 14 + h * 52 : 40 + (h - 0.5) * 248; // eslint-disable-line no-var
  return "hsla(" + hue + "," + (h < 0.5 ? 78 : 62 - (h - 0.5) * 40) + "%," + (50 - Math.abs(h - 0.5) * 8) + "%," + (a == null ? 1 : a) + ")";
}

describe("A/1 fidelity: the health ramp", () => {
  it("matches A/1 byte for byte across the range", () => {
    for (let i = 0; i <= 100; i++) {
      const h = i / 100;
      expect(healthColor(h)).toBe(a1HealthColor(h));
      expect(healthColor(h, 0.35 + 0.6 * h)).toBe(a1HealthColor(h, 0.35 + 0.6 * h));
    }
    expect(healthColor(0)).toBe("hsla(14,78%,46%,1)");
    expect(healthColor(0.5)).toBe("hsla(40,62%,50%,1)");
    expect(healthColor(1)).toBe("hsla(164,42%,46%,1)");
  });
});

describe("A/1 fidelity: grid geometry and hit-testing", () => {
  it("lays 8 x 5 tiles with A/1's gap and finds the tile under a point", () => {
    const l = gridLayout(1000, 500);
    expect(l).toMatchObject({ cols: 8, rows: 5, gap: 8 });
    expect(l.tw).toBeCloseTo((1000 - 8 * 7) / 8);
    expect(l.th).toBeCloseTo((500 - 8 * 4) / 5);
    expect(hitTest(l, 1, 1, 40)).toBe(0);
    const o = tileOrigin(l, 13);
    expect(hitTest(l, o.ox + 2, o.oy + 2, 40)).toBe(13);
    expect(hitTest(l, o.ox + 2, o.oy + 2, 13)).toBe(-1);
    expect(hitTest(l, -5, 10, 40)).toBe(-1);
    expect(hitTest(l, 999, 499, 40)).toBe(39);
  });

  it("keeps A/1's 12 x 10 run grid to 120 runs and fits one cell per run beyond", () => {
    expect(runGrid(120, 120, 100)).toEqual({ cols: 12, rows: 10 });
    expect(runGrid(4, 120, 100)).toEqual({ cols: 12, rows: 10 });
    const big = runGrid(1200, 117, 92);
    expect(big.cols * big.rows).toBeGreaterThanOrEqual(1200);
    expect(big.cols).toBeGreaterThan(12);
  });

  it("shows the latest 40 tournaments", () => {
    const ids = Array.from({ length: 400 }, (_, i) => i);
    expect(tilesInView(ids)).toHaveLength(40);
    expect(tilesInView(ids)[0]).toBe(360);
    expect(tilesInView(ids.slice(0, 3))).toHaveLength(3);
  });
});

describe("results narrowing", () => {
  it("reads a results.json and names the first misfit", () => {
    const w = world(1, 24);
    const raw = w.results.get("t001") as TournamentResults;
    const r = readResults(raw);
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.value.cells.length).toBeGreaterThan(0);
    const bad = readResults({ ...raw, cells: [{ cell: { tone: "x" } }] });
    expect(bad).toEqual({ ok: false, path: "cells.0.cell.core_tier" });
    expect(readResults({ cells: [] })).toEqual({ ok: false, path: "tournament_id" });
  });
});

describe("tiles", () => {
  it("colours runs by their cell, marks red-line fails by ref, leaves unjudged runs unlit", () => {
    const w = world(1, 24);
    const res = readResults(w.results.get("t001") as TournamentResults);
    if (!res.ok) throw new Error("fixture misfit");
    const runs = w.runs.get("t001") ?? [];
    const tile = buildTile("t001", 24, runs, res.value);
    expect(tile.runs).toHaveLength(24);
    expect(tile.runs[9]?.health).toBeUndefined(); // has_verdict false
    expect(tile.runs[0]?.red).toBe(true); // named by the fail ref
    expect(tile.runs.filter((r) => r.red)).toHaveLength(res.value.cells.filter((_, j) => j % 4 === 0).length);
    expect(tile.red).toBeGreaterThan(0);
    expect(tile.health).toBeCloseTo(42 / 56);
    const s = summarise([tile, buildTile("t002", 0, [], undefined)]);
    expect(s).toMatchObject({ runs: 24, red: tile.red });
    expect(s.best?.id).toBe("t001");
  });
});

describe("heatmap (moved from the legacy lib/heatmap.ts)", () => {
  const cell = (tone: string, fail: number, np: number, q?: number): CellResult => ({
    cell: { instrument_hash: "h", tone, core_tier: "haiku", segment: "SEG-01", audience_size: 3 },
    runs: 1,
    judged: 1,
    void: 0,
    failed: 0,
    judge_tiers: [],
    rules: { "a:b": { pass: 1, fail, not_provoked: np, not_evaluable: 0, n: 1 + fail, fail_refs: [] } },
    quality: q === undefined ? {} : { humor: { true: 1, n: 1, rate: q, not_evaluable: 0 } },
  });

  it("sorts worst first: fails, then not provoked, then low quality, then the key", () => {
    const order = worstFirst([cell("e", 0, 0, 0.9), cell("d", 0, 0, 0.2), cell("c", 0, 3), cell("b", 2, 0), cell("a", 0, 0, 0.2)]);
    expect(order.map((c) => c.cell.tone)).toEqual(["b", "c", "a", "d", "e"]);
  });

  it("bounds a rate by the rule of three at 0 or n and by Wilson between", () => {
    expect(rateBound(0, 0)).toBeUndefined();
    expect(rateBound(0, 3)).toEqual({ lo: 0, hi: 1, method: "rule_of_three" });
    expect(rateBound(0, 30)).toEqual({ lo: 0, hi: 0.1, method: "rule_of_three" });
    const w = rateBound(1, 10);
    expect(w?.method).toBe("wilson");
    expect(w?.lo).toBeCloseTo(0.0179, 3);
    expect(w?.hi).toBeCloseTo(0.4042, 3);
    expect(cellFailRate(cell("x", 1, 0))).toMatchObject({ k: 1, n: 2, rate: 0.5 });
  });

  it("round-trips a heatmap route key", () => {
    const k = cell("weary-middle-manager", 0, 0).cell;
    expect(parseCellRouteKey(cellRouteKey("t001", k))).toEqual({ tournamentId: "t001", cell: k });
    expect(parseCellRouteKey("nope")).toBeUndefined();
  });
});

describe("quote verification (A/1 core.js:241-249)", () => {
  it("parses refs", () => {
    expect(parseRef("run-1#12")).toEqual({ runId: "run-1", line: 12 });
    expect(parseRef("run-1")).toBeUndefined();
    expect(parseRef("run-1#0")).toBeUndefined();
  });

  it("verifies on the cited line, finds nearby within ±2, and reports not found", () => {
    expect(verifyQuote(TRANSCRIPT, 2, "kneel,   MINIONS")).toEqual({ state: "verified", line: 2 });
    expect(verifyQuote(TRANSCRIPT, 2, "your \"hail\" is noted")).toEqual({ state: "nearby", line: 4, offset: 2 });
    expect(verifyQuote(TRANSCRIPT, 4, "...hail the lich...")).toEqual({ state: "nearby", line: 3, offset: -1 });
    expect(verifyQuote(TRANSCRIPT, 1, "your \"hail\" is noted")).toEqual({ state: "not_found" });
    expect(verifyQuote(TRANSCRIPT, 2, "   ")).toEqual({ state: "not_found" });
    expect(verifyQuote(TRANSCRIPT, 99, "kneel")).toEqual({ state: "not_found" });
  });

  it("reads the words of every event kind", () => {
    expect(TRANSCRIPT.map(eventText)).toEqual(["s1", "Kneel, minions. The Lord of the Darkness is watching.", "hail the lich", "Your “hail” is noted, thrall.", "sim_complete"]);
  });
});
