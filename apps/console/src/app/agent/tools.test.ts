// The tools against a fake RpcClient and router: refusals that never send, follow-along
// navigation, orientation, bounded lists and the daemon's errors carried through.
import { describe, expect, it } from "vitest";
import type { ReviewSessionRow, RunRow, StreamEvent } from "@shadowrunr/contract";
import { bigSession, sessionRows } from "../../pages/review/fixture";
import { pkgRoute } from "../../pages/characters/routes";
import { encodeRoute, pageRoute, WALL } from "../../router";
import { transcriptRoute } from "../../router/brain";
import { buildTools } from "./catalog";
import { fakes, no, ok, run, type Script } from "./testkit";

function setup(script: Script, start = pageRoute("onair")) {
  const f = fakes(script, start);
  const tools = new Map(buildTools(f.deps).map((t) => [t.name, t]));
  return { ...f, tool: (name: string) => tools.get(name) };
}

const SIM_SPEC = {
  audience_size: 30,
  character: "malakor",
  core_effort: "low",
  core_tier: "haiku",
  duration_s: 120,
  population: "growing_channel",
  run_id: "theatrical.haiku.SEG-01.a30.s1",
  seed: 1,
  segment: "SEG-01",
  tone_variant: "theatrical",
};
const status = (mode: string) => ok({ accounts: [], api_fallback: false, fallbacks: [], fleet: { mode, since_ms: 0 }, routes: [] });

describe("query", () => {
  it("refuses a method outside the read allowlist by name and never sends it", async () => {
    const s = setup({ "character.doc.set": () => ok({}) });
    await expect(run(s.tool("query"), { method: "character.doc.set", params: {} })).rejects.toThrow(/^refused: character\.doc\.set is not in the read allowlist/);
    await expect(run(s.tool("query"), { method: "clips.publish" })).rejects.toThrow(/refused: clips\.publish/);
    expect(s.sent).toEqual([]);
  });

  it("sends an allowlisted read and bounds a list answer", async () => {
    const rows = Array.from({ length: 30 }, (_, i) => ({ id: `d${i}` }));
    const s = setup({ "directive.list": () => ok(rows) });
    const body = await run(s.tool("query"), { method: "directive.list" });
    expect(s.sent).toEqual(["directive.list"]);
    expect(body["result"]).toMatchObject({ showing: 20, total: 30, footer: expect.stringMatching(/^showing 20 of 30/) });
  });
});

describe("start_sim", () => {
  it("refuses unless the engine fleet is scripted, and never starts the run", async () => {
    for (const mode of ["primary", "degraded", "down"]) {
      const s = setup({ "engine.status": () => status(mode), "sim.start": () => ok({ run_id: "x" }) });
      await expect(run(s.tool("start_sim"), { spec: SIM_SPEC })).rejects.toThrow(/^refused: .*live sims are out of scope/);
      expect(s.sent).toEqual(["engine.status"]);
      expect(s.navs).toEqual([]);
    }
  });

  it("starts a scripted run through mutate and shows the simulator", async () => {
    const s = setup({ "engine.status": () => status("scripted"), "sim.start": () => ok({ run_id: SIM_SPEC.run_id }) });
    const body = await run(s.tool("start_sim"), { spec: SIM_SPEC, judge: true });
    expect(body).toMatchObject({ run_id: SIM_SPEC.run_id, view: "#/simulator" });
    expect(s.writes).toEqual([{ method: "sim.start", params: { spec: SIM_SPEC, judge: true } }]);
    expect(s.navs).toEqual(["#/simulator"]);
  });
});

