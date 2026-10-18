import { describe, expect, it } from "vitest";

import type { Case, StreamEvent } from "./contract";
import { diagramFor } from "./diagram";
import { axialToPixel, buildMap, hexPoints, mapBounds, spiral } from "./hexgrid";
import { layoutRects, REGIONS } from "./layouts";
import { CANVAS_H, CANVAS_W, Timeline, clipMarkers, fitCanvas, segmentSpans } from "./playback";
import { readParams, resolveCase, sidecarCaseUrl } from "./source";
import { CAPTION_HOLD_MS, captionOpacity, isEmphatic, visibleWords, wordTimings } from "./timing";

describe("kinetic caption timing", () => {
  it("gives every word an increasing onset with punctuation pauses", () => {
    const line = wordTimings("Rise, minions. The ledger... waits");
    expect(line.words.map((w) => w.word)).toEqual(["Rise,", "minions.", "The", "ledger...", "waits"]);
    const starts = line.words.map((w) => w.start);
    expect(starts[0]).toBe(0);
    for (let i = 1; i < starts.length; i++) expect(starts[i]!).toBeGreaterThan(starts[i - 1]!);
    const gapAfterStop = starts[2]! - starts[1]!;
    const plain = wordTimings("minions The").words[1]!.start;
    expect(gapAfterStop).toBeGreaterThan(plain);
    expect(line.spokenMs).toBeGreaterThan(starts[4]!);
  });

  it("reveals words over time and fades after the hold", () => {
    const line = wordTimings("one two three");
    expect(visibleWords(line, -1)).toBe(0);
    expect(visibleWords(line, 0)).toBe(1);
    expect(visibleWords(line, line.spokenMs)).toBe(3);
    expect(captionOpacity(line, line.spokenMs + CAPTION_HOLD_MS)).toBe(1);
    expect(captionOpacity(line, line.spokenMs + CAPTION_HOLD_MS + 300)).toBeCloseTo(0.5);
    expect(captionOpacity(line, line.spokenMs + CAPTION_HOLD_MS + 5000)).toBe(0);
  });

  it("emphasises caps, numbers and the show's vocabulary", () => {
    expect(isEmphatic("ALLEGED.")).toBe(true);
    expect(isEmphatic("4%")).toBe(true);
    expect(isEmphatic("ledger,")).toBe(true);
    expect(isEmphatic("the")).toBe(false);
    expect(isEmphatic("I")).toBe(false);
    expect(isEmphatic("—")).toBe(false);
  });
});

describe("explainer diagrams", () => {
  it("picks a flow for arrow chains", () => {
    expect(diagramFor("Funnel", "Hype -> Subscription tier -> Rocket -> Bankruptcy")).toEqual({
      kind: "flow",
      steps: ["Hype", "Subscription tier", "Rocket", "Bankruptcy"],
    });
  });
  it("picks figures for two or more label:number pairs", () => {
    const d = diagramFor("Yield", "Promised weekly yield: 4%; Paid out after week 3: 0%");
    expect(d).toEqual({
      kind: "figures",
      items: [
        { label: "Promised weekly yield", value: 4, unit: "%" },
        { label: "Paid out after week 3", value: 0, unit: "%" },
      ],
    });
  });
  it("picks a gauge for exactly one percentage", () => {
    expect(diagramFor("Faith", "12% of minions would subscribe.")).toEqual({ kind: "gauge", value: 12, label: "Faith" });
  });
  it("falls back to a keyword constellation", () => {
    const d = diagramFor("Moon maintenance", "Withdrawals frozen while promoters service the moon.");
    expect(d.kind).toBe("runes");
    expect(d.kind === "runes" && d.words).toEqual(["Moon", "maintenance", "Withdrawals", "frozen", "while", "promoters"]);
  });
});

