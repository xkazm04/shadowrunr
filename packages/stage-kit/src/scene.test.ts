import { describe, expect, it } from "vitest";

import type { Case, ChatMsg, Command, Rank, ShownRecord, StreamEvent } from "@shadowrunr/contract";
import { captionAt, enqueueCaption, foldEvents, matchOption, reduce, shownLane } from "./scene";
import {
  BROWSER_TABS_MAX,
  CAPTION_QUEUE_MAX,
  CHAT_EVICT_SLACK,
  CHAT_MEMORY_MAX,
  DECREE_MAX,
  LANE_MAX,
  QUEUED_HOLD_MS,
  RECORD_MAX,
  defaultScreen,
  initialScene,
  isContentScreen,
  mapFocusOf,
  type LayoutId,
} from "./sceneModel";
import { wordTimings } from "./timing";
import { gradeTallies, pollBars } from "./tallies";

function msg(id: string, rank: Rank = "thrall", command?: Command, viewerId = `v-${id}`): ChatMsg {
  const m: ChatMsg = {
    id,
    ts_ms: 0,
    viewer: { id: viewerId, name: `name-${id}`, platform: "sim", rank, months: 0, gifted: 0, is_mod: false },
    text: "hello",
  };
  if (command) m.command = command;
  return m;
}
const chat = (ts: number, m: ChatMsg): StreamEvent => ({ type: "chat", ts_ms: ts, msg: { ...m, ts_ms: ts } });
const op = (ts: number, o: Extract<StreamEvent, { type: "op" }>["op"]): StreamEvent => ({ type: "op", ts_ms: ts, op: o });

describe("layouts and segments", () => {
  it("opens each segment on its default screen", () => {
    expect(defaultScreen("SEG-01")).toBe("host");
    expect(defaultScreen("SEG-02A")).toBe("hexmap");
    expect(defaultScreen("SEG-02B-I")).toBe("host");
    const all: LayoutId[] = ["boot", "host", "hexmap", "browser", "desk"];
    expect(all.filter(isContentScreen)).toEqual(["hexmap", "browser", "desk"]);
  });

  it("starts in boot, switches on segment_start, and animates a handover", () => {
    let s = initialScene();
    expect(s.layout).toBe("boot");
    s = reduce(s, { type: "stream_start", ts_ms: 0, stream_id: "s", character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 3 });
    expect(s.meta?.audienceSize).toBe(3);
    s = reduce(s, { type: "segment_start", ts_ms: 100, segment: "SEG-01" });
    expect(s.layout).toBe("host");
    expect(s.screen).toBe("host");
    expect(s.transition).toMatchObject({ from: "boot", to: "host", cause: "segment_start", segment: "SEG-01", at: 100 });
    s = reduce(s, op(200, { op: "switch_segment", to: "SEG-02A" }));
    expect(s.pendingSwitch).toEqual({ to: "SEG-02A", at: 200 });
    s = reduce(s, { type: "segment_end", ts_ms: 300, segment: "SEG-01", ses: 0.5 });
    expect(s.seals).toEqual([{ segment: "SEG-01", ses: 0.5, at: 300 }]);
    s = reduce(s, { type: "handover", ts_ms: 400, from: "SEG-01", to: "SEG-02A", reason: "dare" });
    expect(s.layout).toBe("hexmap");
    expect(s.segment).toBe("SEG-02A");
    expect(s.pendingSwitch).toBeNull();
    const handoverTransition = s.transition;
    expect(handoverTransition).toMatchObject({ from: "host", to: "hexmap", cause: "handover", segment: "SEG-02A", reason: "dare" });
    // The segment_start that follows a handover does not trigger a second transition.
    s = reduce(s, { type: "segment_start", ts_ms: 500, segment: "SEG-02A" });
    expect(s.transition).toBe(handoverTransition);
  });

  it("records stream_end and sign_off", () => {
    let s = reduce(initialScene(), op(1, { op: "sign_off", reason: "sealed" }));
    expect(s.outro).toEqual({ reason: "sealed", at: 1 });
    s = reduce(s, { type: "stream_end", ts_ms: 2, reason: "sim_complete" });
    expect(s.ended).toEqual({ reason: "sim_complete", at: 2 });
  });
});

