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
  query: "READ",
  list_characters: "READ",
  get_character: "READ",
  read_character_doc: "READ",
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

  it("is 25 tools, edit_character_doc among them", () => {
    expect(tools).toHaveLength(25);
  });

  it("answers app_map as an index of every page and topic, each at or under MAP_MAX", () => {
    const index = mapAnswer(undefined);
    expect(Object.keys(index?.["pages"] as object)).toEqual(["wall", ...PAGE_IDS]);
    expect(index?.["topics"]).toEqual(MAP_TOPICS);
    for (const t of [undefined, ...MAP_TOPICS]) expect(JSON.stringify(mapAnswer(t)).length, String(t)).toBeLessThanOrEqual(MAP_MAX);
    expect(mapAnswer("nowhere")).toBeUndefined();
  });
});

// ---- the capability block's footprint ----
// Athena lists every host tool as one line of her prompt's capability block, which holds at most
// 12,000 chars; a page entry that would pass the bound is cut, and every one after it
// (athena-portable src/athena/core/catalog.py render_capabilities, _capability_line,
// _params_line(foreign=True), _foreign_enum, neutralise_text). This renders each line the way
// she does.

const PLAIN = /^[A-Za-z0-9_-]{1,40}$/;
/** neutralise_text: whitespace runs to one space, trimmed, cut to `cap` with an ellipsis. */
const neutralise = (text: string, cap: number): string => {
  const kept = text.replace(/\s+/g, " ").trim();
  return kept.length <= cap ? kept : `${kept.slice(0, cap - 1).trimEnd()}…`;
};
/** _quoted: a JSON string of the value's text, cut to PARAM_TEXT_CAP (40). */
const quoted = (v: unknown): string => JSON.stringify(neutralise(typeof v === "string" ? v : JSON.stringify(v), 40));
/** _foreign_enum: the first ENUM_SHOWN (12) values, then a count of the rest. */
const foreignEnum = (values: readonly unknown[]): string => {
  const shown = values.slice(0, 12).map(quoted).join(", ");
  return values.length > 12 ? `[${shown}, +${values.length - 12} more]` : `[${shown}]`;
};
/** _params_line(foreign=True): `name?: type`, or an enum in place of the type. */
const paramsLine = (schema: Record<string, unknown>): string => {
  const props = (schema["properties"] ?? {}) as Record<string, Record<string, unknown>>;
  const required = new Set((schema["required"] ?? []) as string[]);
  return Object.entries(props)
    .map(([name, sub]) => {
      const kind = sub["type"] ?? "any";
      const kindText = typeof kind === "string" && PLAIN.test(kind) ? kind : quoted(kind);
      const e = sub["enum"];
      const value = Array.isArray(e) && e.length > 0 ? foreignEnum(e) : kindText;
      return `${PLAIN.test(name) ? name : quoted(name)}${required.has(name) ? "" : "?"}: ${value}`;
    })
    .join(", ");
};
/** _capability_line for a host entry: AUTO and GATED carry no cap suffix (a host tool is never READ). */
export const capabilityLine = (name: string, description: string, schema: Record<string, unknown>): string =>
  `- \`host.shadowrunr.${name}\`(${paramsLine(schema)}) — ${JSON.stringify(neutralise(description, 240))}`;

/**
 * The most chars our tools' lines may take, newlines included (the block charges each line its
 * length plus one). 12,000 budget, minus the block's fixed header, class headings and footer
 * (~1,160), minus Athena's core entries (790), minus the shell's DOM hands and tabs_read
 * (~1,900), leaves ~8,150 for apps; 5,000 for this one keeps ~3,150 for core growth and a
 * second app. Measured 2026-10-23 with Athena's renderer (measure_caps.py) on the desktop shell.
 */
const FOOTPRINT_MAX = 5000;

describe("the capability block", () => {
  const lines = tools.map((t) => [t.name, capabilityLine(t.name, t.description, t.inputSchema)] as const);

  it("renders our tools in at most FOOTPRINT_MAX chars", () => {
    const total = lines.reduce((n, [, l]) => n + l.length + 1, 0);
    console.log(`capability footprint: ${total} of ${FOOTPRINT_MAX} chars over ${lines.length} tools`);
    expect(total).toBeLessThanOrEqual(FOOTPRINT_MAX);
  });

  it("never lets Athena's 240-char cut or the 12-value enum cut fire", () => {
    for (const t of tools) {
      expect(neutralise(t.description, 240), t.name).toBe(t.description);
      for (const [p, ps] of Object.entries((t.inputSchema["properties"] ?? {}) as Record<string, Record<string, unknown>>)) {
        const e = ps["enum"];
        if (Array.isArray(e)) expect(e.length, `${t.name}.${p}`).toBeLessThanOrEqual(12);
      }
    }
  });

  it("renders a line the way Athena's _capability_line does", () => {
    const schema = { type: "object", properties: { minutes: { type: "integer", enum: [5, 10, 30] }, tone: { type: "string" } }, required: ["minutes"] };
    expect(capabilityLine("x", "Says \"hi\".", schema)).toBe('- `host.shadowrunr.x`(minutes: ["5", "10", "30"], tone?: string) — "Says \\"hi\\"."');
    const many = { type: "object", properties: { m: { type: "string", enum: Array.from({ length: 14 }, (_, i) => `v${i}`) } }, required: [] };
    expect(capabilityLine("y", "d", many)).toContain('"v11", +2 more]');
  });
});
