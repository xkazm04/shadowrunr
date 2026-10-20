// The timeline's rows: the session cut into equal stretches of stream time, each holding
// Malakor's lines, the chat count, show marks (ops, handovers, engine health) and gate marks.
// Stretches with nothing of the shown kinds fold. Pure; recomputed per view, never stored.
import type { FleetHealth, ReviewSession } from "@shadowrunr/contract";
import { tsOf, type Derived, type SayLine } from "./derive";
import { modName, type GateMarkId, type ModGroup, type ShowMarkId, type ShownKind } from "./names";

export interface Bucket {
  /** Row index. */
  readonly i: number;
  /** Stream seconds. */
  readonly from: number;
  readonly to: number;
  readonly says: readonly SayLine[];
  readonly chat: number;
  /** In stream order, one per event. */
  readonly show: readonly ShowMarkId[];
  readonly gates: Readonly<Partial<Record<Exclude<GateMarkId, "judge_fail">, number>>>;
  /** Lines in the row the judge cited in a failed rule. */
  readonly fails: number;
}

interface Building {
  i: number;
  from: number;
  to: number;
  says: SayLine[];
  chat: number;
  show: ShowMarkId[];
  gates: Partial<Record<Exclude<GateMarkId, "judge_fail">, number>>;
  fails: number;
}

const OP_MARK: Readonly<Partial<Record<string, ShowMarkId>>> = {
  start_poll: "poll", close_poll: "poll_closed", reveal_hex: "reveal", summon_foil: "foil_in", dismiss_foil: "foil_out", show_card: "card", sign_off: "sign_off",
};
const HEALTH_MARK: Readonly<Record<FleetHealth, ShowMarkId>> = { down: "engine_down", degraded: "engine_degraded", healthy: "engine_back" };
const GATE_OF: Readonly<Partial<Record<ModGroup, Exclude<GateMarkId, "judge_fail">>>> = { "Safety & care": "care", Persona: "persona", Accuracy: "alleged" };

/** The row a stream second falls in. */
export const rowOf = (t: number, size: number, count: number): number => Math.max(0, Math.min(count - 1, Math.floor(t / size)));

export function buckets(rs: ReviewSession, d: Derived, size: number): Bucket[] {
  const n = Math.max(1, Math.ceil((d.lengthS + 0.01) / size));
  const B: Building[] = Array.from({ length: n }, (_, i) => ({ i, from: i * size, to: (i + 1) * size, says: [], chat: 0, show: [], gates: {}, fails: 0 }));
  const at = (t: number): Building => B[rowOf(t, size, n)] as Building;
  // Engine health marks only a change: the first healthy record is not "Engine back".
  let health: FleetHealth = "healthy";
  for (const e of rs.events) {
    const b = at(tsOf(e));
    if (e.type === "chat") b.chat++;
    else if (e.type === "op") {
      const m = OP_MARK[e.op.op];
      if (m) b.show.push(m);
    } else if (e.type === "handover") b.show.push("handover");
    else if (e.type === "health") {
      if (e.state !== health) b.show.push(HEALTH_MARK[e.state]);
      health = e.state;
    } else if (e.type === "moderation") {
      const g = GATE_OF[modName(e.stage, e.rule, e.action).group];
      if (g) b.gates[g] = (b.gates[g] ?? 0) + 1;
    }
  }
  for (const s of d.says) {
    const b = at(s.t);
    b.says.push(s);
    if (d.failLines.has(s.line)) b.fails++;
  }
  return B;
}

/** A row's visible lines under the kind filter. */
export const visibleSays = (b: Bucket, shown: ReadonlySet<ShownKind>): readonly SayLine[] => b.says.filter((s) => shown.has(s.kind));

/** A row with nothing to show: no line of the shown kinds, no show mark, no gate, no judge fail. */
export const isQuiet = (b: Bucket, shown: ReadonlySet<ShownKind>): boolean =>
  b.show.length === 0 && b.fails === 0 && Object.keys(b.gates).length === 0 && !b.says.some((s) => shown.has(s.kind));

export type Item = { readonly kind: "row"; readonly b: Bucket } | { readonly kind: "fold"; readonly from: number; readonly to: number; readonly rows: number };

/** The rows as the timeline lists them: two or more quiet rows in a run fold into one item. */
export function fold(B: readonly Bucket[], shown: ReadonlySet<ShownKind>): Item[] {
  const out: Item[] = [];
  let run: Bucket[] = [];
  const flush = () => {
    const [first] = run;
    const last = run[run.length - 1];
    if (run.length > 1 && first && last) out.push({ kind: "fold", from: first.from, to: last.to, rows: run.length });
    else if (first) out.push({ kind: "row", b: first });
    run = [];
  };
  for (const b of B) {
    if (isQuiet(b, shown)) {
      run.push(b);
      continue;
    }
    flush();
    out.push({ kind: "row", b });
  }
  flush();
  return out;
}

/** Show marks merged by name in first-seen order: "Reveal ×2". */
export function mergeMarks(show: readonly ShowMarkId[]): { readonly id: ShowMarkId; readonly n: number }[] {
  const counts = new Map<ShowMarkId, number>();
  for (const m of show) counts.set(m, (counts.get(m) ?? 0) + 1);
  return [...counts].map(([id, n]) => ({ id, n }));
}

/** The line's row, or undefined for a line outside the transcript. */
export function rowOfLine(rs: ReviewSession, line: number, size: number, count: number): number | undefined {
  const e = rs.events[line - 1];
  return e ? rowOf(tsOf(e), size, count) : undefined;
}