describe("screens", () => {
  const rec = (id: string, hex = "HEX-01"): ShownRecord => ({
    id,
    hex,
    grade: "REP",
    title: `Record ${id}`,
    source: `https://records.example/${id}`,
    summary: "a fictional record",
    fictional: true,
  });
  const shown = (ts: number, r: ShownRecord): StreamEvent => ({ type: "record_shown", ts_ms: ts, record: r });
  const screen = (ts: number, sc: "host" | "hexmap" | "browser" | "desk", record_id?: string): StreamEvent =>
    op(ts, record_id === undefined ? { op: "show_screen", screen: sc } : { op: "show_screen", screen: sc, record_id });
  const seg02a: StreamEvent[] = [
    { type: "segment_start", ts_ms: 0, segment: "SEG-01" },
    { type: "handover", ts_ms: 10, from: "SEG-01", to: "SEG-02A", reason: "dare" },
    { type: "segment_start", ts_ms: 11, segment: "SEG-02A" },
  ];

  it("keeps boot before the first segment and opens each segment on its default screen", () => {
    expect(initialScene()).toMatchObject({ layout: "boot", screen: "host", records: {}, tabs: [], activeRecord: null, pendingScreen: null });
    expect(foldEvents(seg02a, 0)).toMatchObject({ layout: "host", screen: "host" });
    expect(foldEvents(seg02a)).toMatchObject({ layout: "hexmap", screen: "hexmap" });
    // A transcript that starts in SEG-02A (no handover) still gets the hexmap.
    expect(foldEvents([{ type: "segment_start", ts_ms: 0, segment: "SEG-02A" }]).layout).toBe("hexmap");
  });

  it("opens a record in the browser when record_shown comes first, and moves it to the desk", () => {
    let s = foldEvents([...seg02a, shown(20, rec("REP-01")), screen(21, "browser", "REP-01")]);
    expect(s).toMatchObject({ layout: "browser", screen: "browser", tabs: ["REP-01"], activeRecord: "REP-01", pendingScreen: null });
    expect(s.records["REP-01"]?.title).toBe("Record REP-01");
    expect(s.transition).toMatchObject({ from: "hexmap", to: "browser", cause: "screen", at: 21 });
    expect(s.transition?.segment).toBeUndefined();
    s = reduce(s, screen(22, "desk"));
    expect(s).toMatchObject({ layout: "desk", activeRecord: "REP-01" });
    s = reduce(s, screen(23, "hexmap"));
    expect(s).toMatchObject({ layout: "hexmap", tabs: ["REP-01"], activeRecord: "REP-01" });
    s = reduce(s, screen(24, "host"));
    expect(s.layout).toBe("host");
  });

  it("waits on hexmap for a record named before it arrives (the opposite order)", () => {
    let s = foldEvents([...seg02a, screen(20, "browser", "REP-02")]);
    expect(s).toMatchObject({ layout: "hexmap", screen: "hexmap", pendingScreen: { screen: "browser", recordId: "REP-02", at: 20 } });
    s = reduce(s, shown(21, rec("REP-01")));
    expect(s.layout).toBe("hexmap");
    expect(s.tabs).toEqual([]);
    s = reduce(s, shown(25, rec("REP-02")));
    expect(s).toMatchObject({ layout: "browser", tabs: ["REP-02"], activeRecord: "REP-02", pendingScreen: null });
    expect(s.transition).toMatchObject({ to: "browser", cause: "screen", at: 25 });
  });

  it("opens the latest record for a browser or desk without a record_id, or waits for the next one", () => {
    let s = foldEvents([...seg02a, screen(20, "desk")]);
    expect(s).toMatchObject({ layout: "hexmap", pendingScreen: { screen: "desk", at: 20 } });
    expect(s.pendingScreen?.recordId).toBeUndefined();
    s = reduce(s, shown(21, rec("REP-01")));
    expect(s).toMatchObject({ layout: "desk", activeRecord: "REP-01" });
    s = reduce(s, shown(22, rec("REP-02")));
    s = reduce(s, screen(23, "hexmap"));
    s = reduce(s, screen(24, "browser"));
    // The active record stays the one the window had open.
    expect(s).toMatchObject({ layout: "browser", activeRecord: "REP-01", tabs: ["REP-01"] });
    // With nothing open yet, the latest record received.
    const fresh = foldEvents([...seg02a, shown(20, rec("REP-01")), shown(21, rec("REP-02")), screen(22, "browser")]);
    expect(fresh).toMatchObject({ layout: "browser", activeRecord: "REP-02" });
  });

  it("a host or hexmap switch drops a waiting screen, and so does a handover", () => {
    let s = foldEvents([...seg02a, screen(20, "browser", "REP-09"), screen(21, "host")]);
    expect(s).toMatchObject({ layout: "host", pendingScreen: null });
    s = reduce(s, shown(22, rec("REP-09")));
    expect(s.layout).toBe("host");
    s = reduce(s, screen(23, "browser", "REP-10"));
    expect(s.pendingScreen).not.toBeNull();
    s = reduce(s, { type: "handover", ts_ms: 30, from: "SEG-02A", to: "SEG-01", reason: "back" });
    expect(s).toMatchObject({ layout: "host", pendingScreen: null });
  });

  it("keeps at most BROWSER_TABS_MAX tabs, most recent first, and re-opening moves a tab to the front", () => {
    let s = foldEvents(seg02a);
    for (let i = 1; i <= BROWSER_TABS_MAX + 2; i++) {
      s = reduce(s, shown(20 + i, rec(`REP-0${i}`)));
      s = reduce(s, screen(20 + i, "browser", `REP-0${i}`));
    }
    expect(s.tabs).toEqual(["REP-06", "REP-05", "REP-04", "REP-03"]);
    s = reduce(s, screen(40, "browser", "REP-04"));
    expect(s.tabs).toEqual(["REP-04", "REP-06", "REP-05", "REP-03"]);
    expect(s.activeRecord).toBe("REP-04");
  });

  it("remembers at most RECORD_MAX records and never forgets an open tab", () => {
    let s = foldEvents([...seg02a, shown(12, rec("KEEP")), screen(13, "browser", "KEEP")]);
    for (let i = 0; i < RECORD_MAX + 10; i++) s = reduce(s, shown(20 + i, rec(`R${i}`)));
    expect(Object.keys(s.records)).toHaveLength(RECORD_MAX);
    expect(s.records.KEEP).toBeDefined();
    expect(s.records[`R${RECORD_MAX + 9}`]).toBeDefined();
    expect(s.records.R0).toBeUndefined();
  });

  it("a record_shown alone opens nothing", () => {
    const s = foldEvents([...seg02a, shown(20, rec("REP-01"))]);
    expect(s).toMatchObject({ layout: "hexmap", tabs: [], activeRecord: null });
  });

  it("zooms the map to a focus on hexmap and desk, the desk to its record's hex, and back to the overview", () => {
    const focus = (ts: number, sc: "host" | "hexmap" | "browser" | "desk", f: string, record_id?: string): StreamEvent =>
      op(ts, record_id === undefined ? { op: "show_screen", screen: sc, focus: f } : { op: "show_screen", screen: sc, record_id, focus: f });
    let s = foldEvents([...seg02a, focus(20, "hexmap", "HEX-04")]);
    expect(s).toMatchObject({ layout: "hexmap", mapFocus: "HEX-04" });
    expect(mapFocusOf(s)).toBe("HEX-04");
    // A new focus on the same screen is no screen transition: the map just moves.
    const before = s.transition;
    s = reduce(s, focus(21, "hexmap", "HEX-01"));
    expect(s.transition).toBe(before);
    expect(mapFocusOf(s)).toBe("HEX-01");
    // No focus: the overview.
    s = reduce(s, screen(22, "hexmap"));
    expect(mapFocusOf(s)).toBeNull();
    // The desk shows its record's hex unless told otherwise; a focus on the browser is ignored.
    s = reduce(s, shown(23, rec("DOC-03", "HEX-07")));
    s = reduce(s, screen(24, "desk", "DOC-03"));
    expect(mapFocusOf(s)).toBe("HEX-07");
    s = reduce(s, focus(25, "desk", "HEX-02", "DOC-03"));
    expect(mapFocusOf(s)).toBe("HEX-02");
    s = reduce(s, focus(26, "browser", "HEX-05", "DOC-03"));
    expect(s.mapFocus).toBeNull();
    expect(mapFocusOf(s)).toBeNull();
    // A handover drops it.
    s = reduce(s, focus(27, "hexmap", "HEX-05"));
    s = reduce(s, { type: "handover", ts_ms: 30, from: "SEG-02A", to: "SEG-01", reason: "back" });
    expect(s.mapFocus).toBeNull();
  });
});

