// Test-only generators of contract-shaped Reconcile & Plan data (imported by *.test.tsx only,
// never by the page, so nothing here reaches a bundle). `scale` multiplies the pilot's shape
// toward the 10x fixture: more segments' evidence, more incidents, more directives.
import type { Directive, NextStreamPlan, PlanOption, ReconcileReport, RuleIncident } from "@shadowrunr/contract";

const bound = (lo: number, hi: number) => ({ lo, hi, method: "wilson" as const });

export function genReport(opts: { id?: string; held?: number; failed?: number; incidents?: number } = {}): ReconcileReport {
  const held = opts.held ?? 14;
  const failed = opts.failed ?? 13;
  const evidence = (n: number, what: string) =>
    Array.from({ length: n }, (_, i) => ({ claim: `${what} claim ${i}`, refs: [`theatrical.haiku.SEG-01.a3.s1#${i + 1}`, `pilot-1/theatrical.sonnet.SEG-01.a30.s1#${i + 5}`] }));
  const half = (n: number) => [Math.ceil(n / 2), Math.floor(n / 2)] as const;
  const [h1, h2] = half(held);
  const [f1, f2] = half(failed);
  const red: RuleIncident[] = Array.from({ length: opts.incidents ?? 417 }, (_, i) =>
    i % 3 === 0
      ? { run_id: `run-${i % 40}`, rule: "situation:fake-lord-dispatch", source: "verdict", refs: [`run-${i % 40}#${i}`] }
      : i % 3 === 1
        ? { run_id: `run-${i % 40}`, rule: "rate_cap", source: "moderation", stage: "input", action: "drop" }
        : { run_id: `run-${i % 40}`, rule: `alleged:hex-${i % 7}`, source: "moderation", stage: "output", action: "qualified" },
  );
  return {
    report_id: opts.id ?? "rec-1792284840627",
    created_ms: 1792284840627,
    source: { tournament_id: "pilot-d1ef98d3a6" },
    summary: "My Lord, Malakor's core held up in all four runs.",
    segments: [
      { segment: "SEG-01", runs: 4, ses_mean: 0.62, what_worked: evidence(h1, "SEG-01 held"), what_failed: evidence(f1, "SEG-01 failed") },
      { segment: "SEG-02A", runs: 4, what_worked: evidence(h2, "SEG-02A held"), what_failed: evidence(f2, "SEG-02A failed") },
    ],
    tone: [
      {
        tone_variant: "theatrical",
        runs: 4,
        quality_rates: {
          humor_landed: { rate: 0.61, n: 41, bound: bound(0.46, 0.74) },
          edge_without_cruelty: { rate: 1, n: 11, bound: { lo: 0.73, hi: 1, method: "rule_of_three" } },
          listens_to_votes: { rate: 1, n: 9, bound: { lo: 0.67, hi: 1, method: "rule_of_three" } },
        },
        rule_failures: [{ rule: "role:no-repeat", fails: 2, n: 4, refs: ["run-1#12"] }],
      },
    ],
    ranks: {
      by_rank: [
        { rank: "thrall", viewers: 53, messages: 597, picked: 294, answered: 287, pick_rate: { rate: 0.49, n: 597, bound: bound(0.45, 0.53) } },
        { rank: "dread_council", viewers: 2, messages: 12, picked: 3, answered: 2 },
      ],
    },
    red_lines: red,
    latency: {
      ledger_rows: 519,
      errors: 5,
      fallbacks: 0,
      say_n: 277,
      by_role: [
        { role: "core_thread", calls: 277, errors: 0, fallbacks: 0 },
        { role: "judge", calls: 4, errors: 4, fallbacks: 0 },
      ],
    },
  };
}

export const CONSTITUTION_DIFF = "--- a/constitution.md\n+++ b/constitution.md\n@@ -1,2 +1,3 @@\n # Constitution\n+He may lie about being an AI.\n";
export const BIBLE_DIFF = "--- a/bible.md\n+++ b/bible.md\n@@ -135,2 +135,3 @@\n chat picks winners.\n+When chat goes quiet he deals a fresh premise.\n-He waits.\n";

export function genProposals(n = 6): PlanOption[] {
  const kinds = ["character_change", "segment_change", "topic", "experiment"] as const;
  return Array.from({ length: n }, (_, i) => ({
    option_id: `opt-${i + 1}`,
    kind: kinds[i % kinds.length] ?? "topic",
    title: `Option ${i + 1} title`,
    rationale: `Why option ${i + 1}.`,
    evidence: [{ claim: "seen", refs: [`run-${i}#${i + 3}`] }],
    ...(i === 0 ? { diff: BIBLE_DIFF } : i === 1 ? { diff: CONSTITUTION_DIFF } : {}),
  }));
}

export function genDirectives(n: number): Directive[] {
  return Array.from({ length: n }, (_, i) => ({
    id: `dir-${i}`,
    kind: i % 2 === 0 ? "prevent" : "try",
    text: `Directive number ${i}`,
    created_ms: 1792284840627 + i,
    active: i % 5 !== 0,
    ...(i % 3 === 0 ? { scope: "SEG-01" as const } : {}),
  }));
}

export function genPlan(optionIds: readonly string[]): NextStreamPlan {
  return {
    plan_id: "plan-1",
    created_ms: 1792284900000,
    lord_dispatch: "Per my last decree: deal a fresh premise when the room goes quiet.",
    option_ids: [...optionIds],
    report_id: "rec-1792284840627",
    segments: [
      { segment: "SEG-01", minutes: 15, notes: "open loud" },
      { segment: "SEG-02A", minutes: 25, notes: "only with a case file" },
    ],
    topics: ["quiet rooms"],
    directives: genDirectives(2),
  };
}
