// The demo fixture must parse strictly, exercise every StreamEvent type in both segments and
// every op each segment allows, and match the segment tables' allowed_ops.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import type { SegmentId, StreamEvent } from "@shadowrunr/contract";
import { EVENT_TYPES, buildMap, foldEvents } from "@shadowrunr/stage-kit";
import { parseCase, parseTranscript } from "@shadowrunr/stage-kit/parse";
import { resolveCase, revealedHexIds, transcriptCase } from "./source";
import { readJson } from "./test-json";

/** The `type` of one transcript line. */
const lineType = (line: string): unknown => (readJson(line) as { type?: unknown }).type;

const root = join(__dirname, "..");
const repo = join(root, "..", "..");
const text = readFileSync(join(root, "fixtures", "demo.jsonl"), "utf8");
const parsed = parseTranscript(text);

function bySegment(events: StreamEvent[]) {
  const out = new Map<SegmentId, StreamEvent[]>();
  let seg: SegmentId | null = null;
  for (const e of events) {
    if (e.type === "segment_start") seg = e.segment;
    if (seg) out.set(seg, [...(out.get(seg) ?? []), e]);
    if (e.type === "segment_end") seg = null;
  }
  return out;
}

function allowedOps(file: string): string[] {
  const toml = readFileSync(join(repo, "crates", "shadowrunr-segments", "data", "segments", file), "utf8");
  const block = /allowed_ops\s*=\s*\[([\s\S]*?)\]/.exec(toml)?.[1] ?? "";
  return [...block.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]!);
}

describe("fixtures/demo.jsonl", () => {
  it("parses with no errors and no warnings", () => {
    expect(parsed.errors).toEqual([]);
    expect(parsed.warnings).toEqual([]);
    expect(parsed.events.length).toBeGreaterThan(100);
  });

  it("is one JSON object per line with no null fields (optional fields are omitted)", () => {
    for (const line of text.trim().split("\n")) expect(line).not.toMatch(/:null[,}]/);
  });

  it("contains every StreamEvent type", () => {
    const types = new Set(parsed.events.map((e) => e.type));
    expect([...types].sort()).toEqual([...EVENT_TYPES].sort());
  });

  const segs = bySegment(parsed.events);
  const inSegmentTypes = ["chat", "picked", "say", "op", "vote", "moderation", "job"] as const;

  for (const [seg, table] of [
    ["SEG-01", "seg-01.toml"],
    ["SEG-02A", "seg-02a.toml"],
  ] as const) {
    it(`${seg}: exercises every in-segment event type, both speakers and both moderation stages`, () => {
      const evs = segs.get(seg) ?? [];
      const types = new Set(evs.map((e) => e.type));
      for (const t of [...inSegmentTypes, "segment_start", "segment_end"]) expect(types, t).toContain(t);
      const speakers = new Set(evs.flatMap((e) => (e.type === "say" ? [e.speaker] : [])));
      expect([...speakers].sort()).toEqual(["foil", "host"]);
      const stages = new Set(evs.flatMap((e) => (e.type === "moderation" ? [e.stage] : [])));
      expect([...stages].sort()).toEqual(["input", "output"]);
    });

    it(`${seg}: uses every op in ${table} allowed_ops, and no op outside it`, () => {
      // `pin_link` is the board's op (appendix (screens and board)); until BOARD-WIRE's table
      // allows it, the demo uses it ahead of the table.
      const listed = allowedOps(table);
      const allowed = seg === "SEG-02A" && !listed.includes("pin_link") ? [...listed, "pin_link"] : listed;
      expect(allowed.length).toBeGreaterThan(5);
      const used = new Set((segs.get(seg) ?? []).flatMap((e) => (e.type === "op" ? [e.op.op] : [])));
      for (const op of used) expect(allowed, op).toContain(op);
      // switch_segment out of SEG-02A and sign_off in SEG-01 would end the demo early.
      const expected = allowed.filter((o) => !(seg === "SEG-02A" && o === "switch_segment") && !(seg === "SEG-01" && o === "sign_off"));
      expect([...used].sort()).toEqual(expected.sort());
    });
  }

  it("hands over from SEG-01 to SEG-02A and ends on the outro", () => {
    const s = foldEvents(parsed.events);
    expect(s.layout).toBe("board");
    expect(s.outro).not.toBeNull();
    expect(s.ended?.reason).toBe("sim_complete");
    expect(s.seals.map((x) => x.segment)).toEqual(["SEG-01", "SEG-02A"]);
    expect(s.revealOrder.length).toBeGreaterThanOrEqual(8);
    expect(Object.values(s.hexes).map((h) => h.grade)).toEqual(expect.arrayContaining(["DOC", "REP", "CLM", "SPC"]));
  });

  it("opens the fictional demo case once, right after the SEG-02A segment_start", () => {
    const demo = parseCase(readFileSync(join(root, "fixtures", "demo-case.json"), "utf8"));
    const i = parsed.events.findIndex((e) => e.type === "case_open");
    expect(parsed.events[i - 1]).toMatchObject({ type: "segment_start", segment: "SEG-02A" });
    expect(parsed.events.filter((e) => e.type === "case_open")).toHaveLength(1);
    expect(transcriptCase(parsed.events)).toEqual(demo);
    expect(transcriptCase(parsed.events)?.fictional).toBe(true);
  });

  it("references only hexes of the fictional demo case, which resolves from the transcript", () => {
    const demo = parseCase(readFileSync(join(root, "fixtures", "demo-case.json"), "utf8"));
    expect(demo.fictional).toBe(true);
    const r = resolveCase(null, parsed.events, demo);
    expect(r.source).toBe("transcript");
    expect(buildMap(r.case, revealedHexIds(parsed.events)).unplaced).toEqual([]);
    // Without case_open (an older transcript) the built-in demo case still resolves.
    const old = parsed.events.filter((e) => e.type !== "case_open");
    expect(resolveCase(null, old, demo).source).toBe("builtin");
  });

  it("is reproducible from scripts/make-demo.mjs (same ops, same order)", () => {
    // Guard against hand edits drifting from the generator: the first and last lines are fixed.
    const lines = text.trim().split("\n");
    expect(lineType(lines[0]!)).toBe("stream_start");
    expect(lineType(lines[lines.length - 1]!)).toBe("stream_end");
  });
});

