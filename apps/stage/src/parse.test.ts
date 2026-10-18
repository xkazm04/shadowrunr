import { describe, expect, it } from "vitest";

import { EVENT_TYPES, OP_NAMES, type StreamEvent } from "./contract";
import { parseCase, parseEvent, parseOp, parseTranscript } from "./parse";

const viewer = {
  id: "v1",
  name: "MossyTombstone",
  platform: "sim",
  rank: "cultist",
  sub_tier: 1,
  months: 4,
  gifted: 0,
  is_mod: false,
};

/** One wire-shaped sample per StreamEvent type, as serde would write them. */
const SAMPLES: Record<StreamEvent["type"], unknown> = {
  stream_start: {
    type: "stream_start",
    ts_ms: 0,
    stream_id: "s1",
    character: "malakor",
    tone_variant: "theatrical",
    core_tier: "haiku",
    audience_size: 30,
  },
  segment_start: { type: "segment_start", ts_ms: 1, segment: "SEG-01" },
  segment_end: { type: "segment_end", ts_ms: 2, segment: "SEG-02A", ses: 0.4 },
  case_open: {
    type: "case_open",
    ts_ms: 2,
    case: {
      id: "DEMO-C01",
      title: "A Fictional Affair",
      fictional: true,
      hexes: [
        { id: "HEX-01", q: 0, r: 0, kind: "entity", label: "Glimmerquay", fog: true },
        { id: "HEX-02", q: 1, r: -1, kind: "wallet", label: "0xmoon", grade: "REP", fog: false, sources: ["https://example.test/a"] },
      ],
      links: [["HEX-01", "HEX-02"]],
    },
  },
  chat: {
    type: "chat",
    ts_ms: 3,
    msg: { id: "m1", ts_ms: 3, viewer, text: "!grade HEX-01 DOC", command: { cmd: "grade", hex: "HEX-01", grade: "DOC" } },
  },
  picked: { type: "picked", ts_ms: 4, msg_ids: ["m1"], reason: "funny" },
  say: { type: "say", ts_ms: 5, speaker: "foil", foil: "the-inspector", text: "Aha!", replying_to: ["m1"], first_text_ms: 900 },
  op: { type: "op", ts_ms: 6, op: { op: "dismiss_foil" } },
  vote: { type: "vote", ts_ms: 7, poll_id: "p1", tallies: [{ option: "a", voters: 2, weight: 3.4 }], closed: false },
  moderation: { type: "moderation", ts_ms: 8, stage: "input", rule: "anti-brigade", msg_id: "m1", action: "withheld" },
  handover: { type: "handover", ts_ms: 9, from: "SEG-01", to: "SEG-02A", reason: "dare" },
  job: { type: "job", ts_ms: 10, job_id: "r-01", status: "done", summary: "two hops" },
  stream_end: { type: "stream_end", ts_ms: 11, reason: "sim_complete" },
};

const OPS: Record<string, unknown> = {
  highlight: { op: "highlight", msg_id: "m1" },
  start_poll: { op: "start_poll", poll_id: "p1", question: "?", options: ["a", "b"], min_rank: "acolyte", secs: 30 },
  close_poll: { op: "close_poll", poll_id: "p1" },
  summon_foil: { op: "summon_foil", foil: "the-disruptor" },
  dismiss_foil: { op: "dismiss_foil" },
  reveal_hex: { op: "reveal_hex", hex: "HEX-01", grade: "SPC" },
  dispatch_research: { op: "dispatch_research", job_id: "r-1", brief: "trace" },
  show_card: { op: "show_card", card_id: "c1", title: "T", body: "B" },
  mark_moment: { op: "mark_moment", label: "clip" },
  switch_segment: { op: "switch_segment", to: "SEG-02A" },
  sign_off: { op: "sign_off", reason: "done" },
};

