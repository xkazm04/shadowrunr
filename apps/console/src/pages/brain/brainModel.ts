// The brain's layer-1 model: A/1's `SR.brain` (core.js:262-288) rebuilt as pure functions over
// the contract types, aggregated so the drawing cost is bounded at any scale. Runs collapse into
// one lane per (core tier, audience) cell (A/1's lane label), and a lane holds at most
// `maxPulses` pulses: beyond that, pulses are binned by x and each bin keeps its longest call.
import type { LedgerEntry, ModelTier, RunRow, StreamEvent } from "@shadowrunr/contract";
import { isMemoryMoment } from "../../kit/events";

export interface Pulse {
  /** ms since the run's first ledger row. */
  readonly t: number;
  readonly dur: number;
  readonly role: string;
  readonly err: boolean;
}

export interface Lane {
  readonly key: string;
  /** A/1's lane label, `tier · a<audience>`. */
  readonly label: string;
  readonly tier: ModelTier;
  readonly audience: number;
  readonly runIds: readonly string[];
  readonly pulses: readonly Pulse[];
  /** The time axis (ms), A/1's `span * 1.06`. */
  readonly span: number;
  /** How many ledger rows the lane read; more than `pulses.length` when binned. */
  readonly calls: number;
}

export interface MemoryCurve {
  readonly span: number;
  readonly points: readonly { readonly t: number; readonly n: number }[];
}

export interface Funnel {
  readonly chat: number;
  readonly dropped: number;
  readonly collapsed: number;
  readonly picked: number;
  readonly answered: number;
  /** Transcripts the funnel counted. */
  readonly runs: number;
}

export interface RoleStat {
  readonly role: string;
  readonly calls: number;
  readonly errors: number;
}

export interface Latency {
  readonly p50: number | undefined;
  readonly p95: number | undefined;
  readonly n: number;
}

export const LIMITS = { maxLanes: 12, maxPulses: 480, maxMemoryPoints: 400 } as const;

/** Nearest-rank percentile, the reconcile crate's `stats::percentile`. */
export function percentile(sorted: readonly number[], p: number): number | undefined {
  if (sorted.length === 0) return undefined;
  const rank = Math.ceil((p / 100) * sorted.length);
  return sorted[Math.min(sorted.length, Math.max(1, rank)) - 1];
}

const laneKey = (r: RunRow) => `${r.spec.core_tier}|${r.spec.audience_size}`;
/** A/1 leaves judge and consolidator calls out of the lane's time axis (they run after). */
const onAxis = (role: string) => role !== "judge" && role !== "consolidator";

function binPulses(pulses: readonly Pulse[], span: number, bins: number): Pulse[] {
  const out: (Pulse | undefined)[] = new Array<Pulse | undefined>(bins).fill(undefined);
  const errs = new Array<boolean>(bins).fill(false);
  for (const p of pulses) {
    const b = Math.min(bins - 1, Math.floor(Math.min(0.99, p.t / span) * bins));
    const cur = out[b];
    if (cur === undefined || p.dur > cur.dur) out[b] = p;
    if (p.err) errs[b] = true;
  }
  const kept: Pulse[] = [];
  out.forEach((p, b) => {
    if (p) kept.push(errs[b] && !p.err ? { ...p, err: true } : p);
  });
  return kept;
}

/** Lanes per (tier, audience) in run order, with run-relative pulses. */
export function buildLanes(runs: readonly RunRow[], ledger: readonly LedgerEntry[], limits = LIMITS): Lane[] {
  const groups: { key: string; tier: ModelTier; audience: number; runIds: string[] }[] = [];
  const laneOf = new Map<string, number>();
  const byKey = new Map<string, number>();
  for (const r of runs) {
    const k = laneKey(r);
    let i = byKey.get(k);
    if (i === undefined) {
      if (groups.length >= limits.maxLanes) continue;
      i = groups.length;
      byKey.set(k, i);
      groups.push({ key: k, tier: r.spec.core_tier, audience: r.spec.audience_size, runIds: [] });
    }
    groups[i]?.runIds.push(r.run_id);
    laneOf.set(r.run_id, i);
  }
  const t0 = new Map<string, number>();
  for (const l of ledger) {
    if (l.run_id === undefined || !laneOf.has(l.run_id)) continue;
    const cur = t0.get(l.run_id);
    if (cur === undefined || l.ts_ms < cur) t0.set(l.run_id, l.ts_ms);
  }
  const raw: Pulse[][] = groups.map(() => []);
  for (const l of ledger) {
    if (l.run_id === undefined) continue;
    const i = laneOf.get(l.run_id);
    const start = t0.get(l.run_id);
    if (i === undefined || start === undefined) continue;
    raw[i]?.push({ t: l.ts_ms - start, dur: l.duration_ms, role: l.role, err: l.error_kind !== undefined });
  }
  return groups.map((g, i) => {
    const ps = (raw[i] ?? []).sort((a, b) => a.t - b.t);
    let end = 0;
    for (const p of ps) if (onAxis(p.role)) end = Math.max(end, p.t + p.dur);
    const span = (end || 1) * 1.06;
    return {
      key: g.key,
      label: `${g.tier} · a${g.audience}`,
      tier: g.tier,
      audience: g.audience,
      runIds: g.runIds,
      pulses: ps.length > limits.maxPulses ? binPulses(ps, span, limits.maxPulses) : ps,
      span,
      calls: ps.length,
    };
  });
}

