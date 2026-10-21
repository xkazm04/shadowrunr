// Test-only (imported by *.test.ts[x] only): contract-valid generated data at any scale and a
// scripted daemon that pages it by cursor. Every value is generated, never presented as real.
import type { AccountSummary, LedgerEntry, LedgerSummary, ModelTier, Role, RouteSummary, RunRow, StreamEvent, TournamentRow } from "@shadowrunr/contract";
import { ok, page, param, type Script } from "../../kit/testing";
import type { Ctx } from "./draw";

const TIERS: readonly ModelTier[] = ["haiku", "sonnet", "opus"];
const AUDIENCES = [3, 30, 300] as const;

export function tournament(i: number, runs: number): TournamentRow {
  return { tournament_id: `gen-t${String(i).padStart(4, "0")}`, instrument_hash: "gen", runs, completed: runs };
}

export function runRow(i: number, tid: string): RunRow {
  const tier = TIERS[i % TIERS.length] ?? "haiku";
  const aud = AUDIENCES[Math.floor(i / TIERS.length) % AUDIENCES.length] ?? 3;
  const run_id = `gen.${tier}.SEG-01.a${aud}.s${i}`;
  return {
    run_id,
    tournament_id: tid,
    has_verdict: i % 2 === 0,
    spec: { run_id, audience_size: aud, character: "malakor", core_effort: "low", core_tier: tier, duration_s: 600, population: "gen", seed: i, segment: "SEG-01", tone_variant: "theatrical" },
  };
}

const ROLES: readonly Role[] = ["core_thread", "chatter", "core_thread", "chat_picker", "researcher"];

export function ledgerRows(run: RunRow, n: number, t0 = 1_800_000_000_000): LedgerEntry[] {
  return Array.from({ length: n }, (_, k) => {
    const role = ROLES[k % ROLES.length] ?? "core_thread";
    const row: LedgerEntry = {
      ts_ms: t0 + k * 4000,
      role,
      tier: run.spec.core_tier,
      engine: "claude_cli",
      account: k % 7 === 0 ? "spare" : "main",
      model: `claude-${run.spec.core_tier}-gen`,
      duration_ms: 800 + ((k * 37) % 5000),
      first_text_ms: 400 + ((k * 13) % 2000),
      usage: { input_tokens: 4, output_tokens: 100 + k },
      cost_usd: 0.0002,
      run_id: run.run_id,
      tournament_id: run.tournament_id,
    };
    if (k % 41 === 2) row.error_kind = "timeout";
    if (k % 53 === 1) row.fallback_reason = "rate_limited";
    return row;
  });
}

/** A transcript of `n` lines: chat, moderation, picks, replies and kept moments. */
export function transcript(n: number, runId = "gen.run"): StreamEvent[] {
  const out: StreamEvent[] = [{ type: "stream_start", ts_ms: 0, stream_id: runId, character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 30 }];
  for (let i = 1; i < n; i++) {
    const ts_ms = i * 1000;
    const id = `m${i}`;
    switch (i % 6) {
      case 1:
        out.push({ type: "chat", ts_ms, msg: { id, text: `generated chat line ${i}`, ts_ms, viewer: { id: `v${i % 40}`, name: `viewer${i % 40}`, platform: "sim", rank: "thrall" } } });
        break;
      case 2:
        out.push({ type: "moderation", ts_ms, stage: "input", rule: "rate_cap", action: i % 4 === 0 ? "dropped" : "collapsed:flood", msg_id: `m${i - 1}` });
        break;
      case 3:
        out.push({ type: "picked", ts_ms, msg_ids: [`m${i - 2}`], reason: "generated pick" });
        break;
      case 4:
        out.push({ type: "say", ts_ms, speaker: "host", text: `generated reply ${i}`, replying_to: [`m${i - 3}`], first_text_ms: 1000 + ((i * 97) % 3000) });
        break;
      case 5:
        out.push({ type: "op", ts_ms, op: { op: "mark_moment", label: `moment ${i}` } });
        break;
      default:
        out.push({ type: "say", ts_ms, speaker: "host", text: `Memory candidates: line ${i}` });
    }
  }
  return out;
}

