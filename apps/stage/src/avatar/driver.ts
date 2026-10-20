// The face driver: turns the stream of `FaceSignal`s into one rig pose a frame.
//
// Pure and deterministic: the only randomness is a seeded generator consumed in time order, so the
// same seed and the same signals stepped by the same deltas give the same poses (the tests hold
// it). Two layers, in order:
//
// 1. **Procedural motion per phase.** Slow drift (incommensurate sines), blinks every 2-6 s with
//    occasional doubles, micro-saccades, and a per-phase profile: speaking sways a little more and
//    nods on stressed vowels. Profiles cross-blend, so a phase change never jumps. The head runs
//    at 0.7x, as the ported glow forms did.
// 2. **Speech.** With a viseme the mouth opens to that shape scaled by the level (a closure dips);
//    without one it opens from the level alone, with a fast attack and a slower release.
//
// There is no neural motion in v1: the head, the gaze and the blinks are always procedural.
//
// **It settles.** At rest (idle and silent) it keeps living for a short linger and then eases
// every motion to zero and reports `settled`; the runtime then stops its frame loop. Any new
// signal wakes it.

import type { FacePhase, FaceSignal, Viseme } from "./types";
import { levelOpen, VISEME_SHAPES, visemeOpen, VOWELS } from "./visemes";

/** One frame of the rig. Reused between frames: read it, do not keep it. */
export interface Pose {
  /** Face clock, seconds: what breathing and the floating parts are timed by. */
  t: number;
  /** Degrees: yaw right-positive, pitch down-positive, roll clockwise-positive. */
  yaw: number;
  pitch: number;
  roll: number;
  /** 0 open .. 1 closed. */
  blinkL: number;
  blinkR: number;
  /** -1..1. */
  gazeX: number;
  gazeY: number;
  /** Mouth (seam, jaw) opening, 0..1, smoothed. */
  open: number;
  /** Voice level, smoothed (fast attack, slow release), 0..1.2. */
  voice: number;
  /** How alive the idle motion is, 0 (at rest, still) .. 1: scales breath, sway and floats. */
  activity: number;
  /** Voice level history at 40 Hz, [0] = now: a rig's shimmer ripple reads it. */
  history: Float32Array;
}

export interface DriverOptions {
  seed?: number;
  /** Procedural head scale (the glow forms: 0.7). */
  motionScale?: number;
  /** Seconds an idle face keeps moving after its last signal change before it settles. */
  idleLinger?: number;
}

export interface Driver {
  /** Hand in the latest signal. Returns whether anything in it changed (an identical one is not news). */
  setSignal(signal: FaceSignal): boolean;
  /** Advance by `dt` seconds and return the pose. */
  step(dt: number): Pose;
  /** The current pose without advancing. */
  readonly pose: Pose;
  /** True when nothing moves and nothing will until the next signal change. */
  readonly settled: boolean;
  /** Jump to rest (still, eyes open, mouth shut): what reduced motion draws. */
  rest(): Pose;
}

/** A seeded generator (mulberry32). */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const clamp = (x: number, a: number, b: number) => (x < a ? a : x > b ? b : x);
const smooth = (x: number) => {
  const y = clamp(x, 0, 1);
  return y * y * (3 - 2 * y);
};
/** 0 -> 1 over `up` s before `t0`, back to 0 over `down` s after it. */
function bump(t: number, t0: number, up: number, down: number): number {
  if (t < t0 - up || t > t0 + down) return 0;
  return t < t0 ? smooth((t - (t0 - up)) / up) : 1 - smooth((t - t0) / down);
}
/** Exponential approach with time constant `tau`. */
const approach = (x: number, target: number, dt: number, tau: number) => x + (target - x) * (1 - Math.exp(-dt / Math.max(1e-4, tau)));

/** What a phase does to the procedural motion. Numbers blend, so a change never jumps. */
interface Profile {
  /** Drift amplitude. */
  drift: number;
  /** Drift speed (time warp of the sines). */
  speed: number;
  /** Saccade size. */
  sacc: number;
  /** Speech sway depth. */
  speech: number;
}

const PROFILES: Record<FacePhase, Profile> = {
  idle: { drift: 1, speed: 1, sacc: 1, speech: 0 },
  speaking: { drift: 1, speed: 1, sacc: 0.8, speech: 1 },
};
const PROFILE_KEYS = Object.keys(PROFILES.idle) as (keyof Profile)[];

