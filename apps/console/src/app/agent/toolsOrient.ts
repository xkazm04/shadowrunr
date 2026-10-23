// Orientation and navigation, the read allowlist, and the one AUTO run: where_am_i, app_map,
// go_to, query and start_sim.
import type { RpcMethod, SimStartParams } from "@shadowrunr/contract";
import { isPageId, pageRoute, WALL } from "../../router";
import { MAP_TOPICS, mapAnswer } from "./appMap";
import { buildSpec, coreRoute, MAX_AUDIENCE, SIM_DEFAULTS, SIM_SEGMENTS, SIM_TIERS } from "./sim";
import { argInt, argStr, bounded, fail, int, obj, optStr, RESULT_BUDGET, sizeOf, str, type ToolSpec } from "./tool";
import { checkRoute, describe, toolsHere } from "./where";

/** The read methods `query` may send (design brief, Data & API). Nothing else is ever sent. */
export const QUERY_ALLOWLIST: readonly RpcMethod[] = [
  "plan.current",
  "plan.proposals",
  "directive.list",
  "reconcile.list",
  "reconcile.report",
  "reconcile.incidents",
  "segments.tables",
  "segments.directions.get",
  "tone.directions.get",
  "tone.lines.list",
  "tone.inbox",
  "tone.variants",
  "prep.list",
  "prep.get",
  "clips.list",
  "clips.get",
  "sandbox.status",
  "engine.status",
  "ledger.page",
  "review.feedback.list",
  "runs.list",
  "tournament.list",
  "character.rules",
  "moderator.status",
  "moderator.rules.list",
  "moderator.log",
  "voice.settings.get",
];

const isAllowed = (m: string): m is RpcMethod => (QUERY_ALLOWLIST as readonly string[]).includes(m);

const QUERY_ITEMS = 20;

const isListy = (value: unknown): boolean => Array.isArray(value) || (value !== null && typeof value === "object" && "items" in value && Array.isArray(value.items));

function boundResult(value: unknown): Record<string, unknown> {
  if (Array.isArray(value)) return { result: bounded(value, QUERY_ITEMS) };
  if (value !== null && typeof value === "object" && "items" in value && Array.isArray(value.items)) {
    return { result: { ...value, ...bounded(value.items, QUERY_ITEMS) } };
  }
  return { result: value };
}