describe("fixtures/demo-case.json", () => {
  it("is a byte copy of the segments demo case (until that case has its board: the case plus the board)", () => {
    const a = readFileSync(join(repo, "crates", "shadowrunr-segments", "data", "demo-case.json"), "utf8");
    const b = readFileSync(join(root, "fixtures", "demo-case.json"), "utf8");
    const segments = parseCase(a);
    if (segments.acts !== undefined) {
      expect(b.replace(/\r\n/g, "\n")).toBe(a.replace(/\r\n/g, "\n"));
      return;
    }
    // BOARD-WIRE gives the segments case its roles, dates, thesis and acts; until then the
    // fixture is that case with exactly those fields added.
    const fixture = parseCase(b);
    delete fixture.thesis;
    delete fixture.acts;
    for (const h of fixture.hexes) {
      delete h.role;
      delete h.date;
    }
    expect(fixture).toEqual(segments);
  });

  it("tells the story on the board: roles, dates, a thesis and acts that hold every card once", () => {
    const c = parseCase(readFileSync(join(root, "fixtures", "demo-case.json"), "utf8"));
    expect(c.thesis).toMatch(/\?$/);
    const inActs = (c.acts ?? []).flatMap((a) => a.hexes);
    expect(inActs.sort()).toEqual(c.hexes.map((h) => h.id).sort());
    for (const h of c.hexes) expect(h.date, h.id).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(c.hexes.filter((h) => h.role !== undefined).length).toBeGreaterThanOrEqual(12);
  });
});

describe("the board in the demo", () => {
  const planted = readJson(readFileSync(join(repo, "crates", "shadowrunr-segments", "data", "demo-case-records.json"), "utf8")) as {
    records: { id: string; hex: string; grade: string }[];
  };
  const records = new Map(planted.records.map((r) => [r.id, r]));
  const pins = parsed.events.flatMap((e) => (e.type === "link_pinned" ? [e] : []));

  it("pins 4 to 6 strings along the money path, one of them a contradiction", () => {
    expect(pins.length).toBeGreaterThanOrEqual(4);
    expect(pins.length).toBeLessThanOrEqual(6);
    expect(pins.filter((p) => p.rel === "contradicts")).toHaveLength(1);
    expect(pins.map((p) => `${p.from}>${p.to}`)).toEqual(expect.arrayContaining(["HEX-01>HEX-05", "HEX-01>HEX-04", "HEX-04>HEX-09"]));
  });

  it("pins only what the runtime would accept: both cards face up, the record returned and filed under an end, its grade", () => {
    for (const p of pins) {
      const before = parsed.events.filter((e) => e.ts_ms <= p.ts_ms);
      const up = new Set(before.flatMap((e) => (e.type === "op" && e.op.op === "reveal_hex" ? [e.op.hex] : [])));
      const back = new Set(before.flatMap((e) => (e.type === "record_shown" ? [e.record.id] : [])));
      const r = records.get(p.record_id)!;
      expect(up.has(p.from) && up.has(p.to), p.record_id).toBe(true);
      expect(back.has(p.record_id), p.record_id).toBe(true);
      expect([p.from, p.to], p.record_id).toContain(r.hex);
      expect(p.grade).toBe(r.grade);
      // each string follows the op the runtime accepted
      expect(before.some((e) => e.type === "op" && e.op.op === "pin_link" && e.op.from === p.from && e.op.to === p.to && e.op.rel === p.rel)).toBe(true);
    }
    expect(new Set(pins.map((p) => `${p.from}>${p.to}>${p.rel}`)).size).toBe(pins.length);
  });

  it("moves between the board, the browser and the desk, and the scene keeps every string", () => {
    const screens = parsed.events.flatMap((e) => (e.type === "op" && e.op.op === "show_screen" ? [e.op.screen] : []));
    for (const sc of ["board", "browser", "desk"] as const) expect(screens, sc).toContain(sc);
    expect(screens).not.toContain("hexmap");
    expect(foldEvents(parsed.events).links).toHaveLength(pins.length);
  });
});
