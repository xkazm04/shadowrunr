// Mouth shapes for Malakor's face, the speech drive.
//
// The voice's timeline speaks in the Oculus 15 visemes; a glow rig has no lips, only a seam or a
// hinged jaw, so all it needs from a viseme is how far it opens and whether it is a closure. The
// figures are the ported rig's, tuned by eye on its glow forms. Pure.

import type { Viseme } from "./types";

/** How far a viseme opens (0..1) and how hard it presses closed (0..1, the bilabials). */
export const VISEME_SHAPES: Readonly<Record<Viseme, { open: number; press: number }>> = Object.freeze({
  sil: { open: 0, press: 0 },
  PP: { open: 0, press: 1 },
  FF: { open: 0.12, press: 0 },
  TH: { open: 0.2, press: 0 },
  DD: { open: 0.28, press: 0 },
  kk: { open: 0.33, press: 0 },
  CH: { open: 0.26, press: 0 },
  SS: { open: 0.14, press: 0 },
  nn: { open: 0.2, press: 0 },
  RR: { open: 0.24, press: 0 },
  aa: { open: 0.9, press: 0 },
  E: { open: 0.55, press: 0 },
  ih: { open: 0.38, press: 0 },
  oh: { open: 0.62, press: 0 },
  ou: { open: 0.32, press: 0 },
});

/** The open vowels: a stressed syllable lands on one of these, so a nod may follow it. */
export const VOWELS: ReadonlySet<Viseme> = new Set<Viseme>(["aa", "E", "ih", "oh", "ou"]);

/**
 * The target opening for a viseme at a playback level: the shape's opening, shut by a closure
 * (`PP` dips to 0), and scaled by loudness (0.6 + 0.55 x level, level capped at 1.2), so a quiet
 * syllable opens less than a loud one.
 */
export function visemeOpen(viseme: Viseme, level: number): number {
  const s = VISEME_SHAPES[viseme];
  const lv = Math.min(1.2, Math.max(0, level));
  return clamp01(s.open * (1 - 0.85 * s.press) * (0.6 + 0.55 * lv));
}

/**
 * The target opening from the playback level alone, for a line with no timeline. A gentle knee so
 * breath noise does not open the mouth and a loud syllable does not throw it fully open.
 */
export function levelOpen(level: number): number {
  const lv = Math.max(0, level - 0.04) / 0.96;
  return clamp01(0.85 * Math.pow(lv, 0.8));
}

function clamp01(x: number): number {
  return x < 0 ? 0 : x > 1 ? 1 : x;
}
