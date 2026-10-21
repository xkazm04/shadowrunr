import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { buildTile, gridLayout, hitTest, runGrid, summarise, tilesInView, tileOrigin } from "./grid";
import { cellChecks, cellRouteKey, parseCellRouteKey, rateBound, THIN_N, worstFirst } from "./cells";
import { pivot } from "./matrix";
import { healthColor } from "./ramp";
import { eventText, parseRef } from "../../kit/events";
import { verifyQuote } from "./quote";
import type { CellResult } from "./results";
import { tileLabel } from "./draw";
import { abWorld, TRANSCRIPT, world } from "./testgen";

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

describe("tiles", () => {
  it("colours runs by their own health, marks their red-line fails, leaves unjudged runs unlit", () => {
    const w = world(1, 24);
    const runs = w.runs.get("t001") ?? [];
    const row = w.tournaments[0];
    if (!row) throw new Error("no row");
    const tile = buildTile(row, runs);
    expect(tile.runs).toHaveLength(24);
    expect(tile.runs[9]?.health).toBeUndefined(); // has_verdict false
    expect(tile.runs[0]?.red).toBe(true); // its verdict failed a red line
    expect(tile.runs.filter((r) => r.red)).toHaveLength(runs.filter((r) => (r.red_line_fails ?? 0) > 0).length);
    expect(tile.red).toBe(runs.reduce((n, r) => n + (r.red_line_fails ?? 0), 0));
    expect(tile.red).toBeGreaterThan(0);
    const judged = runs.filter((r) => r.health !== undefined);
    expect(tile.health).toBeCloseTo(judged.reduce((a, r) => a + (r.health ?? 0), 0) / judged.length);
    const s = summarise([tile, buildTile({ tournament_id: "t002", instrument_hash: "h", runs: 0, completed: 0 }, [])]);
    expect(s).toMatchObject({ runs: 24, red: tile.red });
    expect(s.best?.id).toBe("t001");
  });
});

describe("held: one derivation, the daemon's", () => {
  it("the tile reads the row's held, the mean over judged runs (75%), not the pooled checks (50%)", () => {
    const w = abWorld();
    const row = w.tournaments[0];
    if (!row) throw new Error("no row");
    expect(row).toMatchObject({ judged: 2, held: 0.75 });
    const tile = buildTile(row, w.runs.get("t001") ?? []);
    expect(tileLabel(tile)).toBe("T001 · 75%");
    expect(tile.red).toBe(row.red_line_fails ?? 0);
    // A row from a daemon that does not fold held yet: the tile says so, it does not recompute.
    const older = { ...row };
    delete older.held;
    expect(buildTile(older, w.runs.get("t001") ?? []).health).toBeUndefined();
  });

  it("no console source defines a tournament- or cell-level held of its own", () => {
    const src = join(process.cwd(), "src");
    const files: string[] = [];
    const walk = (d: string) => {
      for (const f of readdirSync(d)) {
        const p = join(d, f);
        if (statSync(p).isDirectory()) walk(p);
        else if (/\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f)) files.push(p);
      }
    };
    walk(src);
    const defined = files.filter((f) => /(function|const|let)\s+(resultsHealth|cellHealth|redLineFails)\b/.test(readFileSync(f, "utf8")));
    expect(defined).toEqual([]);
  });
});

describe("heatmap ordering and bounds (cells.ts)", () => {
  const cell = (tone: string, fail: number, np: number, q?: number): CellResult => ({
    cell: { instrument_hash: "h", tone, core_tier: "haiku", segment: "SEG-01", audience_size: 3 },
    runs: 1,
    judged: 1,
    void: 0,
    failed: 0,
    judge_tiers: [],
    rules: { "a:b": { pass: 1, fail, not_provoked: np, not_evaluable: 0, n: 1 + fail, fail_refs: [] } },
    quality: q === undefined ? {} : { humor: { true: 1, n: 1, rate: q, not_evaluable: 0 } },
    core_latency: { n: 0 },
    usage: { calls: 0, errors: 0, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_creation_tokens: 0, cost_usd: 0 },
    usage_by_role: {},
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
    expect(cellChecks(cell("x", 1, 0))).toEqual({ fail: 1, evaluated: 2 });
    // Counted, never bounded: checks inside one run are one observation, not many.
    expect(cellChecks(cell("x", 1, 0))).not.toHaveProperty("bound");
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
    expect(TRANSCRIPT.map(eventText)).toEqual(["s1 · malakor · theatrical · haiku · 3 viewers", "Kneel, minions. The Lord of the Darkness is watching.", "hail the lich", "Your “hail” is noted, thrall.", "sim_complete"]);
  });
});

