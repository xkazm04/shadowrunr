import { describe, expect, it } from "vitest";
import { buildLanes, memoryCurve } from "./brainModel";
import { drawMemory, drawScope, laneX } from "./draw";
import { TOKENS_BRAIN } from "@shadowrunr/tokens";
import { MEMORY } from "./draw";
import { ledgerRows, Recorder, runRow, season, transcript } from "./testgen";

describe("the oscilloscope (A/1 app.js:370-386)", () => {
  it("draws A/1's grid, lane label, trace and dots for one lane", () => {
    const r = runRow(0, "t");
    const lanes = buildLanes([r], ledgerRows(r, 5));
    const rec = new Recorder();
    drawScope(rec, { w: 800, h: 400 }, lanes);
    expect(rec.texts).toEqual(["HAIKU · A3"]);
    expect(rec.calls["lineTo"]).toBe(21 + 5 * 4 + 1);
    expect(rec.calls["arc"]).toBe(5 + 1); // one error ring (row 2 of 41)
    expect(rec.writes["strokeStyle"]).toContain(TOKENS_BRAIN.scopeTrace);
    expect(rec.writes["shadowBlur"]).toEqual([8, 0]);
  });

  it("puts pulses on A/1's x axis (1% margins, capped at 99%)", () => {
    expect(laneX(0, 100, 1000)).toBe(10);
    expect(laneX(1000, 100, 1000)).toBeCloseTo(0.99 * 980 + 10);
  });

  it("costs a bounded number of canvas calls at 48k runs", () => {
    const s = season({ tournaments: 1, runs: 48_000, rowsPerRun: 3, transcriptLines: 10 });
    const rec = new Recorder();
    drawScope(rec, { w: 1200, h: 600 }, buildLanes(s.runs, s.ledger));
    expect(rec.total()).toBeLessThan(40_000);
  });

  it("draws one memory curve per loaded lane, coloured in A/1's order", () => {
    const rec = new Recorder();
    drawMemory(rec, { w: 400, h: 200 }, [memoryCurve(transcript(60)), undefined, memoryCurve(transcript(30))], 10);
    expect(rec.calls["stroke"]).toBe(2);
    expect(rec.writes["strokeStyle"]).toEqual([MEMORY[0], MEMORY[2]]);
  });
});
