import { describe, expect, it } from "vitest";
import { readConstitution, readFoil, readTone, parseBlocks, parseDoc, ruleBody, splitRule } from "./doc";
import {
  avatarSpec,
  factsLine,
  failingRuns,
  foilName,
  foilShort,
  hash32,
  parseRef,
  rosterKicker,
  rosterWindow,
  selectedIndex,
  shortName,
  situationTallies,
  stepIndex,
  toneLabel,
} from "./model";
import { crumbLabel, readSub } from "./routes";
import { genResults, genRoster, malakorDetail, malakorSummary } from "./testgen";

describe("the roster", () => {
  it("draws everyone up to twenty, then a window of nineteen around the selection plus +N", () => {
    expect(rosterWindow(1, 0)).toEqual({ start: 0, end: 1, more: 0 });
    expect(rosterWindow(20, 19)).toEqual({ start: 0, end: 20, more: 0 });
    expect(rosterWindow(2000, 0)).toEqual({ start: 0, end: 19, more: 1981 });
    expect(rosterWindow(2000, 1000)).toEqual({ start: 991, end: 1010, more: 1981 });
    expect(rosterWindow(2000, 1999)).toEqual({ start: 1981, end: 2000, more: 1981 });
    for (const at of [0, 7, 500, 1999]) {
      const w = rosterWindow(2000, at);
      expect(at >= w.start && at < w.end).toBe(true);
    }
  });

  it("selects the remembered id, else the first, and wraps the arrow keys", () => {
    const rows = genRoster(5);
    expect(selectedIndex(rows, undefined)).toBe(0);
    expect(selectedIndex(rows, "char-0003")).toBe(3);
    expect(selectedIndex(rows, "gone")).toBe(0);
    expect(stepIndex(4, 1, 5)).toBe(0);
    expect(stepIndex(0, -1, 5)).toBe(4);
    expect(stepIndex(0, 1, 0)).toBe(0);
  });

  it("prints the name plate kicker, short names and the facts line from character.list", () => {
    expect(rosterKicker(0, 1)).toBe("01 of 1 · the first character");
    expect(rosterKicker(11, 2000)).toBe("12 of 2,000");
    expect(shortName("Arch-Lich Malakor")).toBe("Malakor");
    expect(shortName("Warden Number 3")).toBe("Number");
    expect(factsLine(malakorSummary())).toBe("5 tones · 5 foils · 2 of 2 optional rules on");
    expect(factsLine(malakorSummary({ tones: ["a"], foils: [], rules_total: 0, rules_enabled: 0 }))).toBe("1 tone · 0 foils");
    expect(toneLabel("weary-middle-manager")).toBe("Weary middle manager");
  });

  it("names foils from their heading, else from the id", () => {
    const d = malakorDetail();
    expect(d.foils.map(foilName)).toEqual(["The Coin Prophet", "The Disruptor", "The Has-Been Hero", "The Inspector", "The Life Coach"]);
    expect(foilName({ id: "the-has-been-hero" })).toBe("The Has Been Hero");
    expect(foilShort("The Disruptor")).toBe("Disruptor");
  });

  it("seeds the generic avatar exactly like the winner (FNV-1a, xorshift32)", () => {
    expect(hash32("malakor")).toBe(hash32("malakor"));
    expect(hash32("")).toBe(2166136261);
    const a = avatarSpec(1);
    expect(avatarSpec(1)).toEqual(a);
    expect(a.head).toBeGreaterThanOrEqual(0);
    expect(a.head).toBeLessThan(6);
    expect(a.accent).toMatch(/^hsl\(\d+,55%,58%\)$/);
  });
});

describe("situation verdicts from results.json", () => {
  it("sums every cell's situation:* tallies and keeps failing refs once", () => {
    const t = situationTallies(genResults(3, 2));
    expect(t).not.toBeNull();
    const first = malakorDetail().situations[0]?.id ?? "";
    expect(t?.get(first)).toMatchObject({ pass: 9, fail: 6, not_provoked: 3, not_evaluable: 0 });
    expect(t?.get(first)?.failRefs).toHaveLength(6);
    expect(failingRuns(t?.get(first))).toEqual(["run-0", "run-1", "run-2"]);
    expect(t?.size).toBe(malakorDetail().situations.length);
  });

  it("ignores non-situation rules and malformed tallies, and rejects a reply without cells", () => {
    const t = situationTallies({ cells: [{ rules: { "red-line:doxxing": { pass: 4, fail: 0 }, "situation:x": { pass: "no", fail: 1 }, "situation:y": 7 } }] });
    expect([...(t?.keys() ?? [])]).toEqual(["x"]);
    expect(t?.get("x")).toMatchObject({ pass: 0, fail: 1 });
    expect(situationTallies({ tournament_id: "t" })).toBeNull();
  });

  it("splits a transcript ref", () => {
    expect(parseRef("theatrical.30.r1#42")).toEqual({ run: "theatrical.30.r1", line: "42" });
    expect(parseRef("run-a")).toEqual({ run: "run-a" });
    expect(parseRef("run#x")).toEqual({ run: "run#x" });
  });
});