const rank = (sorted: readonly number[], p: number): number | undefined =>
  sorted.length === 0 ? undefined : sorted[Math.min(sorted.length, Math.max(1, Math.ceil((p / 100) * sorted.length))) - 1];

/** What the daemon's `ledger.summary` answers for `rows`: every row folded once (its definitions:
 * failed calls out of the durations, nearest rank, a missing cost counted as uncosted). */
export function ledgerSummary(rows: readonly LedgerEntry[]): LedgerSummary {
  const out: LedgerSummary = { calls: rows.length, errors: 0, fallbacks: 0, cost_usd: 0, uncosted_calls: 0, cache_read_tokens: 0, unrouted: 0, routes: [], accounts: [] };
  const roles = new Map<string, { r: RouteSummary; dur: number[]; first: number[] }>();
  const accounts = new Map<string, AccountSummary>();
  const add = <T,>(xs: T[], x: T | undefined) => {
    if (x !== undefined && !xs.includes(x)) xs.push(x);
  };
  for (const l of rows) {
    const failed = l.error_kind !== undefined;
    if (failed) out.errors++;
    if (l.fallback_reason !== undefined) out.fallbacks++;
    if (l.cost_usd === undefined) out.uncosted_calls++;
    else out.cost_usd += l.cost_usd;
    out.cache_read_tokens += l.usage.cache_read_tokens ?? 0;
    let e = roles.get(l.role);
    if (!e) {
      e = { r: { role: l.role, calls: 0, errors: 0, fallbacks: 0, tiers: [], efforts: [], engines: [], models: [] }, dur: [], first: [] };
      roles.set(l.role, e);
    }
    e.r.calls++;
    if (failed) e.r.errors++;
    if (l.fallback_reason !== undefined) e.r.fallbacks++;
    add(e.r.tiers, l.tier);
    add(e.r.efforts, l.effort);
    add(e.r.engines, l.engine);
    add(e.r.models, l.model);
    if (!failed) e.dur.push(l.duration_ms);
    if (l.first_text_ms !== undefined) e.first.push(l.first_text_ms);
    if (l.engine === undefined || l.account === undefined) {
      out.unrouted++;
      continue;
    }
    const key = `${l.engine}|${l.account}`;
    const a = accounts.get(key) ?? { engine: l.engine, account: l.account, calls: 0, errors: 0, models: [], last_ts_ms: 0 };
    a.calls++;
    if (failed) a.errors++;
    const m = l.model === undefined ? undefined : a.models.find((x) => x.model === l.model);
    if (m) m.calls++;
    else if (l.model !== undefined) a.models.push({ model: l.model, calls: 1 });
    if (l.ts_ms >= a.last_ts_ms) {
      a.last_ts_ms = l.ts_ms;
      if (l.error_kind === undefined) delete a.last_error;
      else a.last_error = l.error_kind;
    }
    accounts.set(key, a);
  }
  const num = (x: number, y: number) => x - y;
  for (const { r, dur, first } of roles.values()) {
    dur.sort(num);
    first.sort(num);
    const p50 = rank(dur, 50);
    const p95 = rank(dur, 95);
    const f50 = rank(first, 50);
    out.routes.push({ ...r, ...(p50 === undefined ? {} : { duration_p50_ms: p50 }), ...(p95 === undefined ? {} : { duration_p95_ms: p95 }), ...(f50 === undefined ? {} : { first_text_p50_ms: f50 }) });
  }
  out.routes.sort((x, y) => y.calls - x.calls);
  out.accounts = [...accounts.values()].sort((x, y) => y.calls - x.calls);
  return out;
}

export interface Season {
  readonly tournaments: TournamentRow[];
  readonly runs: RunRow[];
  readonly ledger: LedgerEntry[];
  readonly transcriptLines: number;
}

