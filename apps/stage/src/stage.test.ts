// The stage's own modules: the 1920x1080 layouts and where a transcript and its case come from.
// The scene, playback, timing and geometry tests live with them in @shadowrunr/stage-kit.
import { describe, expect, it } from "vitest";

import type { Case, StreamEvent } from "@shadowrunr/contract";
import { CANVAS_H, CANVAS_W, foldEvents } from "@shadowrunr/stage-kit";
import { layoutRects, REGIONS } from "./layouts";
import { readParams, resolveCase, sidecarCaseUrl } from "./source";

describe("layouts", () => {
  for (const layout of ["boot", "improv", "hexmap", "descoped"] as const) {
    for (const poll of [false, true]) {
      for (const foil of [false, true]) {
        it(`${layout} poll=${poll} foil=${foil}: every visible region stays inside 1920x1080`, () => {
          const rects = layoutRects(layout, { poll, foil });
          for (const k of REGIONS) {
            const r = rects[k];
            expect(r, k).toBeDefined();
            if (r.hidden) continue;
            expect(r.x, k).toBeGreaterThanOrEqual(0);
            expect(r.y, k).toBeGreaterThanOrEqual(0);
            expect(r.x + r.w, k).toBeLessThanOrEqual(CANVAS_W);
            expect(r.y + r.h, k).toBeLessThanOrEqual(CANVAS_H);
            expect(r.h, k).toBeGreaterThan(40);
          }
        });
      }
    }
  }
  it("gives the hexmap the dominant area in SEG-02A", () => {
    const r = layoutRects("hexmap", { poll: true, foil: true });
    const area = (k: (typeof REGIONS)[number]) => r[k].w * r[k].h;
    for (const k of REGIONS) if (k !== "hexmap" && !r[k].hidden) expect(area("hexmap")).toBeGreaterThan(area(k) * 3);
  });
});

describe("source", () => {
  it("reads URL parameters", () => {
    const p = readParams("?file=runs/x/transcript.jsonl&broadcast=1&speed=2&loop=1");
    expect(p).toMatchObject({ file: "runs/x/transcript.jsonl", broadcast: true, speed: 2, loop: true, run: null });
    expect(readParams("?broadcast=0&speed=-3").broadcast).toBe(false);
    expect(readParams("?speed=-3").speed).toBe(1);
    expect(readParams("?run=abc").run).toBe("abc");
    expect(readParams("?run=abc").ws).toBe("ws://127.0.0.1:7717/events");
    expect(readParams("?ws=ws://localhost:9000/events")).toMatchObject({ run: "live", ws: "ws://localhost:9000/events" });
    expect(readParams("?ws=javascript:alert(1)").ws).toBe("ws://127.0.0.1:7717/events");
    expect(readParams("?t=90&paused=1")).toMatchObject({ startAt: 90, paused: true });
    expect(readParams("?t=abc").startAt).toBe(0);
  });
  it("derives the sidecar case url", () => {
    expect(sidecarCaseUrl("runs/a/transcript.jsonl")).toBe("runs/a/transcript.case.json");
    expect(sidecarCaseUrl("x.jsonl?v=2")).toBe("x.case.json?v=2");
    expect(sidecarCaseUrl("x.txt")).toBeNull();
  });
  it("uses the built-in case only when it contains every revealed hex", () => {
    const builtin: Case = { id: "d", title: "Demo", fictional: true, hexes: [{ id: "HEX-01", q: 0, r: 0, kind: "entity", label: "x", fog: true }] };
    const reveal = (hex: string): StreamEvent => ({ type: "op", ts_ms: 1, op: { op: "reveal_hex", hex, grade: "DOC" } });
    expect(resolveCase(null, [reveal("HEX-01")], builtin).source).toBe("builtin");
    expect(resolveCase(null, [reveal("HEX-99")], builtin)).toEqual({ case: null, source: "none" });
    expect(resolveCase(null, [], builtin).source).toBe("none");
    expect(resolveCase({ case: builtin, source: "param" }, [reveal("HEX-99")], builtin).source).toBe("param");
  });
});

describe("case source", () => {
  const hex = (id: string, q: number, r: number) => ({ id, q, r, kind: "entity" as const, label: `label-${id}`, fog: true });
  const real: Case = { id: "C-REAL", title: "The Transcript's Case", fictional: true, hexes: [hex("HEX-01", 2, -1), hex("HEX-07", 0, 0)] };
  const other: Case = { id: "C-OTHER", title: "Another Case", fictional: true, hexes: [hex("HEX-07", 0, 0)] };
  const events: StreamEvent[] = [
    { type: "segment_start", ts_ms: 0, segment: "SEG-01" },
    { type: "handover", ts_ms: 10, from: "SEG-01", to: "SEG-02A", reason: "dare" },
    { type: "segment_start", ts_ms: 11, segment: "SEG-02A" },
    { type: "case_open", ts_ms: 11, case: real },
    { type: "op", ts_ms: 20, op: { op: "reveal_hex", hex: "HEX-07", grade: "DOC" } },
  ];
  it("prefers the transcript's case over ?case=, sidecar, dropped and the built-in demo", () => {
    for (const source of ["param", "sidecar", "dropped"] as const) {
      expect(resolveCase({ case: other, source }, events, other)).toEqual({ case: real, source: "transcript" });
    }
    expect(resolveCase(null, events, other)).toEqual({ case: real, source: "transcript" });
    // The scene's case at the playback position (or on a live stream) wins over the file scan.
    expect(resolveCase(null, events, other, other)).toEqual({ case: other, source: "transcript" });
    expect(resolveCase(null, [], other, foldEvents(events).caseFile)).toEqual({ case: real, source: "transcript" });
  });

  it("keeps the old fallbacks for transcripts without case_open", () => {
    const old = events.filter((e) => e.type !== "case_open");
    expect(foldEvents(old).caseFile).toBeNull();
    expect(resolveCase({ case: other, source: "sidecar" }, old, other)).toEqual({ case: other, source: "sidecar" });
    expect(resolveCase(null, old, other)).toEqual({ case: other, source: "builtin" });
    expect(resolveCase(null, old, { ...other, hexes: [] })).toEqual({ case: null, source: "none" });
  });
});
