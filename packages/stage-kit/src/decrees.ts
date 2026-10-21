// The decree slot's schedule: one card at a time. Picked and highlighted lines lift out of the
// chat lane in the order they came; a card that arrives while another shows waits behind it, and
// the one showing gives way once it has held DECREE_MIN_SHOW_MS (or its full time when nothing
// waits). A card fades out over the last DECREE_FADE_MS of its turn and the next enters only when
// that turn has ended, so a leaving card never overlaps an entering one. After the last card's
// turn the slot keeps that line, held quietly, until the next pick. Pure: nothing reads a clock.

import type { Decree } from "./sceneModel";
import type { TsMs } from "./vocab";

/** How long a decree shows when nothing waits behind it. */
export const DECREE_MS = { highlight: 9000, picked: 6500 } as const;
/** A decree shows at least this long before a waiting one takes its turn. */
export const DECREE_MIN_SHOW_MS = 3000;
/** A decree fades out over the last part of its turn. */
export const DECREE_FADE_MS = 450;

/** One decree's turn in the slot: shown from `start` until `end`. */
export interface DecreeTurn {
  decree: Decree;
  start: TsMs;
  end: TsMs;
}

/** Each decree's turn, in the order they came (the scene keeps the latest DECREE_MAX). */
export function decreeTurns(decrees: readonly Decree[]): DecreeTurn[] {
  const sorted = [...decrees].sort((a, b) => a.at - b.at);
  const turns: DecreeTurn[] = [];
  for (const d of sorted) {
    const prev = turns[turns.length - 1];
    const start = prev ? Math.max(d.at, prev.start + DECREE_MIN_SHOW_MS) : d.at;
    // the one before gives way when this one's turn comes, if its own time has not run out first
    if (prev) prev.end = Math.min(prev.end, start);
    turns.push({ decree: d, start, end: start + DECREE_MS[d.kind] });
  }
  return turns;
}

/**
 * What the slot shows at `now`: the decree whose turn it is (`held: false`), else the last one
 * whose turn has ended (`held: true`), else nothing.
 */
export function decreeAt(decrees: readonly Decree[], now: TsMs): { turn: DecreeTurn; held: boolean } | null {
  const turns = decreeTurns(decrees);
  let held: DecreeTurn | null = null;
  for (const t of turns) {
    if (now >= t.start && now < t.end) return { turn: t, held: false };
    if (t.end <= now) held = t;
  }
  return held ? { turn: held, held: true } : null;
}
