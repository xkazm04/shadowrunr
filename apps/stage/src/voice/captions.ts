// Captions that follow the voice. While his voice sounds, a line's caption is on the audio clock,
// not the stream's: it appears when the line's first part starts playing, and each part reveals
// its own words over that part's real duration (spread by the estimated word timing inside the
// part, so a long word still takes longer than a short one). Mouth shapes are not used: a part's
// start and length already pin its words to within a word or two, and they need no phoneme map.
//
// **The unvoiced-line rule.** A line that will never sound must not leave a caption waiting for
// it. A line is unvoiced when the daemon skipped it (`skipped`), the stage dropped it to respect
// MAX_AHEAD_S, or neither audio nor a skip came within VOICE_WAIT_S of the stage seeing it. From
// the moment that is known it shows at once on the estimated word timing, then holds only
// UNVOICED_HOLD_MS (not the full CAPTION_HOLD_MS) before it fades. A line that is sounding keeps
// the caption slot: an unvoiced line yields to it rather than cut its captions off. A line that
// was partly spoken and then skipped reveals the rest of its words at once.
//
// A line first seen more than VOICE_WAIT_S after it was said (the feed's backlog on joining a run)
// is history: it is never shown here.

import { CAPTION_HOLD_MS, visibleWords, wordTimings } from "@shadowrunr/stage-kit";

import type { ScheduledPart } from "./player";

/** One of Malakor's lines as the live feed said it, with the `say` event's seq. */
export interface SaidLine {
  /** The scene's caption id for it. */
  id: number;
  seq: number;
  text: string;
  /** When it was said, on the stream clock (ms). */
  at: number;
  replyingTo: string[];
}

/** No audio and no skip this long after the stage saw a line: it counts as unvoiced (seconds). */
export const VOICE_WAIT_S = 20;
/** How long an unvoiced line holds once its words are out. */
export const UNVOICED_HOLD_MS = 1500;
/** A caption fades out over this long. */
export const FADE_MS = 600;

export interface VoiceCaptionView {
  caption: { id: number; text: string; replyingTo: string[] };
  /** Words revealed. */
  shown: number;
  opacity: number;
  /** He is speaking it now (its audio sounds, or an unvoiced line is still revealing). */
  speaking: boolean;
  /** Its words follow audio (false: an unvoiced line on the estimate). */
  voiced: boolean;
}

interface Line {
  parts: Map<number, Omit<ScheduledPart, "runId" | "seq">>;
  /** When it was known unvoiced (audio clock, seconds). */
  unvoicedAt?: number;
}

const HISTORY = -1;
const LINES_MAX = 64;

const words = (text: string) => text.split(/\s+/).filter((w) => w.length > 0);

/** Words of a part revealed `elapsedMs` into it, when it holds `n` of the line's words. */
function partShown(text: string, durationS: number, elapsedMs: number, n: number): number {
  const line = wordTimings(text);
  const scale = line.spokenMs > 0 ? (durationS * 1000) / line.spokenMs : 1;
  const k = visibleWords(line, elapsedMs / scale);
  const own = line.words.length;
  if (own === n) return k;
  // the part's words and the caption's disagree (folded text): spread, never ahead of the audio
  return Math.min(n, Math.max(k > 0 ? 1 : 0, Math.floor((k * n) / Math.max(1, own))));
}

export class VoiceCaptions {
  private runId: string | null = null;
  private lines = new Map<number, Line>();
  /** When the stage first saw each line, on the audio clock (HISTORY: the backlog). */
  private seen = new Map<number, number>();

  /** A part was placed (or moved) on the audio clock. */
  scheduled(p: ScheduledPart): void {
    this.line(p.runId, p.seq).parts.set(p.part, { part: p.part, text: p.text, last: p.last, start: p.start, duration: p.duration });
  }

  /** The line will not sound (the daemon skipped it, or the stage dropped it); `now` on the audio clock. */
  unvoiced(runId: string, seq: number, now: number): void {
    const l = this.line(runId, seq);
    l.unvoicedAt ??= now;
    // a dropped line's parts never play
    for (const [k, p] of l.parts) if (p.start > now) l.parts.delete(k);
  }

