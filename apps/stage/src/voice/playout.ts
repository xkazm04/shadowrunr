// The playout clock: the one place that answers "when is it heard". The stage has three clocks,
// the AudioContext's (where a sample is scheduled), the speakers' (where it is heard) and the
// wall's (what the daemon's reports and the recording read), and every consumer of the voice's
// timing (the face, the captions, the `played` report) converts through this module instead of by
// hand.
//
// - **Heard time** is on the context's axis: `heardNow()` is the context time of the sample at the
//   speakers now, `currentTime` minus the device's `outputLatency`. It never goes backwards (a
//   device swap that raises the latency holds it still until the context catches up). A browser
//   that reports no latency gives 0, and heard time is the context time (the old behaviour).
// - **Wall time** of a context time comes from the browser's coherent output timestamp
//   (`getOutputTimestamp()`: the context time at the speakers and the performance time it was
//   there). That pair already includes the output path, so the latency is never added to it
//   again. Without a usable timestamp it falls back to the wall now plus how far the sample is
//   from the heard time now.

import type { AudioLike } from "./player";

export interface PlayoutClockOptions {
  /** The wall clock in epoch ms (default `Date.now`): the fallback without an output timestamp. */
  wall?: (() => number) | undefined;
  /** A performance time (ms) as epoch ms (default `performance.timeOrigin + p`). */
  perfWall?: ((performanceTimeMs: number) => number) | undefined;
}

/** A performance time as epoch ms; NaN where the page has no `performance` (the fallback then). */
const defaultPerfWall = (p: number): number => (typeof performance === "undefined" ? NaN : performance.timeOrigin + p);

export class PlayoutClock {
  private last = -Infinity;

  constructor(
    private readonly audio: AudioLike,
    private readonly o: PlayoutClockOptions = {},
  ) {}

  /** The device's output latency now (seconds; 0 where the browser reports none or nonsense). */
  latency(): number {
    const l = this.audio.outputLatency;
    return typeof l === "number" && Number.isFinite(l) && l > 0 ? l : 0;
  }

  /** The context time of the sample heard now (seconds). Monotone. */
  heardNow(): number {
    const t = this.audio.currentTime - this.latency();
    if (t > this.last) this.last = t;
    return this.last;
  }

  /** The epoch ms at which the sample at context time `ctxTime` is (or was) heard. */
  wallOf(ctxTime: number): number {
    const ts = this.audio.outputTimestamp?.();
    const c = ts?.contextTime;
    const p = ts?.performanceTime;
    if (typeof c === "number" && typeof p === "number" && Number.isFinite(c) && Number.isFinite(p) && p > 0) {
      const at = (this.o.perfWall ?? defaultPerfWall)(p);
      if (Number.isFinite(at)) return at + (ctxTime - c) * 1000;
    }
    const raw = this.audio.currentTime - this.latency();
    return (this.o.wall ?? Date.now)() + (ctxTime - raw) * 1000;
  }
}

const shared = new WeakMap<AudioLike, PlayoutClock>();

/** The page's playout clock for `audio`: one per context, so its monotone clamp is shared. */
export function playoutOf(audio: AudioLike): PlayoutClock {
  let c = shared.get(audio);
  if (!c) {
    c = new PlayoutClock(audio);
    shared.set(audio, c);
  }
  return c;
}