describe("the heatmap map (matrix.ts): tone × tier by segment × audience", () => {
  const w = world(1, 72);
  const results = w.results.get("t001");
  if (!results) throw new Error("no results");
  const runs = w.runs.get("t001") ?? [];

  it("pivots one instrument's 36 cells into 6 rows by 6 columns, tiers in model order, audiences ascending", () => {
    const blocks = pivot(results.cells, runs);
    expect(blocks).toHaveLength(1);
    const b = blocks[0];
    if (!b) throw new Error("no block");
    expect(b.hash).toBe("a3f9c2e17b44d0e5");
    expect(b.rows.map((r) => `${r.tone}/${r.tier}`)).toEqual([
      "theatrical/haiku",
      "theatrical/sonnet",
      "theatrical/opus",
      "weary-middle-manager/haiku",
      "weary-middle-manager/sonnet",
      "weary-middle-manager/opus",
    ]);
    expect(b.cols.map((c) => `${c.segment}/${c.audience}`)).toEqual(["SEG-01/3", "SEG-01/30", "SEG-01/300", "SEG-02A/3", "SEG-02A/30", "SEG-02A/300"]);
    expect(b.positions.flat().filter((p) => p.cell !== undefined)).toHaveLength(36);
  });

  it("keeps two instrument hashes as two blocks, never one merged grid", () => {
    const other = results.cells.slice(0, 5).map((c) => ({ ...c, cell: { ...c.cell, instrument_hash: "bbbbbbbbbbbbbbbb" } }));
    const blocks = pivot([...results.cells, ...other], runs);
    expect(blocks.map((b) => b.hash)).toEqual(["a3f9c2e17b44d0e5", "bbbbbbbbbbbbbbbb"]);
    expect(blocks[0]?.positions.flat().filter((p) => p.cell)).toHaveLength(36);
    expect(blocks[1]?.positions.flat().filter((p) => p.cell)).toHaveLength(5);
  });

  it("marks a combination absent from results 'not run': no figure, no held, out of every marginal", () => {
    const gone = results.cells.filter((c) => !(c.cell.tone === "theatrical" && c.cell.core_tier === "haiku" && c.cell.segment === "SEG-01" && c.cell.audience_size === 3));
    const b = pivot(gone, runs)[0];
    if (!b) throw new Error("no block");
    const p = b.positions[0]?.[0];
    expect(p?.cell).toBeUndefined();
    expect(p?.held).toBeUndefined();
    expect(b.rowMargins[0]?.cells).toBe(5);
    expect(b.colMargins[0]?.cells).toBe(5);
    const full = pivot(results.cells, runs)[0];
    expect(b.rowMargins[0]?.evaluated).toBeLessThan(full?.rowMargins[0]?.evaluated ?? 0);
  });

  it("sums a row's counted fails, evaluated checks and judged runs, and says how many of its cells are thin", () => {
    const b = pivot(results.cells, runs)[0];
    if (!b) throw new Error("no block");
    const row = results.cells.filter((c) => c.cell.tone === "theatrical" && c.cell.core_tier === "haiku");
    expect(row).toHaveLength(6);
    const m = b.rowMargins[0];
    expect(m).toEqual({
      cells: 6,
      fail: row.reduce((a, c) => a + cellChecks(c).fail, 0),
      evaluated: row.reduce((a, c) => a + cellChecks(c).evaluated, 0),
      judged: row.reduce((a, c) => a + c.judged, 0),
      thin: row.filter((c) => c.judged < THIN_N).length,
    });
    expect(m?.thin).toBe(6); // two judged runs a cell: every one is thin
    expect(b.positions[0]?.every((p) => p.thin)).toBe(true);
  });

  it("marks a cell whose red-line rule failed", () => {
    const b = pivot(results.cells, runs)[0];
    const red = b?.positions.flat().filter((p) => p.red) ?? [];
    const expected = results.cells.filter((c) => (c.rules["red-line:hateful-conduct"]?.fail ?? 0) > 0);
    expect(expected.length).toBeGreaterThan(0);
    expect(red.map((p) => p.cell)).toEqual(expect.arrayContaining(expected));
    expect(red).toHaveLength(expected.length);
  });

  it("colours a cell by the row's held derivation over its own runs: the run-weighted cells give back the row's held", () => {
    const b = pivot(results.cells, runs)[0];
    const row = w.tournaments[0];
    let sum = 0;
    let n = 0;
    for (const p of b?.positions.flat() ?? []) {
      if (p.held === undefined) continue;
      sum += p.held * p.heldRuns;
      n += p.heldRuns;
    }
    expect(n).toBe(runs.filter((r) => r.health !== undefined).length);
    expect(sum / n).toBeCloseTo(row?.held ?? -1, 10);
  });
});
