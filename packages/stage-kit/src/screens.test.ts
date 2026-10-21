// The streamer screens' pure parts: fitting the hexmap around what stands over it, and the
// browser window's text helpers.

import { describe, expect, it } from "vitest";

import type { Hex, ShownRecord } from "@shadowrunr/contract";
import { fitMap, hexBoxes, hexDistance, MAP_ZOOM_MAX, spiral, zoomMap, type Box } from "./hexgrid";
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

describe("zoomMap", () => {
  const area: Box = { x: 20, y: 16, w: 1216, h: 742 };
  const corner: Box = { x: -16, y: 528, w: 448, h: 448 };
  const hs = hexes(37);

  it("zooms to the focused hex and its ring, larger than the overview and capped", () => {
    const overview = fitMap(hs, 74, area, [corner]);
    const z = zoomMap(hs, 74, "HEX-0", area, [corner])!;
    expect(z.ring.sort()).toEqual(hs.filter((h) => hexDistance(h, hs[0]!) <= 1).map((h) => h.id).sort());
    expect(z.ring).toHaveLength(7);
    expect(z.fit.scale).toBeGreaterThan(overview.scale * 2);
    expect(z.fit.scale).toBeLessThanOrEqual(MAP_ZOOM_MAX);
    // Every focus zooms alike: the fit is the ring's cells, not the hexes the case has there.
    const edge = zoomMap(hs, 74, "HEX-36", area, [corner])!;
    expect(edge.ring.length).toBeLessThan(7);
    expect(edge.fit.scale).toBeCloseTo(z.fit.scale, 1);
  });

  for (const focus of ["HEX-0", "HEX-5", "HEX-20", "HEX-36"]) {
    it(`focused on ${focus}: the ring stays inside the area and no drawn hex lies under the corner`, () => {
      const z = zoomMap(hs, 74, focus, area, [corner])!;
      const boxes = hexBoxes(hs, 74, z.fit);
      hs.forEach((h, i) => {
        if (z.ring.includes(h.id)) expect(inside(boxes[i]!, area), h.id).toBe(true);
        if (!z.under.includes(h.id)) expect(hits(boxes[i]!, corner), h.id).toBe(false);
      });
      for (const id of z.under) expect(z.ring).not.toContain(id);
    });
  }

  it("leaves out the hexes the zoom puts under the corner", () => {
    const z = zoomMap(hs, 74, "HEX-0", area, [corner])!;
    expect(z.under.length).toBeGreaterThan(0);
  });

  it("is null for a hex that is not on the map", () => {
    expect(zoomMap(hs, 74, "HEX-99", area, [corner])).toBeNull();
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
