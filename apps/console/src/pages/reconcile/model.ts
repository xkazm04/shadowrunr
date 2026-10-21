// Pure derivations for Reconcile & Plan: the triptych counts, incident groups, the caution
// slip, evidence refs, sources and diffs (ref parsing is kit/events). No React, no DOM, so every rule is unit-tested.
import type { DirectiveOutcome, DirectiveVerdict, Evidence, PlanOption, ReconcileReport, Source } from "@shadowrunr/contract";
import type { Tone } from "../../ui/Chip";

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

const groupKey = (rule: string): string => (rule.split(":")[0] === "alleged" ? "alleged" : rule);

/**
 * Incidents per rule, most frequent first; every `alleged:<hex>` folds into `alleged`. From the
 * report's `red_line_counts` (every incident, even when the wire cut the list to a page), else
 * from the incidents listed.
 */
export function incidentGroups(report: Pick<ReconcileReport, "red_lines" | "red_line_counts">): IncidentGroup[] {
  const counts = new Map<string, number>();
  if (report.red_line_counts && report.red_line_counts.length > 0) {
    for (const c of report.red_line_counts) counts.set(groupKey(c.rule), (counts.get(groupKey(c.rule)) ?? 0) + c.count);
  } else {
    for (const i of report.red_lines) counts.set(groupKey(i.rule), (counts.get(groupKey(i.rule)) ?? 0) + 1);
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

export function dateTime(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** Each directive's judged outcome in a report, by directive id; empty for a report without them. */
export function outcomeIndex(report: Pick<ReconcileReport, "directives"> | undefined): ReadonlyMap<string, DirectiveOutcome> {
  return new Map((report?.directives ?? []).map((o) => [o.id, o]));
}

/** Worst first: what the character broke, then ignored, never measured, attempted, held. */
export const VERDICT_RANK: Readonly<Record<DirectiveVerdict, number>> = { broken: 0, ignored: 1, unmeasured: 2, attempted: 3, held: 4 };

/** A sorted copy, worst first (stable within a verdict). */
export function outcomeOrder(outcomes: readonly DirectiveOutcome[]): DirectiveOutcome[] {
  return [...outcomes].sort((a, b) => VERDICT_RANK[a.verdict] - VERDICT_RANK[b.verdict]);
}

export const VERDICT_TONE: Readonly<Record<DirectiveVerdict, Tone>> = { broken: "fail", ignored: "caution", unmeasured: "muted", attempted: "signal", held: "ok" };

export const VERDICT_LABEL: Readonly<Record<DirectiveVerdict, string>> = { broken: "Broken", ignored: "Ignored", unmeasured: "Unmeasured", attempted: "Attempted", held: "Held" };

/** `k of n` judged runs that held (prevent) or attempted (try); undefined when none was judged. */
export function outcomeCount(o: DirectiveOutcome): { k: number; n: number } | undefined {
  return o.rate ? { k: Math.round(o.rate.rate * o.rate.n), n: o.rate.n } : undefined;
}

/** The same small-sample rule as the caution slip: a rate on fewer than SMALL_SAMPLE_N judged runs. */
export const smallSample = (o: DirectiveOutcome): boolean => o.rate !== undefined && o.rate.n < SMALL_SAMPLE_N;