/** `runs` runs in the last tournament, `rowsPerRun` ledger rows each. */
export function season(opts: { tournaments: number; runs: number; rowsPerRun: number; transcriptLines: number }): Season {
  const tournaments = Array.from({ length: opts.tournaments }, (_, i) => tournament(i, opts.runs));
  const tid = tournaments[tournaments.length - 1]?.tournament_id ?? "gen-t0000";
  const runs = Array.from({ length: opts.runs }, (_, i) => runRow(i, tid));
  const ledger = runs.flatMap((r) => ledgerRows(r, opts.rowsPerRun));
  return { tournaments, runs, ledger, transcriptLines: opts.transcriptLines };
}

/** A daemon over a generated season; `null` season = the daemon is unreachable. */
export function daemon(s: Season | null, transcripts = new Map<string, StreamEvent[]>()): Script {
  return (method, sent) => {
    if (s === null) return { network: true };
    switch (method) {
      case "tournaments.page":
        return page(s.tournaments, sent);
      case "runs.page":
        return page(s.runs, sent);
      case "ledger.page":
        return page(param(sent, "tournament_id") === undefined ? [] : s.ledger, sent);
      case "ledger.summary":
        return ok(ledgerSummary(param(sent, "tournament_id") === undefined ? [] : s.ledger));
      case "run.transcript": {
        const id = param(sent, "run_id") ?? "";
        let tr = transcripts.get(id);
        if (!tr) {
          tr = transcript(s.transcriptLines, id);
          transcripts.set(id, tr);
        }
        return ok(tr);
      }
      default:
        return { status: 404, body: { error: { code: "method_not_found", message: method } } };
    }
  };
}

/** A canvas context that records calls and style writes; the brain's draws are gated on it. */
export class Recorder implements Ctx {
  readonly calls: Record<string, number> = {};
  readonly texts: string[] = [];
  readonly writes: Record<string, unknown[]> = {};
  private readonly vals: Record<string, unknown> = {};
  private count(k: string) {
    this.calls[k] = (this.calls[k] ?? 0) + 1;
  }
  private put(k: string, v: unknown) {
    (this.writes[k] ??= []).push(v);
    this.vals[k] = v;
  }
  clearRect() {
    this.count("clearRect");
  }
  beginPath() {
    this.count("beginPath");
  }
  moveTo() {
    this.count("moveTo");
  }
  lineTo() {
    this.count("lineTo");
  }
  stroke() {
    this.count("stroke");
  }
  fill() {
    this.count("fill");
  }
  arc() {
    this.count("arc");
  }
  setTransform() {
    this.count("setTransform");
  }
  fillText(t: string) {
    this.count("fillText");
    this.texts.push(t);
  }
  get strokeStyle(): string {
    return String(this.vals["strokeStyle"] ?? "");
  }
  set strokeStyle(v: string | CanvasGradient | CanvasPattern) {
    this.put("strokeStyle", v);
  }
  get fillStyle(): string {
    return String(this.vals["fillStyle"] ?? "");
  }
  set fillStyle(v: string | CanvasGradient | CanvasPattern) {
    this.put("fillStyle", v);
  }
  get lineWidth(): number {
    return Number(this.vals["lineWidth"] ?? 1);
  }
  set lineWidth(v: number) {
    this.put("lineWidth", v);
  }
  get font(): string {
    return String(this.vals["font"] ?? "");
  }
  set font(v: string) {
    this.put("font", v);
  }
  get shadowColor(): string {
    return String(this.vals["shadowColor"] ?? "");
  }
  set shadowColor(v: string) {
    this.put("shadowColor", v);
  }
  get shadowBlur(): number {
    return Number(this.vals["shadowBlur"] ?? 0);
  }
  set shadowBlur(v: number) {
    this.put("shadowBlur", v);
  }
  get globalAlpha(): number {
    return Number(this.vals["globalAlpha"] ?? 1);
  }
  set globalAlpha(v: number) {
    this.put("globalAlpha", v);
  }
  total(): number {
    return Object.values(this.calls).reduce((a, b) => a + b, 0);
  }
}

