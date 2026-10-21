// The evidence board's pure parts: the reducer's board state, the regions and card layout, the
// focus zoom, the strings' geometry and the timeline strip.

import { describe, expect, it } from "vitest";

import type { Hex, ShownRecord, StreamEvent } from "@shadowrunr/contract";
import { ACT_HEAD, BOARD_ZOOM_MAX, boardArea, boardClip, boardRegions, boardView, fitBox, layoutBoard, regionOf, stripBox, thesisEnd, type BoardFrame } from "./board";
import { REL_LABEL, ROLE_LABEL } from "./boardNames";
import { labelSpot, stringAt, stringGeom, stringLanes, timelineMarks } from "./boardStrings";
import type { Box } from "./hexgrid";
import { foldEvents, reduce } from "./scene";
import { LINK_MAX, mapFocusOf } from "./sceneModel";

const hits = (a: Box, b: Box) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
const inside = (a: Box, o: Box) => a.x >= o.x - 0.01 && a.y >= o.y - 0.01 && a.x + a.w <= o.x + o.w + 0.01 && a.y + a.h <= o.y + o.h + 0.01;
const hex = (id: string, kind: Hex["kind"] = "entity", extra: Partial<Hex> = {}): Hex => ({ id, q: 0, r: 0, kind, label: `label ${id}`, fog: true, ...extra });
const ids = (n: number) => Array.from({ length: n }, (_, i) => `HEX-${String(i + 1).padStart(2, "0")}`);

/** The board screen's window (layouts.ts: the content area, Malakor's corner in its coordinates). */
const CORNER: Box = { x: -16, y: 528, w: 448, h: 448 };
const FRAME: BoardFrame = { w: 1256, h: 804, avoid: [CORNER] };

describe("board state in the scene", () => {
  const op = (ts: number, o: Extract<StreamEvent, { type: "op" }>["op"]): StreamEvent => ({ type: "op", ts_ms: ts, op: o });
  const pinned = (ts: number, from: string, to: string, rel: "paid" | "contradicts" = "paid"): StreamEvent => ({
    type: "link_pinned",
    ts_ms: ts,
    from,
    to,
    rel,
    record_id: "CLM-02",
    grade: "CLM",
  });
  const rec: ShownRecord = { id: "DOC-03", hex: "HEX-04", grade: "DOC", title: "t", source: "https://records.example/x", summary: "s", fictional: true };
  const seg02a: StreamEvent[] = [{ type: "segment_start", ts_ms: 0, segment: "SEG-02A" }];

  it("pins strings from link_pinned in order, keeps the first of a repeat, and bounds the list", () => {
    let s = foldEvents([...seg02a, pinned(10, "HEX-04", "HEX-09"), pinned(11, "HEX-01", "HEX-04", "contradicts")]);
    expect(s.links.map((l) => l.key)).toEqual(["HEX-04>HEX-09>paid", "HEX-01>HEX-04>contradicts"]);
    expect(s.links[0]).toEqual({ key: "HEX-04>HEX-09>paid", from: "HEX-04", to: "HEX-09", rel: "paid", recordId: "CLM-02", grade: "CLM", at: 10 });
    const again = reduce(s, pinned(20, "HEX-04", "HEX-09"));
    expect(again.links).toBe(s.links);
    for (let i = 0; i < LINK_MAX + 5; i++) s = reduce(s, pinned(30 + i, `A${i}`, `B${i}`));
    expect(s.links).toHaveLength(LINK_MAX);
    expect(s.links[s.links.length - 1]!.from).toBe(`A${LINK_MAX + 4}`);
  });

  it("draws nothing for a pin_link alone: the runtime may refuse it", () => {
    const s = foldEvents([...seg02a, op(10, { op: "pin_link", from: "HEX-04", to: "HEX-09", rel: "paid", record_id: "CLM-02" })]);
    expect(s.links).toEqual([]);
  });

  it("opens SEG-02A on the board, focuses a card there, and the desk shows the board behind", () => {
    let s = foldEvents(seg02a);
    expect(s).toMatchObject({ layout: "board", screen: "board", mapScreen: "board" });
    s = reduce(s, op(5, { op: "show_screen", screen: "board", focus: "HEX-04" }));
    expect(mapFocusOf(s)).toBe("HEX-04");
    s = reduce(s, { type: "record_shown", ts_ms: 6, record: rec });
    s = reduce(s, op(7, { op: "show_screen", screen: "desk", record_id: "DOC-03" }));
    expect(s).toMatchObject({ layout: "desk", mapScreen: "board" });
    expect(mapFocusOf(s)).toBe("HEX-04");
  });

  it("keeps the hexmap behind the desk and under a waiting screen when the hexmap was the last map shown", () => {
    let s = foldEvents([...seg02a, op(5, { op: "show_screen", screen: "hexmap" }), op(6, { op: "show_screen", screen: "browser", record_id: "DOC-03" })]);
    expect(s).toMatchObject({ layout: "hexmap", mapScreen: "hexmap", pendingScreen: { screen: "browser" } });
    s = reduce(s, op(7, { op: "show_screen", screen: "board" }));
    s = reduce(s, op(8, { op: "show_screen", screen: "desk", record_id: "DOC-09" }));
    expect(s).toMatchObject({ layout: "board", mapScreen: "board", pendingScreen: { screen: "desk" } });
    // A new segment opens on its default map again.
    s = reduce(s, op(9, { op: "show_screen", screen: "hexmap" }));
    s = reduce(s, { type: "handover", ts_ms: 10, from: "SEG-02A", to: "SEG-01", reason: "r" });
    expect(s.mapScreen).toBe("board");
  });
});