describe("chat, decrees, moderation", () => {
  it("keeps a bounded lane of recent messages", () => {
    let s = initialScene();
    for (let i = 0; i < LANE_MAX + 5; i++) s = reduce(s, chat(i, msg(`m${i}`)));
    expect(s.lane).toHaveLength(LANE_MAX);
    expect(s.lane[s.lane.length - 1]).toBe(`m${LANE_MAX + 4}`);
  });

  it("lifts a highlighted message into a decree card and ignores unknown ids", () => {
    let s = reduce(initialScene(), chat(1, msg("m1", "lich")));
    s = reduce(s, op(2, { op: "highlight", msg_id: "m1" }));
    expect(s.decrees).toHaveLength(1);
    expect(s.decrees[0]).toMatchObject({ msgId: "m1", kind: "highlight", at: 2 });
    const before = s;
    s = reduce(s, op(3, { op: "highlight", msg_id: "nope" }));
    expect(s.decrees).toEqual(before.decrees);
  });

  it("marks picked messages as heard and lifts them into decrees, capped", () => {
    let s = initialScene();
    for (const id of ["a", "b", "c", "d"]) s = reduce(s, chat(1, msg(id)));
    s = reduce(s, { type: "picked", ts_ms: 5, msg_ids: ["a", "b", "c", "d", "ghost"], reason: "funny" });
    expect([...s.heard.keys()].sort()).toEqual(["a", "b", "c", "d", "ghost"]);
    expect(s.decrees).toHaveLength(DECREE_MAX);
    expect(s.decrees.map((d) => d.msgId)).toEqual(["b", "c", "d"]);
    expect(s.decrees[0]).toMatchObject({ kind: "picked", reason: "funny" });
  });

  it("draws a line only once the input filter's beat has judged it", () => {
    let s = reduce(initialScene(), chat(100, msg("slur")));
    s = reduce(s, chat(200, msg("ok")));
    // Between arriving and the beat, the dropped line's text must not reach the screen.
    expect(shownLane(s)).toEqual([]);
    s = reduce(s, { type: "moderation", ts_ms: 5000, stage: "input", rule: "term:slur", msg_id: "slur", action: "dropped" });
    // The beat's verdicts may still be arriving: only the line it named is drawn, as purged.
    expect(shownLane(s).map((l) => l.id)).toEqual(["slur"]);
    s = reduce(s, { type: "say", ts_ms: 5000, speaker: "malakor", text: "Next." });
    expect(shownLane(s).map((l) => l.id)).toEqual(["slur"]);
    s = reduce(s, chat(5200, msg("late")));
    expect(shownLane(s).map((l) => l.id)).toEqual(["slur", "ok"]);
    expect(shownLane(s)[1]).toEqual({ id: "ok", shownAt: 5200 });
    expect(s.purged).toEqual({ slur: "term:slur" });
    // A line after the beat waits for the next one, unless that beat named it.
    s = reduce(s, { type: "picked", ts_ms: 10000, msg_ids: ["late"], reason: "top" });
    expect(shownLane(s).map((l) => l.id)).toEqual(["slur", "ok", "late"]);
    s = reduce(s, chat(10100, msg("next")));
    expect(shownLane(s)[1]).toEqual({ id: "ok", shownAt: 200 });
  });

  it("a stall or name notice with no message does not release unjudged lines", () => {
    let s = reduce(initialScene(), chat(100, msg("m1")));
    s = reduce(s, { type: "moderation", ts_ms: 3000, stage: "input", rule: "name_withheld", action: "withheld" });
    expect(shownLane(s)).toEqual([]);
  });

  it("purges only what the filter withheld, never a line it let through", () => {
    let s = initialScene();
    for (const id of ["ex", "rep", "warn", "tag", "safe"]) s = reduce(s, chat(1, msg(id)));
    const mod = (id: string, rule: string, action: string): StreamEvent => ({ type: "moderation", ts_ms: 5, stage: "input", rule, msg_id: id, action });
    s = reduce(s, mod("ex", "rate_cap", "exempt:sincere_question"));
    s = reduce(s, mod("rep", "flood_cluster", "collapsed:7"));
    s = reduce(s, mod("warn", "term:mild", "warned"));
    s = reduce(s, mod("tag", "prompt_injection", "tagged"));
    s = reduce(s, mod("safe", "safety:self_harm", "priority:safety"));
    expect(Object.keys(s.purged).sort()).toEqual(["safe", "tag"]);
    s = reduce(s, { type: "picked", ts_ms: 5, msg_ids: ["ex"], reason: "question" });
    expect(s.decrees.map((d) => d.msgId)).toEqual(["ex"]);
  });

  it("input moderation purges a message and removes its decree; output moderation redacts", () => {
    let s = reduce(initialScene(), chat(1, msg("m1")));
    s = reduce(s, op(2, { op: "highlight", msg_id: "m1" }));
    s = reduce(s, { type: "moderation", ts_ms: 3, stage: "input", rule: "anti-brigade", msg_id: "m1", action: "withheld" });
    expect(s.purged).toEqual({ m1: "anti-brigade" });
    expect(s.decrees).toEqual([]);
    s = reduce(s, op(4, { op: "highlight", msg_id: "m1" }));
    expect(s.decrees).toEqual([]);
    s = reduce(s, { type: "moderation", ts_ms: 5, stage: "output", rule: "alleged-qualifier-missing", action: "held" });
    expect(s.redactions).toHaveLength(1);
    expect(s.redactions[0]).toMatchObject({ at: 5, rule: "alleged-qualifier-missing" });
  });
});

