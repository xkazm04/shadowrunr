// Bounded summaries of the daemon's big answers: transcript events, a review session, a verdict
// and a tournament's results. Nothing here dumps a transcript or a results file whole.
import type { JudgeVerdict, ReviewSession, StreamEvent, TournamentResults } from "@shadowrunr/contract";
import { fitBody, fitting, RESULT_BUDGET, sizeOf, withRoomFor } from "./tool";

export const clip = (s: string, n: number): string => (s.length <= n ? s : `${s.slice(0, n - 1)}…`);
const secs = (ms: number): number => Math.round(ms / 100) / 10;
const lineOf = (ref: string): number | undefined => {
  const n = Number(ref.slice(ref.lastIndexOf("#") + 1));
  return Number.isInteger(n) && n > 0 ? n : undefined;
};

/** One transcript event as a line: event i is line i + 1 (the /events seq, the judge's refs). */
export function eventLine(e: StreamEvent, line: number): Record<string, unknown> {
  const base = { line, t_s: secs(e.ts_ms), type: e.type };
  switch (e.type) {
    case "say":
      return { ...base, speaker: e.speaker, text: clip(e.text, 400) };
    case "chat":
      return { ...base, viewer: e.msg.viewer.name, rank: e.msg.viewer.rank, text: clip(e.msg.text, 300) };
    case "segment_start":
    case "segment_end":
      return { ...base, segment: e.segment };
    case "moderation":
      return { ...base, stage: e.stage, rule: e.rule };
    default:
      return base;
  }
}

/** Rule states counted, and the failed rules with up to `lines` of the lines each cites. */
function ruleFacts(v: JudgeVerdict, lines: number) {
  const states: Record<string, number> = {};
  for (const r of v.rules) states[r.state] = (states[r.state] ?? 0) + 1;
  const failed = v.rules
    .filter((r) => r.state === "fail")
    .map((r) => ({ rule: r.rule, lines: (r.evidence ?? []).map(lineOf).filter((l) => l !== undefined).slice(0, lines) }));
  return { states, failed };
}

/** Quality facts as true of n per dimension. */
function qualityOf(v: JudgeVerdict): Record<string, string> {
  const quality: Record<string, string> = {};
  const tally = new Map<string, [number, number]>();
  for (const q of v.quality) {
    const t = tally.get(q.dimension) ?? [0, 0];
    tally.set(q.dimension, [t[0] + (q.value ? 1 : 0), t[1] + 1]);
  }
  for (const [d, [yes, n]] of tally) quality[d] = `${yes} of ${n}`;
  return quality;
}

/**
 * Failed rules with the lines they cite; quality facts as true of n per dimension; the notes in
 * the room left. Within `budget`: cited lines drop from 8 to 4 per rule, then failed rules are
 * cut, with a footer that says how many.
 */
export function verdictSummary(v: JudgeVerdict, budget = RESULT_BUDGET - 120): Record<string, unknown> {
  const head = { run_id: v.run_id, judge_tier: v.judge_tier };
  const quality = qualityOf(v);
  let { states, failed } = ruleFacts(v, 8);
  let body: Record<string, unknown> = { ...head, rule_states: states, failed_rules: failed, quality };
  if (sizeOf(body) > budget) {
    ({ states, failed } = ruleFacts(v, 4));
    body = { ...head, rule_states: states, failed_rules: failed, quality };
  }
  if (sizeOf(body) > budget) {
    const footer = `showing FEW of ${failed.length} failed rules`;
    const kept = fitting(failed, budget - sizeOf({ ...body, failed_rules: [], failed_footer: footer }));
    body = { ...head, rule_states: states, failed_rules: kept, failed_footer: footer.replace("FEW", String(kept.length)), quality };
  }
  return withRoomFor(body, "notes", v.notes, 600, budget);
}

/**
 * A session in one answer: its row, gate checks, a brief verdict, mix, segments and the notable
 * lines (failures, marks) as a bounded list the bind guard trims to fit.
 */
