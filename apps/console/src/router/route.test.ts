import { describe, expect, it } from "vitest";
import { WALL, crumbs, decodeRoute, encodeRoute, humanize, layerOf, pageRoute, up, type Route } from "./route";

const TRICKY = ["a/b", "100%", "50%25", "space here", "ťěžký", "~tilde~", "#hash", "?q=1&x", "%2F", "..", "SEG-02A|t1"];

describe("URL contract: one encoder, one decoder", () => {
  it("round-trips every page and depth", () => {
    const routes: Route[] = [
      WALL,
      pageRoute("onair"),
      pageRoute("simulator", "heatmap"),
      pageRoute("brain", "transcripts", "t-1/run-7", "42"),
      { view: "legacy", sub: [] },
      { view: "legacy", sub: ["simulations"] },
    ];
    for (const r of routes) {
      const d = decodeRoute(encodeRoute(r));
      expect(d.ok).toBe(true);
      expect(d.route).toEqual(r);
    }
  });

  it("round-trips segments that the winner had to mangle", () => {
    for (const seg of TRICKY) {
      const r = pageRoute("simulator", "heatmap", seg);
      const hash = encodeRoute(r);
      expect(hash.split("/")).toHaveLength(4); // a "/" inside a segment never splits it
      expect(decodeRoute(hash).route).toEqual(r);
    }
  });

  it("decodes exactly once (the winner's heatmap key was double-decoded)", () => {
    // The key "50%25" encodes to "50%2525"; a second decode would turn it into "50%".
    const hash = encodeRoute(pageRoute("simulator", "heatmap", "50%25"));
    expect(hash).toBe("#/simulator/heatmap/50%2525");
    expect(decodeRoute(hash).route).toEqual(pageRoute("simulator", "heatmap", "50%25"));
  });

  it("lands empty and root hashes on the wall", () => {
    for (const h of ["", "#", "#/", "#/wall"]) expect(decodeRoute(h)).toEqual({ ok: true, route: WALL });
    expect(encodeRoute(WALL)).toBe("#/wall");
  });

  it("sends unknown pages and malformed escapes to the wall with a reason", () => {
    const unknown = decodeRoute("#/nope/x");
    expect(unknown.ok).toBe(false);
    expect(unknown.route).toEqual(WALL);
    const malformed = decodeRoute("#/onair/%E0%A4%A");
    expect(malformed.ok).toBe(false);
    expect(decodeRoute("#/wall/extra").ok).toBe(false);
  });

  it("names the layer of each route", () => {
    expect(layerOf(WALL)).toBe("wall");
    expect(layerOf(pageRoute("brain"))).toBe("l1");
    expect(layerOf(pageRoute("brain", "transcripts"))).toBe("l2");
    expect(layerOf({ view: "legacy", sub: ["x"] })).toBe("legacy");
  });
});

describe("Esc goes up one segment (A/1's router)", () => {
  it("climbs layer 2 → layer 1 → wall, and does nothing on the wall", () => {
    let r: Route | null = pageRoute("brain", "transcripts", "run-7", "42");
    const trail: string[] = [];
    while (r !== null) {
      trail.push(encodeRoute(r));
      r = up(r);
    }
    expect(trail).toEqual(["#/brain/transcripts/run-7/42", "#/brain/transcripts/run-7", "#/brain/transcripts", "#/brain", "#/wall"]);
    expect(up(WALL)).toBeNull();
  });

  it("climbs out of the legacy desk to the wall", () => {
    expect(up({ view: "legacy", sub: ["lord"] })).toEqual({ view: "legacy", sub: [] });
    expect(up({ view: "legacy", sub: [] })).toEqual(WALL);
  });
});

describe("breadcrumbs from ids", () => {
  it("labels each segment, falling back to humanised ids", () => {
    const labels: Record<string, string> = { brain: "Brain", transcripts: "Transcripts" };
    const c = crumbs(pageRoute("brain", "transcripts", "rule_of_three"), (seg) => labels[seg]);
    expect(c.map((x) => x.label)).toEqual(["Season", "Brain", "Transcripts", "Rule of three"]);
    expect(c.map((x) => x.current)).toEqual([false, false, false, true]);
    expect(encodeRoute(c[2]?.route ?? WALL)).toBe("#/brain/transcripts");
  });

  it("keeps a slash-bearing id as one crumb", () => {
    const c = crumbs(pageRoute("simulator", "heatmap", "a/b"), () => undefined);
    expect(c).toHaveLength(4);
    expect(c[3]?.label).toBe("A/b");
  });

  it("humanises ids", () => {
    expect(humanize("dread_council")).toBe("Dread council");
    expect(humanize("seg-02a")).toBe("Seg 02a");
  });
});
