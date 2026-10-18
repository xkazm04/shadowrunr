import type { CheckState, JudgeVerdict, StreamEvent } from "../contract";
import { parseRef } from "./format";

export type Annotation =
  | { kind: "rule"; rule: string; state: CheckState }
  | { kind: "quality"; dimension: string; value: boolean; quote: string };

/** Group a verdict's rule evidence and quality facts by the 1-based transcript line they cite. */
export function annotationsByLine(verdict: JudgeVerdict | undefined, runId: string): Map<number, Annotation[]> {
  const m = new Map<number, Annotation[]>();
  if (!verdict) return m;
  const push = (line: number, a: Annotation) => {
    const xs = m.get(line) ?? [];
    xs.push(a);
    m.set(line, xs);
  };
  for (const r of verdict.rules) {
    for (const ref of r.evidence ?? []) {
      const p = parseRef(ref);
      if (p && p.runId === runId) push(p.line, { kind: "rule", rule: r.rule, state: r.state });
    }
  }
  for (const q of verdict.quality) {
    const p = parseRef(q.turn_ref);
    if (p && p.runId === runId) push(p.line, { kind: "quality", dimension: q.dimension, value: q.value, quote: q.quote });
  }
  return m;
}

export interface RuleSummary {
  pass: number;
  fail: number;
  not_provoked: number;
  not_evaluable: number;
}

export function summarizeRules(v: JudgeVerdict | undefined): RuleSummary {
  const s: RuleSummary = { pass: 0, fail: 0, not_provoked: 0, not_evaluable: 0 };
  for (const r of v?.rules ?? []) s[r.state]++;
  return s;
}

/** Event colour group (maps to --ev-<group> tokens). */
export function eventGroup(ev: StreamEvent): string {
  switch (ev.type) {
    case "say":
      return ev.speaker === "foil" ? "foil" : "say";
    case "chat":
      return "chat";
    case "picked":
      return "picked";
    case "op":
      return "op";
    case "vote":
      return "vote";
    case "moderation":
      return "moderation";
    case "job":
      return "job";
    default:
      return "lifecycle";
  }
}

export const EVENT_FILTERS = {
  all: () => true,
  speech: (e: StreamEvent) => e.type === "say",
  chat: (e: StreamEvent) => e.type === "chat" || e.type === "picked",
  ops: (e: StreamEvent) => e.type === "op" || e.type === "vote" || e.type === "job",
  flags: (e: StreamEvent) => e.type === "moderation",
} as const;

export type EventFilter = keyof typeof EVENT_FILTERS;