describe("names", () => {
  it("names every role and relation, never by its id", () => {
    for (const r of Object.values(ROLE_LABEL)) expect(r).toMatch(/^[A-Z][a-z ]+$/);
    for (const r of Object.values(REL_LABEL)) expect(r).toMatch(/^[a-z ]+$/);
    expect(ROLE_LABEL.processor).not.toBe("processor");
  });
});

describe("boardRegions", () => {
  const hexes = [hex("HEX-01"), hex("HEX-02", "claim"), hex("HEX-03", "wallet"), hex("HEX-04", "domain"), hex("HEX-05", "claim")];

  it("follows the case's acts, keeps a twice-named hex in its first act, and sends the rest to Loose ends", () => {
    const r = boardRegions(
      [
        { name: "The promise", hexes: ["HEX-02", "HEX-05", "HEX-99"] },
        { name: "The money", hexes: ["HEX-03", "HEX-02"] },
        { name: "Empty", hexes: [] },
      ],
      hexes,
    );
    expect(r).toEqual([
      { name: "The promise", hexes: ["HEX-02", "HEX-05"] },
      { name: "The money", hexes: ["HEX-03"] },
      { name: "Loose ends", hexes: ["HEX-01", "HEX-04"] },
    ]);
  });

  it("groups by kind when the case has no acts", () => {
    expect(boardRegions(undefined, hexes).map((r) => [r.name, r.hexes])).toEqual([
      ["The claims", ["HEX-02", "HEX-05"]],
      ["The players", ["HEX-01"]],
      ["The websites", ["HEX-04"]],
      ["The money", ["HEX-03"]],
    ]);
  });
});