describe("hex geometry", () => {
  it("places axial coordinates pointy-top", () => {
    expect(axialToPixel(0, 0, 10)).toEqual({ x: 0, y: 0 });
    const p = axialToPixel(1, 0, 10);
    expect(p.x).toBeCloseTo(17.32, 2);
    expect(axialToPixel(0, 1, 10).y).toBe(15);
    expect(hexPoints(10).split(" ")).toHaveLength(6);
  });
  it("walks a spiral of distinct cells", () => {
    const cells = spiral(19);
    expect(cells).toHaveLength(19);
    expect(new Set(cells.map((c) => c.join(","))).size).toBe(19);
    expect(cells[0]).toEqual([0, 0]);
    for (const [q, r] of cells.slice(1, 7)) expect(Math.max(Math.abs(q), Math.abs(r), Math.abs(-q - r))).toBe(1);
  });
  it("places revealed hexes the case does not know on free cells", () => {
    const c: Case = { id: "c", title: "t", fictional: true, hexes: [{ id: "A", q: 0, r: 0, kind: "entity", label: "a", fog: true }] };
    const m = buildMap(c, ["A", "B", "C"]);
    expect(m.unplaced).toEqual(["B", "C"]);
    expect(new Set(m.hexes.map((h) => `${h.q},${h.r}`)).size).toBe(3);
    const none = buildMap(null, ["X"]);
    expect(none.hexes[0]).toMatchObject({ id: "X", q: 0, r: 0, label: "X" });
    const b = mapBounds(m.hexes, 10);
    expect(b.maxX).toBeGreaterThan(b.minX);
  });
});

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

describe("playback", () => {
  const evs: StreamEvent[] = [
    { type: "segment_start", ts_ms: 1000, segment: "SEG-01" },
    { type: "op", ts_ms: 2000, op: { op: "mark_moment", label: "roast" } },
    { type: "handover", ts_ms: 3000, from: "SEG-01", to: "SEG-02A", reason: "r" },
    { type: "stream_end", ts_ms: 5000, reason: "sim_complete" },
  ];
  it("advances incrementally and rebuilds on a backward seek", () => {
    const t = new Timeline(evs);
    expect(t.duration).toBe(4000);
    expect(t.advanceTo(1500).layout).toBe("improv");
    expect(t.advanceTo(3500).layout).toBe("hexmap");
    expect(t.advanceTo(2500).layout).toBe("improv");
    expect(t.scene.applied).toBe(2);
    expect(t.finished).toBe(false);
    t.advanceTo(9999);
    expect(t.finished).toBe(true);
  });
  it("derives clip markers and segment spans for the scrubber", () => {
    expect(clipMarkers(evs)).toEqual([{ at: 2000, label: "roast", frac: 0.25 }]);
    expect(segmentSpans(evs)).toEqual([
      { segment: "SEG-01", from: 1000, to: 3000 },
      { segment: "SEG-02A", from: 3000, to: 5000 },
    ]);
  });
  it("letterboxes the canvas into any window", () => {
    expect(fitCanvas(1920, 1080)).toEqual({ scale: 1, left: 0, top: 0 });
    const tall = fitCanvas(960, 1080);
    expect(tall.scale).toBe(0.5);
    expect(tall.left).toBe(0);
    expect(tall.top).toBe(270);
    const wide = fitCanvas(3000, 1080);
    expect(wide.scale).toBe(1);
    expect(wide.left).toBe(540);
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

describe("ui helpers", async () => {
  const { jobPhase } = await import("./ui/JobTicker");
  const { hexLabelLines } = await import("./ui/Hexmap");
  const { captionSize } = await import("./ui/Caption");

  it("maps free-text job statuses to ticker phases", () => {
    expect(jobPhase("dispatched")).toBe("queued");
    expect(jobPhase("running")).toBe("running");
    expect(jobPhase("in_progress")).toBe("running");
    expect(jobPhase("done")).toBe("done");
    expect(jobPhase("completed")).toBe("done");
    expect(jobPhase("failed")).toBe("failed");
    expect(jobPhase("timeout")).toBe("failed");
  });

  it("fits hex labels into two short lines", () => {
    expect(hexLabelLines("Glimmerquay Yield Syndicate (fictional)")).toEqual(["Glimmerquay Yield", "Syndicate"]);
    expect(hexLabelLines("HEX-99")).toEqual(["HEX-99"]);
    const long = hexLabelLines("Morrowgate Financial Conduct Office, regulator (fictional)");
    expect(long).toHaveLength(2);
    expect(long[1]!.endsWith("…")).toBe(true);
    for (const l of long) expect(l.length).toBeLessThanOrEqual(17);
  });

  it("shrinks long captions so three lines always fit", () => {
    expect(captionSize("Rise.", false)).toBeGreaterThan(captionSize("x".repeat(170), false));
    expect(captionSize("x".repeat(170), true)).toBeLessThan(captionSize("Rise.", true));
  });
});
