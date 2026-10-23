// Orientation and navigation, the read allowlist, and the one AUTO run: where_am_i, app_map,
// go_to, query and start_sim.
import type { RpcMethod, SimStartParams } from "@shadowrunr/contract";
import { pageRoute } from "../../router";
import { APP_MAP } from "./appMap";
import { argStr, bounded, fail, obj, str, type ToolSpec } from "./tool";
import { checkRoute, describe } from "./where";

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

/** The largest query answer returned whole; longer text is cut and says so. */
const QUERY_MAX_CHARS = 16_000;
const QUERY_ITEMS = 20;

function boundResult(value: unknown): Record<string, unknown> {
  if (Array.isArray(value)) return { result: bounded(value, QUERY_ITEMS) };
  if (value !== null && typeof value === "object" && "items" in value && Array.isArray(value.items)) {
    return { result: { ...value, ...bounded(value.items, QUERY_ITEMS) } };
  }
  return { result: value };
}

const SEGMENTS = ["SEG-01", "SEG-02A", "SEG-02B-I", "SEG-02B-II"];

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
      "Returns every page with its purpose, route grammar and an example hash, a glossary (Malakor, foils, tones, segments, sandbox vs sim vs tournament, judge, Tone Lab, constitution vs bible) and how-to recipes.",
    cls: "READ",
    input: obj({}),
    run: async () => ({ ...APP_MAP }),
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
      const text = JSON.stringify(body);
      return text.length <= QUERY_MAX_CHARS ? body : { method, truncated: true, text: text.slice(0, QUERY_MAX_CHARS), footer: `cut at ${QUERY_MAX_CHARS} of ${text.length} chars` };
    },
  },
  {
    name: "start_sim",
    title: "Start a scripted text sim",
    description:
      "Starts one text sim run (SimRunSpec) on a scripted daemon and shows #/simulator. Refuses unless engine.status says the fleet is scripted: live sims are out of scope. Returns run_id.",
    cls: "AUTO",
    input: obj(
      {
        spec: {
          type: "object",
          description: "SimRunSpec; run_id is <tone>.<tier>.<segment>.a<audience>.s<seed> by convention.",
          properties: {
            audience_size: { type: "integer", minimum: 1 },
            character: { type: "string" },
            core_effort: { type: "string", enum: ["low", "medium", "high", "xhigh", "max"] },
            core_tier: { type: "string", enum: ["haiku", "sonnet", "opus"] },
            duration_s: { type: "integer", minimum: 1 },
            population: { type: "string" },
            run_id: { type: "string" },
            seed: { type: "integer", minimum: 0 },
            segment: { type: "string", enum: SEGMENTS },
            tone_variant: { type: "string" },
          },
          required: ["audience_size", "character", "core_effort", "core_tier", "duration_s", "population", "run_id", "seed", "segment", "tone_variant"],
          additionalProperties: false,
        },
        judge: { type: "boolean", description: "Judge the run when it ends (default false)." },
      },
      ["spec"],
    ),
    run: async (input, ctx) => {
      const spec = input["spec"];
      if (spec === null || typeof spec !== "object" || Array.isArray(spec)) fail("invalid_params", "spec must be a SimRunSpec object");
      const status = await ctx.read("engine.status", {});
      if (status.fleet.mode !== "scripted") {
        fail("refused", `the daemon's engine fleet is ${status.fleet.mode}, not scripted; live sims are out of scope for Athena (run the daemon with --scripted)`);
      }
      const params = (typeof input["judge"] === "boolean" ? { spec, judge: input["judge"] } : { spec }) as SimStartParams;
      const started = await ctx.write("sim.start", params);
      ctx.deps.router.navigate(pageRoute("simulator"));
      return { run_id: started.run_id, view: "#/simulator", next: "list_runs, then get_transcript{run} once it ends" };
    },
  },
];
