// Word timing for kinetic captions. There is no TTS alignment in the media-less phase, so word
// onsets are estimated from text: a base speaking rate of ~150 wpm (a deliberate villain), a
// little longer for long words, and pauses after punctuation.

export interface TimedWord {
  word: string;
  /** Onset in ms from the line's start. */
  start: number;
  /** ALL CAPS, numbers and the show's own vocabulary get emphasis. */
  emphasis: boolean;
}

export interface TimedLine {
  words: TimedWord[];
  /** When the last word has landed (ms). */
  spokenMs: number;
}

const EMPHASIS = new Set([
  "deficit",
  "audit",
  "audited",
  "ledger",
  "minion",
  "minions",
  "lord",
  "darkness",
  "decree",
  "alleged",
  "fraud",
  "fictional",
  "phylactery",
  "malakor",
  "spire",
  "thrall",
  "thralls",
  "acolyte",
  "cultist",
  "warlock",
  "lich",
  "council",
  "recorded",
]);

export const BASE_WORD_MS = 230;
export const PER_CHAR_MS = 24;
export const PAUSE_COMMA_MS = 170;
export const PAUSE_STOP_MS = 360;
export const PAUSE_ELLIPSIS_MS = 560;
/** How long a finished caption stays fully visible before it fades. */
export const CAPTION_HOLD_MS = 4200;

export function isEmphatic(word: string): boolean {
  const bare = word.replace(/[^\p{L}\p{N}%$]/gu, "");
  if (bare.length === 0) return false;
  if (/\d/.test(bare)) return true;
  if (bare.length >= 2 && bare === bare.toUpperCase() && /\p{L}/u.test(bare)) return true;
  return EMPHASIS.has(bare.toLowerCase());
}

function pauseAfter(word: string): number {
  if (/(\.\.\.|…)["')\]]*$/.test(word)) return PAUSE_ELLIPSIS_MS;
  if (/[.!?]["')\]]*$/.test(word)) return PAUSE_STOP_MS;
  if (/[,;:—–]["')\]]*$/.test(word) || word === "—" || word === "–") return PAUSE_COMMA_MS;
  return 0;
}

export function wordTimings(text: string): TimedLine {
  const raw = text.split(/\s+/).filter((w) => w.length > 0);
  const words: TimedWord[] = [];
  let t = 0;
  for (const word of raw) {
    words.push({ word, start: t, emphasis: isEmphatic(word) });
    const letters = word.replace(/[^\p{L}\p{N}]/gu, "").length;
    t += BASE_WORD_MS + Math.min(letters, 14) * PER_CHAR_MS + pauseAfter(word);
  }
  return { words, spokenMs: t };
}

/** How many words are visible `elapsed` ms after the line started. */
export function visibleWords(line: TimedLine, elapsed: number): number {
  if (elapsed < 0) return 0;
  let n = 0;
  for (const w of line.words) {
    if (w.start <= elapsed) n++;
    else break;
  }
  return n;
}

/** 1 while the caption should show, falling to 0 over 600 ms after the hold expires. */
export function captionOpacity(line: TimedLine, elapsed: number): number {
  const fadeStart = line.spokenMs + CAPTION_HOLD_MS;
  if (elapsed <= fadeStart) return 1;
  return Math.max(0, 1 - (elapsed - fadeStart) / 600);
}
