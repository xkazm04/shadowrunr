import { describe, expect, it } from "vitest";
import { segmentsLabel } from "./index";
import { commandRows, descoped, glance, handoverMarks, lifecycleColumns, opGrid, parseVotes, pyramid, qualifies, START_VOTES, tally, TICKS_PER_COLUMN } from "./model";
import { BASE_SEGMENTS, genRanks, genTables } from "./testgen";

describe("lifecycle", () => {
  it("puts the pilots in the pilot column and keeps descoped out of retired", () => {
    const cols = lifecycleColumns(BASE_SEGMENTS);
    expect(cols.map((c) => c.status)).toEqual(["pilot", "trial", "regular", "flagship", "retired"]);
    expect(cols[0]?.shown.map((s) => s.id)).toEqual(["SEG-01", "SEG-02A"]);
    expect(cols[4]?.total).toBe(0);
    expect(descoped(BASE_SEGMENTS).map((s) => s.id)).toEqual(["SEG-02B-I", "SEG-02B-II"]);
    expect(glance(BASE_SEGMENTS)).toEqual(["2 pilot", "2 descoped"]);
  });

  it("caps every column at scale and counts the rest", () => {
    const segs = genTables(2_000).segments;
    const cols = lifecycleColumns(segs);
    for (const c of cols) expect(c.shown.length).toBeLessThanOrEqual(TICKS_PER_COLUMN);
    const total = cols.reduce((a, c) => a + c.total, 0) + descoped(segs).length;
    expect(total).toBe(segs.length);
    expect(cols.reduce((a, c) => a + c.shown.length + c.more, 0)).toBe(segs.length - descoped(segs).length);
  });
});

describe("the rank pyramid", () => {
  it("stands apex first with the winner's widths and the ledge under the lowest proposing rank", () => {
    const steps = pyramid(genRanks());
    expect(steps.map((s) => s.rank)).toEqual(["dread_council", "lich", "warlock", "cultist", "acolyte", "thrall"]);
    expect(steps.map((s) => s.widthPct)).toEqual([28, 42, 56, 70, 84, 98]);
    expect(steps.filter((s) => s.proposes).map((s) => s.rank)).toEqual(["dread_council", "lich", "warlock"]);
    expect(steps.filter((s) => s.ledge).map((s) => s.rank)).toEqual(["warlock"]);
  });

  it("words the qualifying conditions", () => {
    const [thrall, , cultist, warlock] = genRanks().ranks;
    expect(thrall && qualifies(thrall)).toEqual({ tiers: "", gifted: "" });
    expect(cultist && qualifies(cultist).tiers).toBe("tier 1, 3+ months · tier 2");
    expect(warlock && qualifies(warlock).gifted).toBe("5+ gifted");
  });
});

describe("commands and ops", () => {
  it("lists where each command is accepted and gates !propose", () => {
    const rows = commandRows(genTables());
    const by = Object.fromEntries(rows.map((r) => [r.name, r]));
    expect(by.summon?.segments).toEqual(["SEG-01"]);
    expect(by.vote?.segments).toEqual(["SEG-01", "SEG-02A"]);
    expect(by.propose?.minRank).toBe("warlock");
    expect(by.hint?.minRank).toBeUndefined();
    expect(by.grade?.args).toBe("<hex> <DOC|REP|CLM|SPC>");
    expect(by.summon?.what).toContain("guest foil");
  });

  it("grids every op against the runnable segments only", () => {
    const g = opGrid(genTables());
    expect(g.columns.map((s) => s.id)).toEqual(["SEG-01", "SEG-02A"]);
    const row = (op: string) => g.rows.find((r) => r.op === op);
    expect(row("summon_foil")?.allowed).toEqual([true, false]);
    expect(row("reveal_hex")?.allowed).toEqual([false, true]);
    expect(row("sign_off")?.count).toBe(2);
  });
});

describe("the vote calculator", () => {
  it("shows the head-count and the weight disagreeing on the winner's start", () => {
    const t = tally(genRanks().ranks, START_VOTES);
    expect(t.heads).toEqual({ a: 6, b: 5 });
    expect(t.weight.a).toBeCloseTo(6);
    expect(t.weight.b).toBeCloseTo(2 + 4 + 2.4);
    expect(t.verdict).toBe("Head-count: A wins. Weight: B wins.");
  });

  it("clamps typed votes", () => {
    expect(parseVotes("")).toBe(0);
    expect(parseVotes("-4")).toBe(0);
    expect(parseVotes("7.9")).toBe(7);
    expect(parseVotes("100000")).toBe(999);
    expect(parseVotes("abc")).toBe(0);
  });
});

it("places the handover marks on the ruler", () => {
  const m = handoverMarks({ sustain_s: 180, max_elapsed_s: 1500, grace_s: 60 });
  expect(m.endS).toBe(1560);
  expect(m.sustainPct).toBeCloseTo((180 / 1560) * 100);
  expect(m.maxPct).toBeCloseTo((1500 / 1560) * 100);
});

it("labels the breadcrumbs", () => {
  const parts = ["segments", "tables", "SEG-01"];
  expect(segmentsLabel("tables", 1, parts)).toBe("Segment tables");
  expect(segmentsLabel("SEG-01", 2, parts)).toBe("SEG-01");
  expect(segmentsLabel("rec-1", 2, ["segments", "ranks", "rec-1"])).toBe("Report rec-1");
});