describe("say", () => {
  it("routes Malakor's lines to the caption with reply names, foil lines to the foil", () => {
    let s = reduce(initialScene(), chat(1, msg("m1")));
    s = reduce(s, { type: "say", ts_ms: 2, speaker: "malakor", text: "Rise.", replying_to: ["m1", "ghost"] });
    expect(s.caption).toMatchObject({ text: "Rise.", at: 2, replyingTo: ["name-m1"] });
    s = reduce(s, op(3, { op: "summon_foil", foil: "the-inspector" }));
    expect(s.foil).toEqual({ id: "the-inspector", since: 3 });
    s = reduce(s, { type: "say", ts_ms: 4, speaker: "foil", foil: "the-inspector", text: "Aha!" });
    expect(s.foilLine).toMatchObject({ foil: "the-inspector", text: "Aha!" });
    expect(s.caption?.text).toBe("Rise.");
    s = reduce(s, op(5, { op: "dismiss_foil" }));
    expect(s.foil).toBeNull();
    expect(s.foilLine).toBeNull();
  });
});

describe("polls", () => {
  const start = op(10, { op: "start_poll", poll_id: "p", question: "Next?", options: ["HEX-04", "HEX-03"], min_rank: "acolyte", secs: 30 });

  it("opens a poll, collects rank-weighted ballots from chat, and closes", () => {
    let s = reduce(initialScene(), start);
    expect(s.activePoll).toBe("p");
    s = reduce(s, chat(11, msg("a", "dread_council", { cmd: "vote", option: "HEX-04" })));
    s = reduce(s, chat(12, msg("b", "cultist", { cmd: "vote", option: "hex-03" })));
    s = reduce(s, chat(13, msg("c", "thrall", { cmd: "vote", option: "HEX-03" }))); // below min rank
    s = reduce(s, chat(14, msg("d", "acolyte", { cmd: "vote", option: "2" }))); // by index
    const p = s.polls.p!;
    expect(p.observed["HEX-04"]).toEqual({ dread_council: 2.4 });
    expect(p.observed["HEX-03"]).toEqual({ cultist: 1.7, acolyte: 1.4 });

    // Before any tally arrives, bars come from observed chat ballots.
    let bars = pollBars(p);
    expect(bars.map((b) => b.weight)).toEqual([2.4, 3.1]);
    expect(bars[1]!.leading).toBe(true);
    expect(bars[1]!.bands.map((b) => b.rank)).toEqual(["acolyte", "cultist"]);

    // The authoritative tally sets lengths; observed chat sets the colour bands.
    s = reduce(s, { type: "vote", ts_ms: 15, poll_id: "p", tallies: [{ option: "HEX-04", voters: 5, weight: 9 }, { option: "HEX-03", voters: 2, weight: 3 }], closed: false });
    bars = pollBars(s.polls.p!);
    expect(bars[0]).toMatchObject({ weight: 9, voters: 5, length: 1, leading: true });
    expect(bars[1]!.length).toBeCloseTo(1 / 3);
    expect(bars[0]!.pct).toBeCloseTo(0.75);

    s = reduce(s, op(16, { op: "close_poll", poll_id: "p" }));
    expect(s.polls.p!.closedAt).toBe(16);
    s = reduce(s, { type: "vote", ts_ms: 17, poll_id: "p", tallies: [{ option: "HEX-04", voters: 5, weight: 9 }], closed: true });
    expect(s.polls.p!.closedAt).toBe(16);
    // Ballots after close are ignored.
    const after = reduce(s, chat(18, msg("e", "lich", { cmd: "vote", option: "HEX-03" })));
    expect(after.polls.p!.observed).toEqual(s.polls.p!.observed);
  });

  it("moves a changed vote instead of doubling it, and counts !summon as a ballot", () => {
    let s = reduce(initialScene(), op(1, { op: "start_poll", poll_id: "f", question: "Foil?", options: ["the-disruptor", "the-inspector"], min_rank: "thrall", secs: 40 }));
    s = reduce(s, chat(2, msg("a", "warlock", { cmd: "summon", foil: "the-disruptor" }, "same")));
    s = reduce(s, chat(3, msg("b", "warlock", { cmd: "summon", foil: "the-inspector" }, "same")));
    const p = s.polls.f!;
    expect(p.observed["the-disruptor"]).toEqual({ warlock: 0 });
    expect(p.observed["the-inspector"]).toEqual({ warlock: 2 });
    expect(pollBars(p)[0]!.bands).toEqual([{ rank: "unattributed", share: 1 }]);
  });

  it("shows a tally for a poll it never saw open", () => {
    const s = reduce(initialScene(), { type: "vote", ts_ms: 1, poll_id: "late", tallies: [{ option: "x", voters: 1, weight: 1 }], closed: true });
    expect(s.polls.late).toMatchObject({ options: ["x"], closedAt: 1 });
    expect(s.activePoll).toBe("late");
  });

  it("matches options by text, index and unique prefix", () => {
    const o = ["the-disruptor", "the-inspector", "HEX-04"];
    expect(matchOption(o, "HEX-04")).toBe("HEX-04");
    expect(matchOption(o, "hex-04")).toBe("HEX-04");
    expect(matchOption(o, "2")).toBe("the-inspector");
    expect(matchOption(o, "9")).toBeNull();
    expect(matchOption(o, "the-d")).toBe("the-disruptor");
    expect(matchOption(o, "the")).toBeNull();
    expect(matchOption(o, " ")).toBeNull();
  });
});