/** A/1's memory curve for one transcript: moments kept, cumulative, over stream time. */
export function memoryCurve(transcript: readonly StreamEvent[], maxPoints: number = LIMITS.maxMemoryPoints): MemoryCurve {
  let n = 0;
  let span = 0;
  const pts: { t: number; n: number }[] = [];
  for (const e of transcript) {
    span = Math.max(span, e.ts_ms);
    if (isMemoryMoment(e)) pts.push({ t: e.ts_ms, n: ++n });
  }
  if (pts.length <= maxPoints) return { span: span || 1, points: pts };
  const step = pts.length / maxPoints;
  const kept: { t: number; n: number }[] = [];
  for (let i = 0; i < maxPoints - 1; i++) {
    const p = pts[Math.floor(i * step)];
    if (p) kept.push(p);
  }
  const last = pts[pts.length - 1];
  if (last) kept.push(last);
  return { span: span || 1, points: kept };
}

/** The picker funnel and the first-word latency over the transcripts that loaded. */
export function transcriptStats(transcripts: readonly (readonly StreamEvent[])[]): { funnel: Funnel; latency: Latency } {
  let chat = 0;
  let dropped = 0;
  let collapsed = 0;
  let picked = 0;
  let answered = 0;
  const first: number[] = [];
  for (const tr of transcripts) {
    const pickedIds = new Set<string>();
    const answeredIds = new Set<string>();
    for (const e of tr) {
      if (e.type === "chat") chat++;
      else if (e.type === "moderation" && e.stage === "input") {
        if (e.action === "dropped") dropped++;
        else if (e.action.includes("collapsed")) collapsed++;
      } else if (e.type === "picked") for (const id of e.msg_ids) pickedIds.add(id);
      else if (e.type === "say") {
        for (const id of e.replying_to ?? []) answeredIds.add(id);
        if (e.speaker === "malakor" && e.first_text_ms !== undefined) first.push(e.first_text_ms);
      }
    }
    picked += pickedIds.size;
    answered += answeredIds.size;
  }
  first.sort((a, b) => a - b);
  return {
    funnel: { chat, dropped, collapsed, picked, answered, runs: transcripts.length },
    latency: { p50: percentile(first, 50), p95: percentile(first, 95), n: first.length },
  };
}

/** Calls and errors per role, most calls first (A/1's worker-pool lamps). */
export function roleStats(ledger: readonly LedgerEntry[]): RoleStat[] {
  const m = new Map<string, { calls: number; errors: number }>();
  for (const l of ledger) {
    const s = m.get(l.role) ?? { calls: 0, errors: 0 };
    s.calls++;
    if (l.error_kind !== undefined) s.errors++;
    m.set(l.role, s);
  }
  return [...m].map(([role, s]) => ({ role, ...s })).sort((a, b) => b.calls - a.calls);
}

export interface BrainModel {
  readonly lanes: readonly Lane[];
  readonly memory: readonly (MemoryCurve | undefined)[];
  readonly memoryTotal: number;
  readonly memMax: number;
  readonly funnel: Funnel;
  readonly latency: Latency;
  readonly roles: readonly RoleStat[];
  readonly calls: number;
  readonly runs: number;
}

/**
 * Everything layer 1 draws. `transcripts[i]` is lane i's sample run (or undefined while it
 * loads); the funnel and latency count the samples that arrived.
 */
export function buildBrainModel(
  runs: readonly RunRow[],
  ledger: readonly LedgerEntry[],
  transcripts: readonly (readonly StreamEvent[] | undefined)[],
): BrainModel {
  const lanes = buildLanes(runs, ledger);
  const memory = lanes.map((_, i) => {
    const tr = transcripts[i];
    return tr ? memoryCurve(tr) : undefined;
  });
  let memMax = 1;
  let memoryTotal = 0;
  for (const c of memory) {
    const last = c?.points[c.points.length - 1];
    if (last) {
      memMax = Math.max(memMax, last.n);
      memoryTotal += last.n;
    }
  }
  const loaded = transcripts.filter((t): t is readonly StreamEvent[] => t !== undefined);
  const { funnel, latency } = transcriptStats(loaded);
  return { lanes, memory, memoryTotal, memMax, funnel, latency, roles: roleStats(ledger), calls: ledger.length, runs: runs.length };
}
