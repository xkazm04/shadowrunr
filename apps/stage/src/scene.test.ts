import { describe, expect, it } from "vitest";

import type { ChatMsg, Command, Rank, StreamEvent } from "./contract";
import {
  foldEvents,
  gradeTallies,
  initialScene,
  layoutFor,
  matchOption,
  pollBars,
  reduce,
  LANE_MAX,
  DECREE_MAX,
} from "./scene";

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
  it("maps segments to named layouts", () => {
    expect(layoutFor("SEG-01")).toBe("improv");
    expect(layoutFor("SEG-02A")).toBe("hexmap");
    expect(layoutFor("SEG-02B-I")).toBe("descoped");
  });

  it("starts in boot, switches on segment_start, and animates a handover", () => {
    let s = initialScene();
    expect(s.layout).toBe("boot");
    s = reduce(s, { type: "stream_start", ts_ms: 0, stream_id: "s", character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 3 });
    expect(s.meta?.audienceSize).toBe(3);
    s = reduce(s, { type: "segment_start", ts_ms: 100, segment: "SEG-01" });
    expect(s.layout).toBe("improv");
    expect(s.transition).toMatchObject({ from: "boot", to: "improv", cause: "segment_start", at: 100 });
    s = reduce(s, op(200, { op: "switch_segment", to: "SEG-02A" }));
    expect(s.pendingSwitch).toEqual({ to: "SEG-02A", at: 200 });
    s = reduce(s, { type: "segment_end", ts_ms: 300, segment: "SEG-01", ses: 0.5 });
    expect(s.seals).toEqual([{ segment: "SEG-01", ses: 0.5, at: 300 }]);
    s = reduce(s, { type: "handover", ts_ms: 400, from: "SEG-01", to: "SEG-02A", reason: "dare" });
    expect(s.layout).toBe("hexmap");
    expect(s.segment).toBe("SEG-02A");
    expect(s.pendingSwitch).toBeNull();
    const handoverTransition = s.transition;
    expect(handoverTransition).toMatchObject({ from: "improv", to: "hexmap", cause: "handover", reason: "dare" });
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
    expect(Object.keys(s.heard).sort()).toEqual(["a", "b", "c", "d", "ghost"]);
    expect(s.decrees).toHaveLength(DECREE_MAX);
    expect(s.decrees.map((d) => d.msgId)).toEqual(["b", "c", "d"]);
    expect(s.decrees[0]).toMatchObject({ kind: "picked", reason: "funny" });
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
    expect(foldEvents(evs, 5).layout).toBe("improv");
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
