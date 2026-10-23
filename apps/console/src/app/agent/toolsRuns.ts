// Reviews and runs: sandbox sessions in Review, sim runs and tournaments in the Simulator,
// transcripts and the engine ledger in Brain, the rank ladder in Segments.
import type { RunParams, RunsPageParams } from "@shadowrunr/contract";
import { pageRoute } from "../../router";
import { transcriptRoute } from "../../router/brain";
import { clip, eventLine, sessionSummary, tournamentSummary, verdictSummary } from "./summary";
import { argInt, argStr, bounded, fail, int, limit, obj, optStr, SHOW, str, type Ctx, type ToolSpec } from "./tool";

const SESSION = str("A sandbox session id (list_sessions).");
const RUN = str("A run id, or <tournament>/<run_id> when the id repeats across tournaments.");

/** `t/r` is a qualified run: the daemon takes it as tournament_id plus run_id. */
function runParams(run: string): RunParams {
  const slash = run.indexOf("/");
  return slash > 0 ? { tournament_id: run.slice(0, slash), run_id: run.slice(slash + 1) } : { run_id: run };
}

async function session(ctx: Ctx, id: string) {
  return ctx.read("review.session", { session_id: id });
}

const withView = (body: Record<string, unknown>, view: string | undefined) => (view === undefined ? body : { ...body, view });