  /**
   * The caption at `now` (audio clock, seconds) for the run's said lines (oldest first);
   * `streamNow` is the stream clock (ms), which tells the backlog from new lines.
   */
  view(runId: string, said: readonly SaidLine[], now: number, streamNow: number): VoiceCaptionView | null {
    this.switchRun(runId);
    const rank = (c: { v: VoiceCaptionView }) => (c.v.voiced && c.v.speaking ? 1 : 0);
    let best: { v: VoiceCaptionView; start: number } | null = null;
    for (const s of said) {
      let first = this.seen.get(s.seq);
      if (first === undefined) {
        first = streamNow - s.at > VOICE_WAIT_S * 1000 ? HISTORY : now;
        this.seen.set(s.seq, first);
      }
      if (first === HISTORY) continue;
      const c = this.candidate(s, this.lines.get(s.seq), first, now);
      // a sounding line keeps the slot; otherwise the latest to start shows
      if (c && (!best || rank(c) > rank(best) || (rank(c) === rank(best) && c.start >= best.start))) best = c;
    }
    this.trim();
    return best?.v ?? null;
  }

  private candidate(s: SaidLine, l: Line | undefined, first: number, now: number): { v: VoiceCaptionView; start: number } | null {
    const caption = { id: s.id, text: s.text, replyingTo: s.replyingTo };
    const total = words(s.text).length;
    const parts = l ? [...l.parts.values()].sort((a, b) => a.part - b.part) : [];
    if (parts.length && parts[0]!.start <= now) {
      // voiced: each part reveals its own words over its real duration
      let from = 0;
      let shown = 0;
      const end = Math.max(...parts.map((p) => p.start + p.duration));
      for (const p of parts) {
        const n = Math.max(0, Math.min(total - from, p.last ? total - from : words(p.text).length));
        if (now >= p.start + p.duration) shown = from + n;
        else if (now >= p.start) shown = from + partShown(p.text, p.duration, (now - p.start) * 1000, n);
        else break;
        from += n;
      }
      const complete = parts.some((p) => p.last) || l?.unvoicedAt !== undefined;
      if (l?.unvoicedAt !== undefined && now >= l.unvoicedAt) shown = total;
      if (complete && now >= end) shown = total;
      const since = (now - end) * 1000 - (complete ? CAPTION_HOLD_MS : VOICE_WAIT_S * 1000);
      const opacity = since <= 0 ? 1 : Math.max(0, 1 - since / FADE_MS);
      if (opacity <= 0) return null;
      return { v: { caption, shown, opacity, speaking: now < end, voiced: true }, start: parts[0]!.start };
    }
    // unvoiced: skipped, dropped, or nothing came in time
    const at = l?.unvoicedAt ?? (parts.length === 0 && now >= first + VOICE_WAIT_S ? first + VOICE_WAIT_S : undefined);
    if (at === undefined || now < at) return null;
    const line = wordTimings(s.text);
    const elapsed = (now - at) * 1000;
    const since = elapsed - line.spokenMs - UNVOICED_HOLD_MS;
    const opacity = since <= 0 ? 1 : Math.max(0, 1 - since / FADE_MS);
    if (opacity <= 0) return null;
    return { v: { caption, shown: visibleWords(line, elapsed), opacity, speaking: elapsed < line.spokenMs, voiced: false }, start: at };
  }

  private line(runId: string, seq: number): Line {
    this.switchRun(runId);
    let l = this.lines.get(seq);
    if (!l) {
      l = { parts: new Map() };
      this.lines.set(seq, l);
    }
    return l;
  }

  private switchRun(runId: string) {
    if (runId === this.runId) return;
    this.runId = runId;
    this.lines.clear();
    this.seen.clear();
  }

  private trim() {
    for (const m of [this.lines, this.seen] as Map<number, unknown>[]) {
      if (m.size <= LINES_MAX) continue;
      const old = [...m.keys()].sort((a, b) => a - b).slice(0, m.size - LINES_MAX);
      for (const k of old) m.delete(k);
    }
  }
}