describe("parseEvent: every StreamEvent type", () => {
  it("has a sample for every type in the contract", () => {
    expect(Object.keys(SAMPLES).sort()).toEqual([...EVENT_TYPES].sort());
  });

  for (const type of EVENT_TYPES) {
    it(`parses ${type} and round-trips it unchanged`, () => {
      const parsed = parseEvent(SAMPLES[type]);
      expect(parsed.type).toBe(type);
      expect(JSON.parse(JSON.stringify(parsed))).toEqual(SAMPLES[type]);
    });

    it(`rejects ${type} without ts_ms`, () => {
      const broken = { ...(SAMPLES[type] as object), ts_ms: undefined };
      expect(() => parseEvent(broken)).toThrow(/ts_ms/);
    });
  }

  it("accepts optional fields omitted and defaults serde(default) viewer fields", () => {
    const say = parseEvent({ type: "say", ts_ms: 1, speaker: "malakor", text: "Rise." });
    expect(say).toEqual({ type: "say", ts_ms: 1, speaker: "malakor", text: "Rise." });
    const end = parseEvent({ type: "segment_end", ts_ms: 1, segment: "SEG-01" });
    expect(end).not.toHaveProperty("ses");
    const chat = parseEvent({
      type: "chat",
      ts_ms: 1,
      msg: { id: "m", ts_ms: 1, viewer: { id: "v", name: "n", platform: "twitch", rank: "thrall" }, text: "hi" },
    });
    expect(chat.type === "chat" && chat.msg.viewer).toEqual({
      id: "v",
      name: "n",
      platform: "twitch",
      rank: "thrall",
      months: 0,
      gifted: 0,
      is_mod: false,
    });
  });

  it("tolerates null optional fields with a warning (the contract omits them)", () => {
    const warnings: string[] = [];
    const ev = parseEvent({ type: "job", ts_ms: 1, job_id: "j", status: "running", summary: null }, (w) => warnings.push(w));
    expect(ev).not.toHaveProperty("summary");
    expect(warnings[0]).toMatch(/summary is null/);
  });

  it("rejects unknown types, bad enums and wrong shapes", () => {
    expect(() => parseEvent({ type: "dance", ts_ms: 1 })).toThrow(/known StreamEvent type/);
    expect(() => parseEvent({ ...(SAMPLES.segment_start as object), segment: "SEG-99" })).toThrow(/segment/);
    expect(() => parseEvent({ ...(SAMPLES.stream_end as object), reason: "bored" })).toThrow(/reason/);
    expect(() => parseEvent({ ...(SAMPLES.vote as object), tallies: [{ option: "a", voters: -1, weight: 1 }] })).toThrow(/voters/);
    expect(() => parseEvent({ ...(SAMPLES.moderation as object), stage: "middle" })).toThrow(/stage/);
    expect(() => parseEvent({ ...(SAMPLES.picked as object), msg_ids: "m1" })).toThrow(/msg_ids/);
    expect(() => parseEvent([1, 2])).toThrow(/object/);
  });

  it("parses every chat command", () => {
    const cmds = [
      { cmd: "summon", foil: "the-inspector" },
      { cmd: "hint", hex: "HEX-04", text: "wallet" },
      { cmd: "grade", hex: "HEX-04", grade: "CLM" },
      { cmd: "propose", text: "audit the rocket" },
      { cmd: "vote", option: "HEX-03" },
    ];
    for (const command of cmds) {
      const ev = parseEvent({ type: "chat", ts_ms: 1, msg: { id: "m", ts_ms: 1, viewer, text: "x", command } });
      expect(ev.type === "chat" && ev.msg.command).toEqual(command);
    }
    expect(() =>
      parseEvent({ type: "chat", ts_ms: 1, msg: { id: "m", ts_ms: 1, viewer, text: "x", command: { cmd: "grade", hex: "H", grade: "MAYBE" } } }),
    ).toThrow(/grade/);
  });
});

