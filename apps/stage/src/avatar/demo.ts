// `?face=` (dev builds only): drive the face from a scripted signal, with no daemon and no voice,
// to look at it and screenshot it. `demo` loops a short scripted line (speech, then rest); `rest`
// holds the face at rest; a viseme name (`aa`, `oh`, `PP` ...) holds that mouth shape mid-speech.
//
// `bench` loops BENCH_SCRIPT, 24 s that exercise every channel a face reads: speech, each emotion's
// eye light and brow with its gesture, the look (host, content, chat) and quiet gaps where the face
// may settle. It is data and a pure `benchAt`, so a bench can replay the same inputs on every face.
// `?perf=<emotion>[:intensity][:gesture]` holds one performance cue, for screenshots.

import type { Emotion } from "@shadowrunr/contract";

import { LOOK_CHAT, LOOK_CONTENT, LOOK_HOST, lookBox, type LookBox } from "./look";
import { frames, type Clock } from "./motion";
import { cueFor, EMOTION_TABLE, GESTURES, type Gesture, type PerformanceCue, type PerformanceSource } from "./performance";
import { RESTING, VISEMES, type FaceSignal, type FaceSource, type Viseme } from "./types";
import type { Look } from "./variants";
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
  return mode === "demo" || mode === "rest" || mode === "bench" || (VISEMES as readonly string[]).includes(mode);
}

