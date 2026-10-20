// derive.ts and buckets.ts on the hand-written `review.session` fixture: stall lines, the mix,
// judge fails and facts by line, then rows, show and gate marks, and folding.
import { describe, expect, it } from "vitest";
import { buckets, fold, isQuiet, mergeMarks, rowOfLine, type Bucket } from "./buckets";
import { derive, feedbackByLine, lineOf, segmentAt } from "./derive";
import { feedbackRow, SID, smallSession } from "./fixture";
import { HUMOUR, KINDS, type ShownKind } from "./names";

const ALL = new Set<ShownKind>(KINDS.map(([k]) => k));

describe("derive", () => {
  const { rs, lines } = smallSession();
  const d = derive(rs);

  it("lists Malakor's lines with the tagger's kind, by transcript line", () => {
    expect(d.says).toHaveLength(10);
    expect(d.sayByLine.get(lines.roast)?.kind).toBe("roast");
    expect(d.sayByLine.get(lines.roast)?.tag?.why).toBe("mocks the viewer's question");
    expect(d.sayByLine.get(lines.welcome)?.replyingTo).toEqual(["c1"]);
    expect(d.chatById.get("c1")?.line).toBe(lines.askChat);
    expect(d.lengthS).toBe(290);
  });

  it("marks the line before an engine stall as a stall line, whatever its tag", () => {
    expect(d.sayByLine.get(lines.stall)?.kind).toBe("stall");
    expect(d.sayByLine.get(lines.stall)?.tag?.kind).toBe("other");
    expect(d.mix.stall).toBe(1);
    expect(d.mix.other).toBeUndefined();
  });

  it("counts the mix per shown kind", () => {
    expect(d.mix).toEqual({ welcome: 1, roast: 2, joke: 1, stall: 1, callout: 1, show: 1, care: 1, disclosure: 1, signoff: 1 });
  });

  it("maps the judge's failed rules and quality facts onto lines", () => {
    expect(d.failLines.get(lines.failing)).toEqual(["situation:edgy-roast-request"]);
    expect(d.failLines.size).toBe(1);
    expect(d.facts.get(lines.roast)).toEqual([
      ["humor_landed", true],
      ["in_character", false],
    ]);
    expect(lineOf(`${SID}#12`)).toBe(12);
    expect(lineOf("bad#x")).toBeUndefined();
  });

  it("shows untagged lines as untagged", () => {
    const untagged = { ...smallSession().rs };
    delete untagged.tags;
    expect(new Set(derive(untagged).says.map((s) => s.kind))).toEqual(new Set(["untagged", "stall"]));
  });

  it("folds feedback rows: an empty row clears its line", () => {
    const m = feedbackByLine([feedbackRow(4, ["landed"]), feedbackRow(5, ["flat"]), { ...feedbackRow(4, []), at_ms: 2 }]);
    expect([...m.keys()]).toEqual([5]);
  });

  it("finds the segment of a stream second", () => {
    expect(segmentAt(rs, 10)).toBe("SEG-01");
    expect(segmentAt(rs, 250)).toBe("SEG-02A");
  });
});

describe("timeline rows", () => {
  const { rs, lines } = smallSession();
  const d = derive(rs);
  const B = buckets(rs, d, 30);

  it("cuts the session into rows of the chosen size", () => {
    expect(B).toHaveLength(10);
    expect(B[0]?.says.map((s) => s.line)).toEqual([lines.welcome, lines.roast, lines.joke, lines.stall]);
    expect(B[0]?.chat).toBe(2);
    expect(buckets(rs, d, 10)).toHaveLength(30);
    expect(buckets(rs, d, 120)).toHaveLength(3);
    expect(rowOfLine(rs, lines.failing, 30, B.length)).toBe(7);
    expect(rowOfLine(rs, 9_999, 30, B.length)).toBeUndefined();
  });

  it("marks show ops, handovers and engine health changes", () => {
    expect(B[0]?.show).toEqual(["poll"]);
    expect(B[1]?.show).toEqual(["engine_down", "engine_back"]);
    expect(B[6]?.show).toEqual(["handover"]);
    expect(mergeMarks(B[7]?.show ?? [])).toEqual([{ id: "reveal", n: 2 }]);
    expect(B[9]?.show).toEqual(["sign_off"]);
  });

  it("marks gates by group and the judge's fails", () => {
    expect(B[1]?.gates).toEqual({ alleged: 1 });
    expect(B[6]?.gates).toEqual({ care: 1 });
    expect(B[7]?.gates).toEqual({ persona: 1 });
    expect(B[7]?.fails).toBe(1);
    // The stall cover is engine cover, not a gate.
    expect(B[0]?.gates).toEqual({});
  });

  it("folds two or more quiet rows and keeps a lone quiet row", () => {
    const items = fold(B, ALL);
    expect(items.map((it) => (it.kind === "fold" ? `fold ${it.from}-${it.to}` : it.b.i))).toEqual([0, 1, "fold 60-180", 6, 7, 8, 9]);
  });

  it("counts a row of hidden kinds alone as quiet", () => {
    const B10 = buckets(rs, d, 10);
    // 00:20-00:30 holds only the stall line.
    expect(B10[2]?.says.map((s) => s.kind)).toEqual(["stall"]);
    expect(isQuiet(B10[2] as Bucket, ALL)).toBe(false);
    expect(isQuiet(B10[2] as Bucket, new Set<ShownKind>(HUMOUR))).toBe(true);
    // A show mark keeps a row whatever the filter.
    expect(isQuiet(B10[1] as Bucket, new Set<ShownKind>())).toBe(false);
  });
});
