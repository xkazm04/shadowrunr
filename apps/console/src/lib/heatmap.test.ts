import { describe, expect, it } from "vitest";
import type { CellResult, QualityTally, RuleTally } from "../contract";
import { heatOf, meanQuality, qualityColumns, worstFirst } from "./heatmap";

function rule(t: Partial<RuleTally>): RuleTally {
  const pass = t.pass ?? 0;
  const fail = t.fail ?? 0;
  return { pass, fail, not_provoked: t.not_provoked ?? 0, not_evaluable: t.not_evaluable ?? 0, n: pass + fail };
}
function q(trueCount: number, n: number): QualityTally {
  const t: QualityTally = { true: trueCount, n, not_evaluable: 0 };
  if (n > 0) t.rate = trueCount / n;
  return t;
}
function cell(tone: string, opts: { rules?: Record<string, RuleTally>; quality?: Record<string, QualityTally>; audience?: number; hash?: string } = {}): CellResult {
  return {
    cell: { instrument_hash: opts.hash ?? "h1", tone, core_tier: "haiku", segment: "SEG-01", audience_size: opts.audience ?? 30 },
    runs: 5,
    judged: 5,
    void: 0,
    failed: 0,
    judge_tiers: ["sonnet"],
    rules: opts.rules ?? {},
    quality: opts.quality ?? {},
    core_latency: { n: 0 },
    usage: { calls: 0, errors: 0, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_creation_tokens: 0, cost_usd: 0 },
    usage_by_role: {},
  };
}
const tones = (cs: CellResult[]) => cs.map((c) => c.cell.tone);

describe("worstFirst (mirrors shadowrunr-sim summary.rs worst_first)", () => {
  it("puts any rule fail first, more fails before fewer", () => {
    const cs = [
      cell("clean", { rules: { a: rule({ pass: 5 }) }, quality: { humor: q(1, 10) } }),
      cell("one-fail", { rules: { a: rule({ pass: 4, fail: 1 }) }, quality: { humor: q(9, 10) } }),
      cell("two-fails", { rules: { a: rule({ pass: 3, fail: 1 }), b: rule({ pass: 4, fail: 1 }) } }),
    ];
    expect(tones(worstFirst(cs))).toEqual(["two-fails", "one-fail", "clean"]);
  });

  it("then ranks not_evaluable above not_provoked, both above quality", () => {
    const cs = [
      cell("np", { rules: { a: rule({ pass: 4, not_provoked: 3 }) }, quality: { humor: q(0, 10) } }),
      cell("ne", { rules: { a: rule({ pass: 4, not_evaluable: 1 }) }, quality: { humor: q(10, 10) } }),
      cell("plain", { rules: { a: rule({ pass: 5 }) }, quality: { humor: q(0, 10) } }),
    ];
    expect(tones(worstFirst(cs))).toEqual(["ne", "np", "plain"]);
  });

  it("then lowest mean quality first; a cell without quality sorts as -1 (first)", () => {
    const cs = [
      cell("high", { quality: { a: q(9, 10), b: q(8, 10) } }),
      cell("low", { quality: { a: q(2, 10), b: q(4, 10) } }),
      cell("none", { quality: { a: q(0, 0) } }),
      cell("mid", { quality: { a: q(5, 10) } }),
    ];
    expect(tones(worstFirst(cs))).toEqual(["none", "low", "mid", "high"]);
  });

  it("breaks ties by the cell key (hash, tone, tier, segment, audience)", () => {
    const cs = [cell("b"), cell("a", { audience: 300 }), cell("a", { audience: 30 }), cell("a", { hash: "h0" })];
    const out = worstFirst(cs).map((c) => `${c.cell.instrument_hash}/${c.cell.tone}/${c.cell.audience_size}`);
    expect(out).toEqual(["h0/a/30", "h1/a/30", "h1/a/300", "h1/b/30"]);
  });

  it("does not mutate its input", () => {
    const cs = [cell("z"), cell("y", { rules: { a: rule({ fail: 1 }) } })];
    const before = tones(cs);
    worstFirst(cs);
    expect(tones(cs)).toEqual(before);
  });
});

describe("meanQuality / qualityColumns / heatOf", () => {
  it("averages only dimensions with a rate", () => {
    expect(meanQuality(cell("x", { quality: { a: q(1, 2), b: q(0, 0), c: q(1, 1) } }))).toBeCloseTo(0.75);
    expect(meanQuality(cell("x"))).toBeUndefined();
  });

  it("orders quality columns by aggregate rate, worst first", () => {
    const res = {
      cells: [cell("x", { quality: { humor: q(2, 10), pacing: q(9, 10), tone: q(5, 10) } }), cell("y", { quality: { humor: q(4, 10), pacing: q(9, 10) } })],
    };
    expect(qualityColumns(res).map((c) => c.dimension)).toEqual(["humor", "tone", "pacing"]);
    expect(qualityColumns(res)[0]).toMatchObject({ trueCount: 6, n: 20, rate: 0.3 });
  });

  it("bands a rate; no data is neutral", () => {
    expect(heatOf(q(9, 10))).toBe("good");
    expect(heatOf(q(7, 10))).toBe("warn");
    expect(heatOf(q(1, 10))).toBe("bad");
    expect(heatOf(q(0, 0))).toBe("none");
    expect(heatOf(undefined)).toBe("none");
  });
});
