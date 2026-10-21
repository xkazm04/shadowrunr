// The stage's own modules: the 1920x1080 layouts and where a transcript and its case come from.
// The scene, playback, timing and geometry tests live with them in @shadowrunr/stage-kit.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import type { Case, StreamEvent } from "@shadowrunr/contract";
import { CANVAS_H, CANVAS_W, fitMap, foldEvents, hexBoxes } from "@shadowrunr/stage-kit";
import { parseCase } from "@shadowrunr/stage-kit/parse";
import { HEX_SIZE, hexArea } from "@shadowrunr/stage-kit/ui";
import { LOOK_CHAT, LOOK_CONTENT, LOOK_HOST, lookFor } from "./avatar/look";
import { CORNER_AVATAR, FACE_PX, hexFrame, layoutRects, REGIONS, type Rect } from "./layouts";
import { readParams, resolveCase, sidecarCaseUrl } from "./source";

const LAYOUTS = ["boot", "host", "hexmap", "browser", "desk"] as const;
const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

describe("layouts", () => {
  for (const layout of LAYOUTS) {
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
  it("stands Malakor in the bottom-left corner of every content screen, on the canvas's bottom edge", () => {
    for (const layout of ["hexmap", "browser", "desk"] as const) {
      const r = layoutRects(layout, { poll: true, foil: true });
      expect(r.avatar).toEqual(CORNER_AVATAR);
      expect(r.avatar.w).toBeGreaterThanOrEqual(420);
      expect(r.avatar.w).toBeLessThanOrEqual(460);
      expect(r.avatar.y + r.avatar.h).toBe(CANVAS_H);
      // the lower third runs from his right edge to the content's right edge
      expect(r.caption.x).toBeGreaterThanOrEqual(r.avatar.x + r.avatar.w);
      expect(r.caption.x + r.caption.w).toBe(r.chat.x - 24);
      // the ticker never sits under him, and nothing but the content does
      expect(overlaps(r.ticker, r.avatar)).toBe(false);
      for (const k of ["caption", "chat", "vote", "inspector", "tally", "card", "decrees", "observer"] as const) {
        if (!r[k].hidden) expect(overlaps(r[k], r.avatar), k).toBe(false);
      }
      // inspector and tally only where the map is
      expect(!!r.inspector.hidden).toBe(layout === "browser");
      expect(!!r.tally.hidden).toBe(layout === "browser");
    }
  });

  it("keeps the host room's big avatar at the face's full size, and scales it in the corner", () => {
    const host = layoutRects("host", { poll: false, foil: false });
    expect(Math.min(host.avatar.w, host.avatar.h)).toBe(FACE_PX);
    expect(CORNER_AVATAR.w / FACE_PX).toBeCloseTo(0.8);
  });

  it("puts the desk's browser window in front of a smaller hexmap window", () => {
    const desk = layoutRects("desk", { poll: false, foil: false });
    const map = layoutRects("hexmap", { poll: false, foil: false });
    expect(desk.hexmap.w * desk.hexmap.h).toBeLessThan(map.hexmap.w * map.hexmap.h);
    expect(overlaps(desk.hexmap, desk.browser)).toBe(true);
    expect(desk.browser.hidden).toBeUndefined();
    expect(map.browser.hidden).toBe(true);
    expect(layoutRects("browser", { poll: false, foil: false }).hexmap.hidden).toBe(true);
  });

  it("gives the hexmap the dominant area in SEG-02A", () => {
    const r = layoutRects("hexmap", { poll: true, foil: true });
    const area = (k: (typeof REGIONS)[number]) => r[k].w * r[k].h;
    for (const k of REGIONS) if (k !== "hexmap" && !r[k].hidden) expect(area("hexmap")).toBeGreaterThan(area(k) * 3);
  });
});

describe("no hex under him", () => {
  const demo = parseCase(readFileSync(join(__dirname, "..", "fixtures", "demo-case.json"), "utf8"));
  for (const layout of ["hexmap", "desk"] as const) {
    for (const poll of [false, true]) {
      it(`${layout} poll=${poll}: no hex of the demo case lies under the avatar's box (or the desk's browser)`, () => {
        const rects = layoutRects(layout, { poll, foil: true });
        const frame = hexFrame(layout, rects)!;
        expect(frame).not.toBeNull();
        const fit = fitMap(demo.hexes, HEX_SIZE, hexArea(frame), frame.avoid, frame.prefer);
        const map = rects.hexmap;
        const boxes = hexBoxes(demo.hexes, HEX_SIZE, fit).map((b) => ({ ...b, x: b.x + map.x, y: b.y + map.y }));
        expect(boxes).toHaveLength(demo.hexes.length);
        for (const b of boxes) {
          expect(overlaps(b, rects.avatar)).toBe(false);
          if (layout === "desk") expect(overlaps(b, rects.browser)).toBe(false);
          // every hex stays inside its window
          expect(b.x).toBeGreaterThanOrEqual(map.x);
          expect(b.y).toBeGreaterThanOrEqual(map.y);
          expect(b.x + b.w).toBeLessThanOrEqual(map.x + map.w);
          expect(b.y + b.h).toBeLessThanOrEqual(map.y + map.h);
        }
        // and the hexes stay legible
        expect(fit.scale).toBeGreaterThan(layout === "desk" ? 0.45 : 0.8);
      });
    }
  }
  it("has no frame where the map is not shown", () => {
    for (const layout of ["boot", "host", "browser"] as const) expect(hexFrame(layout, layoutRects(layout, { poll: false, foil: false }))).toBeNull();
  });
});

describe("look", () => {
  it("looks at the viewer in the host room, at the content from the corner, at chat when answering", () => {
    expect(lookFor("host", [])).toBe(LOOK_HOST);
    expect(lookFor("boot", null)).toBe(LOOK_HOST);
    for (const l of ["hexmap", "browser", "desk"] as const) expect(lookFor(l, null)).toBe(LOOK_CONTENT);
    expect(lookFor("host", ["ashen_quill"])).toBe(LOOK_CHAT);
    expect(lookFor("desk", ["ashen_quill"])).toBe(LOOK_CHAT);
    expect(LOOK_CONTENT).toEqual({ x: 0.6, y: -0.2 });
    expect(LOOK_CHAT).toEqual({ x: 1, y: -0.1 });
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
    expect(readParams("").avatar).toBe("glow");
    expect(readParams("?avatar=cut").avatar).toBe("cut");
    expect(readParams("?avatar=depth").avatar).toBe("depth");
    expect(readParams("?avatar=nope").avatar).toBe("glow");
    expect(readParams("").rig).toBeNull();
    expect(readParams("?rig=http://127.0.0.1:7801/rig").rig).toBe("http://127.0.0.1:7801/rig/");
    expect(readParams("?rig=javascript:alert(1)").rig).toBeNull();
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
