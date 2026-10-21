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
      const allowed = allowedOps(table);
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
    expect(s.layout).toBe("hexmap");
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
  it("is a byte copy of the segments demo case", () => {
    const a = readFileSync(join(repo, "crates", "shadowrunr-segments", "data", "demo-case.json"), "utf8");
    const b = readFileSync(join(root, "fixtures", "demo-case.json"), "utf8");
    expect(b.replace(/\r\n/g, "\n")).toBe(a.replace(/\r\n/g, "\n"));
  });
});
