// `?face=` (dev builds only): drive the face from a scripted signal, with no daemon and no voice,
// to look at it and screenshot it. `demo` loops a short scripted line (speech, then rest); `rest`
// holds the face at rest; a viseme name (`aa`, `oh`, `PP` ...) holds that mouth shape mid-speech.

import { frames, type Clock } from "./motion";
import { RESTING, VISEMES, type FaceSignal, type FaceSource, type Viseme } from "./types";
import { VISEME_SHAPES, VOWELS } from "./visemes";

/** A scripted line: [viseme, ms]. Made up for the demo; it is no recording of anybody. */
const LINE: readonly (readonly [Viseme, number])[] = [
  ["kk", 70], ["ih", 90], ["nn", 60], ["E", 110], ["sil", 60], ["DD", 50], ["aa", 160], ["RR", 60],
  ["oh", 140], ["sil", 120], ["TH", 60], ["E", 90], ["SS", 80], ["PP", 60], ["aa", 150], ["ih", 70],
  ["RR", 70], ["sil", 90], ["ih", 70], ["SS", 90], ["FF", 70], ["oh", 180], ["RR", 60], ["sil", 140],
  ["DD", 50], ["aa", 200], ["kk", 70], ["nn", 60], ["E", 140], ["SS", 120], ["sil", 80],
];
export const LINE_MS = LINE.reduce((s, [, d]) => s + d, 0);
const REST_MS = 2200;

/** The scripted line's signal at `ms` into it (resting past its end). */
export function lineAt(ms: number): FaceSignal {
  let t = 0;
  for (const [v, d] of LINE) {
    if (ms < t + d) {
      const within = (ms - t) / d;
      const base = v === "sil" ? 0 : VOWELS.has(v) ? 0.5 + 0.45 * VISEME_SHAPES[v].open : 0.32;
      const level = Math.round(base * (0.75 + 0.25 * Math.sin(Math.PI * within)) * 100) / 100;
      return { phase: "speaking", level, viseme: v, open: 0, generation: 1 };
    }
    t += d;
  }
  return RESTING;
}

export function isDemoMode(mode: string): boolean {
  return mode === "demo" || mode === "rest" || (VISEMES as readonly string[]).includes(mode);
}

/** A scripted face source for `mode`, timed by `clock`. */
export function demoSource(mode: string, clock: Clock = frames()): FaceSource {
  if (mode !== "demo") {
    const held: FaceSignal = mode === "rest" ? RESTING : { phase: "speaking", level: 0.8, viseme: mode as Viseme, open: 0, generation: 1 };
    return { subscribe: (l) => (l(held), () => {}) };
  }
  return {
    subscribe(listener) {
      let ms = 0;
      return clock.subscribe((_t, delta) => {
        ms = (ms + delta) % (LINE_MS + REST_MS);
        listener(lineAt(ms));
      });
    },
  };
}
