import { describe, expect, it } from "vitest";
import { parseRef, refLabel } from "../../kit/events";
import { barCount, caution, incidentGroups, MAX_BARS, outcomeCount, outcomeIndex, outcomeOrder, parseDiff, smallSample, sourceLabel, triptych, VERDICT_TONE } from "./model";
import { BIBLE_DIFF, CONSTITUTION_DIFF, genOutcome, genProposals, genReport } from "./testgen";

describe("the triptych", () => {
  it("counts held and failed evidence across segments, and the proposals", () => {
    expect(triptych(genReport(), genProposals(6))).toEqual({ held: 14, failed: 13, proposed: 6 });
  });

  it("caps the bars, never the numeral", () => {
    expect(barCount(14)).toBe(14);
    expect(barCount(2_000)).toBe(MAX_BARS);
    expect(barCount(-1)).toBe(0);
  });
});

describe("incident groups", () => {
  it("folds alleged:<hex> into one rule and sorts most frequent first", () => {
    const groups = incidentGroups(genReport({ incidents: 9 }));
    expect(groups).toEqual([
      { rule: "alleged", n: 3 },
      { rule: "rate_cap", n: 3 },
      { rule: "situation:fake-lord-dispatch", n: 3 },
    ]);
  });
});

describe("the caution slip", () => {
  it("reads the judge errors and the small samples from the report", () => {
    expect(caution(genReport())).toBe("Handle with care: all 4 judge calls errored and most rates rest on small samples.");
  });

  it("says nothing when there is nothing to caution", () => {
    const r = genReport();
    const clean = {
      ...r,
      latency: { ...r.latency, by_role: [{ role: "judge" as const, calls: 4, errors: 0, fallbacks: 0 }] },
      tone: [{ tone_variant: "theatrical", quality_rates: { humor_landed: { rate: 0.6, n: 120, bound: { lo: 0.5, hi: 0.7, method: "wilson" as const } } } }],
    };
    expect(caution(clean)).toBeUndefined();
  });
});

describe("refs and sources", () => {
  it("parses run#line, keeping tournament-qualified run ids whole", () => {
    expect(parseRef("pilot-1/theatrical.sonnet.SEG-01.a30.s1#245")).toEqual({ runId: "pilot-1/theatrical.sonnet.SEG-01.a30.s1", line: 245 });
    expect(parseRef("no-line")).toBeUndefined();
    expect(parseRef("run#x")).toBeUndefined();
    expect(refLabel("run-1#12")).toBe("L12 · run-1");
  });

  it("labels a source", () => {
    expect(sourceLabel({ tournament_id: "t1" })).toBe("tournament t1");
    expect(sourceLabel({ run_ids: ["a"] })).toBe("1 run");
  });
});

describe("diffs", () => {
  it("names the files and classifies lines", () => {
    const d = parseDiff(BIBLE_DIFF);
    expect(d.files).toEqual(["bible.md"]);
    expect(d.touchesConstitution).toBe(false);
    expect(d.lines.map((l) => l.kind)).toEqual(["file", "file", "hunk", "ctx", "add", "del", "ctx"]);
  });

  it("flags a diff that touches the constitution", () => {
    expect(parseDiff(CONSTITUTION_DIFF).touchesConstitution).toBe(true);
    expect(parseDiff("--- a/characters/malakor/constitution.md\n+++ b/characters/malakor/constitution.md\n").touchesConstitution).toBe(true);
  });
});

describe("directive outcomes", () => {
  it("indexes a report's outcomes by directive id, and nothing for a report without them", () => {
    const o = genOutcome("dir-1", "broken", 2, 4, { refs: ["run-1#12"] });
    const idx = outcomeIndex({ directives: [o] });
    expect(idx.size).toBe(1);
    expect(idx.get("dir-1")).toBe(o);
    expect(outcomeIndex({}).size).toBe(0);
    expect(outcomeIndex(undefined).size).toBe(0);
  });

  it("orders worst first: broken, ignored, unmeasured, attempted, held", () => {
    const list = [
      genOutcome("h", "held", 4, 4),
      genOutcome("a", "attempted", 1, 3, { kind: "try" }),
      genOutcome("u", "unmeasured", 0, 0),
      genOutcome("i", "ignored", 0, 3, { kind: "try" }),
      genOutcome("b", "broken", 2, 4),
    ];
    expect(outcomeOrder(list).map((o) => o.verdict)).toEqual(["broken", "ignored", "unmeasured", "attempted", "held"]);
    expect(list[0]?.verdict).toBe("held");
  });

  it("counts k of n from the rate and marks a small sample with the caution rule", () => {
    expect(outcomeCount(genOutcome("b", "broken", 2, 4))).toEqual({ k: 2, n: 4 });
    expect(outcomeCount(genOutcome("u", "unmeasured", 0, 0))).toBeUndefined();
    expect(smallSample(genOutcome("h", "held", 1, 1))).toBe(true);
    expect(smallSample(genOutcome("h", "held", 30, 30))).toBe(false);
    expect(smallSample(genOutcome("u", "unmeasured", 0, 0))).toBe(false);
  });

  it("tones a broken directive as a failure and a held one as ok", () => {
    expect(VERDICT_TONE.broken).toBe("fail");
    expect(VERDICT_TONE.held).toBe("ok");
    expect(VERDICT_TONE.unmeasured).toBe("muted");
  });
});
