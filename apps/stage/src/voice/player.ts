// The voice player: schedules utterances back to back on one AudioContext and turns what is
// playing into the face's signal.
//
// **The audio is the one clock.** Each utterance is scheduled at an exact AudioContext time; the
// face reads the mouth shape at `ctx.currentTime` minus that utterance's start (plus a 35 ms lead,
// the mouth leads the sound a little, as a face does), and the loudness from an AnalyserNode on
// the output. Neither the wall clock nor a frame's arrival time moves the mouth.
//
// **Bounded and in order.** Parts play gaplessly in (seq, part) order. A frame at or behind what
// was already accepted is dropped. When more than `MAX_AHEAD_S` of audio is waiting, the oldest
// utterance that has not started is dropped and the rest close up behind what is playing, so a
// stage that fell behind catches up instead of talking ever later. A new run starts afresh.

import { RESTING, type FaceSignal, type FaceSource } from "../avatar/types";
import type { Clock } from "../avatar/motion";
import { decodePcm16, visemeAt, type Utterance } from "./frame";

/** Audio waiting ahead of the playhead, at most (seconds). */
export const MAX_AHEAD_S = 6;
/** The first utterance after silence starts this far ahead of now, so its start is not clipped. */
export const START_LEAD_S = 0.06;
/** The mouth leads the sound by this much. */
export const VISEME_LEAD_MS = 35;

/** The parts of an AudioBuffer the player uses. */
export interface BufferLike {
  readonly duration: number;
  getChannelData(channel: number): Float32Array;
}

/** The parts of an AudioBufferSourceNode the player uses. */
export interface SourceLike {
  buffer: BufferLike | null;
  connect(dest: never): unknown;
  start(when: number): void;
  stop(when?: number): void;
  disconnect(): void;
}

/** The parts of an AudioContext the player uses, with the node it plays into and the level meter. */
export interface AudioLike {
  readonly currentTime: number;
  createBuffer(channels: number, length: number, sampleRate: number): BufferLike;
  createBufferSource(): SourceLike;
  /** Where sources connect (the analyser in front of the speakers). */
  readonly output: unknown;
  /** The output's loudness now, 0..1. */
  level(): number;
}

interface Entry {
  runId: string;
  seq: number;
  part: number;
  generation: number;
  buffer: BufferLike;
  visemes: Utterance["visemes"];
  start: number;
  source: SourceLike | null;
}

export type Accept = "scheduled" | "behind" | "bad";

const order = (a: { seq: number; part: number }, b: { seq: number; part: number }) => a.seq - b.seq || a.part - b.part;

export class VoicePlayer implements FaceSource {
  private queue: Entry[] = [];
  private runId: string | null = null;
  private last: { seq: number; part: number } | null = null;
  private generation = 0;
  private listeners = new Set<(s: FaceSignal) => void>();
  private stopTick: (() => void) | null = null;
  private lastSignal: FaceSignal = RESTING;
  /** Utterances dropped for falling behind (since the last run change). */
  dropped = 0;

  constructor(
    private readonly audio: AudioLike,
    private readonly clock: Clock,
  ) {}

  /** Seconds of audio scheduled past the playhead. */
  get ahead(): number {
    const end = this.end();
    return end === null ? 0 : Math.max(0, end - this.audio.currentTime);
  }

  /** Whether anything is playing or waiting to. */
  get playing(): boolean {
    this.prune();
    return this.queue.length > 0;
  }

  /** The utterances still held, in play order (for tests and the debug surface). */
  scheduled(): readonly { seq: number; part: number; start: number; end: number }[] {
    return this.queue.map((e) => ({ seq: e.seq, part: e.part, start: e.start, end: e.start + e.buffer.duration }));
  }