describe("parseOp: every op", () => {
  it("has a sample for every op in the contract", () => {
    expect(Object.keys(OPS).sort()).toEqual([...OP_NAMES].sort());
  });
  for (const name of OP_NAMES) {
    it(`parses ${name}`, () => {
      expect(parseOp(OPS[name])).toEqual(OPS[name]);
    });
  }
  it("rejects unknown ops and bad fields", () => {
    expect(() => parseOp({ op: "fly" })).toThrow(/known op/);
    expect(() => parseOp({ ...(OPS.start_poll as object), options: [] })).toThrow(/at least one/);
    expect(() => parseOp({ ...(OPS.start_poll as object), min_rank: "emperor" })).toThrow(/min_rank/);
    expect(() => parseOp({ ...(OPS.reveal_hex as object), grade: "doc" })).toThrow(/grade/);
  });
});

describe("parseTranscript", () => {
  it("parses JSONL, skips blank lines, reports bad lines with their line number", () => {
    const text = [
      JSON.stringify(SAMPLES.stream_start),
      "",
      "{not json",
      JSON.stringify({ type: "segment_start", ts_ms: 1 }),
      JSON.stringify(SAMPLES.segment_start),
      "",
    ].join("\r\n");
    const r = parseTranscript(text);
    expect(r.events.map((e) => e.type)).toEqual(["stream_start", "segment_start"]);
    expect(r.errors.map((e) => e.line)).toEqual([3, 4]);
    expect(r.errors[0]!.message).toMatch(/invalid JSON/);
    expect(r.errors[1]!.message).toMatch(/segment/);
  });

  it("warns when timestamps go backwards", () => {
    const r = parseTranscript(
      [JSON.stringify({ ...(SAMPLES.segment_start as object), ts_ms: 10 }), JSON.stringify({ ...(SAMPLES.segment_start as object), ts_ms: 5 })].join("\n"),
    );
    expect(r.events).toHaveLength(2);
    expect(r.warnings[0]!.message).toMatch(/backwards/);
  });
});

describe("case_open", () => {
  it("carries the case's coordinates, labels, title and fictional flag", () => {
    const ev = parseEvent(SAMPLES.case_open);
    if (ev.type !== "case_open") throw new Error(ev.type);
    expect(ev.case.title).toBe("A Fictional Affair");
    expect(ev.case.fictional).toBe(true);
    expect(ev.case.hexes.map((h) => [h.id, h.q, h.r, h.label])).toEqual([
      ["HEX-01", 0, 0, "Glimmerquay"],
      ["HEX-02", 1, -1, "0xmoon"],
    ]);
  });

  it("reports a malformed case with the case_open field path", () => {
    expect(() => parseEvent({ type: "case_open", ts_ms: 1 })).toThrow(/case_open\.case: expected object/);
    expect(() =>
      parseEvent({ type: "case_open", ts_ms: 1, case: { id: "C", title: "T", hexes: [] } }),
    ).toThrow(/case_open\.case\.fictional/);
    expect(() =>
      parseEvent({
        type: "case_open",
        ts_ms: 1,
        case: { id: "C", title: "T", fictional: true, hexes: [{ id: "HEX-01", q: 0.5, r: 0, kind: "entity", label: "x", fog: true }] },
      }),
    ).toThrow(/case_open\.case\.hexes\[0\]\.q/);
  });

  it("parses inside a transcript right after segment_start", () => {
    const text = [SAMPLES.segment_start, SAMPLES.case_open].map((e) => JSON.stringify(e)).join("\n");
    const r = parseTranscript(text);
    expect(r.errors).toEqual([]);
    expect(r.events.map((e) => e.type)).toEqual(["segment_start", "case_open"]);
  });
});

describe("parseCase", () => {
  it("parses a case with links and rejects a bad link", () => {
    const c = parseCase({
      id: "C",
      title: "T",
      fictional: true,
      hexes: [{ id: "HEX-01", q: 0, r: 0, kind: "entity", label: "x", fog: true }],
      links: [["HEX-01", "HEX-02"]],
    });
    expect(c.links).toEqual([["HEX-01", "HEX-02"]]);
    expect(() => parseCase({ id: "C", title: "T", fictional: true, hexes: [], links: [["a"]] })).toThrow(/links/);
    expect(() => parseCase({ id: "C", title: "T", hexes: [] })).toThrow(/fictional/);
  });
});