describe("routes", () => {
  it("reads every nested page and rejects the rest", () => {
    expect(readSub(["all"])).toEqual({ kind: "all" });
    expect(readSub(["malakor"])).toEqual({ kind: "package", id: "malakor", tab: "constitution" });
    expect(readSub(["malakor", "package"])).toEqual({ kind: "package", id: "malakor", tab: "constitution" });
    expect(readSub(["malakor", "package", "rules"])).toEqual({ kind: "package", id: "malakor", tab: "rules" });
    expect(readSub(["malakor", "package", "situations", "empty-room"])).toEqual({ kind: "package", id: "malakor", tab: "situations", situation: "empty-room" });
    for (const bad of [["all", "x"], ["malakor", "nope"], ["malakor", "package", "nope"], ["malakor", "package", "rules", "x"], ["malakor", "package", "situations", "a", "b"]]) {
      expect(readSub(bad).kind).toBe("invalid");
    }
  });

  it("labels the breadcrumbs", () => {
    expect(crumbLabel("all", 1, ["characters", "all"])).toBe("All characters");
    expect(crumbLabel("malakor", 1, ["characters", "malakor"])).toBeUndefined();
    expect(crumbLabel("package", 2, [])).toBe("Package");
    expect(crumbLabel("rules", 3, [])).toBe("Optional rules");
    expect(crumbLabel("empty-room", 4, ["characters", "malakor", "package", "situations", "empty-room"])).toBe("empty-room");
  });
});

describe("package markdown", () => {
  it("parses headings, paragraphs, nested lists and continuation lines", () => {
    const b = parseBlocks("# T\n\nOne\ntwo.\n\n- a: x\n  more\n- b:\n  - b1\n  - b2\n\n1. first\n   cont\n2. second\n");
    expect(b).toEqual([
      { kind: "h", level: 1, text: "T" },
      { kind: "p", text: "One two." },
      { kind: "ul", items: [{ text: "a: x more", children: [] }, { text: "b:", children: [{ text: "b1", children: [] }, { text: "b2", children: [] }] }] },
      { kind: "ol", items: [{ text: "first cont", children: [] }, { text: "second", children: [] }] },
    ]);
    expect(parseDoc("# T\n\nPre.\n\n## A\n\nx\n\n## B\n").sections.map((s) => s.title)).toEqual(["A", "B"]);
  });

  it("reads the real constitution: nine red lines, locked apart from the prose", () => {
    const c = readConstitution(malakorDetail().constitution);
    expect(c.title).toBe("Constitution: Arch-Lich Malakor");
    expect(c.redLines).toHaveLength(9);
    expect(c.redLines[0]).toMatchObject({ n: 1, short: "No hateful conduct" });
    expect(c.redLines[7]?.short).toBe("He never claims to be human.");
    expect(c.redLinesNote).toBe("never crossed, in any tone, under any vote");
    expect(c.sections.map((s) => s.title)).toEqual(["Who he is", "What he knows (awareness)", "How he talks"]);
    expect(splitRule("plain")).toEqual({ short: "plain", text: "" });
  });

  it("reads a tone, a foil and a rule body", () => {
    const d = malakorDetail();
    const w = readTone("weary-middle-manager", d.tones.find((t) => t.id === "weary-middle-manager")?.text ?? "");
    expect(w.title).toBe("weary-middle-manager");
    expect(w.ratio).toMatch(/^about 55% performance/);
    expect(w.edge).toMatch(/^medium/);
    expect(w.moves.length).toBeGreaterThanOrEqual(5);
    expect(w.examples).toHaveLength(5);
    expect(w.examples[0]).toMatch(/^The Lord's dispatch says/);
    expect(w.bet).toMatch(/^That viewers bond with an underdog/);
    const th = readTone("theatrical", d.tones.find((t) => t.id === "theatrical")?.text ?? "");
    expect(th).toMatchObject({ title: "theatrical", baseline: true });
    expect(th.moves).toHaveLength(1);
    const f = readFoil(d.foils[0]?.text ?? "");
    expect(f.fn).toMatch(/^hype without substance/);
    expect(f.redLines).toMatch(/^never named after/);
    expect(ruleBody(d.rules[0]?.text ?? "")).toEqual([expect.objectContaining({ kind: "p" })]);
  });
});

describe("the default tournament for situation verdicts", () => {
  it("is the one with the most finished runs, ties going last", async () => {
    const { mostEvidence } = await import("./SituationsTab");
    const row = (tournament_id: string, completed: number) => ({ tournament_id, completed, runs: 120, instrument_hash: "h" });
    expect(mostEvidence([row("night-0dd8", 8), row("pilot-d1ef", 4), row("weary-0dd8", 57)])).toBe("weary-0dd8");
    expect(mostEvidence([row("a", 4), row("b", 4)])).toBe("b");
    expect(mostEvidence([])).toBeUndefined();
  });
});
