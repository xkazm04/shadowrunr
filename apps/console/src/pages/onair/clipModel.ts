// The clip factory's view model: status order and labels, chip tones, the wording of the
// daemon's owner-act blocks and the window bar's geometry. Pure, so the views stay thin and the rules are testable.
import type { ClipActBlock, ClipCandidate, ClipFeedbackTag, ClipFootage, ClipStatus, ClipWeights, GateState } from "@shadowrunr/contract";
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

/** Why the daemon refuses an act now (`acts`, derived from the contract's one owner-act rule). */
export function blockText(b: ClipActBlock): string {
  if (b.kind === "status") return `The clip's status is ${STATUS_LABEL[b.status]}.`;
  if (b.kind === "no_render") return "There is no render to approve yet.";
  if (b.kind === "needs_reauthoring") return "The setup cannot fit the band: re-author the moment, do not approve the cut.";
  return `Held by ${b.checks.join(", ")}: a failed check holds whatever the score, and no override applies.`;
}

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

/** The watched-feedback tags, in the order the form shows them. */
export const FEEDBACK_TAGS: readonly (readonly [ClipFeedbackTag, string])[] = [
  ["landed", "Landed"],
  ["flat", "Flat"],
  ["voice_off_sync", "Voice off sync"],
  ["face_wrong", "Face wrong"],
  ["caption_wrong", "Caption wrong"],
  ["cut_too_early", "Cut too early"],
  ["cut_too_late", "Cut too late"],
  ["boring", "Boring"],
];

export const FOOTAGE_LABEL: Readonly<Record<ClipFootage, string>> = {
  recording: "Cut from the recording",
  redrawn: "Re-drawn (no recording)",
};

/** A render without a footage field predates the recorded cuts: it was re-drawn. */
export const footageOf = (c: ClipCandidate): ClipFootage => c.render?.footage ?? "redrawn";

export type Aspect = "16x9" | "9x16";
export const ASPECT_LABEL: Readonly<Record<Aspect, string>> = { "16x9": "16:9 broadcast", "9x16": "9:16 short" };

/** The video file for an aspect: a recorded cut, or the re-drawn vertical draft. */
export function cutFile(c: ClipCandidate, aspect: Aspect): string | undefined {
  const files = c.render?.files ?? [];
  const kind = aspect === "16x9" ? "cut_16x9" : "cut_9x16";
  return (files.find((f) => f.kind === kind) ?? (aspect === "9x16" ? files.find((f) => f.kind === "video") : undefined))?.path;
}

/** The aspects this clip has a video for. */
export const aspects = (c: ClipCandidate): Aspect[] => (["9x16", "16x9"] as const).filter((a) => cutFile(c, a) !== undefined);

export interface TimedLine {
  /** Seconds into the cut. */
  readonly at: number;
  /** Stream-clock ms. */
  readonly streamMs: number;
  readonly speaker: string;
  readonly text: string;
  readonly unheard: boolean;
}

/** The cut's lines on its own clock and the stream's (the voice's real timing when it has one). */
export function timedLines(c: ClipCandidate): TimedLine[] {
  return [...c.captions]
    .sort((a, b) => a.start_ms - b.start_ms)
    .map((l) => ({ at: l.start_ms / 1000, streamMs: c.window.spoken_start_ms + l.start_ms, speaker: l.speaker, text: l.text, unheard: l.unvoiced === true }));
}

/** A clip file on the daemon. */
export const clipMediaUrl = (daemonUrl: string, id: string, file: string): string => `${daemonUrl.replace(/\/+$/, "")}/media/clips/${encodeURIComponent(id)}/${encodeURIComponent(file)}`;

/** A caption's speaker as the page names it (the stage's own titles). */
export function speakerName(speaker: string): string {
  if (speaker === "malakor") return "Malakor";
  if (speaker === "viewer") return "A minion asks";
  const s = speaker.replace(/^the-/, "").replace(/-/g, " ");
  return s.charAt(0).toUpperCase() + s.slice(1);
}
