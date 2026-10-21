// What the plan composer is looking at, derived from the daemon's live plan (`plan.current`)
// and the operator's unsent draft. The daemon owns the sealed plan; the console never keeps a
// copy of it, so a reload shows what Malakor will actually receive and a click can never
// silently replace a live plan with an empty one.
import type { NextStreamPlan } from "@shadowrunr/contract";

/** What `plan.current` answered: a plan, none (`not_found`), a failed read, or not yet. */
export type LiveRead = NextStreamPlan | undefined | { readonly failed: string } | "loading";

export type ComposerState =
  /** The read is in flight: sealing waits, so it cannot race a live plan it has not seen. */
  | { readonly kind: "loading" }
  /** The daemon holds no plan yet. */
  | { readonly kind: "none" }
  /** The live plan was sealed from this report; `same` when the draft chooses exactly its options. */
  | { readonly kind: "live-here"; readonly plan: NextStreamPlan; readonly same: boolean; readonly added: readonly string[]; readonly removed: readonly string[] }
  /** The live plan was sealed from another report (`from` undefined: a plan that predates naming it). */
  | { readonly kind: "live-elsewhere"; readonly plan: NextStreamPlan; readonly from: string | undefined }
  /** The live plan could not be read; sealing stays allowed, with a caution. */
  | { readonly kind: "unknown"; readonly reason: string };

const isPlan = (live: LiveRead): live is NextStreamPlan => typeof live === "object" && !("failed" in live);

export function composerState(live: LiveRead, reportId: string, draft: readonly string[]): ComposerState {
  if (live === "loading") return { kind: "loading" };
  if (live === undefined) return { kind: "none" };
  if (!isPlan(live)) return { kind: "unknown", reason: live.failed };
  if (live.report_id !== reportId) return { kind: "live-elsewhere", plan: live, from: live.report_id };
  const sealed = live.option_ids ?? [];
  const added = draft.filter((id) => !sealed.includes(id));
  const removed = sealed.filter((id) => !draft.includes(id));
  return { kind: "live-here", plan: live, same: added.length === 0 && removed.length === 0, added, removed };
}

/**
 * The draft the composer shows: the operator's own once they touched the selection (`chosen`
 * defined, even empty), else the live plan's options when it was sealed from this report.
 */
export function draftOf(live: LiveRead, reportId: string, chosen: readonly string[] | undefined): readonly string[] {
  if (chosen !== undefined) return chosen;
  return isPlan(live) && live.report_id === reportId ? (live.option_ids ?? []) : [];
}

/** The seal button's label for a state. */
export function sealLabel(s: ComposerState): string {
  switch (s.kind) {
    case "loading":
      return "Reading the live plan…";
    case "none":
    case "unknown":
      return "Seal this plan";
    case "live-here":
      return s.same ? "This is the live plan" : "Replace the live plan";
    case "live-elsewhere":
      return "Replace the live plan";
  }
}

/** Whether a click on the seal button may call `plan.choose`. */
export const canSeal = (s: ComposerState): boolean => s.kind !== "loading" && !(s.kind === "live-here" && s.same);

/** One sentence on the draft against the live plan, or undefined when there is nothing to say. */
export function liveLine(s: ComposerState): string | undefined {
  switch (s.kind) {
    case "none":
      return "No live plan yet: sealing makes the first one.";
    case "live-here": {
      if (s.same) return "The live plan was sealed from this report with exactly these options.";
      const parts = [s.added.length ? `adds ${s.added.join(", ")}` : "", s.removed.length ? `drops ${s.removed.join(", ")}` : ""].filter(Boolean);
      return `Differs from the live plan: ${parts.join("; ")}.`;
    }
    case "live-elsewhere":
      return s.from === undefined ? "The live plan was sealed before plans named their report." : `The live plan was sealed from report ${s.from}.`;
    default:
      return undefined;
  }
}

/** Reads a `plan.current` query into a `LiveRead`: `not_found` is no plan, not a failure. */
export function liveRead(q: { readonly data: NextStreamPlan | undefined; readonly error: { readonly code: string } | undefined; readonly errorView: { readonly title: string } | undefined }): LiveRead {
  if (q.data !== undefined) return q.data;
  if (q.error?.code === "not_found") return undefined;
  if (q.errorView !== undefined) return { failed: q.errorView.title };
  return "loading";
}
