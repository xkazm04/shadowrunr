// The clip factory's view model: status order and labels, chip tones, the owner's act rules
// and the window bar's geometry. Pure, so the views stay thin and the rules are testable.
import type { ClipCandidate, ClipStatus, ClipWeights, GateState } from "@shadowrunr/contract";
import type { Tone } from "../../ui/Chip";

/** The gate funnel, in publish order; `rejected` is the side exit. */
export const STATUS_ORDER: readonly ClipStatus[] = ["rendered_draft", "pending_owner_gate", "approved_for_upload", "published", "rejected"];

export const STATUS_LABEL: Readonly<Record<ClipStatus, string>> = {
  rendered_draft: "Rendered draft",
  pending_owner_gate: "Owner gate",
  approved_for_upload: "Approved",
  published: "Published",
  rejected: "Rejected",
};

export const STATUS_TONE: Readonly<Record<ClipStatus, Tone>> = {
  rendered_draft: "muted",
  pending_owner_gate: "caution",
  approved_for_upload: "ok",
  published: "signal",
  rejected: "fail",
};

export const GATE_TONE: Readonly<Record<GateState, Tone>> = {
  pass: "ok",
  fail: "fail",
  unmeasured: "unevaluable",
  not_applicable: "muted",
};

/** The label every ranking carries until an editor's picks score the weights. */
export const WEIGHTS_LABEL = "weights unmeasured";

/** `m:ss` on a stream clock. */
export function clock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export const secs = (ms: number): string => `${(ms / 1000).toFixed(1)} s`;
export const pct = (w: number): string => `${Math.round(w * 100)}%`;
export const score = (s: number): string => s.toFixed(3);

export const SIGNALS: readonly (keyof ClipWeights)[] = ["marker", "chat", "audio", "hook"];

/** Why the owner cannot approve now, or `null` when approval may be signed. */
export function approveBlock(c: ClipCandidate): string | null {
  if (c.status !== "pending_owner_gate") return `The clip is ${STATUS_LABEL[c.status].toLowerCase()}, not waiting at the owner gate.`;
  if (c.render === undefined) return "There is no render to approve yet.";
  if (c.window.needs_reauthoring) return "The setup cannot fit the band: re-author the moment, do not approve the cut.";
  const failed = c.gate.checks.filter((k) => k.state === "fail").map((k) => k.id);
  if (failed.length > 0) return `Held by ${failed.join(", ")}: a failed check holds whatever the score, and no override applies.`;
  return null;
}

/** The checks an approval would override (each needs the recorded reason). */
export const unmeasuredChecks = (c: ClipCandidate): string[] => c.gate.checks.filter((k) => k.state === "unmeasured").map((k) => k.id);

/** Whether a dry-run publish may be asked for. */
export const canPublish = (c: ClipCandidate): boolean => c.status === "approved_for_upload";

/** Whether the draft may be re-rendered (only before the owner acts). */
export const canRender = (c: ClipCandidate): boolean => c.status === "rendered_draft" || c.status === "pending_owner_gate";

/** The window bar: the pre-roll and payoff as shares of the band's maximum, plus the band. */
export function windowBar(c: ClipCandidate): { preroll: number; payoff: number; min: number; over: boolean } {
  const w = c.window;
  const maxMs = Math.max(1, w.band_s[1] * 1000);
  const total = w.spoken_end_ms - w.spoken_start_ms;
  const clamp = (x: number) => Math.max(0, Math.min(1, x));
  const preroll = clamp(w.preroll_ms / maxMs);
  return {
    preroll,
    payoff: Math.max(0, clamp(total / maxMs) - preroll),
    min: clamp((w.band_s[0] * 1000) / maxMs),
    over: total > maxMs,
  };
}