export const RUN_TOOLS: readonly ToolSpec[] = [
  {
    name: "list_sessions",
    title: "List sandbox sessions",
    description:
      "Returns finished sandbox sessions, newest first: id, start, minutes, lines, marked, pass, rule fails. Bounded (showing/total/footer). Shows #/review. Then get_session{session}.",
    cls: "READ",
    input: obj({ limit: limit(10, 50), show: SHOW }),
    run: async (input, ctx) => {
      const rows = [...(await ctx.read("review.sessions", {}))].sort((a, b) => b.started_ms - a.started_ms);
      const page = bounded(
        rows.map((r) => ({
          session_id: r.session_id,
          started: new Date(r.started_ms).toISOString(),
          minutes: r.minutes,
          lines: r.lines,
          marked: r.marked,
          pass: r.pass,
          ...(r.rule_fails === undefined ? {} : { rule_fails: r.rule_fails }),
          tagged: r.tagged,
          ...(r.chaos === undefined ? {} : { chaos: r.chaos }),
        })),
        argInt(input, "limit", 10, 1, 50),
      );
      return withView({ ...page }, ctx.show(input, pageRoute("review")));
    },
  },
  {
    name: "get_session",
    title: "Summarise a session",
    description:
      "Returns one sandbox session summarised: gate checks, verdict (failed rules with lines), mix, segments and up to 10 notable lines (failures, marks). Shows #/review/<session>. Then open_moment.",
    cls: "READ",
    input: obj({ session: SESSION, show: SHOW }, ["session"]),
    run: async (input, ctx) => {
      const id = argStr(input, "session");
      const rs = await session(ctx, id);
      return withView(sessionSummary(rs), ctx.show(input, pageRoute("review", id)));
    },
  },
  {
    name: "open_moment",
    title: "Open a moment",
    description:
      "Returns one transcript line of a session with 3 lines either side, its tag, failing rules and the operator's marks. Shows #/review/<session>/timeline/<line>. Then leave_feedback.",
    cls: "READ",
    input: obj({ session: SESSION, line: int("The 1-based transcript line.", 1), show: SHOW }, ["session", "line"]),
    run: async (input, ctx) => {
      const id = argStr(input, "session");
      const rs = await session(ctx, id);
      const line = argInt(input, "line", 0, 1, Number.MAX_SAFE_INTEGER);
      if (line < 1) fail("invalid_params", `"line" is required`);
      if (line > rs.events.length) fail("invalid_params", `line ${line} is past the end (${rs.events.length} lines)`);
      const from = Math.max(1, line - 3);
      const around = rs.events.slice(from - 1, line + 3).map((e, i) => eventLine(e, from + i));
      const tag = rs.tags?.lines.find((t) => t.line === line);
      const fails = (rs.verdict?.rules ?? []).filter((r) => r.state === "fail" && (r.evidence ?? []).some((ref) => ref.endsWith(`#${line}`))).map((r) => r.rule);
      const marks = rs.feedback.filter((f) => f.line === line).map((f) => ({ tags: f.tags, ...(f.note === undefined ? {} : { note: clip(f.note, 300) }) }));
      return withView(
        { session_id: id, line, lines: around, ...(tag === undefined ? {} : { tag: { kind: tag.kind, target: tag.target, edge: tag.edge } }), fails, marks },
        ctx.show(input, pageRoute("review", id, "timeline", String(line))),
      );
    },
  },
  {
    name: "list_runs",
    title: "List sim runs",
    description:
      "Returns sim runs ordered by tournament and run id: spec (character, tone, tier, segment, audience, seed), verdict, red-line fails. Bounded; pass next_cursor as cursor. Shows #/simulator/runs.",
    cls: "READ",
    input: obj({ limit: limit(15, 100), tournament: str("Only this tournament's runs."), cursor: str("next_cursor of the previous page."), show: SHOW }),
    run: async (input, ctx) => {
      const n = argInt(input, "limit", 15, 1, 100);
      const tournament = optStr(input, "tournament");
      const cursor = optStr(input, "cursor");
      const params: RunsPageParams = { limit: n, ...(tournament === undefined ? {} : { tournament_id: tournament }), ...(cursor === undefined ? {} : { cursor }) };
      const page = await ctx.read("runs.page", params);
      const items = page.items.map((r) => ({
        run_id: r.run_id,
        tournament_id: r.tournament_id,
        character: r.spec.character,
        tone: r.spec.tone_variant,
        tier: r.spec.core_tier,
        segment: r.spec.segment,
        audience: r.spec.audience_size,
        seed: r.spec.seed,
        has_verdict: r.has_verdict,
        ...(r.red_line_fails === undefined ? {} : { red_line_fails: r.red_line_fails }),
        ...(r.health === undefined ? {} : { health: r.health }),
      }));
      let total = items.length;
      if (page.next_cursor !== undefined || cursor !== undefined) {
        // runs.page carries no count: the tournament rows do (`runs` per directory).
        const rows = await ctx.read("tournament.list", {});
        total = rows.filter((t) => tournament === undefined || t.tournament_id === tournament).reduce((s, t) => s + t.runs, 0);
      }
      const footer = `showing ${items.length} of ${total}${page.next_cursor === undefined ? "" : "; pass next_cursor as cursor for more"}`;
      return withView(
        { items, showing: items.length, total, footer, ...(page.next_cursor === undefined ? {} : { next_cursor: page.next_cursor }) },
        ctx.show(input, pageRoute("simulator", "runs")),
      );
    },
  },
  {
    name: "get_transcript",
    title: "Read a run transcript",
    description:
      "Returns a window of a run's transcript (default 20 lines from line 1): speaker, text, chat, segment and moderation lines, with total lines. Shows #/brain/transcripts/<run>/<from>.",
    cls: "READ",
    input: obj({ run: RUN, from: int("First line, 1-based (default 1).", 1), limit: limit(20, 100), show: SHOW }, ["run"]),
    run: async (input, ctx) => {
      const run = argStr(input, "run");
      const events = await ctx.read("run.transcript", runParams(run));
      const from = argInt(input, "from", 1, 1, Number.MAX_SAFE_INTEGER);
      const page = bounded(events, argInt(input, "limit", 20, 1, 100), from - 1);
      const lines = page.items.map((e, i) => eventLine(e, from + i));
      return withView({ run, ...page, items: lines }, ctx.show(input, transcriptRoute({ runId: run }, from > 1 ? from : undefined)));
    },
  },
  {
    name: "get_verdict",
    title: "Read a run verdict",
    description: "Returns the judge's verdict on a run: rule states, failed rules with the lines they cite, quality facts as true-of-n, notes. Shows #/simulator/verdicts/<run>. Then get_transcript.",
    cls: "READ",
    input: obj({ run: RUN, show: SHOW }, ["run"]),
    run: async (input, ctx) => {
      const run = argStr(input, "run");
      const v = await ctx.read("run.verdict", runParams(run));
      return withView(verdictSummary(v), ctx.show(input, pageRoute("simulator", "verdicts", run)));
    },
  },
  {
    name: "list_tournaments",
    title: "List tournaments",
    description: "Returns tournaments, most recently updated first: id, character, runs done of planned, judged, held share, red-line fails. Bounded. Shows #/simulator/tournaments. Then get_tournament.",
    cls: "READ",
    input: obj({ limit: limit(10, 50), show: SHOW }),
    run: async (input, ctx) => {
      const rows = [...(await ctx.read("tournament.list", {}))].sort((a, b) => (b.updated_ms ?? 0) - (a.updated_ms ?? 0));
      const page = bounded(
        rows.map((t) => ({
          tournament_id: t.tournament_id,
          ...(t.character === undefined ? {} : { character: t.character }),
          runs: t.runs,
          completed: t.completed,
          ...(t.judged === undefined ? {} : { judged: t.judged }),
          ...(t.held === undefined ? {} : { held: t.held }),
          ...(t.red_line_fails === undefined ? {} : { red_line_fails: t.red_line_fails }),
          ...(t.updated_ms === undefined ? {} : { updated: new Date(t.updated_ms).toISOString() }),
        })),
        argInt(input, "limit", 10, 1, 50),
      );
      return withView({ ...page }, ctx.show(input, pageRoute("simulator", "tournaments")));
    },
  },
  {
    name: "get_tournament",
    title: "Summarise a tournament",
    description: "Returns a tournament's results summarised: run, judged, failed and void counts, cost, its worst rules over all cells and up to 12 cells (tone, tier, segment, audience). Shows #/simulator/tournaments/<id>.",
    cls: "READ",
    input: obj({ tournament: str("A tournament id (list_tournaments)."), show: SHOW }, ["tournament"]),
    run: async (input, ctx) => {
      const id = argStr(input, "tournament");
      const r = await ctx.read("tournament.results", { tournament_id: id });
      return withView(tournamentSummary(r, 12), ctx.show(input, pageRoute("simulator", "tournaments", id)));
    },
  },
  {
    name: "ranks_table",
    title: "Rank ladder",
    description: "Returns the merged rank ladder of a character (the configured default when omitted): each rank's title, vote weight and gift threshold, and who may !propose. Shows #/segments/ranks.",
    cls: "READ",
    input: obj({ character: str("A character id; the configured default when omitted."), show: SHOW }),
    run: async (input, ctx) => {
      const character = optStr(input, "character");
      const t = await ctx.read("ranks.table", character === undefined ? {} : { character });
      const ranks = t.ranks.map((r) => ({ rank: r.rank, title: r.title, weight: r.weight, ...(r.min_gifted === undefined ? {} : { min_gifted: r.min_gifted }) }));
      return withView({ character: t.character, propose_min_rank: t.propose_min_rank, ...bounded(ranks, ranks.length) }, ctx.show(input, pageRoute("segments", "ranks")));
    },
  },
  {
    name: "ledger_summary",
    title: "Engine ledger summary",
    description: "Returns the engine ledger summarised: calls, cost, errors, fallbacks and per role the calls, engines, tiers and p50 latency; the daemon's own ledger unless tournament or run is given. Shows #/brain/engine.",
    cls: "READ",
    input: obj({ tournament: str("Only this tournament's runs."), run: str("Only this run (with tournament when ids repeat)."), show: SHOW }),
    run: async (input, ctx) => {
      const tournament = optStr(input, "tournament");
      const run = optStr(input, "run");
      const s = await ctx.read("ledger.summary", { ...(tournament === undefined ? {} : { tournament_id: tournament }), ...(run === undefined ? {} : { run_id: run }) });
      const routes = s.routes.map((r) => ({
        role: r.role,
        calls: r.calls,
        errors: r.errors,
        fallbacks: r.fallbacks,
        engines: r.engines,
        tiers: r.tiers,
        ...(r.duration_p50_ms === undefined ? {} : { p50_ms: r.duration_p50_ms }),
      }));
      return withView(
        { calls: s.calls, cost_usd: Math.round(s.cost_usd * 100) / 100, errors: s.errors, fallbacks: s.fallbacks, unrouted: s.unrouted, routes: bounded(routes, 20) },
        ctx.show(input, pageRoute("brain", "engine")),
      );
    },
  },
];
