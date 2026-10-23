// The stage's own modules: the 1920x1080 layouts and where a transcript and its case come from.
// The scene, playback, timing and geometry tests live with them in @shadowrunr/stage-kit.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import type { Case, StreamEvent } from "@shadowrunr/contract";
import { ACT_HEAD, boardArea, boardClip, boardView, buildMap, CANVAS_H, CANVAS_W, fitBox, fitMap, foldEvents, hexBoxes, stripBox, thesisEnd, zoomMap } from "@shadowrunr/stage-kit";
import { parseCase, parseTranscript } from "@shadowrunr/stage-kit/parse";
import { boardLayoutFor, HEX_SIZE, hexArea, placeStrings } from "@shadowrunr/stage-kit/ui";
import { LOOK_CHAT, LOOK_CONTENT, LOOK_HOST, lookFor } from "./avatar/look";
import { BOARD_LAYOUT_FRAME, boardFrame, BROWSER_CHROME, CONTENT, CORNER_AVATAR, FACE_PX, hexFrame, layoutRects, REGIONS, type Rect } from "./layouts";
import { readParams, resolveCase, sidecarCaseUrl } from "./source";

const LAYOUTS = ["boot", "host", "board", "hexmap", "browser", "desk"] as const;
const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
/** `b` (in a window's pixels) on the canvas. */
const shift = (b: Rect, by: Rect): Rect => ({ x: b.x + by.x, y: b.y + by.y, w: b.w, h: b.h });

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
    for (const layout of ["board", "hexmap", "browser", "desk"] as const) {
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

  it("puts the desk's browser window in front of a smaller map window: the board, or the hexmap when that was the last map", () => {
    for (const back of ["board", "hexmap"] as const) {
      const other = back === "board" ? "hexmap" : "board";
      const desk = layoutRects("desk", { poll: false, foil: false, back });
      const full = layoutRects(back, { poll: false, foil: false });
      expect(desk[back].w * desk[back].h).toBeLessThan(full[back].w * full[back].h);
      expect(overlaps(desk[back], desk.browser)).toBe(true);
      expect(desk[other].hidden).toBe(true);
      expect(desk.browser.hidden).toBeUndefined();
      expect(full.browser.hidden).toBe(true);
      expect(full[other].hidden).toBe(true);
    }
    expect(layoutRects("desk", { poll: false, foil: false }).board.hidden).toBeUndefined();
    expect(layoutRects("browser", { poll: false, foil: false }).hexmap.hidden).toBe(true);
    expect(layoutRects("browser", { poll: false, foil: false }).board.hidden).toBe(true);
  });

  it("fills the content area with the board, the rect the clip factory's 9:16 reframe crops", () => {
    const footage = readFileSync(join(__dirname, "..", "..", "..", "crates", "shadowrunr-clips", "src", "footage.rs"), "utf8");
    const m = /pub const CONTENT_SCREEN: Rect = Rect \{\s*x: (\d+),\s*y: (\d+),\s*w: (\d+),\s*h: (\d+),\s*\}/.exec(footage);
    expect(m).not.toBeNull();
    const reframe = { x: Number(m![1]), y: Number(m![2]), w: Number(m![3]), h: Number(m![4]) };
    for (const layout of ["board", "hexmap", "browser"] as const) {
      const r = layoutRects(layout, { poll: true, foil: true });
      expect(r[layout], layout).toEqual(CONTENT);
    }
    expect(CONTENT).toEqual(reframe);
    // The board's layout frame is that rect, Malakor's corner in it.
    expect([BOARD_LAYOUT_FRAME.w, BOARD_LAYOUT_FRAME.h]).toEqual([CONTENT.w, CONTENT.h]);
  });

  it("keeps every overlay off the browser window's chrome (tabs, URL bar, ribbon) on browser and desk", () => {
    for (const layout of ["browser", "desk"] as const) {
      for (const poll of [false, true]) {
        for (const foil of [false, true]) {
          const r = layoutRects(layout, { poll, foil });
          const chrome: Rect = { x: r.browser.x, y: r.browser.y, w: r.browser.w, h: BROWSER_CHROME };
          for (const k of REGIONS) {
            if (k === "browser" || k === "hexmap" || k === "board" || r[k].hidden) continue;
            expect(overlaps(r[k], chrome), `${layout} poll=${poll} foil=${foil}: ${k}`).toBe(false);
          }
          // the lane still ends above the lower third
          for (const k of ["observer", "decrees", "card"] as const) expect(r[k].y + r[k].h, k).toBeLessThanOrEqual(r.caption.y);
        }
      }
    }
    // the hexmap screen's lane is where it was
    const map = layoutRects("hexmap", { poll: false, foil: true });
    expect(map.observer.y).toBe(124);
    expect(map.card).toMatchObject({ y: 510, h: 300 });
  });

  it("gives the board (or the hexmap) the dominant area in SEG-02A", () => {
    for (const main of ["board", "hexmap"] as const) {
      const r = layoutRects(main, { poll: true, foil: true });
      const area = (k: (typeof REGIONS)[number]) => r[k].w * r[k].h;
      for (const k of REGIONS) if (k !== main && !r[k].hidden) expect(area(main), k).toBeGreaterThan(area(k) * 3);
    }
  });
});

