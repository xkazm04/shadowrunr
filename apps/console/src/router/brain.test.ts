// Brain's URL grammar, declared once: the parser and the cross-page transcript links derive from
// BRAIN_ROUTES (the shell's rise key and breadcrumbs are pinned in shell/subroute.dom.test.tsx).
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { BRAIN_ROUTES, floorRoute, transcriptRoute } from "./brain";
import { encodeRoute } from "./route";
import { matchSub } from "./schema";

describe("BRAIN_ROUTES", () => {
  it("reads a qualified run and its line by name, rising at the run", () => {
    expect(matchSub(BRAIN_ROUTES, ["transcripts", "t1/r7", "12"])).toEqual({ ok: true, params: { run: "t1/r7", line: "12" }, riseDepth: 2 });
  });

  it("names no floor called nonsense, and falls back to layer 1", () => {
    expect(matchSub(BRAIN_ROUTES, ["nonsense"])).toEqual({ ok: false, fallback: [] });
    expect(matchSub(BRAIN_ROUTES, ["transcripts", "r1", "x"])).toEqual({ ok: false, fallback: ["transcripts", "r1"] });
    expect(matchSub(BRAIN_ROUTES, ["engine", "extra"])).toEqual({ ok: false, fallback: ["engine"] });
  });
});

describe("transcriptRoute", () => {
  it("qualifies the run whenever the tournament is known", () => {
    expect(encodeRoute(transcriptRoute({ tournamentId: "t2", runId: "r7" }, 12))).toBe("#/brain/transcripts/t2%2Fr7/12");
    expect(encodeRoute(transcriptRoute({ tournamentId: "t2", runId: "r7" }))).toBe("#/brain/transcripts/t2%2Fr7");
  });

  it("links a bare run only when no tournament is known, and never qualifies twice", () => {
    expect(encodeRoute(transcriptRoute({ runId: "r7" }))).toBe("#/brain/transcripts/r7");
    expect(encodeRoute(transcriptRoute({ runId: "t1/r7" }, 3))).toBe("#/brain/transcripts/t1%2Fr7/3");
    expect(encodeRoute(transcriptRoute({ tournamentId: "t2", runId: "t1/r7" }))).toBe("#/brain/transcripts/t1%2Fr7");
  });

  it("builds only routes the schema accepts", () => {
    for (const r of [transcriptRoute({ tournamentId: "t", runId: "r" }, 1), transcriptRoute({ runId: "r" }), floorRoute("engine"), floorRoute("transcripts"), floorRoute("preflight")]) {
      expect(r.view === "page" && r.page === "brain" && matchSub(BRAIN_ROUTES, r.sub).ok).toBe(true);
    }
  });
});

/** Every non-test source file under a folder. */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return sources(p);
    return /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f) ? [p] : [];
  });
}

describe("cross-page transcript links", () => {
  it("go through transcriptRoute: no page outside brain hand-builds a brain/transcripts route", () => {
    const pages = join(process.cwd(), "src/pages");
    const hits = ["onair", "reconcile", "simulator", "characters"]
      .flatMap((p) => sources(join(pages, p)))
      .filter((f) => /pageRoute\("brain", "transcripts"/.test(readFileSync(f, "utf8")));
    expect(hits).toEqual([]);
  });
});
