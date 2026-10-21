// The streamer screens' pure parts: fitting the hexmap around what stands over it, and the
// browser window's text helpers.

import { describe, expect, it } from "vitest";

import type { Hex, ShownRecord } from "@shadowrunr/contract";
import { fitMap, hexBoxes, spiral, type Box } from "./hexgrid";
import { provenance, sourceParts, tabTitle } from "./ui/Browser";

const hexes = (n: number): Hex[] => spiral(n).map(([q, r], i) => ({ id: `HEX-${i}`, q, r, kind: "entity", label: `h${i}`, fog: true }));
const hits = (a: Box, b: Box) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
const inside = (a: Box, o: Box) => a.x >= o.x - 0.01 && a.y >= o.y - 0.01 && a.x + a.w <= o.x + o.w + 0.01 && a.y + a.h <= o.y + o.h + 0.01;

describe("fitMap", () => {
  const area: Box = { x: 20, y: 16, w: 1216, h: 742 };
  const corner: Box = { x: -16, y: 528, w: 448, h: 448 };

  it("fills the area when nothing stands over it, capped at the largest scale", () => {
    const f = fitMap(hexes(16), 74, area, [], [], 1.25);
    for (const b of hexBoxes(hexes(16), 74, f)) expect(inside(b, area)).toBe(true);
    expect(f.scale).toBeGreaterThan(1);
  });

  for (const n of [1, 7, 16, 19, 37]) {
    it(`keeps every one of ${n} hexes out of the corner and inside the area`, () => {
      const hs = hexes(n);
      const f = fitMap(hs, 74, area, [corner]);
      for (const b of hexBoxes(hs, 74, f)) {
        expect(hits(b, corner)).toBe(false);
        expect(inside(b, area)).toBe(true);
      }
    });
  }

  it("keeps a preferred lane clear while that is cheap, and gives it up when it is not", () => {
    const hs = hexes(16);
    const lane: Box = { x: 1100, y: 16, w: 136, h: 200 };
    const kept = fitMap(hs, 74, area, [corner], [lane]);
    for (const b of hexBoxes(hs, 74, kept)) expect(hits(b, lane)).toBe(false);
    const wide: Box = { x: 100, y: 16, w: 1136, h: 742 };
    const given = fitMap(hs, 74, area, [corner], [wide]);
    expect(given.scale).toBeCloseTo(fitMap(hs, 74, area, [corner]).scale);
  });
});

describe("browser window text", () => {
  const rec: ShownRecord = {
    id: "REP-01",
    hex: "HEX-03",
    grade: "REP",
    title: "Glimmerquay landing page promises 4% a week (fictional)",
    source: "https://records.example/glimmerquay/landing?v=2",
    summary: "s",
    fictional: true,
  };

  it("shortens a tab title and drops the fictional marker", () => {
    expect(tabTitle(rec.title)).toBe("Glimmerquay landing pag…");
    expect(tabTitle("Short (fictional)")).toBe("Short");
  });

  it("splits the source into host and path", () => {
    expect(sourceParts(rec.source)).toEqual({ host: "records.example", path: "/glimmerquay/landing?v=2" });
    expect(sourceParts("https://records.example/")).toEqual({ host: "records.example", path: "" });
    expect(sourceParts("not a url")).toEqual({ host: "not a url", path: "" });
  });

  it("names the provenance: Wayback first, else the retrieved date, else nothing", () => {
    expect(provenance(rec)).toBeNull();
    expect(provenance({ ...rec, retrieved: "2026-10-01" })).toBe("retrieved 2026-10-01");
    expect(provenance({ ...rec, retrieved: "2026-10-01", via: "https://web.archive.org/web/2026/x" })).toBe("via Wayback");
  });
});
