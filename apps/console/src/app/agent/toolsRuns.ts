// Reviews and runs: sandbox sessions in Review, sim runs and tournaments in the Simulator,
// transcripts and the engine ledger in Brain, the rank ladder in Segments.
import type { RunParams, RunsPageParams } from "@shadowrunr/contract";
import { pageRoute } from "../../router";
import { transcriptRoute } from "../../router/brain";
import { clip, eventLine, sessionSummary, tournamentSummary, verdictSummary } from "./summary";
import { argInt, argStr, bounded, fail, fitBody, int, limit, obj, optStr, RESULT_BUDGET, SHOW, sizeOf, str, type Ctx, type ToolSpec } from "./tool";

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

const clipText = (l: Record<string, unknown>, n: number) => (typeof l["text"] === "string" ? { ...l, text: clip(l["text"], n) } : l);

const withView = (body: Record<string, unknown>, view: string | undefined) => (view === undefined ? body : { ...body, view });

export const RUN_TOOLS: readonly ToolSpec[] = [
  {
    name: "list_sessions",
    title: "List sandbox sessions",
    description: "Finished sandbox sessions, newest first: id, minutes, lines, pass, rule fails. Then get_session{session}.",
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
    description: "A sandbox session summarised: checks, verdict, mix, notable lines. It passes only if every check does; failed_because names the rest.",
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
    description: "One session line with 3 either side, its tag, failing rules and the operator's marks. Then leave_feedback.",
    cls: "READ",
    input: obj({ session: SESSION, line: int("The 1-based transcript line.", 1), show: SHOW }, ["session", "line"]),
    run: async (input, ctx) => {
      const id = argStr(input, "session");
      const rs = await session(ctx, id);
      const line = argInt(input, "line", 0, 1, Number.MAX_SAFE_INTEGER);
      if (line < 1) fail("invalid_params", `"line" is required`);
      if (line > rs.events.length) fail("invalid_params", `line ${line} is past the end (${rs.events.length} lines)`);
      const from = Math.max(1, line - 3);
      const tag = rs.tags?.lines.find((t) => t.line === line);
      const fails = (rs.verdict?.rules ?? []).filter((r) => r.state === "fail" && (r.evidence ?? []).some((ref) => ref.endsWith(`#${line}`))).map((r) => r.rule);
      const marks = rs.feedback.filter((f) => f.line === line).map((f) => ({ tags: f.tags, ...(f.note === undefined ? {} : { note: clip(f.note, 300) }) }));
      // The line itself keeps up to 400 chars; its neighbours shrink until the answer fits.
      const at = (n: number) =>
        rs.events.slice(from - 1, line + 3).map((e, i) => {
          const l = eventLine(e, from + i);
          return from + i === line ? l : clipText(l, n);
        });
      const body = (n: number) => ({ session_id: id, line, lines: at(n), ...(tag === undefined ? {} : { tag: { kind: tag.kind, target: tag.target, edge: tag.edge } }), fails, marks });
      const fit = [200, 140, 100, 70, 45].map(body).find((b) => sizeOf(b) <= RESULT_BUDGET - 80) ?? body(30);
      return withView(
        fit,
        ctx.show(input, pageRoute("review", id, "timeline", String(line))),
      );
    },
  },
  {
    name: "list_runs",
    title: "List sim runs",
    description: "Sim runs: spec, verdict, red-line fails. To page on, pass next_cursor back as cursor.",
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
    description: "A window of a run's transcript, default 20 lines from line 1; next_from says where to read on.",
    cls: "READ",
    input: obj({ run: RUN, from: int("First line, 1-based (default 1).", 1), limit: limit(20, 100), show: SHOW }, ["run"]),
    run: async (input, ctx) => {
      const run = argStr(input, "run");
      const events = await ctx.read("run.transcript", runParams(run));
      const from = argInt(input, "from", 1, 1, Number.MAX_SAFE_INTEGER);
      const want = argInt(input, "limit", 20, 1, 100);
      const page = bounded(events, want, from - 1);
      // Long lines are clipped harder until at least 6 lines (or all asked for) fit the budget;
      // the bind guard's trim then keeps what fits, and next_from says where to read on.
      const at = (n: number) => ({ run, ...page, items: page.items.map((e, i) => clipText(eventLine(e, from + i), n)) });
      const floor = Math.min(want, page.items.length, 6);
      const fit = [400, 240, 160, 110, 80]
        .map((n) => fitBody(at(n), RESULT_BUDGET - 120))
        .find((b) => b !== undefined && (b["items"] as unknown[]).length >= floor);
      const body = fit ?? fitBody(at(60), RESULT_BUDGET - 120) ?? at(60);
      const next = from + (body["items"] as unknown[]).length;
      return withView({ ...body, ...(next <= events.length ? { next_from: next } : {}) }, ctx.show(input, transcriptRoute({ runId: run }, from > 1 ? from : undefined)));
    },
  },
  {
    name: "get_verdict",
    title: "Read a run verdict",
    description: "The judge's verdict on a run: rule states, failed rules with the lines they cite, quality facts.",
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
    description: "Tournaments, newest first: id, character, runs done, judged, held, red-line fails.",
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
    description: "A tournament summarised: counts, cost, worst rules and up to 12 cells (tone, tier, segment, audience).",
    cls: "READ",
    input: obj({ tournament: str("A tournament id (list_tournaments)."), show: SHOW }, ["tournament"]),
    run: async (input, ctx) => {
      const id = argStr(input, "tournament");
      const r = await ctx.read("tournament.results", { tournament_id: id });
      return withView(tournamentSummary(r, 12), ctx.show(input, pageRoute("simulator", "tournaments", id)));
    },
  },
];