describe("hexmap state", () => {
  it("reveals hexes, collects hints and community grades, and flags disputes", () => {
    let s = initialScene();
    s = reduce(s, chat(1, msg("h1", "acolyte", { cmd: "hint", hex: "HEX-04", text: "wallet" })));
    s = reduce(s, chat(2, msg("h2", "lich", { cmd: "hint", hex: "HEX-04", text: "the wallet!" })));
    expect(s.hints["HEX-04"]).toMatchObject({ count: 2, lastText: "the wallet!", lastBy: "name-h2" });
    s = reduce(s, op(3, { op: "reveal_hex", hex: "HEX-04", grade: "SPC" }));
    s = reduce(s, op(4, { op: "reveal_hex", hex: "HEX-01", grade: "DOC" }));
    s = reduce(s, op(5, { op: "reveal_hex", hex: "HEX-04", grade: "SPC" }));
    expect(s.revealOrder).toEqual(["HEX-01", "HEX-04"]);
    expect(s.hexes["HEX-04"]).toEqual({ grade: "SPC", revealedAt: 5 });
    for (const [i, g] of (["CLM", "CLM", "CLM", "SPC"] as const).entries()) {
      s = reduce(s, chat(6 + i, msg(`g${i}`, "thrall", { cmd: "grade", hex: "HEX-04", grade: g })));
    }
    s = reduce(s, chat(20, msg("g9", "thrall", { cmd: "grade", hex: "HEX-01", grade: "DOC" })));
    const t = gradeTallies(s);
    const h4 = t.find((x) => x.hex === "HEX-04")!;
    expect(h4.counts).toEqual({ DOC: 0, REP: 0, CLM: 3, SPC: 1 });
    expect(h4).toMatchObject({ total: 4, majority: "CLM", official: "SPC", disputed: true });
    const h1 = t.find((x) => x.hex === "HEX-01")!;
    expect(h1).toMatchObject({ majority: "DOC", official: "DOC", disputed: false });
  });

  it("counts one grade per viewer and lets the runtime's jury tally decide", () => {
    let s = initialScene();
    s = reduce(s, op(1, { op: "start_poll", poll_id: "p", question: "Foil?", options: ["A", "B"], min_rank: "thrall", secs: 30 }));
    s = reduce(s, chat(2, msg("a", "thrall", { cmd: "grade", hex: "HEX-03", grade: "DOC" })));
    for (let i = 0; i < 3; i++) s = reduce(s, chat(3 + i, msg(`s${i}`, "thrall", { cmd: "grade", hex: "HEX-03", grade: "SPC" }, "spammer")));
    // Before the jury tally: one ballot each, and a level jury leads with nothing.
    expect(gradeTallies(s)[0]).toMatchObject({ counts: { DOC: 1, REP: 0, CLM: 0, SPC: 1 }, majority: null });
    // The jury tally is rank-weighted and never takes the vote panel.
    s = reduce(s, { type: "vote", ts_ms: 9, poll_id: "grade:HEX-03", closed: false, tallies: [{ option: "DOC", voters: 1, weight: 1 }, { option: "REP", voters: 0, weight: 0 }, { option: "CLM", voters: 1, weight: 2.4 }, { option: "SPC", voters: 1, weight: 1 }] });
    expect(s.activePoll).toBe("p");
    expect(s.polls["grade:HEX-03"]).toBeUndefined();
    expect(gradeTallies(s)[0]).toMatchObject({ counts: { DOC: 1, REP: 0, CLM: 1, SPC: 1 }, total: 3, majority: "CLM" });
  });
});