describe("no hex under him", () => {
  const demo = parseCase(readFileSync(join(__dirname, "..", "fixtures", "demo-case.json"), "utf8"));
  for (const layout of ["hexmap", "desk"] as const) {
    for (const poll of [false, true]) {
      it(`${layout} poll=${poll}: no hex of the demo case lies under the avatar's box (or the desk's browser)`, () => {
        const rects = layoutRects(layout, { poll, foil: true, back: "hexmap" });
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
  for (const layout of ["hexmap", "desk"] as const) {
    it(`${layout}: focused on any demo hex, no drawn hex lies under him (or the desk's browser), and the ring reads large`, () => {
      const rects = layoutRects(layout, { poll: true, foil: true, back: "hexmap" });
      const frame = hexFrame(layout, rects)!;
      const overview = fitMap(demo.hexes, HEX_SIZE, hexArea(frame), frame.avoid, frame.prefer);
      const map = rects.hexmap;
      for (const focus of demo.hexes) {
        const zoom = zoomMap(demo.hexes, HEX_SIZE, focus.id, hexArea(frame), frame.avoid, frame.prefer)!;
        expect(zoom.ring).toContain(focus.id);
        expect(zoom.fit.scale, focus.id).toBeGreaterThan(overview.scale * 1.4);
        const boxes = hexBoxes(demo.hexes, HEX_SIZE, zoom.fit).map((b) => ({ ...b, x: b.x + map.x, y: b.y + map.y }));
        demo.hexes.forEach((h, i) => {
          if (zoom.under.includes(h.id)) return;
          expect(overlaps(boxes[i]!, rects.avatar), `${focus.id}: ${h.id}`).toBe(false);
          if (layout === "desk") expect(overlaps(boxes[i]!, rects.browser), `${focus.id}: ${h.id}`).toBe(false);
        });
        // No overlay card stands over the ring on the hexmap screen (the lane is kept clear).
        for (const lane of frame.prefer ?? []) {
          demo.hexes.forEach((h, i) => {
            if (zoom.ring.includes(h.id)) expect(overlaps(boxes[i]!, { ...lane, x: lane.x + map.x, y: lane.y + map.y }), `${focus.id}: ${h.id}`).toBe(false);
          });
        }
        // The ring itself stays inside its window.
        demo.hexes.forEach((h, i) => {
          if (!zoom.ring.includes(h.id)) return;
          const b = boxes[i]!;
          expect(b.x >= map.x && b.y >= map.y && b.x + b.w <= map.x + map.w && b.y + b.h <= map.y + map.h, `${focus.id}: ${h.id}`).toBe(true);
        });
      }
    });
  }
  it("has no frame where the map is not shown", () => {
    for (const layout of ["boot", "host", "browser", "board"] as const) expect(hexFrame(layout, layoutRects(layout, { poll: false, foil: false }))).toBeNull();
    expect(hexFrame("desk", layoutRects("desk", { poll: false, foil: false }))).toBeNull();
  });
});

describe("no card under him", () => {
  const demo = parseCase(readFileSync(join(__dirname, "..", "fixtures", "demo-case.json"), "utf8"));
  const map = buildMap(demo, []);
  const layout = boardLayoutFor(map, demo, BOARD_LAYOUT_FRAME);

  it("lays every demo card out in its act, none overlapping, none under him, every act inside the content area", () => {
    const rects = layoutRects("board", { poll: true, foil: true });
    const win = rects.board;
    const cards = Object.entries(layout.cards);
    expect(cards).toHaveLength(demo.hexes.length);
    expect(layout.regions.map((r) => r.name)).toEqual(demo.acts!.map((a) => a.name));
    // The acts end left of the overlay lane, so the demo's 16 cards draw smaller than full size
    // in the overview (0.69 measured); a focus zooms them back up.
    expect(layout.scale).toBeGreaterThan(0.65);
    cards.forEach(([id, b], i) => {
      expect(overlaps(shift(b, win), rects.avatar), id).toBe(false);
      cards.forEach(([other, o], j) => j > i && expect(overlaps(b, o), `${id} vs ${other}`).toBe(false));
    });
    for (const r of layout.regions) {
      const box = shift(r.box, win);
      expect(box.x >= win.x && box.y >= win.y && box.x + box.w <= win.x + win.w && box.y + box.h <= win.y + win.h, r.name).toBe(true);
    }
    // The timeline strip runs from his right edge, above the lower third.
    const strip = shift(stripBox(boardFrame("board", rects)!), win);
    expect(overlaps(strip, rects.avatar)).toBe(false);
    expect(strip.y + strip.h).toBeLessThanOrEqual(rects.caption.y);
  });

  for (const layout_ of ["board", "desk"] as const) {
    it(`${layout_}: focused on any demo card, no drawn card lies under him (or the desk's browser), and the card reads large`, () => {
      const rects = layoutRects(layout_, { poll: true, foil: true });
      const frame = boardFrame(layout_, rects)!;
      expect(frame).not.toBeNull();
      const win = rects.board;
      const links = [
        { from: "HEX-01", to: "HEX-05" },
        { from: "HEX-06", to: "HEX-01" },
        { from: "HEX-04", to: "HEX-09" },
      ];
      for (const focus of [null, ...demo.hexes.map((h) => h.id)]) {
        const view = boardView(layout, frame, focus, links);
        for (const [id, b] of Object.entries(layout.cards)) {
          if (view.under.includes(id)) continue;
          const at = shift(fitBox(b, view.fit), win);
          expect(overlaps(at, rects.avatar), `${focus}: ${id}`).toBe(false);
          if (layout_ === "desk") expect(overlaps(at, rects.browser), `${focus}: ${id}`).toBe(false);
        }
        if (focus === null) continue;
        const card = shift(fitBox(layout.cards[focus]!, view.fit), win);
        expect(card.w, focus).toBeGreaterThan(layout_ === "desk" ? 150 : 212);
        // inside its window, under the thesis band
        const area = shift(boardArea(frame), win);
        expect(card.x >= area.x - 0.01 && card.y >= area.y - 0.01 && card.x + card.w <= area.x + area.w + 0.01 && card.y + card.h <= area.y + area.h + 0.01, focus).toBe(true);
      }
    });
  }

  it("has no board frame where the board is not shown", () => {
    expect(BOARD_LAYOUT_FRAME.lane).toBeDefined();
    for (const layout_ of ["boot", "host", "browser", "hexmap"] as const) expect(boardFrame(layout_, layoutRects(layout_, { poll: false, foil: false }))).toBeNull();
    expect(boardFrame("desk", layoutRects("desk", { poll: false, foil: false, back: "hexmap" }))).toBeNull();
  });
});

describe("no overlay over the board", () => {
  const demo = parseCase(readFileSync(join(__dirname, "..", "fixtures", "demo-case.json"), "utf8"));
  const map = buildMap(demo, []);
  const layout = boardLayoutFor(map, demo, BOARD_LAYOUT_FRAME);
  const clipTo = (b: Rect, o: Rect): Rect | null => {
    const x = Math.max(b.x, o.x);
    const y = Math.max(b.y, o.y);
    const w = Math.min(b.x + b.w, o.x + o.w) - x;
    const h = Math.min(b.y + b.h, o.y + o.h) - y;
    return w > 0 && h > 0 ? { x, y, w, h } : null;
  };
  // The demo's six strings and where their labels go.
  const links = foldEvents(parseTranscript(readFileSync(join(__dirname, "..", "fixtures", "demo.jsonl"), "utf8")).events).links;
  const labels = placeStrings(links, layout, Math.max(0.75, layout.scale)).map((p) => p?.label ?? null);

  for (const screen of ["board", "desk"] as const) {
    for (const poll of [false, true]) {
      for (const foil of [false, true]) {
        it(`${screen} poll=${poll} foil=${foil}: no overlay (the observer badge, a decree, an explainer card) stands over any act, overview or zoomed`, () => {
          const rects = layoutRects(screen, { poll, foil });
          const win = rects.board;
          const frame = boardFrame(screen, rects)!;
          // The decree and the card regions are always laid out; the badge's place is kept even
          // with no foil in, so every combination of them showing is these three rects.
          const overlays = (["observer", "decrees", "card"] as const).map((k) => [k, rects[k]] as const);
          // What a zoom moves toward the lane is cut off at its edge.
          const drawn = shift(boardClip(frame), win);
          for (const focus of [null, ...demo.hexes.map((h) => h.id)]) {
            const view = boardView(layout, frame, focus, links, labels);
            // On the board a focused card's string labels are in view, none cut off at the lane.
            if (screen === "board" && focus !== null) {
              links.forEach((l, i) => {
                if (l.from !== focus && l.to !== focus) return;
                const b = shift(fitBox(labels[i]!, view.fit), win);
                expect(b.x >= drawn.x - 0.01 && b.y >= drawn.y - 0.01 && b.x + b.w <= drawn.x + drawn.w + 0.01 && b.y + b.h <= drawn.y + drawn.h + 0.01, `${focus}: ${l.key}`).toBe(true);
              });
            }
            for (const r of layout.regions) {
              const shown = clipTo(shift(fitBox(r.box, view.fit), win), drawn);
              if (!shown) continue;
              for (const [k, o] of overlays) expect(overlaps(shown, o), `${focus}: ${r.name} under ${k}`).toBe(false);
            }
          }
          // The thesis band ends left of the lane.
          for (const [k, o] of overlays) if (o.y < win.y + 64) expect(win.x + thesisEnd(frame), k).toBeLessThanOrEqual(o.x);
        });
      }
    }
  }

  it("keeps the browser screen's lane where it was", () => {
    const r = layoutRects("browser", { poll: false, foil: true });
    expect([r.observer.x, r.observer.y, r.card.x]).toEqual([824, 250, 824]);
  });
});

describe("string labels", () => {
  const demo = parseCase(readFileSync(join(__dirname, "..", "fixtures", "demo-case.json"), "utf8"));
  const layout = boardLayoutFor(buildMap(demo, []), demo, BOARD_LAYOUT_FRAME);
  const transcript = parseTranscript(readFileSync(join(__dirname, "..", "fixtures", "demo.jsonl"), "utf8"));
  const scene = foldEvents(transcript.events);

  it("puts no label of the demo's six strings over a card, an act name, another label or him, and keeps each on the board", () => {
    expect(scene.links).toHaveLength(6);
    const placed = placeStrings(scene.links, layout, Math.max(0.75, layout.scale));
    const cards = Object.entries(layout.cards);
    const heads = layout.regions.map((r) => ({ x: r.box.x, y: r.box.y, w: r.box.w, h: ACT_HEAD * layout.scale }));
    placed.forEach((p, i) => {
      const key = scene.links[i]!.key;
      expect(p, key).not.toBeNull();
      const label = p!.label;
      for (const [id, b] of cards) expect(overlaps(label, b), `${key} over ${id}`).toBe(false);
      for (const h of heads) expect(overlaps(label, h), `${key} over an act name`).toBe(false);
      for (const a of layout.avoid) expect(overlaps(label, a), `${key} under him`).toBe(false);
      placed.forEach((q, j) => j > i && expect(overlaps(label, q!.label), `${key} vs ${scene.links[j]!.key}`).toBe(false));
      const area = layout.area;
      expect(label.x >= area.x && label.y >= area.y && label.x + label.w <= area.x + area.w && label.y + label.h <= area.y + area.h, key).toBe(true);
      // A label off its string hangs close to it.
      expect(Math.hypot(p!.at.x - p!.at.anchor.x, p!.at.y - p!.at.anchor.y), key).toBeLessThan(160);
    });
  });
});

describe("look", () => {
  it("looks at the viewer in the host room, at the content from the corner, at chat when answering", () => {
    expect(lookFor("host", [])).toBe(LOOK_HOST);
    expect(lookFor("boot", null)).toBe(LOOK_HOST);
    for (const l of ["board", "hexmap", "browser", "desk"] as const) expect(lookFor(l, null)).toBe(LOOK_CONTENT);
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
    expect(readParams("").avatar).toBe("lich3d");
    expect(readParams("?avatar=glow").avatar).toBe("glow");
    expect(readParams("?avatar=cut").avatar).toBe("cut");
    expect(readParams("?avatar=depth").avatar).toBe("depth");
    expect(readParams("?avatar=nope").avatar).toBe("lich3d");
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
