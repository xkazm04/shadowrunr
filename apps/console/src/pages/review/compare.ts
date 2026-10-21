// Two sandbox sessions side by side, as a pure diff: A is the session before the change, B the one
// after. Rules newly failing / fixed / still failing, gate checks that flipped, the page's two
// timing figures against their failure lines, line-mix share shifts and quality moves.
// Unmeasured is not a pass: an unjudged side fixes nothing and breaks nothing.
import type { ReviewSession, ReviewSessionRow, SandboxCheck, SandboxCheckState } from "@shadowrunr/contract";
import type { Tone } from "../../ui/Chip";
import { lineOf } from "./derive";
import { dimRows, type DimRow } from "./Judge";
import { CHECK_NAME, CHECK_STATE_NAME, dimName, human, KINDS, REPLY_LIMIT_S, ruleName, SILENCE_LIMIT_S, type ShownKind } from "./names";

/** A line a judged rule points at, in the session it was judged in. */
export interface RuleRef {
  readonly session: string;
  readonly line: number;
}

export interface RuleRow {
  readonly rule: string;
  readonly name: string;
  readonly refs: readonly RuleRef[];
}

export interface RuleDiff {
  readonly aJudged: boolean;
  readonly bJudged: boolean;
  /** Failed in B, not failed in A. */
  readonly newly: readonly RuleRow[];
  /** Failed in A, checked and passed in B. */
  readonly fixed: readonly RuleRow[];
  /** Failed in both. */
  readonly still: readonly RuleRow[];
  /** Failed in A, not checked in B: not a fix. */
  readonly unchecked: readonly RuleRow[];
}

function failsOf(rs: ReviewSession): Map<string, RuleRef[]> {
  const out = new Map<string, RuleRef[]>();
  for (const r of rs.verdict?.rules ?? []) {
    if (r.state !== "fail") continue;
    const refs = (r.evidence ?? []).map(lineOf).filter((l): l is number => l !== undefined).map((line) => ({ session: rs.row.session_id, line }));
    out.set(r.rule, refs);
  }
  return out;
}

const byName = (a: RuleRow, b: RuleRow) => a.name.localeCompare(b.name);

export function ruleDiff(a: ReviewSession, b: ReviewSession): RuleDiff {
  const aJudged = a.verdict !== undefined;
  const bJudged = b.verdict !== undefined;
  if (!aJudged || !bJudged) return { aJudged, bJudged, newly: [], fixed: [], still: [], unchecked: [] };
  const fa = failsOf(a);
  const fb = failsOf(b);
  const checkedB = new Set((b.verdict?.rules ?? []).map((r) => r.rule));
  const row = (rule: string, refs: readonly RuleRef[]): RuleRow => ({ rule, name: ruleName(rule), refs });
  const newly = [...fb].filter(([r]) => !fa.has(r)).map(([r, refs]) => row(r, refs));
  const still = [...fb].filter(([r]) => fa.has(r)).map(([r, refs]) => row(r, [...(fa.get(r) ?? []), ...refs]));
  const gone = [...fa].filter(([r]) => !fb.has(r));
  const fixed = gone.filter(([r]) => checkedB.has(r)).map(([r, refs]) => row(r, refs));
  const unchecked = gone.filter(([r]) => !checkedB.has(r)).map(([r, refs]) => row(r, refs));
  return { aJudged, bJudged, newly: newly.sort(byName), fixed: fixed.sort(byName), still: still.sort(byName), unchecked: unchecked.sort(byName) };
}

export interface CheckRow {
  readonly id: string;
  readonly label: string;
  readonly tone: Tone;
}

/** The gate checks whose state differs, named `Say gap: passed -> failed`; the rest are counted. */
export function checkDiff(a: readonly SandboxCheck[], b: readonly SandboxCheck[]): { changed: CheckRow[]; unchanged: number } {
  const sa = new Map(a.map((c) => [c.id, c.state]));
  const sb = new Map(b.map((c) => [c.id, c.state]));
  const ids = [...new Set([...a.map((c) => c.id), ...b.map((c) => c.id)])];
  const changed: CheckRow[] = [];
  let unchanged = 0;
  for (const id of ids) {
    const from: SandboxCheckState = sa.get(id) ?? "unmeasured";
    const to: SandboxCheckState = sb.get(id) ?? "unmeasured";
    if (from === to) {
      unchanged++;
      continue;
    }
    const tone: Tone = to === "fail" ? "fail" : to === "pass" ? "ok" : "unevaluable";
    changed.push({ id, label: `${CHECK_NAME[id] ?? human(id)}: ${CHECK_STATE_NAME[from]} -> ${CHECK_STATE_NAME[to]}`, tone });
  }
  return { changed, unchanged };
}