describe("poll outcome on screen", () => {
  const closedWith = (tallies: { option: string; voters: number; weight: number }[]) => {
    let s = reduce(initialScene(), op(1, { op: "start_poll", poll_id: "p", question: "?", options: tallies.map((t) => t.option), min_rank: "thrall", secs: 30 }));
    s = reduce(s, { type: "vote", ts_ms: 2, poll_id: "p", closed: true, tallies });
    return pollBars(s.polls.p!);
  };

  it("breaks a weight tie toward more voters, as the runtime does", () => {
    const bars = closedWith([{ option: "A", voters: 1, weight: 2.4 }, { option: "B", voters: 2, weight: 2.4 }]);
    expect(bars.map((b) => [b.leading, b.tied])).toEqual([[false, false], [true, false]]);
  });

  it("marks options level on weight and voters as tied, with no winner", () => {
    const bars = closedWith([{ option: "A", voters: 2, weight: 2.4 }, { option: "B", voters: 2, weight: 2.4 }, { option: "C", voters: 1, weight: 1 }]);
    expect(bars.map((b) => [b.leading, b.tied])).toEqual([[true, true], [true, true], [false, false]]);
  });
});

describe("case_open", () => {
  const hex = (id: string, q: number, r: number) => ({ id, q, r, kind: "entity" as const, label: `label-${id}`, fog: true });
  const real: Case = { id: "C-REAL", title: "The Transcript's Case", fictional: true, hexes: [hex("HEX-01", 2, -1), hex("HEX-07", 0, 0)] };
  const other: Case = { id: "C-OTHER", title: "Another Case", fictional: true, hexes: [hex("HEX-07", 0, 0)] };
  const events: StreamEvent[] = [
    { type: "segment_start", ts_ms: 0, segment: "SEG-01" },
    { type: "handover", ts_ms: 10, from: "SEG-01", to: "SEG-02A", reason: "dare" },
    { type: "segment_start", ts_ms: 11, segment: "SEG-02A" },
    { type: "case_open", ts_ms: 11, case: real },
    op(20, { op: "reveal_hex", hex: "HEX-07", grade: "DOC" }),
  ];

  it("stores the transcript's case in the scene from its case_open onward", () => {
    expect(initialScene().caseFile).toBeNull();
    expect(foldEvents(events, 10).caseFile).toBeNull();
    const s = foldEvents(events);
    expect(s.caseFile).toBe(real);
    expect(s.layout).toBe("hexmap");
    expect(s.revealOrder).toEqual(["HEX-07"]);
  });

  it("a later case_open replaces the case", () => {
    const s = foldEvents([...events, { type: "case_open", ts_ms: 30, case: other }]);
    expect(s.caseFile?.id).toBe("C-OTHER");
  });

  it("leaves the case empty for transcripts without case_open", () => {
    expect(foldEvents(events.filter((e) => e.type !== "case_open")).caseFile).toBeNull();
  });
});

