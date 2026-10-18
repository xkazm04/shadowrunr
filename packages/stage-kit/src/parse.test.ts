import { describe, expect, it } from "vitest";

import { OP_NAMES, EVENT_TYPES } from "./vocab";
import type { StreamEvent } from "@shadowrunr/contract";
import { checkCase, parseCase, parseStreamEvent, parseTranscript, type Parsed } from "./parse";

function ok<T>(r: Parsed<T>): T {
  if (!r.ok) throw new Error(r.message);
  return r.value;
}
function bad<T>(r: Parsed<T>): string {
  if (r.ok) throw new Error("expected a failure");
  return r.message;
}

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

describe("parseStreamEvent: every StreamEvent type, through the contract door", () => {
  it("has a sample for every type in the contract", () => {
    expect(Object.keys(SAMPLES).sort()).toEqual([...EVENT_TYPES].sort());
  });

  for (const type of EVENT_TYPES) {
    it(`parses ${type} from JSONL text and round-trips it unchanged`, () => {
      const parsed = ok(parseStreamEvent(JSON.stringify(SAMPLES[type])));
      expect(parsed.type).toBe(type);
      expect(parsed).toEqual(SAMPLES[type]);
    });

    it(`rejects ${type} without ts_ms`, () => {
      const broken: Record<string, unknown> = { ...(SAMPLES[type] as object) };
      delete broken["ts_ms"];
      expect(bad(parseStreamEvent(broken))).toMatch(/ts_ms/);
    });
  }

  it("accepts an enveloped /events frame and returns its event", () => {
    const frame = { seq: 3, run_id: "r1", event: SAMPLES.segment_start };
    expect(ok(parseStreamEvent(JSON.stringify(frame)))).toEqual(SAMPLES.segment_start);
  });

  it("turns an error frame into a failure with its code", () => {
    const frame = { error: { code: "not_found", message: "no such run" } };
    expect(bad(parseStreamEvent(frame))).toMatch(/not_found: no such run/);
  });

  it("accepts optional fields omitted (the contract omits them, never null)", () => {
    expect(ok(parseStreamEvent({ type: "say", ts_ms: 1, speaker: "malakor", text: "Rise." }))).toEqual({
      type: "say",
      ts_ms: 1,
      speaker: "malakor",
      text: "Rise.",
    });
    const chat = ok(
      parseStreamEvent({
        type: "chat",
        ts_ms: 1,
        msg: { id: "m", ts_ms: 1, viewer: { id: "v", name: "n", platform: "twitch", rank: "thrall" }, text: "hi" },
      }),
    );
    expect(chat.type === "chat" && chat.msg.viewer.months).toBeUndefined();
  });

  it("rejects unknown types, bad enums and wrong shapes, naming the field", () => {
    bad(parseStreamEvent({ type: "dance", ts_ms: 1 }));
    expect(bad(parseStreamEvent({ ...(SAMPLES.segment_start as object), segment: "SEG-99" }))).toMatch(/segment/);
    bad(parseStreamEvent({ ...(SAMPLES.stream_end as object), reason: "bored" }));
    expect(bad(parseStreamEvent({ ...(SAMPLES.vote as object), tallies: [{ option: "a", voters: -1, weight: 1 }] }))).toMatch(/voters/);
    expect(bad(parseStreamEvent({ ...(SAMPLES.moderation as object), stage: "middle" }))).toMatch(/stage/);
    expect(bad(parseStreamEvent({ ...(SAMPLES.picked as object), msg_ids: "m1" }))).toMatch(/msg_ids/);
    bad(parseStreamEvent([1, 2]));
    expect(bad(parseStreamEvent("{not json"))).toMatch(/not JSON/);
  });

  it("parses every chat command and rejects a bad grade", () => {
    const cmds = [
      { cmd: "summon", foil: "the-inspector" },
      { cmd: "hint", hex: "HEX-04", text: "wallet" },
      { cmd: "grade", hex: "HEX-04", grade: "CLM" },
      { cmd: "propose", text: "audit the rocket" },
      { cmd: "vote", option: "HEX-03" },
    ];
    for (const command of cmds) {
      const ev = ok(parseStreamEvent({ type: "chat", ts_ms: 1, msg: { id: "m", ts_ms: 1, viewer, text: "x", command } }));
      expect(ev.type === "chat" && ev.msg.command).toEqual(command);
    }
    expect(
      bad(parseStreamEvent({ type: "chat", ts_ms: 1, msg: { id: "m", ts_ms: 1, viewer, text: "x", command: { cmd: "grade", hex: "H", grade: "MAYBE" } } })),
    ).toMatch(/grade/);
  });
});

describe("every op", () => {
  it("has a sample for every op in the contract", () => {
    expect(Object.keys(OPS).sort()).toEqual([...OP_NAMES].sort());
  });
  for (const name of OP_NAMES) {
    it(`parses ${name}`, () => {
      const ev = ok(parseStreamEvent({ type: "op", ts_ms: 1, op: OPS[name] }));
      expect(ev.type === "op" && ev.op).toEqual(OPS[name]);
    });
  }
  it("rejects unknown ops and bad fields", () => {
    bad(parseStreamEvent({ type: "op", ts_ms: 1, op: { op: "fly" } }));
    expect(bad(parseStreamEvent({ type: "op", ts_ms: 1, op: { ...(OPS.start_poll as object), min_rank: "emperor" } }))).toMatch(/min_rank/);
    bad(parseStreamEvent({ type: "op", ts_ms: 1, op: { ...(OPS.reveal_hex as object), grade: "doc" } }));
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
    expect(r.events.map((e: StreamEvent) => e.type)).toEqual(["stream_start", "segment_start"]);
    expect(r.errors.map((e) => e.line)).toEqual([3, 4]);
    expect(r.errors[0]!.message).toMatch(/not JSON/);
    expect(r.errors[1]!.message).toMatch(/StreamEvent/);
  });

  it("warns when timestamps go backwards", () => {
    const r = parseTranscript(
      [JSON.stringify({ ...(SAMPLES.segment_start as object), ts_ms: 10 }), JSON.stringify({ ...(SAMPLES.segment_start as object), ts_ms: 5 })].join("\n"),
    );
    expect(r.events).toHaveLength(2);
    expect(r.warnings[0]!.message).toMatch(/backwards/);
  });

  it("parses a case_open right after segment_start", () => {
    const r = parseTranscript([SAMPLES.segment_start, SAMPLES.case_open].map((e) => JSON.stringify(e)).join("\n"));
    expect(r.errors).toEqual([]);
    expect(r.events.map((e) => e.type)).toEqual(["segment_start", "case_open"]);
  });
});

describe("checkCase / parseCase", () => {
  it("parses a case with links and names the bad field", () => {
    const c = parseCase({
      id: "C",
      title: "T",
      fictional: true,
      hexes: [{ id: "HEX-01", q: 0, r: 0, kind: "entity", label: "x", fog: true }],
      links: [["HEX-01", "HEX-02"]],
    });
    expect(c.links).toEqual([["HEX-01", "HEX-02"]]);
    expect(bad(checkCase({ id: "C", title: "T", fictional: true, hexes: [], links: [["a"]] }))).toMatch(/links/);
    expect(bad(checkCase({ id: "C", title: "T", hexes: [] }))).toMatch(/fictional/);
    expect(bad(checkCase({ id: "C", title: "T", fictional: true, hexes: [{ id: "HEX-01", q: 0.5, r: 0, kind: "entity", label: "x", fog: true }] }))).toMatch(/hexes\/0\/q/);
    expect(() => parseCase({ id: "C" })).toThrow(/case:/);
  });
});
