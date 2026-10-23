// The size guard: every tool, run against the realistic daemon at its largest inputs, answers
// in at most RESULT_BUDGET chars of JSON (Athena cuts a result at 1,600), and the bind guard
// trims a list or refuses rather than let her cut land mid-JSON.
import { describe, expect, it } from "vitest";
import { pageRoute } from "../../router";
import { buildTools, SPECS } from "./catalog";
import { MAP_MAX, MAP_TOPICS } from "./appMap";
import { fakes } from "./testkit";
import { BIG_RUN, BIG_SESSION, LINES, worldScript } from "./testkit.world";
import { bind, fitBody, RESULT_BUDGET, type ToolSpec } from "./tool";
import { QUERY_ALLOWLIST } from "./toolsOrient";

type Input = Record<string, unknown>;
const DIGEST = `sha256:${"c3a1".repeat(16)}`;

/** Every tool with the calls that make its answer largest. */
const CASES: Readonly<Record<string, readonly Input[]>> = {
  where_am_i: [{}],
  app_map: [{}, ...MAP_TOPICS.map((topic) => ({ topic }))],
  go_to: [{ view: "#/characters/malakor/package/bible" }, { view: `#/review/${BIG_SESSION}/timeline/95` }],
  list_sessions: [{}, { limit: 50 }],
  get_session: [{ session: BIG_SESSION }],
  open_moment: [{ session: BIG_SESSION, line: 2 }, { session: BIG_SESSION, line: 95 }, { session: BIG_SESSION, line: LINES }],
  list_runs: [{}, { limit: 100 }, { limit: 100, tournament: "pilot-d1ef98d3a6" }],
  get_transcript: [{ run: BIG_RUN }, { run: BIG_RUN, from: 40, limit: 100 }],
  get_verdict: [{ run: BIG_RUN }],
  list_tournaments: [{}, { limit: 50 }],
  get_tournament: [{ tournament: "pilot-d1ef98d3a6" }],
  ranks_table: [{}, { character: "malakor" }],
  ledger_summary: [{}, { tournament: "pilot-d1ef98d3a6" }],
  query: QUERY_ALLOWLIST.map((method) => ({ method, params: method === "ledger.page" ? { limit: 500 } : {} })),
  list_characters: [{}],
  get_character: [{ id: "malakor" }],
  read_character_doc: [{ id: "malakor", doc: "bible" }, { id: "malakor", doc: "bible", offset: 6000 }, { id: "malakor", doc: "constitution" }, { id: "malakor", doc: "situations" }],
  validate_character: [{ id: "malakor" }],
  set_character_doc: [{ id: "malakor", doc: "foils/the-auditor", text: "# The Auditor", base_digest: "" }],
  edit_character_doc: [{ id: "malakor", doc: "bible", base_digest: DIGEST, find: "## Running jokes", replace: "## Running jokes, kept" }],
  set_character_rules: [{ character: "malakor", id: "no-financial-promotion", enabled: false }],
  set_tone: [{ id: "malakor", tone: "theatrical" }],
  add_tone_line: [{ character: "malakor", tone: "theatrical", text: "x".repeat(1200), tags: ["roast_request"] }],
  set_tone_line_status: [{ character: "malakor", tone: "theatrical", id: "l-0042", status: "retired" }],
  leave_feedback: [{ session_id: BIG_SESSION, line: 95, tags: ["flat"], note: "n".repeat(2000) }],
  start_sandbox: [{ minutes: 30, seed: 7, tone: "theatrical" }],
  stop_sandbox: [{}],
  start_sim: [{}, { character: "malakor", tone: "theatrical", segment: "SEG-02A", audience_size: 300, seed: 99, judge: true, tier: "opus" }],
};

/** Methods with no realistic fixture: the daemon answers method_not_found, which is a refusal. */
const UNSCRIPTED = new Set(["plan.current", "plan.proposals", "directive.list", "reconcile.list", "reconcile.incidents", "segments.directions.get", "tone.directions.get", "tone.lines.list", "tone.inbox", "tone.variants", "prep.list", "prep.get", "clips.list", "clips.get", "review.feedback.list", "runs.list", "moderator.status", "moderator.rules.list", "moderator.log", "voice.settings.get"]);

