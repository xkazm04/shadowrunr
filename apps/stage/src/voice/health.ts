// The voice's health for the followed run: what the session saw go wrong, kept so the operator
// can see it while it happens instead of only in a post-run `voiced` check. A pure reducer over the
// events the session already handles (a skipped line with its reason, a line the stage's cap
// dropped, a frame the guard refused, a reconnect scheduled or begun, a run change) and the rows
// the controls' voice panel reads. The counts are per followed run, as the daemon's voice.jsonl
// keeps them, so the panel and the `voiced` check describe the same lines.

import type { VoiceSkip } from "@shadowrunr/contract";

import { SKIP_LABEL } from "./names";

export type SkipReason = VoiceSkip;

const REASONS: readonly SkipReason[] = ["late", "queue_full", "error"];

/** What the panel shows, at one moment. */
export interface VoiceHealth {
  /** Lines the daemon will not voice, by its reason (each line once, at its first reason). */
  skipped: Record<SkipReason, number>;
  /** Whole lines the stage's cap dropped to keep up. */
  stageDropped: number;
  /** Frames the guard refused, and the last one's reason. */
  badFrames: number;
  lastBad: string | null;
  /** Seconds of audio waiting behind the playhead (0.1 s steps). */
  behindS: number;
  /** The reconnect pending: its attempt number and how long until it opens; null when none waits. */
  reconnect: { attempt: number; nextInMs: number } | null;
}

export interface HealthState {
  runId: string | null;
  skipped: Record<SkipReason, number>;
  /** The skipped seqs already counted (a line can be skipped part by part). */
  skippedSeqs: readonly number[];
  stageDropped: number;
  badFrames: number;
  lastBad: string | null;
  retry: { attempt: number; dueAt: number } | null;
}

export type HealthEvent =
  | { type: "run"; runId: string | null }
  | { type: "skipped"; seq: number; reason: SkipReason }
  | { type: "stage_dropped" }
  | { type: "bad"; message: string }
  | { type: "retrying"; attempt: number; dueAt: number }
  | { type: "connecting" };

const zero = (): Record<SkipReason, number> => ({ late: 0, queue_full: 0, error: 0 });

export const EMPTY_HEALTH: HealthState = { runId: null, skipped: zero(), skippedSeqs: [], stageDropped: 0, badFrames: 0, lastBad: null, retry: null };

export function reduceHealth(s: HealthState, e: HealthEvent): HealthState {
  switch (e.type) {
    case "run":
      // a new run starts its counts afresh; the socket's reconnect is not the run's
      return e.runId === s.runId ? s : { ...EMPTY_HEALTH, skipped: zero(), runId: e.runId, retry: s.retry };
    case "skipped":
      if (s.skippedSeqs.includes(e.seq)) return s;
      return { ...s, skipped: { ...s.skipped, [e.reason]: s.skipped[e.reason] + 1 }, skippedSeqs: [...s.skippedSeqs, e.seq] };
    case "stage_dropped":
      return { ...s, stageDropped: s.stageDropped + 1 };
    case "bad":
      return { ...s, badFrames: s.badFrames + 1, lastBad: e.message };
    case "retrying":
      return { ...s, retry: { attempt: e.attempt, dueAt: e.dueAt } };
    case "connecting":
      return s.retry === null ? s : { ...s, retry: null };
  }
}

/** The state as the panel reads it at wall time `now`, with `aheadS` of audio waiting. */
export function healthSnapshot(s: HealthState, aheadS: number, now: number): VoiceHealth {
  return {
    skipped: { ...s.skipped },
    stageDropped: s.stageDropped,
    badFrames: s.badFrames,
    lastBad: s.lastBad,
    behindS: Math.round(aheadS * 10) / 10,
    reconnect: s.retry && { attempt: s.retry.attempt, nextInMs: Math.max(0, s.retry.dueAt - now) },
  };
}

/** Lines of the run that will not sound whole: the daemon's skips and the stage's drops. */
export function linesLost(h: VoiceHealth): number {
  return REASONS.reduce((n, r) => n + h.skipped[r], 0) + h.stageDropped;
}

const lines = (n: number) => `${n} ${n === 1 ? "line" : "lines"}`;

/** The panel's rows, worst news last; a quiet voice has only the audio waiting. */
export function healthRows(h: VoiceHealth): string[] {
  const rows = [`${h.behindS.toFixed(1)} s of audio waiting`];
  if (h.stageDropped > 0) rows.push(`${lines(h.stageDropped)} dropped by the stage to keep up`);
  const skipped = REASONS.reduce((n, r) => n + h.skipped[r], 0);
  if (skipped > 0) {
    const why = REASONS.filter((r) => h.skipped[r] > 0)
      .sort((a, b) => h.skipped[b] - h.skipped[a] || REASONS.indexOf(a) - REASONS.indexOf(b))
      .map((r) => `${h.skipped[r]} ${SKIP_LABEL[r]}`)
      .join(", ");
    rows.push(`${lines(skipped)} not voiced by the daemon (${why})`);
  }
  if (h.badFrames > 0) rows.push(`${h.badFrames} bad ${h.badFrames === 1 ? "frame" : "frames"}${h.lastBad ? `, last: ${h.lastBad}` : ""}`);
  if (h.reconnect) rows.push(`reconnecting: attempt ${h.reconnect.attempt}, next in ${Math.ceil(h.reconnect.nextInMs / 1000)} s`);
  return rows;
}
