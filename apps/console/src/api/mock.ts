// `?mock=1`: an in-memory daemon over the shipped fixtures (src/fixtures/*.json), so the UI
// can be reviewed without WP5/WP6. All fixture data is fabricated (`fictional: true`).

import type {
  Directive,
  DirectiveKind,
  HealthReport,
  JudgeVerdict,
  NextStreamPlan,
  PlanOption,
  ReconcileReport,
  RunRow,
  SegmentId,
  StreamEvent,
  TournamentResults,
  TournamentRow,
} from "../contract";
import type { EventSource } from "./events";
import { RpcError, type Transport } from "./rpc";
import simFixture from "../fixtures/sim.json";
import transcriptFixture from "../fixtures/transcripts.json";
import deskFixture from "../fixtures/desk.json";

interface Template {
  events: StreamEvent[];
  verdict: Omit<JudgeVerdict, "run_id">;
}

type FixtureRun = RunRow & { template: string };

const sim = simFixture as unknown as {
  tournaments: TournamentRow[];
  runs: FixtureRun[];
  results: Record<string, TournamentResults>;
};
const templates = (transcriptFixture as unknown as { templates: Record<string, Template> }).templates;
const desk = deskFixture as unknown as {
  preflight: HealthReport[];
  report: ReconcileReport;
  proposals: PlanOption[];
  plan: NextStreamPlan;
  directives: Directive[];
};

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;
const withRun = <T>(v: T, runId: string): T =>
  JSON.parse(JSON.stringify(v).replaceAll("{run}", runId)) as T;

function findRun(runId: string): FixtureRun {
  const r = sim.runs.find((x) => x.run_id === runId);
  if (!r) throw new RpcError("rpc", "run", `unknown run_id ${runId}`, { code: "not_found" });
  return r;
}

export function transcriptFor(runId: string): StreamEvent[] {
  const run = findRun(runId);
  const t = templates[run.template];
  if (!t) throw new RpcError("rpc", "run.transcript", `no transcript for ${runId}`, { code: "not_found" });
  const events = withRun(t.events, runId);
  const first = events[0];
  if (first && first.type === "stream_start") {
    first.tone_variant = run.spec.tone_variant;
    first.core_tier = run.spec.core_tier;
    first.audience_size = run.spec.audience_size;
  }
  return events;
}

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

export interface MockOptions {
  /** Artificial latency per call (ms). */
  latencyMs?: number;
  now?: () => number;
}