describe("go_to", () => {
  it("refuses a hash the router does not decode, with its reason", async () => {
    const s = setup({});
    await expect(run(s.tool("go_to"), { view: "#/nowhere" })).rejects.toThrow(/^invalid_params: not a console view: unknown page "nowhere"/);
    await expect(run(s.tool("go_to"), { view: "#/wall/extra" })).rejects.toThrow(/the wall has no sub-pages/);
    await expect(run(s.tool("go_to"), { view: "#/brain/bogus" })).rejects.toThrow(/nearest is #\/brain/);
    expect(s.navs).toEqual([]);
  });

  it("navigates a valid hash and answers like where_am_i", async () => {
    const s = setup({});
    const body = await run(s.tool("go_to"), { view: "#/characters/malakor/package/bible" });
    expect(s.navs).toEqual(["#/characters/malakor/package/bible"]);
    expect(body).toMatchObject({ view: "#/characters/malakor/package/bible", page: "characters", layer: "l2", ids: { character: "malakor", tab: "bible" } });
  });
});

describe("where_am_i", () => {
  it("reads a character package tab", async () => {
    const s = setup({}, pkgRoute("malakor", "bible"));
    const body = await run(s.tool("where_am_i"));
    expect(body).toEqual({
      view: "#/characters/malakor/package/bible",
      page: "characters",
      layer: "l2",
      title: "Bible",
      breadcrumbs: ["Season", "Characters", "Malakor", "Package", "Bible"],
      ids: { character: "malakor", tab: "bible" },
      tools_here: ["get_character", "read_character_doc", "set_character_doc", "validate_character", "set_tone"],
    });
  });

  it("reads a Review timeline line", async () => {
    const s = setup({}, pageRoute("review", "sbx-1", "timeline", "12"));
    const body = await run(s.tool("where_am_i"));
    expect(body).toMatchObject({ page: "review", layer: "l2", ids: { session: "sbx-1", tab: "timeline", line: 12 }, tools_here: ["get_session", "open_moment", "leave_feedback", "list_sessions"] });
    expect(body["breadcrumbs"]).toEqual(["Season", "Session review", "sbx-1", "Timeline", "12"]);
  });

  it("reads a qualified Brain transcript line, and the wall", async () => {
    const s = setup({}, transcriptRoute({ tournamentId: "pilot-1", runId: "r1" }, 7));
    expect(await run(s.tool("where_am_i"))).toMatchObject({ view: "#/brain/transcripts/pilot-1%2Fr1/7", page: "brain", ids: { tab: "transcripts", run: "pilot-1/r1", line: 7 }, title: "Line 7" });
    const w = setup({}, WALL);
    expect(await run(w.tool("where_am_i"))).toMatchObject({ view: "#/wall", page: "wall", layer: "wall", title: "Season", ids: {} });
  });
});

describe("show", () => {
  const rows = (n: number): ReviewSessionRow[] => sessionRows(n);

  it("navigates to the fetched view and returns it", async () => {
    const s = setup({ "review.sessions": () => ok(rows(3)) });
    const body = await run(s.tool("list_sessions"));
    expect(s.navs).toEqual(["#/review"]);
    expect(body["view"]).toBe("#/review");
  });

  it("show:false fetches without moving the console", async () => {
    const s = setup({ "review.sessions": () => ok(rows(3)) });
    const body = await run(s.tool("list_sessions"), { show: false });
    expect(s.navs).toEqual([]);
    expect(body).not.toHaveProperty("view");
    expect(body["total"]).toBe(3);
  });

  it("does not navigate when the read fails, and carries the daemon's code", async () => {
    const s = setup({ "review.session": () => no("not_found", "no session sbx-9") });
    await expect(run(s.tool("get_session"), { session: "sbx-9" })).rejects.toThrow("not_found: no session sbx-9");
    expect(s.navs).toEqual([]);
  });

  it("open_moment shows the timeline line with three lines either side", async () => {
    const rs = bigSession({ events: 40, minutes: 5, id: "sbx-m" });
    const s = setup({ "review.session": () => ok(rs) });
    const body = await run(s.tool("open_moment"), { session: "sbx-m", line: 10 });
    expect(s.navs).toEqual(["#/review/sbx-m/timeline/10"]);
    expect((body["lines"] as { line: number }[]).map((l) => l.line)).toEqual([7, 8, 9, 10, 11, 12, 13]);
    await expect(run(s.tool("open_moment"), { session: "sbx-m", line: 41 })).rejects.toThrow(/past the end/);
  });

  it("read_character_doc shows the document's package tab and returns the digest", async () => {
    const s = setup({ "character.doc.get": () => ok({ id: "malakor", doc: "foils/the-inspector.md", text: "# The Inspector", digest: "sha256:ab" }) });
    const body = await run(s.tool("read_character_doc"), { id: "malakor", doc: "foils/the-inspector.md" });
    expect(body).toMatchObject({ digest: "sha256:ab", text: "# The Inspector", view: "#/characters/malakor/package/foils" });
    await expect(run(s.tool("read_character_doc"), { id: "malakor", doc: "../x" })).rejects.toThrow(/^invalid_params/);
    expect(s.sent).toEqual(["character.doc.get"]);
  });
});

describe("bounded lists", () => {
  it("list_sessions: newest first, at most limit, with showing, total and footer", async () => {
    const s = setup({ "review.sessions": () => ok([...sessionRows(15)].reverse()) });
    const body = await run(s.tool("list_sessions"), { limit: 10, show: false });
    expect(body).toMatchObject({ showing: 10, total: 15, footer: "showing 10 of 15; more follow (raise limit or page on)" });
    const items = body["items"] as { session_id: string }[];
    expect(items[0]?.session_id).toBe("sbx-014");
  });

  it("get_transcript: a window from a line, never the whole run", async () => {
    const events: StreamEvent[] = Array.from({ length: 50 }, (_, i) => ({ type: "say", ts_ms: i * 1000, speaker: "host", text: `line ${i + 1}` }));
    const s = setup({ "run.transcript": () => ok(events) });
    const body = await run(s.tool("get_transcript"), { run: "t1/r1", from: 11, limit: 5 });
    expect(body).toMatchObject({ showing: 5, total: 50, footer: expect.stringMatching(/^showing 5 of 50/) });
    expect((body["items"] as { line: number; text: string }[]).map((l) => [l.line, l.text])).toEqual([[11, "line 11"], [12, "line 12"], [13, "line 13"], [14, "line 14"], [15, "line 15"]]);
    expect(s.navs).toEqual([encodeRoute(transcriptRoute({ runId: "t1/r1" }, 11))]);
  });

  it("list_runs: the page with its total from the tournament rows", async () => {
    const spec = { audience_size: 30, character: "malakor", core_effort: "low", core_tier: "haiku", duration_s: 60, population: "p", run_id: "r", seed: 1, segment: "SEG-01", tone_variant: "theatrical" } as const;
    const items: RunRow[] = Array.from({ length: 3 }, (_, i) => ({ run_id: `r${i}`, tournament_id: "t1", spec: { ...spec, run_id: `r${i}` }, has_verdict: false }));
    const s = setup({
      "runs.page": () => ok({ items, next_cursor: "c3" }),
      "tournament.list": () => ok([{ tournament_id: "t1", instrument_hash: "h", runs: 40, completed: 40 }]),
    });
    const body = await run(s.tool("list_runs"), { limit: 3, show: false });
    expect(body).toMatchObject({ showing: 3, total: 40, next_cursor: "c3", footer: "showing 3 of 40; pass next_cursor as cursor for more" });
  });
});

describe("writes", () => {
  it("set_character_doc goes through mutate with the base digest and reports a conflict", async () => {
    const s = setup({ "character.doc.set": () => no("conflict", "bible.md changed since sha256:aa") });
    await expect(run(s.tool("set_character_doc"), { id: "malakor", doc: "bible.md", text: "new", base_digest: "sha256:aa" })).rejects.toThrow(/^conflict: bible\.md changed/);
    expect(s.writes).toEqual([{ method: "character.doc.set", params: { id: "malakor", doc: "bible.md", text: "new", base_digest: "sha256:aa" } }]);
  });

  it("stop_sandbox refuses when no session is running", async () => {
    const s = setup({ "sandbox.status": () => ok({ session_id: "sbx-1", state: "done", core_errors: 0, elapsed_s: 300, events: 10, max_say_gap_s: 3, planned_s: 300 }) });
    await expect(run(s.tool("stop_sandbox"))).rejects.toThrow(/^refused: no sandbox session is running/);
    expect(s.writes).toEqual([]);
  });
});
