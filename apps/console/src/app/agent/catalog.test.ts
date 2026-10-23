// The catalog as Athena sees it: exactly the design brief's tool names, each with its class
// flags, a description inside her 240-char cut, a WebMCP-legal name and a closed schema.
import { describe, expect, it } from "vitest";
import { buildTools, SPECS } from "./catalog";
import { MAP_MAX, MAP_TOPICS, mapAnswer } from "./appMap";
import { fakes } from "./testkit";
import { toolsHere } from "./where";
import { PAGE_IDS, pageRoute, WALL } from "../../router";
import type { ToolClass } from "./types";

/** The brief's catalog and the class of each tool (docs/guide/athena.md, the table). */
const CLASS: Readonly<Record<string, ToolClass>> = {
  where_am_i: "READ",
  app_map: "READ",
  go_to: "AUTO",
  list_sessions: "READ",
  get_session: "READ",
  open_moment: "READ",
  list_runs: "READ",
  get_transcript: "READ",
  get_verdict: "READ",
  list_tournaments: "READ",
  get_tournament: "READ",
  ranks_table: "READ",
  ledger_summary: "READ",
  query: "READ",
  list_characters: "READ",
  get_character: "READ",
  read_character_doc: "READ",
  validate_character: "READ",
  set_character_doc: "GATED",
  edit_character_doc: "GATED",
  set_character_rules: "GATED",
  set_tone: "GATED",
  add_tone_line: "GATED",
  set_tone_line_status: "GATED",
  leave_feedback: "GATED",
  start_sandbox: "GATED",
  stop_sandbox: "GATED",
  start_sim: "AUTO",
};

const FLAGS = {
  READ: { annotations: { readOnlyHint: true, consequentialHint: false }, athena: { reversible: true, side_effects: "none" } },
  AUTO: { annotations: { readOnlyHint: false, consequentialHint: false }, athena: { reversible: true, side_effects: "internal" } },
  GATED: { annotations: { readOnlyHint: false, consequentialHint: true }, athena: { reversible: false, side_effects: "internal" } },
} as const;

const tools = buildTools(fakes({}).deps);

describe("the catalog", () => {
  it("is exactly the brief's tool list", () => {
    expect(tools.map((t) => t.name).sort()).toEqual(Object.keys(CLASS).sort());
    expect(new Set(tools.map((t) => t.name)).size).toBe(tools.length);
    expect(tools.length).toBeLessThanOrEqual(96);
  });

  it.each(tools.map((t) => [t.name, t] as const))("%s carries its class flags, both hints explicit", (name, tool) => {
    const cls = CLASS[name];
    expect(cls).toBeDefined();
    const want = FLAGS[cls ?? "GATED"];
    expect(tool.annotations).toEqual(want.annotations);
    expect(tool.athena).toEqual(want.athena);
    expect(SPECS.find((s) => s.name === name)?.cls).toBe(cls);
  });

  it.each(tools.map((t) => [t.name, t] as const))("%s has a legal name, a short description and a closed schema", (name, tool) => {
    expect(name).toMatch(/^[A-Za-z0-9_-]{1,64}$/);
    expect(tool.title.length).toBeGreaterThan(0);
    expect(tool.description.length).toBeGreaterThan(40);
    expect(tool.description.length).toBeLessThanOrEqual(240);
    const s = tool.inputSchema;
    expect(s["type"]).toBe("object");
    expect(s["additionalProperties"]).toBe(false);
    expect(Array.isArray(s["required"])).toBe(true);
    const props = Object.keys((s["properties"] ?? {}) as Record<string, unknown>);
    expect(props.length).toBeLessThanOrEqual(24);
    for (const p of props) expect(p.length).toBeLessThanOrEqual(40);
    for (const r of s["required"] as string[]) expect(props).toContain(r);
    // Athena refuses a whole manifest when any array parameter has no maxItems
    // (athena-portable src/athena/contracts/manifest.py, HostManifest.validate).
    for (const [p, ps] of Object.entries((s["properties"] ?? {}) as Record<string, Record<string, unknown>>)) {
      if (ps["type"] === "array") expect(typeof ps["maxItems"], `${name}.${p}`).toBe("number");
    }
  });

  it("offers show on every READ tool that has a view, and on no write", () => {
    const viewless = new Set(["where_am_i", "app_map", "query"]);
    for (const t of tools) {
      const props = (t.inputSchema["properties"] ?? {}) as Record<string, unknown>;
      expect("show" in props, t.name).toBe(CLASS[t.name] === "READ" && !viewless.has(t.name));
    }
  });

  it("names only catalog tools in tools_here, 3 to 6 per place", () => {
    const routes = [WALL, ...PAGE_IDS.flatMap((p) => [pageRoute(p), pageRoute(p, "x")])];
    for (const r of routes) {
      const here = toolsHere(r);
      expect(here.length).toBeGreaterThanOrEqual(3);
      expect(here.length).toBeLessThanOrEqual(6);
      for (const n of here) expect(CLASS[n], n).toBeDefined();
    }
  });

  it("is 28 tools, edit_character_doc among them", () => {
    expect(tools).toHaveLength(28);
  });

  it("answers app_map as an index of every page and topic, each at or under MAP_MAX", () => {
    const index = mapAnswer(undefined);
    expect(Object.keys(index?.["pages"] as object)).toEqual(["wall", ...PAGE_IDS]);
    expect(index?.["topics"]).toEqual(MAP_TOPICS);
    for (const t of [undefined, ...MAP_TOPICS]) expect(JSON.stringify(mapAnswer(t)).length, String(t)).toBeLessThanOrEqual(MAP_MAX);
    expect(mapAnswer("nowhere")).toBeUndefined();
  });
});