describe("the size guard", () => {
  it("covers every tool in the catalog", () => {
    expect(Object.keys(CASES).sort()).toEqual(SPECS.map((s) => s.name).sort());
  });

  const largest: Record<string, number> = {};
  it.each(SPECS.map((s) => [s.name] as const))("%s answers within the result budget at realistic sizes", async (name) => {
    for (const input of CASES[name] ?? []) {
      const f = fakes(worldScript(), pageRoute("characters", "malakor", "package", "bible"));
      const tool = buildTools(f.deps).find((t) => t.name === name);
      const r = await tool?.execute(input).then(
        (o) => ({ ok: true as const, text: o.content[0]?.text ?? "" }),
        (e: unknown) => ({ ok: false as const, error: String(e) }),
      );
      if (r === undefined) throw new Error(`no tool ${name}`);
      if (!r.ok) {
        // Only a method the realistic daemon does not script may fail, and never for size.
        expect(name === "query" && UNSCRIPTED.has(String(input["method"])), `${name} ${JSON.stringify(input).slice(0, 80)}: ${r.error}`).toBe(true);
        continue;
      }
      expect(r.text.length, `${name} ${JSON.stringify(input).slice(0, 80)}`).toBeLessThanOrEqual(RESULT_BUDGET);
      largest[name] = Math.max(largest[name] ?? 0, r.text.length);
    }
  });

  it("keeps every app_map answer at or under MAP_MAX", async () => {
    const tool = buildTools(fakes({}).deps).find((t) => t.name === "app_map");
    for (const input of CASES["app_map"] ?? []) {
      const text = (await tool?.execute(input))?.content[0]?.text ?? "";
      expect(text.length, JSON.stringify(input)).toBeLessThanOrEqual(MAP_MAX);
    }
  });

  it("reports the largest answer per tool", () => {
    // Printed for the report; the assertions above are the gate.
    console.log(`largest answers: ${JSON.stringify(largest)}`);
    expect(Object.keys(largest).length).toBeGreaterThan(20);
  });
});

describe("the bind guard", () => {
  const spec = (body: Record<string, unknown>): ToolSpec => ({ name: "probe", title: "Probe", description: "A probe.", cls: "READ", input: {}, run: async () => body });
  const text = async (body: Record<string, unknown>) => (await bind(spec(body), fakes({}).deps).execute({})).content[0]?.text ?? "";

  it("trims a list's items to fit and says so in its footer", async () => {
    const items = Array.from({ length: 50 }, (_, i) => ({ i, pad: "p".repeat(80) }));
    const t = await text({ items, showing: 50, total: 200, footer: "showing 50 of 200" });
    expect(t.length).toBeLessThanOrEqual(RESULT_BUDGET);
    expect(t).toMatch(/"footer":"showing \d+ of 200; trimmed from 50 to fit the result budget/);
  });

  it("trims a nested list and drops the cursor that would skip what it trimmed", () => {
    const items = Array.from({ length: 50 }, (_, i) => ({ i, pad: "p".repeat(80) }));
    const body = fitBody({ method: "ledger.page", result: { items, next_cursor: "50", showing: 50, total: 50, footer: "showing 50 of 50" } });
    const result = body?.["result"] as Record<string, unknown>;
    expect(result).not.toHaveProperty("next_cursor");
    expect(String(result["footer"])).toMatch(/ask again with limit \d+ to page by cursor/);
  });

  it("refuses a body with no list rather than let the cut land mid-JSON", async () => {
    await expect(text({ blob: "b".repeat(3000) })).rejects.toThrow(/^too_large: probe's answer is \d+ chars, over the 1500-char result budget/);
  });

  it("never reports a write that ran as failed: an over-long write answer says it was done", async () => {
    const write = bind({ ...spec({ blob: "b".repeat(3000) }), cls: "GATED" }, fakes({}).deps);
    const t = (await write.execute({})).content[0]?.text ?? "";
    expect(t).toMatch(/^\{"done":true,"footer":"probe's answer is \d+ chars, over the 1500-char result budget; read the result back/);
  });
});