  /** Take one utterance: decode it and schedule it after everything already scheduled. */
  accept(u: Utterance): Accept {
    if (u.run_id !== this.runId) {
      this.reset();
      this.runId = u.run_id;
    }
    if (this.last && order(u, this.last) <= 0) return "behind";
    let samples: Float32Array;
    try {
      samples = decodePcm16(u.pcm);
    } catch {
      return "bad";
    }
    if (samples.length === 0) return "bad";
    this.last = { seq: u.seq, part: u.part };
    const buffer = this.audio.createBuffer(1, samples.length, u.sample_rate);
    buffer.getChannelData(0).set(samples);
    const now = this.audio.currentTime;
    const start = Math.max(this.end() ?? 0, now + START_LEAD_S);
    const e: Entry = { runId: u.run_id, seq: u.seq, part: u.part, generation: ++this.generation, buffer, visemes: u.visemes, start, source: null };
    this.play(e);
    this.queue.push(e);
    this.bound();
    this.wake();
    return "scheduled";
  }

  /** Stop everything and forget the run (a run change, the voice turned off). */
  reset(): void {
    for (const e of this.queue) this.silence(e);
    this.queue = [];
    this.last = null;
    this.runId = null;
    this.dropped = 0;
  }

  dispose(): void {
    this.reset();
    this.stopTick?.();
    this.stopTick = null;
    this.listeners.clear();
  }

  /** The face signal at the audio clock's `now`. */
  signalAt(now: number): FaceSignal {
    const e = this.queue.find((x) => x.start <= now && now < x.start + x.buffer.duration);
    if (!e) return RESTING;
    const ms = (now - e.start) * 1000 + VISEME_LEAD_MS;
    const level = Math.min(1, Math.max(0, this.audio.level()));
    return { phase: "speaking", level: Math.round(level * 100) / 100, viseme: visemeAt(e.visemes, ms), open: 0, generation: e.generation };
  }

  subscribe(listener: (signal: FaceSignal) => void): () => void {
    this.listeners.add(listener);
    listener(this.lastSignal);
    return () => this.listeners.delete(listener);
  }

  private end(): number | null {
    const e = this.queue[this.queue.length - 1];
    return e ? e.start + e.buffer.duration : null;
  }

  private play(e: Entry) {
    const s = this.audio.createBufferSource();
    s.buffer = e.buffer;
    s.connect(this.audio.output as never);
    s.start(e.start);
    e.source = s;
  }

  private silence(e: Entry) {
    if (!e.source) return;
    try {
      e.source.stop();
    } catch {
      // never started, or already stopped: nothing to silence
    }
    e.source.disconnect();
    e.source = null;
  }

  private prune() {
    const now = this.audio.currentTime;
    while (this.queue.length && this.queue[0]!.start + this.queue[0]!.buffer.duration <= now) this.queue.shift();
  }

  /** Over the cap: drop the oldest unstarted utterance and close the gap; keep the newest one. */
  private bound() {
    const now = this.audio.currentTime;
    let changed = false;
    while (this.ahead > MAX_AHEAD_S) {
      const i = this.queue.findIndex((e) => e.start > now);
      if (i < 0 || i === this.queue.length - 1) break;
      this.silence(this.queue[i]!);
      this.queue.splice(i, 1);
      this.dropped++;
      changed = true;
      // everything after it moves up behind what precedes it (a source starts once: remake them)
      let at = i > 0 ? this.queue[i - 1]!.start + this.queue[i - 1]!.buffer.duration : now + START_LEAD_S;
      for (let k = i; k < this.queue.length; k++) {
        const e = this.queue[k]!;
        this.silence(e);
        e.start = Math.max(at, now + START_LEAD_S);
        this.play(e);
        at = e.start + e.buffer.duration;
      }
    }
    return changed;
  }

  /** Emit a signal every frame while anything is held; one resting signal when it all has played. */
  private wake() {
    if (this.stopTick) return;
    this.stopTick = this.clock.subscribe(() => {
      this.prune();
      const s = this.signalAt(this.audio.currentTime);
      this.emit(s);
      if (this.queue.length === 0) {
        this.stopTick = null;
        return false;
      }
      return true;
    });
  }

  private emit(s: FaceSignal) {
    const p = this.lastSignal;
    if (p.phase === s.phase && p.level === s.level && p.viseme === s.viseme && p.generation === s.generation) return;
    this.lastSignal = s;
    for (const l of this.listeners) l(s);
  }
}
