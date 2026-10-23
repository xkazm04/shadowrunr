import { describe, expect, it } from "vitest";
import { parseBlocks, parseDoc } from "../../ui/doc";
import { readConstitution, readFoil, readTone, ruleBody, splitRule } from "./packageDocs";
import {
  avatarSpec,
  factsLine,
  failingRuns,
  foilName,
  foilShort,
  hash32,
  offeredTournaments,
  pickTournament,
  rosterKicker,
  rosterWindow,
  selectedIndex,
  shortName,
  situationTallies,
  stepIndex,
  toneEvidence,
  toneLabel,
} from "./model";
import { crumbLabel, readSub } from "./routes";
import { genResults, genRoster, genToneResults, malakorDetail, malakorSummary } from "./testgen";
import { parseRef, refRun } from "../../kit/events";

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
    expect(shortName("Clerk-Bailiff Vellum")).toBe("Vellum");
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

  it("ignores rules that are not situations", () => {
    const r = genResults(1);
    const cell = r.cells[0];
    if (!cell) throw new Error("no cell");
    const t = situationTallies({ ...r, cells: [{ ...cell, rules: { "red-line:doxxing": { pass: 4, fail: 0, not_provoked: 0, not_evaluable: 0, n: 4 }, "situation:x": { pass: 0, fail: 1, not_provoked: 0, not_evaluable: 0, n: 1 } } }] });
    expect([...t.keys()]).toEqual(["x"]);
    expect(t.get("x")).toMatchObject({ pass: 0, fail: 1 });
  });

  it("reads a transcript ref's run, with or without a line", () => {
    expect(parseRef("theatrical.30.r1#42")).toEqual({ runId: "theatrical.30.r1", line: 42 });
    expect(refRun("theatrical.30.r1#42")).toBe("theatrical.30.r1");
    expect(refRun("run-a")).toBe("run-a");
    expect(refRun("run#x")).toBe("run#x");
  });
});

describe("routes", () => {
  it("reads every nested page and rejects the rest", () => {
    expect(readSub(["all"])).toEqual({ kind: "all" });
    expect(readSub(["malakor"])).toEqual({ kind: "package", id: "malakor", tab: "constitution" });
    expect(readSub(["malakor", "package"])).toEqual({ kind: "package", id: "malakor", tab: "constitution" });
    expect(readSub(["malakor", "package", "rules"])).toEqual({ kind: "package", id: "malakor", tab: "rules" });
    expect(readSub(["malakor", "package", "situations", "empty-room"])).toEqual({ kind: "package", id: "malakor", tab: "situations", situation: "empty-room" });
    expect(readSub(["malakor", "package", "tone-lab"])).toEqual({ kind: "package", id: "malakor", tab: "tone-lab" });
    expect(readSub(["malakor", "package", "tone-lab", "sbx-1", "12"])).toEqual({ kind: "package", id: "malakor", tab: "tone-lab", from: { session: "sbx-1", line: 12 } });
    for (const bad of [["all", "x"], ["malakor", "nope"], ["malakor", "package", "nope"], ["malakor", "package", "rules", "x"], ["malakor", "package", "situations", "a", "b"], ["malakor", "package", "tone-lab", "sbx-1"], ["malakor", "package", "tone-lab", "sbx-1", "0"], ["malakor", "package", "tone-lab", "sbx-1", "x"], ["malakor", "package", "tone-lab", "sbx-1", "2", "3"]]) {
      expect(readSub(bad).kind).toBe("invalid");
    }
  });

  it("labels the breadcrumbs", () => {
    expect(crumbLabel("all", 1, ["characters", "all"])).toBe("All characters");
    expect(crumbLabel("malakor", 1, ["characters", "malakor"])).toBeUndefined();
    expect(crumbLabel("package", 2, [])).toBe("Package");
    expect(crumbLabel("rules", 3, [])).toBe("Optional rules");
    expect(crumbLabel("empty-room", 4, ["characters", "malakor", "package", "situations", "empty-room"])).toBe("empty-room");
    expect(crumbLabel("tone-lab", 3, [])).toBe("Tone Lab");
    expect(crumbLabel("sbx-1", 4, ["characters", "malakor", "package", "tone-lab", "sbx-1", "12"])).toBe("From Review");
    expect(crumbLabel("12", 5, ["characters", "malakor", "package", "tone-lab", "sbx-1", "12"])).toBe("Line 12");
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
  const row = (tournament_id: string, o: { character?: string; updated_ms?: number; completed?: number } = {}) => ({
    tournament_id,
    completed: o.completed ?? 4,
    runs: 120,
    instrument_hash: "h",
    ...(o.character === undefined ? {} : { character: o.character }),
    ...(o.updated_ms === undefined ? {} : { updated_ms: o.updated_ms }),
  });

  it("is this character's newest tournament, not the one with the most runs", () => {
    const big = row("malakor-big", { character: "malakor", updated_ms: 100, completed: 40 });
    const fresh = row("malakor-fresh", { character: "malakor", updated_ms: 200, completed: 3 });
    expect(pickTournament([big, fresh], "malakor")?.tournament_id).toBe("malakor-fresh");
    expect(pickTournament([fresh, big], "malakor")?.tournament_id).toBe("malakor-fresh");
  });

  it("offers a row without a character but never defaults to it, and never to another character's", () => {
    const unknown = row("adhoc", { updated_ms: 900, completed: 90 });
    const other = row("kestrel-new", { character: "kestrel", updated_ms: 950 });
    const own = row("malakor-old", { character: "malakor", updated_ms: 100 });
    expect(pickTournament([unknown, other, own], "malakor")?.tournament_id).toBe("malakor-old");
    expect(pickTournament([unknown, other], "malakor")).toBeUndefined();
    expect(offeredTournaments([unknown, other, own], "malakor").map((r) => r.tournament_id)).toEqual(["malakor-old", "adhoc"]);
    expect(pickTournament([], "malakor")).toBeUndefined();
  });
});

describe("each tone's measured record (toneEvidence)", () => {
  it("folds cells by tone with the simulator's per-cell counts and keeps tones apart", () => {
    const results = genToneResults("t1", {
      "gleeful-chaos": [
        { judged: 2, fail: 1, redLine: 1 },
        { judged: 4, fail: 2 },
      ],
      theatrical: [{ judged: 1 }],
    });
    const ev = toneEvidence(results);
    // Every rule is checked once per judged run: two rules, 6 runs -> 12 evaluated checks; the
    // red-line fail is also a rule fail (1 voice + 2 voice + 1 red line = 4).
    expect(ev.get("gleeful-chaos")).toEqual({ tone: "gleeful-chaos", cells: 2, judged: 6, fail: 4, evaluated: 12, redLine: 1 });
    expect(ev.get("theatrical")).toEqual({ tone: "theatrical", cells: 1, judged: 1, fail: 0, evaluated: 2, redLine: 0 });
    expect(ev.has("weary-middle-manager")).toBe(false);
  });
});