export function mockTransport(opts: MockOptions = {}): Transport {
  const latency = opts.latencyMs ?? 120;
  const now = opts.now ?? (() => Date.now());
  const directives: Directive[] = clone(desk.directives);
  const reports = new Map<string, ReconcileReport>();
  let seq = 0;

  const addDirective = (kind: DirectiveKind, text: string, scope?: SegmentId): Directive => {
    const d: Directive = { id: `dir-m${++seq}`, kind, text, created_ms: now(), active: true };
    if (scope) d.scope = scope;
    directives.unshift(d);
    return d;
  };

  const handlers: Record<string, (p: Record<string, unknown>) => unknown> = {
    "preflight.run": () => clone(desk.preflight),
    "tournament.list": () => clone(sim.tournaments),
    "tournament.results": (p) => {
      const r = sim.results[String(p.tournament_id)];
      if (!r) throw new RpcError("rpc", "tournament.results", "no results.json yet", { code: "not_found" });
      return clone(r);
    },
    "runs.list": (p) =>
      sim.runs
        .filter((r) => !p.tournament_id || r.tournament_id === p.tournament_id)
        .map(({ template: _t, ...row }) => clone(row)),
    "run.transcript": (p) => transcriptFor(String(p.run_id)),
    "run.verdict": (p) => {
      const runId = String(p.run_id);
      const run = findRun(runId);
      const t = templates[run.template];
      if (!run.has_verdict || !t) {
        throw new RpcError("rpc", "run.verdict", `run ${runId} has no verdict yet`, { code: "not_found" });
      }
      return { run_id: runId, ...withRun(t.verdict, runId) };
    },
    "sim.start": () => {
      throw new RpcError("rpc", "sim.start", "sim.start is not available in mock mode", {
        code: "unavailable",
      });
    },
    "reconcile.run": (p) => {
      const src = p.source as { tournament_id?: string; run_ids?: string[] } | undefined;
      if (!src || (!src.tournament_id && !src.run_ids?.length)) {
        throw new RpcError("rpc", "reconcile.run", "source is required", { code: "bad_params" });
      }
      const report_id = `rr-mock-${++seq}`;
      const report = clone(desk.report);
      report.report_id = report_id;
      report.created_ms = now();
      report.source = src.tournament_id ? { tournament_id: src.tournament_id } : { run_ids: src.run_ids ?? [] };
      reports.set(report_id, report);
      return { report_id };
    },
    "reconcile.report": (p) => {
      const r = reports.get(String(p.report_id)) ?? (p.report_id === desk.report.report_id ? desk.report : undefined);
      if (!r) throw new RpcError("rpc", "reconcile.report", "unknown report_id", { code: "not_found" });
      return clone(r);
    },
    "plan.proposals": () => clone(desk.proposals),
    "plan.choose": (p) => {
      const ids = (p.option_ids as string[] | undefined) ?? [];
      if (!ids.length) throw new RpcError("rpc", "plan.choose", "choose at least one option", { code: "bad_params" });
      const plan = clone(desk.plan);
      plan.plan_id = `plan-mock-${++seq}`;
      plan.created_ms = now();
      plan.directives = directives.filter((d) => d.active).map((d) => clone(d));
      const chosen = desk.proposals.filter((o) => ids.includes(o.option_id));
      plan.topics = [...plan.topics, ...chosen.filter((o) => o.kind === "topic").map((o) => o.title)];
      return plan;
    },
    "directive.list": () => clone(directives),
    "directive.add": (p) => {
      const kind = p.kind as DirectiveKind;
      const text = String(p.text ?? "").trim();
      if ((kind !== "prevent" && kind !== "try") || !text) {
        throw new RpcError("rpc", "directive.add", "kind and text are required", { code: "bad_params" });
      }
      return clone(addDirective(kind, text, p.scope as SegmentId | undefined));
    },
    "directive.deactivate": (p) => {
      const d = directives.find((x) => x.id === p.id);
      if (!d) throw new RpcError("rpc", "directive.deactivate", "unknown directive", { code: "not_found" });
      d.active = false;
      return clone(d);
    },
    "lord.chat": (p) => {
      const message = String(p.message ?? "").trim();
      if (!message) throw new RpcError("rpc", "lord.chat", "message is required", { code: "bad_params" });
      const lower = message.toLowerCase();
      const added: Directive[] = [];
      const scope: SegmentId | undefined = /seg-?02a|hexmap|investigation/.test(lower)
        ? "SEG-02A"
        : /seg-?01|improv/.test(lower)
          ? "SEG-01"
          : undefined;
      if (/\b(never|don't|do not|stop|prevent|no more)\b/.test(lower)) {
        added.push(clone(addDirective("prevent", message, scope)));
      } else if (/\b(try|experiment|let him|more of|start)\b/.test(lower)) {
        added.push(clone(addDirective("try", message, scope)));
      }
      const reply = added.length
        ? `It is written, my Lord. Malakor will receive it with the next dispatch as a ${added[0]!.kind} directive${scope ? ` for ${scope}` : ""}.`
        : "Understood, my Lord. Nothing here asks for a standing directive; say 'never…' or 'try…' when you want one recorded.";
      return { reply, directives_added: added };
    },
  };

  return {
    async call(method, params, signal) {
      await delay(latency);
      if (signal?.aborted) throw new DOMException("aborted", "AbortError");
      const h = handlers[method];
      if (!h) throw new RpcError("rpc", method, `unknown method ${method}`, { code: "method_not_found" });
      try {
        return h((params ?? {}) as Record<string, unknown>);
      } catch (e) {
        if (e instanceof RpcError && e.method !== method) {
          throw new RpcError(e.kind, method, e.message, { code: e.code });
        }
        throw e;
      }
    },
  };
}

/** Replays a fixture transcript on a timer, standing in for `/events`. */
export function mockEvents(): EventSource {
  return {
    subscribe(target, speed, onEvent, onStatus) {
      let events: StreamEvent[];
      try {
        events = transcriptFor(target);
      } catch (e) {
        onStatus("error", e instanceof Error ? e.message : String(e));
        return () => {};
      }
      onStatus("open");
      let i = 0;
      let timer: ReturnType<typeof setTimeout> | undefined;
      const step = () => {
        const ev = events[i];
        if (!ev) {
          onStatus("closed");
          return;
        }
        onEvent(ev);
        i++;
        const next = events[i];
        // Compress stream time so a 5-minute fixture replays in about 20 s at 1x.
        const gap = next ? Math.min(1500, Math.max(60, (next.ts_ms - ev.ts_ms) / 15)) : 0;
        timer = setTimeout(step, gap / Math.max(0.1, speed));
      };
      timer = setTimeout(step, 200);
      return () => clearTimeout(timer);
    },
  };
}