describe("layoutBoard", () => {
  const shapes: [string, number[]][] = [
    ["the demo's acts", [5, 4, 4, 3]],
    ["one region", [16]],
    ["many small acts", [2, 2, 2, 2, 2, 2]],
    ["a crowded case", [9, 8, 9, 8]],
    ["a single card", [1]],
  ];
  for (const [name, counts] of shapes) {
    it(`${name}: no card under him, none overlapping, every card in its region, every region inside the area`, () => {
      let n = 0;
      const regions = counts.map((c, i) => ({ name: `Act ${i}`, hexes: Array.from({ length: c }, () => `H${n++}`) }));
      const l = layoutBoard(regions, FRAME);
      const area = boardArea(FRAME);
      const boxes = Object.values(l.cards);
      expect(boxes).toHaveLength(n);
      boxes.forEach((b, i) => {
        expect(hits(b, CORNER), `card ${i}`).toBe(false);
        boxes.forEach((o, j) => j > i && expect(hits(b, o), `${i} vs ${j}`).toBe(false));
      });
      for (const r of l.regions) {
        expect(inside(r.box, area), r.name).toBe(true);
        for (const id of r.hexes) expect(inside(l.cards[id]!, r.box), id).toBe(true);
        // under the act's name, never over it
        for (const id of r.hexes) expect(l.cards[id]!.y).toBeGreaterThanOrEqual(r.box.y + ACT_HEAD * l.scale - 0.01);
      }
      l.regions.forEach((r, i) => i > 0 && expect(r.box.x).toBeGreaterThanOrEqual(l.regions[i - 1]!.box.x + l.regions[i - 1]!.box.w));
      // the regions fill the board's width
      const last = l.regions[l.regions.length - 1]!.box;
      expect(last.x + last.w).toBeGreaterThan(area.x + area.w - 2);
    });
  }

  it("draws the demo's acts at full size", () => {
    let n = 0;
    const regions = [5, 4, 4, 3].map((c, i) => ({ name: `Act ${i}`, hexes: Array.from({ length: c }, () => `H${n++}`) }));
    expect(layoutBoard(regions, FRAME).scale).toBe(1);
  });

  it("keeps a card's place whatever is face up: positions come from the case alone", () => {
    const regions = boardRegions(undefined, ids(12).map((id, i) => hex(id, i % 2 ? "claim" : "wallet")));
    expect(layoutBoard(regions, FRAME)).toEqual(layoutBoard(regions, FRAME));
    expect(regionOf(layoutBoard(regions, FRAME), "HEX-02")?.name).toBe("The claims");
  });
});

describe("the overlay lane", () => {
  const lane: Box = { x: 792, y: 0, w: 440, h: 706 };
  const framed: BoardFrame = { ...FRAME, lane };
  let n = 0;
  const regions = [6, 5, 5].map((c, i) => ({ name: `Act ${i}`, hexes: Array.from({ length: c }, () => `H${n++}`) }));
  const layout = layoutBoard(regions, framed);

  it("lays every act out left of it, and the overview stays the identity", () => {
    const area = boardArea(framed);
    expect(area.x + area.w).toBeLessThanOrEqual(lane.x);
    expect(layout.area).toEqual(area);
    for (const r of layout.regions) expect(r.box.x + r.box.w, r.name).toBeLessThanOrEqual(lane.x);
    for (const [id, b] of Object.entries(layout.cards)) expect(hits(b, lane), id).toBe(false);
    expect(boardView(layout, framed, null, [])).toEqual({ fit: { scale: 1, x: 0, y: 0 }, ring: [], under: [] });
  });

  it("keeps every zoomed ring left of it, and cuts off the rest of the board at its edge", () => {
    const clip = boardClip(framed);
    expect(clip.x + clip.w).toBeLessThanOrEqual(lane.x);
    expect(clip.x + clip.w).toBeGreaterThanOrEqual(boardArea(framed).x + boardArea(framed).w);
    for (const focus of Object.keys(layout.cards)) {
      const v = boardView(layout, framed, focus, [{ from: "H0", to: "H7" }]);
      for (const id of v.ring) expect(hits(fitBox(layout.cards[id]!, v.fit), lane), `${focus}: ${id}`).toBe(false);
    }
    expect(boardClip(FRAME)).toEqual({ x: 0, y: 0, w: FRAME.w, h: FRAME.h });
  });

  it("ends the thesis band where it starts", () => {
    expect(thesisEnd(framed)).toBeLessThanOrEqual(lane.x);
    expect(thesisEnd(FRAME)).toBe(FRAME.w);
  });
});

