// Go to anything: the pure resolver behind Ctrl+K. A pasted ref, hash or segment id, a page name,
// or a place visited this session, each resolved through the router's own vocabulary.
import { describe, expect, it } from "vitest";
import { decodeRoute, encodeRoute, pageRoute, WALL, type Route } from "../router";
import { BRAIN_ROUTES } from "../router/brain";
import { nextRecents } from "./keyActions";
import { placesOf, resolveQuery } from "./goto";
import { pageLabeller } from "./rise";

const hashes = (rs: readonly Route[]) => rs.map(encodeRoute);

describe("resolveQuery", () => {
  it("opens a pasted run#line ref at its line", () => {
    const [first] = resolveQuery("calm.t1.SEG-02A.a40.s7#12", []);
    expect(first && encodeRoute(first.route)).toBe("#/brain/transcripts/calm.t1.SEG-02A.a40.s7/12");
    expect(first?.label).toBe("Transcript line 12");
  });

  it("keeps a tournament-qualified ref as one segment", () => {
    const [first] = resolveQuery("t2/r7#3", []);
    expect(first && encodeRoute(first.route)).toBe("#/brain/transcripts/t2%2Fr7/3");
  });

  it("reads a pasted hash or console URL through decodeRoute, and offers nothing for an unknown page", () => {
    const want = pageRoute("simulator", "heatmap", "a/b");
    expect(resolveQuery("#/simulator/heatmap/a%2Fb")[0]?.route).toEqual(want);
    expect(resolveQuery("http://127.0.0.1:1420/#/simulator/heatmap/a%2Fb")[0]?.route).toEqual(want);
    expect(resolveQuery("#/nope").filter((c) => c.kind === "link")).toEqual([]);
    expect(resolveQuery("#/nope")).toEqual([]);
  });

  it("routes a segment id to its table, and matches pages from the poster copy", () => {
    expect(resolveQuery("SEG-02A")[0]?.route).toEqual(pageRoute("segments", "tables", "SEG-02A"));
    const tri = resolveQuery("tri");
    expect(tri[0]).toMatchObject({ kind: "page", route: pageRoute("simulator") });
    expect(tri.filter((c) => c.kind === "page")).toHaveLength(1);
  });

  it("lists recent places first for an empty query, and filters them by label", () => {
    const places = [
      { route: pageRoute("brain", "engine"), label: "Brain / The engine" },
      { route: pageRoute("review", "s1"), label: "Session review / S1" },
    ];
    expect(resolveQuery("", places).slice(0, 2).map((c) => c.kind)).toEqual(["recent", "recent"]);
    expect(resolveQuery("engine", places).map((c) => c.label)).toContain("Brain / The engine");
  });
});

describe("recent places", () => {
  it("keeps visits deduped, newest first, capped at 8, without the wall, labelled from crumbs()", () => {
    let recents: readonly Route[] = [];
    for (const h of ["#/brain/engine", "#/wall", "#/review/s1/timeline", "#/brain/engine"]) recents = nextRecents(recents, decodeRoute(h).route);
    expect(hashes(recents)).toEqual(["#/brain/engine", "#/review/s1/timeline"]);
    for (let i = 0; i < 12; i++) recents = nextRecents(recents, pageRoute("segments", "tables", `S${i}`));
    expect(recents).toHaveLength(8);
    expect(nextRecents([], WALL)).toEqual([]);

    const label = pageLabeller({ brain: { routes: BRAIN_ROUTES } });
    expect(placesOf([pageRoute("brain", "engine"), pageRoute("review", "s1", "timeline")], label).map((p) => p.label)).toEqual([
      "Brain / The engine",
      "Session review / S1 / Timeline",
    ]);
  });
});