export const ORIENT_TOOLS: readonly ToolSpec[] = [
  {
    name: "where_am_i",
    title: "Where am I",
    description:
      "Returns the console's current view: hash, page, layer, title, breadcrumbs, the ids in view (character, session, line, tab, run, tournament) and tools_here, the tools that fit it. Call first in a turn.",
    cls: "READ",
    input: obj({}),
    run: async (_input, ctx) => ({ ...describe(ctx.deps.router.getRoute()) }),
  },
  {
    name: "app_map",
    title: "Console map",
    description:
      "With no topic: one line per page (purpose, example hash) and the topics. Call app_map{topic} for detail: a page id for its route grammar and tools, glossary (Malakor, foils, tones, sandbox vs sim...) or recipes.",
    cls: "READ",
    input: obj({ topic: str("glossary, recipes, or a page id; omit for the index.", { enum: MAP_TOPICS }) }),
    run: async (input) => {
      const topic = optStr(input, "topic");
      const answer = mapAnswer(topic);
      if (answer === undefined) fail("invalid_params", `topic "${topic ?? ""}" is not one of ${MAP_TOPICS.join(", ")}`);
      if (topic === undefined || topic === "glossary" || topic === "recipes") return answer;
      return { ...answer, tools_here: toolsHere(topic === "wall" || !isPageId(topic) ? WALL : pageRoute(topic)) };
    },
  },
  {
    name: "go_to",
    title: "Go to a view",
    description:
      "Moves the console to a hash such as #/review/<session>/timeline/12 (grammar in app_map). Refuses an invalid hash with its reason. Returns the new place like where_am_i.",
    cls: "AUTO",
    input: obj({ view: str("The hash, e.g. #/characters/malakor/package/bible.", { maxLength: 512 }) }, ["view"]),
    run: async (input, ctx) => {
      const d = checkRoute(argStr(input, "view"));
      if (!d.ok) fail("invalid_params", `not a console view: ${d.reason}`);
      ctx.deps.router.navigate(d.route);
      return { ...describe(d.route) };
    },
  },
  {
    name: "query",
    title: "Read any allowlisted RPC",
    description:
      "Calls one read-only daemon RPC from the allowlist (plan, directives, reconcile, segments, tone, prep, clips, sandbox.status, engine.status, ledger.page, feedback, runs, moderator reads). Returns its result, lists bounded.",
    cls: "READ",
    input: obj(
      {
        method: str("An allowlisted read method, e.g. tone.lines.list.", { enum: QUERY_ALLOWLIST }),
        params: { type: "object", description: "The method's params (see docs/spec/contract.md); {} when omitted." },
      },
      ["method"],
    ),
    run: async (input, ctx) => {
      const method = argStr(input, "method");
      if (!isAllowed(method)) fail("refused", `${method} is not in the read allowlist; writes go only through the named tools`);
      const params = input["params"] ?? {};
      if (params === null || typeof params !== "object" || Array.isArray(params)) fail("invalid_params", "params must be an object");
      // The allowlist decides the method; the daemon's params check decides the params.
      const value: unknown = await ctx.read(method, params);
      const body = { method, ...boundResult(value) };
      // A bounded list is trimmed by the bind guard. Any other answer over the budget comes back
      // as the head of its JSON text inside a string, so the cut never lands mid-JSON.
      if (sizeOf(body) <= RESULT_BUDGET || isListy(value)) return body;
      const text = JSON.stringify(value);
      const footer = `cut to fit the ${RESULT_BUDGET}-char result budget, of ${text.length} chars; narrow the params for the rest`;
      const room = RESULT_BUDGET - sizeOf({ method, truncated: true, text: "", footer });
      let n = room;
      while (n > 0 && sizeOf(text.slice(0, n)) - 2 > room) n -= 20;
      return { method, truncated: true, text: text.slice(0, Math.max(0, n)), footer };
    },
  },
  {
    name: "start_sim",
    title: "Start a scripted text sim",
    description:
      "Starts one text sim on a scripted daemon (else refused) and shows #/simulator. Every arg optional: malakor, his active tone, SEG-01, audience 30, seed 1, tier from engine.status, 600 s. Returns run_id and spec_used.",
    cls: "AUTO",
    input: obj({
      character: str(`A character id (default ${SIM_DEFAULTS.character}).`, { maxLength: 64 }),
      tone: str("A tone variant of the character (default its active tone).", { maxLength: 64 }),
      segment: str(`Default ${SIM_DEFAULTS.segment}.`, { enum: SIM_SEGMENTS }),
      audience_size: int(`Simulated viewers, 3, 30 and 300 are calibrated (default ${SIM_DEFAULTS.audience}).`, 1, MAX_AUDIENCE),
      seed: int(`The room's seed (default ${SIM_DEFAULTS.seed}).`, 0, 1_000_000),
      tier: str("The core tier (default engine.status's core_thread route).", { enum: SIM_TIERS }),
      judge: { type: "boolean", description: "Judge the run when it ends (default false)." },
    }),
    run: async (input, ctx) => {
      const status = await ctx.read("engine.status", {});
      if (status.fleet.mode !== "scripted") {
        fail("refused", `the daemon's engine fleet is ${status.fleet.mode}, not scripted; live sims are out of scope for Athena (run the daemon with --scripted)`);
      }
      const character = optStr(input, "character") ?? SIM_DEFAULTS.character;
      const c = await ctx.read("character.get", { id: character });
      const tones = c.tones.map((t) => t.id);
      const tone = optStr(input, "tone") ?? c.active_tone;
      if (!tones.includes(tone)) fail("invalid_params", `tone "${tone}" is not one of ${character}'s tones: ${tones.join(", ")}`);
      const segment = SIM_SEGMENTS.find((x) => x === (input["segment"] ?? SIM_DEFAULTS.segment));
      if (segment === undefined) fail("invalid_params", `segment must be one of ${SIM_SEGMENTS.join(", ")}`);
      const route = coreRoute(status);
      const tier = SIM_TIERS.find((x) => x === (input["tier"] ?? route.tier));
      if (tier === undefined) fail("invalid_params", `tier must be one of ${SIM_TIERS.join(", ")}`);
      const spec = buildSpec({
        character,
        tone,
        tier,
        effort: route.effort,
        segment,
        audience: argInt(input, "audience_size", SIM_DEFAULTS.audience, 1, MAX_AUDIENCE),
        seed: argInt(input, "seed", SIM_DEFAULTS.seed, 0, 1_000_000),
      });
      const judge = input["judge"] === true;
      const params: SimStartParams = { spec, judge };
      const started = await ctx.write("sim.start", params);
      ctx.deps.router.navigate(pageRoute("simulator"));
      return { run_id: started.run_id, spec_used: spec, judge, view: "#/simulator", next: "list_runs, then get_verdict{run} or get_transcript{run} once it ends" };
    },
  },
];
