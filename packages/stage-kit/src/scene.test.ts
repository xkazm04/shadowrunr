import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import type { Case, ChatMsg, Command, Rank, ShownRecord, StreamEvent } from "@shadowrunr/contract";
import { matchOption, observedTally } from "./ballots";
import { parseTranscript } from "./parse";
import { captionAt, enqueueCaption, foldEvents, reduce, shownLane } from "./scene";
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
    expect(defaultScreen("SEG-02A")).toBe("board");
    expect(defaultScreen("SEG-02B-I")).toBe("host");
    const all: LayoutId[] = ["boot", "host", "board", "hexmap", "browser", "desk"];
    expect(all.filter(isContentScreen)).toEqual(["board", "hexmap", "browser", "desk"]);
  });

  it("starts in boot, switches on segment_start, and animates a handover", () => {
    let s = initialScene();
    expect(s.layout).toBe("boot");
    s = reduce(s, { type: "stream_start", ts_ms: 0, stream_id: "s", character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 3 });
    expect(s.meta?.audienceSize).toBe(3);
    expect(s.meta?.hostName).toBeUndefined();
    expect(reduce(initialScene(), { type: "stream_start", ts_ms: 0, stream_id: "s", character: "keeper", tone_variant: "calm", core_tier: "haiku", audience_size: 3, host_name: "Odile" }).meta?.hostName).toBe("Odile");
    s = reduce(s, { type: "segment_start", ts_ms: 100, segment: "SEG-01" });
    expect(s.layout).toBe("host");
    expect(s.screen).toBe("host");
    expect(s.transition).toMatchObject({ from: "boot", to: "host", cause: "segment_start", segment: "SEG-01", at: 100 });
    s = reduce(s, op(200, { op: "switch_segment", to: "SEG-02A" }));
    expect(s.pendingSwitch).toEqual({ to: "SEG-02A", at: 200 });
    s = reduce(s, { type: "segment_end", ts_ms: 300, segment: "SEG-01", ses: 0.5 });
    expect(s.seals).toEqual([{ segment: "SEG-01", ses: 0.5, at: 300 }]);
    s = reduce(s, { type: "handover", ts_ms: 400, from: "SEG-01", to: "SEG-02A", reason: "dare" });
    expect(s.layout).toBe("board");
    expect(s.segment).toBe("SEG-02A");
    expect(s.pendingSwitch).toBeNull();
    const handoverTransition = s.transition;
    expect(handoverTransition).toMatchObject({ from: "host", to: "board", cause: "handover", segment: "SEG-02A", reason: "dare" });
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
    expect(foldEvents(seg02a)).toMatchObject({ layout: "board", screen: "board" });
    // A transcript that starts in SEG-02A (no handover) still gets the board.
    expect(foldEvents([{ type: "segment_start", ts_ms: 0, segment: "SEG-02A" }]).layout).toBe("board");
  });

  it("opens a record in the browser when record_shown comes first, and moves it to the desk", () => {
    let s = foldEvents([...seg02a, shown(20, rec("REP-01")), screen(21, "browser", "REP-01")]);
    expect(s).toMatchObject({ layout: "browser", screen: "browser", tabs: ["REP-01"], activeRecord: "REP-01", pendingScreen: null });
    expect(s.records["REP-01"]?.title).toBe("Record REP-01");
    expect(s.transition).toMatchObject({ from: "board", to: "browser", cause: "screen", at: 21 });
    expect(s.transition?.segment).toBeUndefined();
    s = reduce(s, screen(22, "desk"));
    expect(s).toMatchObject({ layout: "desk", activeRecord: "REP-01" });
    s = reduce(s, screen(23, "hexmap"));
    expect(s).toMatchObject({ layout: "hexmap", tabs: ["REP-01"], activeRecord: "REP-01" });
    s = reduce(s, screen(24, "host"));
    expect(s.layout).toBe("host");
  });

  it("waits on the board for a record named before it arrives (the opposite order)", () => {
    let s = foldEvents([...seg02a, screen(20, "browser", "REP-02")]);
    expect(s).toMatchObject({ layout: "board", screen: "board", pendingScreen: { screen: "browser", recordId: "REP-02", at: 20 } });
    s = reduce(s, shown(21, rec("REP-01")));
    expect(s.layout).toBe("board");
    expect(s.tabs).toEqual([]);
    s = reduce(s, shown(25, rec("REP-02")));
    expect(s).toMatchObject({ layout: "browser", tabs: ["REP-02"], activeRecord: "REP-02", pendingScreen: null });
    expect(s.transition).toMatchObject({ to: "browser", cause: "screen", at: 25 });
  });

  it("opens the latest record for a browser or desk without a record_id, or waits for the next one", () => {
    let s = foldEvents([...seg02a, screen(20, "desk")]);
    expect(s).toMatchObject({ layout: "board", pendingScreen: { screen: "desk", at: 20 } });
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
    expect(s).toMatchObject({ layout: "board", tabs: [], activeRecord: null });
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
    s = reduce(s, { type: "say", ts_ms: 5000, speaker: "host", text: "Next." });
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
    s = reduce(s, { type: "say", ts_ms: 2, speaker: "host", text: "Rise.", replying_to: ["m1", "ghost"] });
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

  it("moves a changed vote instead of doubling it, and counts !summon as a ballot in a foil vote", () => {
    let s = reduce(initialScene(), op(1, { op: "start_poll", poll_id: "foil-vote-1", question: "Foil?", options: ["the-disruptor", "the-inspector"], min_rank: "thrall", secs: 40 }));
    s = reduce(s, chat(2, msg("a", "warlock", { cmd: "summon", foil: "the-disruptor" }, "same")));
    s = reduce(s, chat(3, msg("b", "warlock", { cmd: "summon", foil: "the-inspector" }, "same")));
    const p = s.polls["foil-vote-1"]!;
    expect(p.observed["the-disruptor"]).toEqual({ warlock: 0 });
    expect(p.observed["the-inspector"]).toEqual({ warlock: 2 });
    expect(pollBars(p)[0]!.bands).toEqual([{ rank: "unattributed", share: 1 }]);
  });

  it("shows a tally for a poll it never saw open", () => {
    const s = reduce(initialScene(), { type: "vote", ts_ms: 1, poll_id: "late", tallies: [{ option: "x", voters: 1, weight: 1 }], closed: true });
    expect(s.polls.late).toMatchObject({ options: ["x"], closedAt: 1 });
    expect(s.activePoll).toBe("late");
  });

  // The runtime's rule (Poll::option_index): a unique prefix is NOT a ballot, so the screen may
  // not count one either. ballots.test.ts holds the screen to runtime-generated vectors.
  it("matches options by index and exact text, never by prefix, as the runtime does", () => {
    const o = ["the-disruptor", "the-inspector", "HEX-04"];
    expect(matchOption(o, "HEX-04")).toBe("HEX-04");
    expect(matchOption(o, "hex-04")).toBe("HEX-04");
    expect(matchOption(o, "2")).toBe("the-inspector");
    expect(matchOption(o, "9")).toBeNull();
    expect(matchOption(o, "the-d")).toBeNull();
    expect(matchOption(o, "the")).toBeNull();
    expect(matchOption(o, " ")).toBeNull();
    let s = reduce(initialScene(), op(1, { op: "start_poll", poll_id: "p", question: "?", options: o, min_rank: "thrall", secs: 30 }));
    s = reduce(s, chat(2, msg("a", "warlock", { cmd: "vote", option: "the-d" })));
    expect(s.polls.p!.ballots).toEqual({});
  });

  it("counts !summon only in a foil vote: a crowd poll ignores it", () => {
    let s = reduce(initialScene(), op(1, { op: "start_poll", poll_id: "p", question: "?", options: ["the-disruptor", "the-inspector"], min_rank: "thrall", secs: 30 }));
    s = reduce(s, chat(2, msg("a", "warlock", { cmd: "summon", foil: "the-disruptor" })));
    expect(s.polls.p!.ballots).toEqual({});
    s = reduce(s, chat(3, msg("b", "warlock", { cmd: "vote", option: "the-disruptor" })));
    expect(Object.keys(s.polls.p!.ballots)).toEqual(["v-b"]);
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
    expect(s.layout).toBe("board");
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
    expect(foldEvents(evs).layout).toBe("board");
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
  const say = (ts: number, text: string): StreamEvent => ({ type: "say", ts_ms: ts, speaker: "host", text });

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

// Guards for the ballot and grade books (stage-kit-scene-engine B): whatever structure holds a
// poll's ballots and a hex's grades, every derived view a page renders must stay exactly what the
// Record-based books produced, and every retained scene must stay valid after later folds.
describe("ballot and grade books", () => {
  const RANK_CYCLE: readonly Rank[] = ["thrall", "cultist", "acolyte", "warlock", "lich", "dread_council"];
  const GRADES = ["DOC", "REP", "CLM", "SPC"] as const;
  const voteLine = (ts: number, viewer: string, rank: Rank, option: string): StreamEvent =>
    chat(ts, msg(`m${ts}`, rank, { cmd: "vote", option }, viewer));
  const gradeLine = (ts: number, viewer: string, hex: string, grade: (typeof GRADES)[number]): StreamEvent =>
    chat(ts, msg(`m${ts}`, "thrall", { cmd: "grade", hex, grade }, viewer));

  /** A seeded show: two polls with re-votes, an under-rank and an unknown option, a mid-poll
   * tally, a foil vote opened by `!summon`, grades on three hexes with re-grades and a jury. */
  function syntheticShow(): StreamEvent[] {
    let seed = 0x5eed;
    const rnd = (n: number) => {
      seed = (seed * 1103515245 + 12345) >>> 0;
      return (seed >>> 8) % n;
    };
    const evs: StreamEvent[] = [];
    let t = 1;
    evs.push(op(t++, { op: "start_poll", poll_id: "p1", question: "?", options: ["HEX-01", "HEX-02", "HEX-03"], min_rank: "cultist", secs: 60 }));
    for (let i = 0; i < 400; i++) {
      const v = rnd(150);
      const opt = ["HEX-01", "HEX-02", "hex-03", "2", "HEX-09", " HEX-01 "][rnd(6)]!;
      evs.push(voteLine(t++, `v${v}`, RANK_CYCLE[v % 6]!, opt));
      if (i === 200) evs.push({ type: "vote", ts_ms: t++, poll_id: "p1", tallies: [{ option: "HEX-01", voters: 9, weight: 12.5 }, { option: "HEX-02", voters: 4, weight: 6 }, { option: "HEX-03", voters: 0, weight: 0 }], closed: false });
      const hex = ["HEX-01", "HEX-02", "HEX-05"][rnd(3)]!;
      evs.push(gradeLine(t++, `g${rnd(120)}`, hex, GRADES[rnd(4)]!));
    }
    evs.push(op(t++, { op: "close_poll", poll_id: "p1" }));
    evs.push(op(t++, { op: "reveal_hex", hex: "HEX-01", grade: "REP" }));
    evs.push(op(t++, { op: "reveal_hex", hex: "HEX-05", grade: "CLM" }));
    evs.push({ type: "vote", ts_ms: t++, poll_id: "grade:HEX-02", closed: false, tallies: [{ option: "DOC", voters: 3, weight: 3 }, { option: "REP", voters: 0, weight: 0 }, { option: "CLM", voters: 3, weight: 3 }, { option: "SPC", voters: 1, weight: 1 }] });
    evs.push(chat(t++, msg(`m${t}`, "warlock", { cmd: "summon", foil: "the-inspector" }, "f0")));
    evs.push(op(t++, { op: "start_poll", poll_id: "foil-vote-1", question: "Which foil?", options: ["the-disruptor", "the-inspector"], min_rank: "thrall", secs: 30 }));
    for (let i = 0; i < 300; i++) {
      const v = rnd(90);
      const cmd: Command = rnd(2) === 0 ? { cmd: "summon", foil: ["the-disruptor", "the-inspector", "1"][rnd(3)]! } : { cmd: "vote", option: ["the-disruptor", "THE-INSPECTOR", "3"][rnd(3)]! };
      evs.push(chat(t++, msg(`m${t}`, RANK_CYCLE[v % 6]!, cmd, `f${v}`)));
    }
    return evs;
  }

  function views(s: ReturnType<typeof initialScene>) {
    return {
      polls: Object.fromEntries(Object.entries(s.polls).map(([id, p]) => [id, { bars: pollBars(p), observed: observedTally(p) }])),
      grades: gradeTallies(s),
    };
  }

  /** FNV-1a over a string: one number standing for every intermediate view of a fold. */
  function fnv(text: string, h = 0x811c9dc5): number {
    for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193) >>> 0;
    return h;
  }

  /** Every intermediate scene's views hashed in order, then the final views. */
  function trace(events: readonly StreamEvent[]) {
    let s = initialScene();
    let h = 0x811c9dc5;
    for (const ev of events) {
      s = reduce(s, ev);
      h = fnv(JSON.stringify(views(s)), h);
    }
    return { hash: h, final: views(s) };
  }

  it("persistence: a retained scene's bars and grades never change when later ballots fold over it", () => {
    let s1 = reduce(initialScene(), op(1, { op: "start_poll", poll_id: "p", question: "?", options: ["A", "B", "C"], min_rank: "thrall", secs: 30 }));
    for (let i = 0; i < 100; i++) {
      s1 = reduce(s1, voteLine(2 + i, `v${i}`, RANK_CYCLE[i % 6]!, ["A", "B", "C"][i % 3]!));
      s1 = reduce(s1, gradeLine(200 + i, `g${i}`, "HEX-04", GRADES[i % 4]!));
    }
    const bars = structuredClone(pollBars(s1.polls.p!));
    const grades = structuredClone(gradeTallies(s1));
    const observed = structuredClone(observedTally(s1.polls.p!));
    let s2 = s1;
    for (let i = 0; i < 100; i++) {
      // Half re-votes by s1's voters (moving them), half new voters.
      const viewer = i % 2 === 0 ? `v${i}` : `w${i}`;
      s2 = reduce(s2, voteLine(400 + i, viewer, RANK_CYCLE[i % 6]!, ["C", "A", "B"][i % 3]!));
      s2 = reduce(s2, gradeLine(600 + i, i % 2 === 0 ? `g${i}` : `h${i}`, "HEX-04", GRADES[(i + 1) % 4]!));
    }
    expect(observedTally(s2.polls.p!)).not.toEqual(observed);
    expect(pollBars(s1.polls.p!)).toEqual(bars);
    expect(gradeTallies(s1)).toEqual(grades);
    expect(observedTally(s1.polls.p!)).toEqual(observed);
  });

  it("a re-vote after many other ballots moves the voter's weight rather than doubling it", () => {
    let s = reduce(initialScene(), op(1, { op: "start_poll", poll_id: "p", question: "?", options: ["A", "B"], min_rank: "thrall", secs: 30 }));
    s = reduce(s, voteLine(2, "v1", "warlock", "A"));
    for (let i = 0; i < 70; i++) s = reduce(s, voteLine(3 + i, `o${i}`, "thrall", "A"));
    const before = observedTally(s.polls.p!);
    expect(before).toEqual([
      { option: "A", voters: 71, weight: 70 + 2 },
      { option: "B", voters: 0, weight: 0 },
    ]);
    s = reduce(s, voteLine(100, "v1", "warlock", "B"));
    const after = observedTally(s.polls.p!);
    expect(after[0]).toMatchObject({ option: "A", voters: 70 });
    expect(after[0]!.weight).toBeCloseTo(70, 9);
    expect(after[1]).toMatchObject({ option: "B", voters: 1 });
    expect(after[1]!.weight).toBeCloseTo(2, 9);
    expect(after[0]!.weight + after[1]!.weight).toBeCloseTo(before[0]!.weight, 9);
    // A grade re-cast after many others moves, too.
    for (let i = 0; i < 70; i++) s = reduce(s, gradeLine(200 + i, i === 0 ? "j" : `k${i}`, "HEX-02", "DOC"));
    s = reduce(s, gradeLine(300, "j", "HEX-02", "SPC"));
    expect(gradeTallies(s)[0]).toMatchObject({ hex: "HEX-02", counts: { DOC: 69, REP: 0, CLM: 0, SPC: 1 }, total: 70 });
  });

  it("equivalence: the demo transcript and a seeded show fold to the views the Record books produced", () => {
    const demo = parseTranscript(readFileSync(join(__dirname, "..", "..", "..", "apps", "stage", "fixtures", "demo.jsonl"), "utf8"));
    expect(demo.errors).toEqual([]);
    const got = { demo: trace(demo.events), synthetic: trace(syntheticShow()) };
    expect(got).toEqual(BOOK_VIEWS);
  });
});

/** Captured from the Record-based books at 829eb0a, before the ViewerBook change. */

const BOOK_VIEWS: unknown = {"demo":{"hash":1123063819,"final":{"polls":{"poll-foil-1":{"bars":[{"option":"the-disruptor","weight":10.1,"voters":6,"bands":[{"rank":"unattributed","share":1}],"length":1,"pct":0.4999999999999999,"leading":true,"tied":false},{"option":"the-inspector","weight":5.7,"voters":3,"bands":[{"rank":"unattributed","share":1}],"length":0.5643564356435644,"pct":0.28217821782178215,"leading":false,"tied":false},{"option":"the-life-coach","weight":4.4,"voters":4,"bands":[{"rank":"unattributed","share":1}],"length":0.4356435643564357,"pct":0.21782178217821782,"leading":false,"tied":false}],"observed":[{"option":"the-disruptor","voters":0,"weight":0},{"option":"the-inspector","voters":0,"weight":0},{"option":"the-life-coach","voters":0,"weight":0}]},"poll-next-hex":{"bars":[{"option":"HEX-04","weight":7.4,"voters":4,"bands":[{"rank":"acolyte","share":0.37837837837837834},{"rank":"lich","share":0.2972972972972973},{"rank":"dread_council","share":0.3243243243243243}],"length":1,"pct":0.45679012345679004,"leading":true,"tied":false},{"option":"HEX-03","weight":5.7,"voters":3,"bands":[{"rank":"cultist","share":0.2982456140350877},{"rank":"warlock","share":0.7017543859649122}],"length":0.7702702702702703,"pct":0.3518518518518518,"leading":false,"tied":false},{"option":"HEX-07","weight":3.1,"voters":2,"bands":[{"rank":"acolyte","share":0.4516129032258065},{"rank":"cultist","share":0.5483870967741936}],"length":0.4189189189189189,"pct":0.191358024691358,"leading":false,"tied":false}],"observed":[{"option":"HEX-04","voters":4,"weight":7.4},{"option":"HEX-03","voters":3,"weight":5.7},{"option":"HEX-07","voters":2,"weight":3.0999999999999996}]}},"grades":[{"hex":"HEX-05","counts":{"DOC":0,"REP":0,"CLM":3,"SPC":1},"total":4,"majority":"CLM","official":"CLM","disputed":false},{"hex":"HEX-04","counts":{"DOC":0,"REP":0,"CLM":4,"SPC":0},"total":4,"majority":"CLM","official":"SPC","disputed":true},{"hex":"HEX-01","counts":{"DOC":0,"REP":0,"CLM":0,"SPC":0},"total":0,"majority":null,"official":"DOC","disputed":false},{"hex":"HEX-06","counts":{"DOC":0,"REP":0,"CLM":0,"SPC":0},"total":0,"majority":null,"official":"REP","disputed":false},{"hex":"HEX-09","counts":{"DOC":0,"REP":0,"CLM":0,"SPC":0},"total":0,"majority":null,"official":"REP","disputed":false},{"hex":"HEX-13","counts":{"DOC":0,"REP":0,"CLM":0,"SPC":0},"total":0,"majority":null,"official":"DOC","disputed":false},{"hex":"HEX-03","counts":{"DOC":0,"REP":0,"CLM":0,"SPC":0},"total":0,"majority":null,"official":"REP","disputed":false},{"hex":"HEX-02","counts":{"DOC":0,"REP":0,"CLM":0,"SPC":0},"total":0,"majority":null,"official":"CLM","disputed":false},{"hex":"HEX-08","counts":{"DOC":0,"REP":0,"CLM":0,"SPC":0},"total":0,"majority":null,"official":"DOC","disputed":false},{"hex":"HEX-07","counts":{"DOC":0,"REP":0,"CLM":0,"SPC":0},"total":0,"majority":null,"official":"CLM","disputed":false}]}},"synthetic":{"hash":295384440,"final":{"polls":{"p1":{"bars":[{"option":"HEX-01","weight":12.5,"voters":9,"bands":[{"rank":"cultist","share":0.07203389830508475},{"rank":"warlock","share":0.16949152542372886},{"rank":"lich","share":0.6059322033898306},{"rank":"dread_council","share":0.15254237288135594}],"length":1,"pct":0.6756756756756757,"leading":true,"tied":false},{"option":"HEX-02","weight":6,"voters":4,"bands":[{"rank":"cultist","share":0.367965367965368},{"rank":"warlock","share":0.43290043290043295},{"rank":"lich","share":0.09523809523809526},{"rank":"dread_council","share":0.1038961038961039}],"length":0.48,"pct":0.32432432432432434,"leading":false,"tied":false},{"option":"HEX-03","weight":0,"voters":0,"bands":[{"rank":"cultist","share":0.07906976744186048},{"rank":"lich","share":0.9209302325581395}],"length":0,"pct":0,"leading":false,"tied":false}],"observed":[{"option":"HEX-01","voters":22,"weight":47.19999999999999},{"option":"HEX-02","voters":12,"weight":23.099999999999998},{"option":"HEX-03","voters":10,"weight":21.499999999999996}]},"foil-vote-1":{"bars":[{"option":"the-disruptor","weight":59.9,"voters":36,"bands":[{"rank":"thrall","share":0.16694490818030053},{"rank":"acolyte","share":0.18697829716193656},{"rank":"cultist","share":0.08514190317195326},{"rank":"warlock","share":0.06677796327212021},{"rank":"lich","share":0.2938230383973289},{"rank":"dread_council","share":0.20033388981636063}],"length":1,"pct":0.5960199004975124,"leading":true,"tied":false},{"option":"the-inspector","weight":40.6,"voters":24,"bands":[{"rank":"thrall","share":0.12315270935960591},{"rank":"acolyte","share":0.2413793103448276},{"rank":"warlock","share":0.19704433497536944},{"rank":"lich","share":0.37931034482758613},{"rank":"dread_council","share":0.059113300492610835}],"length":0.6777963272120201,"pct":0.4039800995024876,"leading":false,"tied":false}],"observed":[{"option":"the-disruptor","voters":36,"weight":59.89999999999999},{"option":"the-inspector","voters":24,"weight":40.6}]}},"grades":[{"hex":"HEX-02","counts":{"DOC":3,"REP":0,"CLM":3,"SPC":1},"total":7,"majority":null,"official":null,"disputed":false},{"hex":"HEX-01","counts":{"DOC":32,"REP":2,"CLM":13,"SPC":5},"total":52,"majority":"DOC","official":"REP","disputed":true},{"hex":"HEX-05","counts":{"DOC":33,"REP":1,"CLM":18,"SPC":2},"total":54,"majority":"DOC","official":"CLM","disputed":true}]}}};