describe("boardView", () => {
  let n = 0;
  const regions = [5, 4, 4, 3].map((c, i) => ({ name: `Act ${i}`, hexes: Array.from({ length: c }, () => `H${n++}`) }));
  const layout = layoutBoard(regions, FRAME);
  const lane: Box = { x: 760, y: 0, w: 440, h: 700 };

  it("is the identity on the board screen's own frame with nothing focused", () => {
    expect(boardView(layout, FRAME, null, [])).toEqual({ fit: { scale: 1, x: 0, y: 0 }, ring: [], under: [] });
  });

  it("zooms to a card and the cards strung to it, clear of him and of the lane, capped", () => {
    const links = [
      { from: "H0", to: "H5" },
      { from: "H9", to: "H0" },
      { from: "H3", to: "H4" },
    ];
    for (const focus of Object.keys(layout.cards)) {
      const v = boardView(layout, { ...FRAME, prefer: [lane] }, focus, links);
      expect(v.ring[0]).toBe(focus);
      expect(v.fit.scale).toBeLessThanOrEqual(BOARD_ZOOM_MAX);
      // A zoom only closes in; a lone card reads large.
      expect(v.fit.scale, focus).toBeGreaterThanOrEqual(1);
      const lone = boardView(layout, { ...FRAME, prefer: [lane] }, focus, []);
      expect(lone.fit.scale, focus).toBeGreaterThan(1.4);
      // ... with no overlay card standing over it.
      expect(hits(fitBox(layout.cards[focus]!, lone.fit), lane), focus).toBe(false);
      for (const id of v.ring) {
        const b = fitBox(layout.cards[id]!, v.fit);
        expect(hits(b, CORNER), `${focus}: ${id}`).toBe(false);
        expect(inside(b, boardArea(FRAME)), `${focus}: ${id}`).toBe(true);
      }
      for (const [id, b] of Object.entries(layout.cards)) {
        if (!v.under.includes(id)) expect(hits(fitBox(b, v.fit), CORNER), `${focus}: ${id}`).toBe(false);
      }
    }
    expect(boardView(layout, FRAME, "H0", links).ring.sort()).toEqual(["H0", "H5", "H9"]);
  });

  it("fits the whole layout into a smaller window, and leaves out what stands under a window in front", () => {
    const desk: BoardFrame = { w: 800, h: 540, avoid: [{ x: 488, y: 132, w: 768, h: 672 }, { x: -16, y: 528, w: 448, h: 448 }] };
    const v = boardView(layout, desk, "H14", []);
    expect(v.ring).toEqual(["H14"]);
    expect(hits(fitBox(layout.cards.H14!, v.fit), desk.avoid![0]!)).toBe(false);
    for (const [id, b] of Object.entries(layout.cards)) if (!v.under.includes(id)) for (const a of desk.avoid!) expect(hits(fitBox(b, v.fit), a), id).toBe(false);
    const overview = boardView(layout, desk, null, []);
    expect(overview.fit.scale).toBeLessThan(1);
  });

  it("is the overview for a focus that is not on the board", () => {
    expect(boardView(layout, FRAME, "HEX-99", []).ring).toEqual([]);
  });
});

describe("strings", () => {
  const a: Box = { x: 0, y: 0, w: 200, h: 100 };
  const b: Box = { x: 500, y: 0, w: 200, h: 100 };
  const onEdge = (p: { x: number; y: number }, box: Box, pad: number) =>
    p.x >= box.x - pad - 0.5 && p.x <= box.x + box.w + pad + 0.5 && p.y >= box.y - pad - 0.5 && p.y <= box.y + box.h + pad + 0.5 && !inside({ ...p, w: 0, h: 0 }, { x: box.x + 1, y: box.y + 1, w: box.w - 2, h: box.h - 2 });

  it("runs from edge to edge, sags below the straight line, and ends in an arrowhead on the target", () => {
    const g = stringGeom(a, b)!;
    expect(onEdge(g.start, a, 4)).toBe(true);
    expect(onEdge(g.end, b, 3)).toBe(true);
    expect(g.control.y).toBeGreaterThan(50);
    expect(g.mid.y).toBeGreaterThan(50);
    expect(g.head.split(" ")[0]).toBe(`${Math.round(g.end.x * 10) / 10},${Math.round(g.end.y * 10) / 10}`);
    expect(g.d).toMatch(/^M [\d.-]+ [\d.-]+ Q [\d.-]+ [\d.-]+ [\d.-]+ [\d.-]+$/);
  });

  it("sags a second string between the same cards further, either way round", () => {
    expect(stringLanes([{ from: "A", to: "B" }, { from: "B", to: "A" }, { from: "A", to: "C" }])).toEqual([0, 1, 0]);
    expect(stringGeom(a, b, 1)!.mid.y).toBeGreaterThan(stringGeom(a, b, 0)!.mid.y + 10);
  });

  it("puts a label on the curve where it is free, else beside it on a leader, never over a card", () => {
    const free = stringGeom(a, b)!;
    const on = labelSpot(free, 100, 38, [a, b]);
    expect(on.anchor).toEqual({ x: on.x, y: on.y });
    // Two cards a few px apart: no room on the string, so the label steps off it.
    const top: Box = { x: 0, y: 0, w: 200, h: 100 };
    const under: Box = { x: 0, y: 120, w: 200, h: 100 };
    const right: Box = { x: 230, y: 0, w: 200, h: 220 };
    const g = stringGeom(top, under)!;
    const bounds: Box = { x: -400, y: -400, w: 1200, h: 1200 };
    const off = labelSpot(g, 100, 38, [top, under, right], bounds);
    const box = { x: off.x - 50, y: off.y - 19, w: 100, h: 38 };
    for (const c of [top, under, right]) expect(hits(box, c)).toBe(false);
    expect(Math.hypot(off.x - off.anchor.x, off.y - off.anchor.y)).toBeGreaterThan(1);
    // ... hanging from a point on its string.
    expect([0.2, 0.34, 0.5, 0.66, 0.8].some((t) => Math.hypot(stringAt(g, t).x - off.anchor.x, stringAt(g, t).y - off.anchor.y) < 0.01)).toBe(true);
  });

  it("sags downward whichever way it runs, and is null between touching cards", () => {
    expect(stringGeom(b, a)!.mid.y).toBeGreaterThan(50);
    const below: Box = { x: 0, y: 400, w: 200, h: 100 };
    expect(stringGeom(a, below)).not.toBeNull();
    expect(stringGeom(a, { x: 150, y: 50, w: 200, h: 100 })).toBeNull();
  });
});