/** A scripted face source for `mode`, timed by `clock`. */
export function demoSource(mode: string, clock: Clock = frames()): FaceSource {
  if (mode === "bench") return benchSources(clock).face;
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

/**
 * One beat of the bench: from `at` for `ms` (script ms), speaking the scripted line or quiet,
 * looking at `look`, and setting `cue` (the line's delivery, as the stage would) as it starts.
 */
export interface BenchBeat {
  at: number;
  ms: number;
  speak: boolean;
  look: "host" | "content" | "chat";
  cue?: { emotion: Emotion; intensity: number; gesture?: Gesture };
}

/** The bench's loop, ms. */
export const BENCH_MS = 24_000;

/**
 * The bench: neutral, then menace with a glare, glee with a tilt at the chat, outrage's recoil and
 * flare, hushed leaning in at the content, triumph's flare and a weary nod, with quiet gaps where
 * the light holds. Each cue is what `cueFor` makes of that delivery (the emotion table's gesture).
 */
export const BENCH_SCRIPT: readonly BenchBeat[] = Object.freeze([
  { at: 0, ms: 1500, speak: false, look: "host", cue: { emotion: "neutral", intensity: 0 } },
  { at: 1500, ms: 2500, speak: true, look: "host", cue: { emotion: "neutral", intensity: 0.3 } },
  { at: 4000, ms: 2800, speak: true, look: "host", cue: { emotion: "menace", intensity: 0.9, gesture: "glare" } },
  { at: 6800, ms: 1000, speak: false, look: "host" },
  { at: 7800, ms: 2500, speak: true, look: "chat", cue: { emotion: "glee", intensity: 0.8, gesture: "tilt" } },
  { at: 10300, ms: 2700, speak: true, look: "host", cue: { emotion: "outrage", intensity: 1, gesture: "recoil" } },
  { at: 13000, ms: 1000, speak: false, look: "content" },
  { at: 14000, ms: 2500, speak: true, look: "content", cue: { emotion: "hushed", intensity: 0.8, gesture: "lean_in" } },
  { at: 16500, ms: 2500, speak: true, look: "host", cue: { emotion: "triumph", intensity: 0.9, gesture: "flare" } },
  { at: 19000, ms: 2500, speak: true, look: "host", cue: { emotion: "weary", intensity: 0.7, gesture: "nod" } },
  { at: 21500, ms: 2500, speak: false, look: "host", cue: { emotion: "neutral", intensity: 0 } },
] satisfies BenchBeat[]);

const BENCH_LOOKS: Record<BenchBeat["look"], Look> = { host: LOOK_HOST, content: LOOK_CONTENT, chat: LOOK_CHAT };
const BENCH_CUES = BENCH_SCRIPT.filter((b) => b.cue).length;

/** What the bench gives every face at `ms` from its start (looping): the signal, the cue, the look. */
export function benchAt(ms: number): { signal: FaceSignal; cue: PerformanceCue; look: Look } {
  const t = Math.max(0, Number.isFinite(ms) ? ms : 0);
  const loop = Math.floor(t / BENCH_MS);
  const within = t - loop * BENCH_MS;
  let i = 0;
  let cues = 0;
  let cue: NonNullable<BenchBeat["cue"]> = { emotion: "neutral", intensity: 0 };
  for (let k = 0; k < BENCH_SCRIPT.length; k++) {
    const b = BENCH_SCRIPT[k]!;
    if (b.at > within) break;
    i = k;
    if (b.cue) {
      cues++;
      cue = b.cue;
    }
  }
  const beat = BENCH_SCRIPT[i]!;
  const line = beat.speak ? lineAt(within - beat.at) : RESTING;
  // each spoken beat is its own utterance, so the driver hears every line start
  const signal = line.phase === "speaking" ? { ...line, generation: loop * BENCH_SCRIPT.length + i + 1 } : RESTING;
  return { signal, cue: { id: loop * BENCH_CUES + cues, ...cue }, look: BENCH_LOOKS[beat.look] };
}

const sameSignal = (a: FaceSignal, b: FaceSignal) => a.phase === b.phase && a.level === b.level && a.viseme === b.viseme && a.generation === b.generation;

/** The bench's three sources on `clock`: one script clock, running while anything listens. */
export function benchSources(clock: Clock = frames()): { face: FaceSource; performance: PerformanceSource; look: LookBox } {
  const look = lookBox(LOOK_HOST);
  const faces = new Set<(s: FaceSignal) => void>();
  const cues = new Set<(c: PerformanceCue) => void>();
  let ms = 0;
  let now = benchAt(0);
  let users = 0;
  let stop: (() => void) | null = null;
  const tick = (_t: number, delta: number) => {
    ms += delta;
    const prev = now;
    now = benchAt(ms);
    if (!sameSignal(now.signal, prev.signal)) for (const fn of [...faces]) fn(now.signal);
    if (now.cue.id !== prev.cue.id) for (const fn of [...cues]) fn(now.cue);
    look.set(now.look);
  };
  const hold = (): (() => void) => {
    users++;
    stop ??= clock.subscribe(tick);
    return () => {
      if (--users > 0) return;
      stop?.();
      stop = null;
    };
  };
  return {
    face: {
      subscribe(listener) {
        faces.add(listener);
        listener(now.signal);
        const off = hold();
        return () => {
          faces.delete(listener);
          off();
        };
      },
    },
    performance: {
      get: () => now.cue,
      subscribe(fn) {
        cues.add(fn);
        const off = hold();
        return () => {
          cues.delete(fn);
          off();
        };
      },
    },
    look,
  };
}

/** `?perf=<emotion>[:intensity][:gesture]`: the cue fields (intensity 1 by default), or null. */
export function parsePerf(param: string | null): Omit<PerformanceCue, "id"> | null {
  if (!param) return null;
  const [emotion, level, gesture] = param.split(":");
  if (!emotion || !Object.hasOwn(EMOTION_TABLE, emotion)) return null;
  const n = level === undefined || level === "" ? 1 : Number(level);
  const base = cueFor({ emotion: emotion as Emotion, intensity: Number.isFinite(n) ? n : 1, pace: "normal" });
  if (gesture && (GESTURES as readonly string[]).includes(gesture)) return { ...base, gesture: gesture as Gesture };
  return base;
}

/** A source that holds one cue (`?perf=`): the face reads it on its first frame. */
export function heldPerformance(cue: Omit<PerformanceCue, "id">): PerformanceSource {
  const held: PerformanceCue = { id: 1, ...cue };
  return { get: () => held };
}