describe("jobs, cards, moments", () => {
  it("merges dispatch_research and job events into one ticker entry", () => {
    let s = reduce(initialScene(), op(1, { op: "dispatch_research", job_id: "r-1", brief: "trace the wallet" }));
    expect(s.jobs[0]).toMatchObject({ id: "r-1", brief: "trace the wallet", status: "dispatched", at: 1 });
    s = reduce(s, { type: "job", ts_ms: 2, job_id: "r-1", status: "running" });
    s = reduce(s, { type: "job", ts_ms: 3, job_id: "r-1", status: "done", summary: "two hops" });
    expect(s.jobs).toHaveLength(1);
    expect(s.jobs[0]).toMatchObject({ brief: "trace the wallet", status: "done", summary: "two hops", at: 1, updatedAt: 3 });
    s = reduce(s, { type: "job", ts_ms: 4, job_id: "mem-1", status: "running" });
    expect(s.jobs.map((j) => j.id)).toEqual(["r-1", "mem-1"]);
  });

  it("shows the latest card and records clip moments", () => {
    let s = reduce(initialScene(), op(1, { op: "show_card", card_id: "c1", title: "A", body: "x" }));
    s = reduce(s, op(2, { op: "show_card", card_id: "c2", title: "B", body: "y" }));
    expect(s.card).toEqual({ id: "c2", title: "B", body: "y", at: 2 });
    s = reduce(s, op(3, { op: "mark_moment", label: "roast" }));
    expect(s.moments).toEqual([{ label: "roast", at: 3 }]);
  });
});

describe("foldEvents", () => {
  it("folds up to a timestamp", () => {
    const evs: StreamEvent[] = [
      { type: "segment_start", ts_ms: 0, segment: "SEG-01" },
      { type: "handover", ts_ms: 10, from: "SEG-01", to: "SEG-02A", reason: "r" },
    ];
    expect(foldEvents(evs, 5).layout).toBe("host");
    expect(foldEvents(evs).layout).toBe("hexmap");
    expect(foldEvents(evs).applied).toBe(2);
  });

  it("never mutates the previous state", () => {
    const s0 = reduce(initialScene(), chat(1, msg("m1")));
    const snapshot = JSON.stringify(s0);
    reduce(s0, op(2, { op: "highlight", msg_id: "m1" }));
    reduce(s0, op(3, { op: "start_poll", poll_id: "p", question: "q", options: ["a"], min_rank: "thrall", secs: 5 }));
    expect(JSON.stringify(s0)).toBe(snapshot);
  });
});