/** The page's timing figures for one session, in seconds. */
export interface Figures {
  /** Reply wait p95; absent when no line answered chat. */
  readonly replyP95?: number;
  readonly silence: number;
}

export interface FigureRow {
  readonly id: "reply" | "silence";
  readonly label: string;
  readonly a: string;
  readonly b: string;
  /** Signed change in seconds, `–` when a side has no figure. */
  readonly delta: string;
  /** `fail` when B is over the limit, `ok` when B came back under it, else neutral. */
  readonly tone: "fail" | "ok" | "neutral";
  readonly limit: number;
}

const sec = (s: number | undefined) => (s === undefined ? "–" : `${s.toFixed(1)} s`);
const signed = (n: number, unit: string) => {
  const r = Math.round(n * 10) / 10;
  return `${r > 0 ? "+" : r < 0 ? "-" : "±"}${Math.abs(r).toFixed(1)} ${unit}`;
};

export function figureDiff(a: Figures, b: Figures): FigureRow[] {
  const row = (id: FigureRow["id"], label: string, x: number | undefined, y: number | undefined, limit: number): FigureRow => ({
    id,
    label,
    a: sec(x),
    b: sec(y),
    delta: x === undefined || y === undefined ? "–" : signed(y - x, "s"),
    tone: y !== undefined && y > limit ? "fail" : x !== undefined && x > limit && y !== undefined ? "ok" : "neutral",
    limit,
  });
  return [row("reply", "Reply wait p95", a.replyP95, b.replyP95, REPLY_LIMIT_S), row("silence", "Longest silence", a.silence, b.silence, SILENCE_LIMIT_S)];
}

export interface MixRow {
  readonly kind: ShownKind;
  /** `Roast -18 pts`. */
  readonly label: string;
  readonly a: number;
  readonly b: number;
  readonly pts: number;
}

/** Share shifts per line kind, largest move first; kinds that held their share are counted. */
export function mixDiff(
  a: { readonly mix: Readonly<Partial<Record<ShownKind, number>>>; readonly lines: number },
  b: { readonly mix: Readonly<Partial<Record<ShownKind, number>>>; readonly lines: number },
): { rows: MixRow[]; unchanged: number } {
  const rows: MixRow[] = [];
  let unchanged = 0;
  for (const [kind, name] of KINDS) {
    const na = a.mix[kind] ?? 0;
    const nb = b.mix[kind] ?? 0;
    if (na === 0 && nb === 0) continue;
    const sa = a.lines ? na / a.lines : 0;
    const sb = b.lines ? nb / b.lines : 0;
    const pts = Math.round((sb - sa) * 100);
    if (pts === 0) {
      unchanged++;
      continue;
    }
    rows.push({ kind, label: `${name} ${pts > 0 ? "+" : "-"}${Math.abs(pts)} pts`, a: sa, b: sb, pts });
  }
  rows.sort((x, y) => Math.abs(y.pts) - Math.abs(x.pts));
  return { rows, unchanged };
}

export interface DimDiffRow {
  readonly dim: string;
  readonly name: string;
  readonly a?: DimRow;
  readonly b?: DimRow;
  /** B's yes share minus A's; 0 when a side has no facts. */
  readonly change: number;
}

/** Quality per dimension in both sessions (dimRows of each), the largest move first. */
export function dimDiff(a: ReviewSession, b: ReviewSession): DimDiffRow[] {
  const cat = (rs: ReviewSession) => new Map(rs.catalog.map((c) => [c.id, c]));
  const ra = new Map(dimRows(a, cat(a)).map((r) => [r.dim, r]));
  const rb = new Map(dimRows(b, cat(b)).map((r) => [r.dim, r]));
  const rate = (r: DimRow | undefined) => (r && r.total ? r.yes / r.total : undefined);
  return [...new Set([...ra.keys(), ...rb.keys()])]
    .map((dim) => {
      const x = ra.get(dim);
      const y = rb.get(dim);
      const px = rate(x);
      const py = rate(y);
      return { dim, name: dimName(dim), ...(x ? { a: x } : {}), ...(y ? { b: y } : {}), change: px === undefined || py === undefined ? 0 : py - px };
    })
    .sort((x, y) => Math.abs(y.change) - Math.abs(x.change) || x.name.localeCompare(y.name));
}

/** The session started just before `id` (by started_ms), if any. */
export function previousOf(rows: readonly ReviewSessionRow[], id: string): ReviewSessionRow | undefined {
  const me = rows.find((r) => r.session_id === id);
  if (!me) return undefined;
  let best: ReviewSessionRow | undefined;
  for (const r of rows) if (r.started_ms < me.started_ms && (!best || r.started_ms > best.started_ms)) best = r;
  return best;
}