const BLINK_S = 0.28;
const HISTORY = 48;

export function createDriver(options: DriverOptions = {}): Driver {
  const rnd = mulberry32(options.seed ?? 7);
  const motionScale = options.motionScale ?? 0.7;
  const idleLinger = options.idleLinger ?? 6;

  const ph = Array.from({ length: 10 }, () => rnd() * Math.PI * 2);
  const phase = (i: number) => ph[i] ?? 0;
  const pose: Pose = {
    t: 0,
    yaw: 0,
    pitch: 0,
    roll: 0,
    blinkL: 0,
    blinkR: 0,
    gazeX: 0,
    gazeY: 0,
    open: 0,
    voice: 0,
    activity: 0,
    history: new Float32Array(HISTORY),
  };

  let signal: FaceSignal = { phase: "idle", level: 0, viseme: null, open: 0, generation: 0 };
  const prof: Profile = { ...PROFILES.idle };
  let t = 0;
  let clock = 0; // drift clock, warped by the profile's speed
  let linger = idleLinger;
  let awake = 0; // 0..1, linear; eased when used
  let speech = 0; // speaking amount, smoothed

  // blinks
  let nextBlink = 0.6 + rnd() * 1.6;
  const blinks: number[] = [];
  // saccades
  let nextSacc = 0.4 + rnd();
  let saccFrom = { x: 0, y: 0 };
  let saccTo = { x: 0, y: 0 };
  let saccAt = -1;
  // nods
  const nods: { t: number; amp: number }[] = [];
  let lastNod = -10;
  let lastViseme: Viseme | null = null;
  // level history at 40 Hz
  let histAcc = 0;

  const resting = () => signal.phase === "idle" && signal.level < 0.005;

  function scheduleBlinkSoon(at: number) {
    if (blinks.every((b) => Math.abs(b - at) > 0.35) && Math.abs(nextBlink - at) > 0.35) blinks.push(at);
  }

  function setSignal(next: FaceSignal): boolean {
    const changed =
      next.phase !== signal.phase ||
      next.level !== signal.level ||
      next.viseme !== signal.viseme ||
      next.open !== signal.open ||
      next.generation !== signal.generation;
    if (!changed) return false;
    const prevPhase = signal.phase;
    signal = { ...next };
    // a blink near the start of speech, as people do
    if (next.phase !== prevPhase && next.phase === "speaking") scheduleBlinkSoon(t + 0.12);
    // a stressed vowel while speaking may earn a nod
    if (next.phase === "speaking" && next.viseme !== lastViseme && next.viseme !== null && VOWELS.has(next.viseme)) {
      const score = VISEME_SHAPES[next.viseme].open * (0.4 + Math.min(1, next.level));
      if (score > 0.3 && t - lastNod > 0.65 && rnd() < 0.5) {
        nods.push({ t: t + 0.06, amp: 1.4 + 1.6 * rnd() });
        lastNod = t;
      }
    }
    lastViseme = next.viseme;
    // A phase change restarts the linger; a quiet idle signal that only twitches does not.
    if (next.phase !== prevPhase) linger = idleLinger;
    return true;
  }

  function blinkAt(time: number): number {
    let v = 0;
    for (const b of blinks) {
      const d = time - b; // close 75 ms, hold 35 ms, open 170 ms
      if (d < 0 || d > BLINK_S) continue;
      v = Math.max(v, d < 0.075 ? smooth(d / 0.075) : d < 0.11 ? 1 : 1 - smooth((d - 0.11) / 0.17));
    }
    return v;
  }

  function step(dtIn: number): Pose {
    const dt = clamp(Number.isFinite(dtIn) ? dtIn : 0, 0, 0.1);
    t += dt;

    // rest or awake
    if (resting()) linger = Math.max(0, linger - dt);
    else linger = idleLinger;
    const wantAwake = linger > 0;
    awake = clamp(awake + (wantAwake ? dt / 0.5 : -dt / 1.2), 0, 1);
    const act = smooth(awake);

    // profile blend
    const target = PROFILES[signal.phase];
    for (const k of PROFILE_KEYS) prof[k] = approach(prof[k], target[k], dt, 0.45);
    clock += dt * prof.speed;
    speech = approach(speech, signal.phase === "speaking" ? 1 : 0, dt, 0.4);

    // events: blinks (only while awake), saccades, expiry
    if (wantAwake && t >= nextBlink) {
      scheduleBlinkSoon(t);
      if (rnd() < 0.2) blinks.push(t + 0.26 + rnd() * 0.08);
      nextBlink = t + 2 + 4 * Math.pow(rnd(), 1.3);
    }
    for (let i = blinks.length - 1; i >= 0; i--) if (t - (blinks[i] ?? 0) > BLINK_S) blinks.splice(i, 1);
    if (t >= nextSacc) {
      const r = (0.03 + 0.07 * rnd()) * prof.sacc;
      const a = rnd() * Math.PI * 2;
      saccFrom = currentGaze();
      saccTo = { x: Math.cos(a) * r, y: Math.sin(a) * r * 0.6 };
      saccAt = t;
      nextSacc = t + (0.5 + 1.7 * rnd()) / Math.max(0.4, prof.sacc);
    }
    for (let i = nods.length - 1; i >= 0; i--) if (t - (nods[i]?.t ?? 0) > 0.4) nods.splice(i, 1);

    // procedural head
    const c = clock;
    const d = prof.drift;
    const sp = speech * prof.speech;
    let yaw = d * (1.5 * Math.sin(c * 0.43 + phase(0)) + 0.8 * Math.sin(c * 1.07 + phase(1))) + sp * 0.9 * Math.sin(c * 2.9 + phase(2));
    let pitch = d * (0.8 * Math.sin(c * 0.37 + phase(3)) + 0.45 * Math.sin(c * 0.93 + phase(4))) + sp * 0.6 * Math.sin(c * 3.4 + phase(5));
    const roll = d * (0.9 * Math.sin(c * 0.29 + phase(6)) + 0.4 * Math.sin(c * 0.81 + phase(7)));
    for (const n of nods) {
      const b = bump(t, n.t, 0.13, 0.34);
      pitch += n.amp * b;
      yaw += 0.25 * n.amp * b * Math.sin(phase(8));
    }
    const g = currentGaze();
    yaw += g.x * 3.0; // the head trails a big gaze shift a little
    pitch += g.y * 1.5;
    const blink = blinkAt(t);

    // at rest (act 0) everything is exactly 0, never -0, so a still frame compares equal
    const k = act === 0 ? 0 : motionScale * act;
    pose.yaw = k && yaw * k;
    pose.pitch = k && pitch * k;
    pose.roll = k && roll * k;
    pose.blinkL = blink;
    pose.blinkR = blink;
    pose.gazeX = act && g.x * act;
    pose.gazeY = act && g.y * act;

    // speech
    const level = clamp(signal.level, 0, 1.2);
    if (signal.viseme !== null) {
      pose.open = approach(pose.open, visemeOpen(signal.viseme, level), dt, 0.035);
    } else {
      const want = Math.max(levelOpen(level), clamp(signal.open, 0, 1));
      pose.open = approach(pose.open, want, dt, want > pose.open ? 0.03 : 0.12);
    }
    pose.voice = approach(pose.voice, level, dt, level > pose.voice ? 0.03 : 0.14);
    histAcc += dt;
    while (histAcc >= 0.025) {
      histAcc -= 0.025;
      pose.history.copyWithin(1, 0, HISTORY - 1);
      pose.history[0] = level;
    }
    pose.history[0] = level;

    pose.activity = act;
    pose.t = t;

    // settle: snap the last crumbs so a still frame is exactly at rest
    if (isSettled()) {
      pose.open = 0;
      pose.voice = 0;
      pose.history.fill(0);
    }
    return pose;
  }

  function currentGaze() {
    if (saccAt < 0) return { x: saccTo.x, y: saccTo.y };
    const k = smooth((t - saccAt) / 0.045);
    return { x: saccFrom.x + (saccTo.x - saccFrom.x) * k, y: saccFrom.y + (saccTo.y - saccFrom.y) * k };
  }

  function isSettled(): boolean {
    return linger === 0 && awake === 0 && blinks.length === 0 && nods.length === 0 && pose.open < 1e-3 && pose.voice < 1e-3 && resting();
  }

  function rest(): Pose {
    pose.yaw = pose.pitch = pose.roll = 0;
    pose.blinkL = pose.blinkR = 0;
    pose.gazeX = pose.gazeY = 0;
    pose.open = pose.voice = pose.activity = 0;
    pose.history.fill(0);
    return pose;
  }

  return {
    setSignal,
    step,
    rest,
    get pose() {
      return pose;
    },
    get settled() {
      return isSettled();
    },
  };
}