describe("a 100k-line transcript (10× scale)", () => {
  it("folds with bounded chat memory", () => {
    const viewer = { id: "v", name: "v", platform: "sim" as const, rank: "thrall" as const };
    const events: StreamEvent[] = [];
    for (let i = 0; i < 100_000; i++) {
      const t = i * 40;
      events.push(
        i % 10 === 3
          ? { type: "picked", ts_ms: t, msg_ids: [`m${i - 1}`], reason: "top 1" }
          : { type: "chat", ts_ms: t, msg: { id: `m${i}`, ts_ms: t, text: "x", viewer } },
      );
    }
    const s = foldEvents(events);
    expect(s.applied).toBe(100_000);
    expect(s.chats.size).toBeLessThanOrEqual(CHAT_MEMORY_MAX + CHAT_EVICT_SLACK);
    expect(s.heard.size).toBeLessThanOrEqual(s.chats.size);
    expect(s.lane.every((id) => s.chats.has(id))).toBe(true);
  });
});

describe("caption queue", () => {
  const say = (ts: number, text: string): StreamEvent => ({ type: "say", ts_ms: ts, speaker: "malakor", text });

  it("shows a lone caption when it is said", () => {
    const s = foldEvents([say(1000, "Rise.")]);
    expect(captionAt(s, 999)).toBeNull();
    expect(captionAt(s, 1000)?.text).toBe("Rise.");
    expect(s.caption?.text).toBe("Rise.");
  });

  it("queues a burst: each waits until the one before has revealed and held 2 s", () => {
    const a = "The ledger opens, minions, and every line of it is mine.";
    const s = foldEvents([say(0, a), say(100, "Second."), say(200, "Third.")]);
    expect(s.captions.map((c) => c.text)).toEqual([a, "Second.", "Third."]);
    const [first, second, third] = s.captions;
    expect(second!.at).toBe(wordTimings(a).spokenMs + QUEUED_HOLD_MS);
    expect(third!.at).toBe(second!.at + wordTimings("Second.").spokenMs + QUEUED_HOLD_MS);
    // The first holds for its whole reveal: the burst does not overwrite it.
    expect(captionAt(s, first!.spokenMs - 1)?.text).toBe(a);
    expect(captionAt(s, second!.at)?.text).toBe("Second.");
    expect(captionAt(s, third!.at + 10_000)?.text).toBe("Third.");
    // `scene.caption` stays the latest line said, as the console reads it.
    expect(s.caption?.text).toBe("Third.");
  });

  it("does not delay a line said after the one before has revealed and held", () => {
    const s = foldEvents([say(0, "Rise."), say(60_000, "Kneel.")]);
    expect(s.captions.map((c) => [c.text, c.at])).toEqual([
      ["Rise.", 0],
      ["Kneel.", 60_000],
    ]);
    const later = reduce(s, say(120_000, "Pay."));
    expect(later.captions.map((c) => c.text)).toEqual(["Kneel.", "Pay."]);
  });

  it("keeps at most 3 waiting and skips the oldest waiting, never the one showing", () => {
    const evs = [say(0, "One, the showing line, which is long enough to hold.")];
    for (let i = 2; i <= 7; i++) evs.push(say(i, `Line ${i}.`));
    const s = foldEvents(evs);
    expect(s.captions.map((c) => c.text)).toEqual([
      "One, the showing line, which is long enough to hold.",
      "Line 5.",
      "Line 6.",
      "Line 7.",
    ]);
    expect(s.captions.length).toBe(1 + CAPTION_QUEUE_MAX);
    // Rescheduled after the skip: the first waiting follows the showing one directly.
    expect(s.captions[1]!.at).toBe(s.captions[0]!.spokenMs + QUEUED_HOLD_MS);
    // A queued caption shows long enough to reveal every word.
    for (let i = 1; i < s.captions.length; i++) {
      const prev = s.captions[i - 1]!;
      expect(s.captions[i]!.at - prev.at).toBeGreaterThanOrEqual(prev.spokenMs);
    }
  });

  it("enqueueCaption drops captions that have been followed", () => {
    let q = enqueueCaption([], { id: 1, text: "A.", at: 0, replyingTo: [] });
    q = enqueueCaption(q, { id: 2, text: "B.", at: 50_000, replyingTo: [] });
    q = enqueueCaption(q, { id: 3, text: "C.", at: 50_001, replyingTo: [] });
    expect(q.map((c) => c.id)).toEqual([2, 3]);
  });
});