describe("timeline strip", () => {
  const hexes = [hex("HEX-01", "entity", { date: "2025-01-01" }), hex("HEX-02", "claim", { date: "2026-01-01" }), hex("HEX-03", "claim", { date: "2025-07-01" }), hex("HEX-04")];
  const rec: ShownRecord = { id: "DOC-01", hex: "HEX-01", grade: "DOC", title: "t", source: "s", summary: "s", fictional: true, date: "2025-03-01" };
  const short = (h: Hex) => h.id;

  it("marks face-up cards and records with dates, never a face-down card's date, on an axis that does not move", () => {
    const none = timelineMarks(hexes, {}, [], 800, short);
    expect(none.marks).toEqual([]);
    expect([none.from, none.to]).toEqual(["2025-01-01", "2026-01-01"]);
    const some = timelineMarks(hexes, { "HEX-03": "CLM", "HEX-04": "REP" }, [rec], 800, short);
    expect(some.marks.map((m) => [m.kind, m.id])).toEqual([
      ["record", "DOC-01"],
      ["card", "HEX-03"],
    ]);
    const all = timelineMarks(hexes, { "HEX-01": "DOC", "HEX-02": "CLM", "HEX-03": "CLM" }, [rec], 800, short);
    expect(all.marks.find((m) => m.id === "HEX-03")!.x).toBeCloseTo(some.marks.find((m) => m.id === "HEX-03")!.x);
    for (const m of all.marks) expect(m.x > 0 && m.x < 800, m.id).toBe(true);
  });

  it("gives labels the first free row and a bare mark when both are taken", () => {
    const crowd = ids(5).map((id) => hex(id, "claim", { date: "2025-05-01" }));
    const up = Object.fromEntries(crowd.map((h) => [h.id, "REP" as const]));
    const t = timelineMarks(crowd, up, [], 600, short);
    expect(t.marks.map((m) => m.row)).toEqual([0, 1, null, null, null]);
  });

  it("is empty when nothing is dated", () => {
    expect(timelineMarks([hex("HEX-01")], { "HEX-01": "DOC" }, [], 800, short)).toEqual({ marks: [], from: null, to: null });
  });
});

describe("stripBox", () => {
  it("starts right of his corner and stays inside the window", () => {
    const s = stripBox(FRAME);
    expect(s.x).toBeGreaterThanOrEqual(CORNER.x + CORNER.w);
    expect(s.x + s.w).toBeLessThanOrEqual(FRAME.w);
    expect(s.y + s.h).toBeLessThanOrEqual(FRAME.h);
    expect(s.y).toBeGreaterThanOrEqual(boardArea(FRAME).y + boardArea(FRAME).h);
  });
});