export function sessionSummary(rs: ReviewSession): Record<string, unknown> {
  const row = rs.row;
  const say = new Map<number, string>();
  rs.events.forEach((e, i) => {
    if (e.type === "say" && e.speaker === "host") say.set(i + 1, e.text);
  });
  const notable = new Map<number, string[]>();
  const mark = (line: number, why: string) => notable.set(line, [...(notable.get(line) ?? []), why]);
  for (const r of rs.verdict?.rules ?? []) {
    if (r.state !== "fail") continue;
    for (const ref of r.evidence ?? []) {
      const l = lineOf(ref);
      if (l !== undefined) mark(l, `fails ${r.rule}`);
    }
  }
  for (const f of rs.feedback) mark(f.line, `marked ${f.tags.join(", ") || "note"}`);
  const all = [...notable.entries()].sort(([a], [b]) => a - b);
  // Richest first; each later form gives up detail so the notable lines keep their room.
  const forms = [
    { detail: 70, rules: 6, lines: 4, why: 80, text: 140 },
    { detail: 70, rules: 6, lines: 4, why: 80, text: 70 },
    { detail: 50, rules: 4, lines: 2, why: 60, text: 70 },
    { detail: 40, rules: 3, lines: 0, why: 50, text: 0 },
  ].map((f) => {
    const items = all.slice(0, 10).map(([line, why]) => ({ line, why: clip(why.join("; "), f.why), ...(f.text > 0 && say.has(line) ? { text: clip(say.get(line) ?? "", f.text) } : {}) }));
    const v = rs.verdict;
    const facts = v === undefined ? undefined : ruleFacts(v, f.lines);
    const failed = facts === undefined ? [] : facts.failed.slice(0, f.rules).map((r) => (f.lines > 0 ? r : r.rule));
    return {
      session_id: row.session_id,
      started: new Date(row.started_ms).toISOString(),
      minutes: row.minutes,
      length_s: row.length_s,
      pass: row.pass,
      checks: row.checks.map((c) => (c.state === "pass" ? `${c.id}: pass` : `${c.id}: ${c.state}, ${clip(c.detail, f.detail)}`)),
      events: rs.events.length,
      host_lines: say.size,
      marked: row.marked,
      tagged: row.tagged,
      mix: row.mix,
      segments: rs.segments.map((s) => `${s.segment} ${s.from_s}-${s.to_s} s`),
      ...(v === undefined || facts === undefined
        ? {}
        : { verdict: { judge_tier: v.judge_tier, rule_states: facts.states, failed_rules: failed, ...(facts.failed.length > failed.length ? { failed_footer: `showing ${failed.length} of ${facts.failed.length}` } : {}) } }),
      moments: list(items, all.length),
    };
  });
  // The first form that keeps at least four notable lines (or all of them) within the budget,
  // leaving room for the view the tool adds.
  const floor = Math.min(4, all.length);
  for (const f of forms) {
    const fitted = fitBody(f, RESULT_BUDGET - 80);
    if (fitted !== undefined && (fitted["moments"] as { items: unknown[] }).items.length >= floor) return fitted;
  }
  return forms[forms.length - 1] ?? {};
}

const list = <T,>(items: readonly T[], total: number) => ({
  items,
  showing: items.length,
  total,
  footer: `showing ${items.length} of ${total} notable lines; open_moment{session, line} opens one`,
});

/** The tournament's totals, its worst rules over all cells and its cells, bounded. */
export function tournamentSummary(r: TournamentResults, cells: number): Record<string, unknown> {
  const rules = new Map<string, { fail: number; n: number }>();
  for (const c of r.cells) {
    for (const [rule, t] of Object.entries(c.rules)) {
      if (t === undefined) continue;
      const acc = rules.get(rule) ?? { fail: 0, n: 0 };
      rules.set(rule, { fail: acc.fail + t.fail, n: acc.n + t.n });
    }
  }
  const worst = [...rules.entries()]
    .filter(([, t]) => t.fail > 0)
    .sort(([, a], [, b]) => b.fail - a.fail)
    .slice(0, 8)
    .map(([rule, t]) => ({ rule, fails: t.fail, of: t.n }));
  const shown = r.cells.slice(0, cells).map((c) => ({
    tone: c.cell.tone,
    tier: c.cell.core_tier,
    segment: c.cell.segment,
    audience: c.cell.audience_size,
    runs: c.runs,
    judged: c.judged,
    failed: c.failed,
    ...(c.runs_with_fail === undefined ? {} : { runs_with_fail: c.runs_with_fail }),
    core_p50_ms: c.core_latency.p50_ms,
  }));
  return {
    tournament_id: r.tournament_id,
    runs: r.runs,
    judged: r.judged,
    failed: r.failed,
    void: r.void,
    ...(r.engine_failed === undefined ? {} : { engine_failed: r.engine_failed }),
    ...(r.judge_pending === undefined ? {} : { judge_pending: r.judge_pending }),
    cost_usd: Math.round(r.usage_total.cost_usd * 100) / 100,
    calls: r.usage_total.calls,
    worst_rules: worst,
    cells: { items: shown, showing: shown.length, total: r.cells.length, footer: `showing ${shown.length} of ${r.cells.length}` },
  };
}
