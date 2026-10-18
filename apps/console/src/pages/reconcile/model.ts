// Pure derivations for Reconcile & Plan: the triptych counts, incident groups, the caution
// slip, evidence refs, sources and diffs. No React, no DOM, so every rule is unit-tested.
import type { Evidence, PlanOption, ReconcileReport, RuleIncident, Source } from "@shadowrunr/contract";

export interface Triptych {
  readonly held: number;
  readonly failed: number;
  readonly proposed: number;
}

/** The winner's counts: every `what_worked` item held, every `what_failed` item failed. */
export function triptych(report: ReconcileReport, proposals: readonly PlanOption[]): Triptych {
  let held = 0;
  let failed = 0;
  for (const s of report.segments) {
    held += s.what_worked?.length ?? 0;
    failed += s.what_failed?.length ?? 0;
  }
  return { held, failed, proposed: proposals.length };
}

/** Bars drawn per column. The numeral carries the true count; past this the bars stop growing. */
export const MAX_BARS = 60;

export const barCount = (n: number): number => Math.max(0, Math.min(MAX_BARS, Math.floor(n)));

export interface IncidentGroup {
  readonly rule: string;
  readonly n: number;
}

/** Incidents per rule, most frequent first; every `alleged:<hex>` folds into `alleged`. */
export function incidentGroups(incidents: readonly RuleIncident[]): IncidentGroup[] {
  const counts = new Map<string, number>();
  for (const i of incidents) {
    const key = i.rule.split(":")[0] === "alleged" ? "alleged" : i.rule;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts]
    .map(([rule, n]) => ({ rule, n }))
    .sort((a, b) => b.n - a.n || a.rule.localeCompare(b.rule));
}

/** A rate rests on a small sample below this many judged items. */
export const SMALL_SAMPLE_N = 30;

/**
 * The caution slip under the triptych, from the report itself: judge calls that errored, and
 * quality rates that rest on small samples. `undefined` when there is nothing to caution.
 */
export function caution(report: ReconcileReport): string | undefined {
  const parts: string[] = [];
  const judge = report.latency.by_role?.find((r) => r.role === "judge");
  if (judge && judge.errors > 0) {
    // `calls` counts successful calls; a judge call that errored and was retried counts in both.
    parts.push(
      judge.calls > 0 && judge.errors >= judge.calls
        ? `all ${judge.calls} judge ${judge.calls === 1 ? "call" : "calls"} errored`
        : `${judge.errors} judge ${judge.errors === 1 ? "call" : "calls"} errored`,
    );
  }
  const rates = report.tone.flatMap((t) => Object.values(t.quality_rates)).filter((r) => r !== undefined);
  const small = rates.filter((r) => r.n < SMALL_SAMPLE_N).length;
  if (rates.length > 0 && small * 2 > rates.length) parts.push("most rates rest on small samples");
  else if (small > 0) parts.push(`${small} of ${rates.length} rates rest on small samples`);
  if (parts.length === 0) return undefined;
  return `Handle with care: ${parts.join(" and ")}.`;
}

export interface Ref {
  readonly runId: string;
  readonly line: number;
}

/** `"<run_id>#<line>"` (run ids may be tournament-qualified with `/`). */
export function parseRef(ref: string): Ref | undefined {
  const at = ref.lastIndexOf("#");
  if (at <= 0) return undefined;
  const line = Number(ref.slice(at + 1));
  if (!Number.isInteger(line) || line < 0) return undefined;
  return { runId: ref.slice(0, at), line };
}

/** The short chip text for a ref: line first, then the run's tail. */
export function refLabel(ref: string): string {
  const p = parseRef(ref);
  if (!p) return ref;
  const run = p.runId.length > 28 ? `…${p.runId.slice(-26)}` : p.runId;
  return `L${p.line} · ${run}`;
}

export const evidenceRefs = (items: readonly Evidence[] | undefined): string[] => (items ?? []).flatMap((e) => e.refs ?? []);

export function sourceLabel(source: Source): string {
  if ("tournament_id" in source) return `tournament ${source.tournament_id}`;
  const n = source.run_ids.length;
  return `${n} ${n === 1 ? "run" : "runs"}`;
}

export type DiffLineKind = "add" | "del" | "hunk" | "file" | "ctx";

export interface DiffLine {
  readonly kind: DiffLineKind;
  readonly text: string;
}

export interface ParsedDiff {
  readonly files: readonly string[];
  /** True when any file the diff touches is the constitution (the verifier should drop it). */
  readonly touchesConstitution: boolean;
  readonly lines: readonly DiffLine[];
}

const stripSide = (p: string): string => p.replace(/^[ab]\//, "").replace(/\t.*$/, "").trim();

export function isConstitutionPath(path: string): boolean {
  return /(^|\/)constitution\.md$/i.test(path);
}

/** Classifies a unified diff's lines and names the files it touches. */
export function parseDiff(diff: string): ParsedDiff {
  const files = new Set<string>();
  const lines: DiffLine[] = diff.split(/\r?\n/).map((text) => {
    if (text.startsWith("+++ ") || text.startsWith("--- ")) {
      const path = stripSide(text.slice(4));
      if (path !== "/dev/null" && path.length > 0) files.add(path);
      return { kind: "file", text };
    }
    if (text.startsWith("@@")) return { kind: "hunk", text };
    if (text.startsWith("+")) return { kind: "add", text };
    if (text.startsWith("-")) return { kind: "del", text };
    return { kind: "ctx", text };
  });
  const list = [...files];
  return { files: list, touchesConstitution: list.some(isConstitutionPath), lines };
}

export const OPTION_KIND_LABEL: Readonly<Record<PlanOption["kind"], string>> = {
  character_change: "Character change",
  segment_change: "Segment change",
  topic: "Topic",
  experiment: "Experiment",
};

/** `situation:fake-lord-dispatch` → `fake lord dispatch`. */
export function ruleLabel(id: string): string {
  return id.replace(/^(red-line|role|situation|talk):/, "").replace(/[-_]+/g, " ");
}

export const pct = (x: number, digits = 0): string => `${(Math.min(1, Math.max(0, x)) * 100).toFixed(digits)}%`;

export function dateTime(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
