// The voice player: schedules utterances back to back on one AudioContext and turns what is
// playing into the face's signal.
//
// **The audio is the one clock, read as heard.** Each utterance is scheduled at an exact
// AudioContext time; the face reads the mouth shape at the heard time (`PlayoutClock.heardNow()`:
// the context time minus the device's output latency) minus that utterance's start, plus a 35 ms
// lead (the mouth leads the heard sound a little, as a face does; the renderer owns the lead), and
// the loudness from an AnalyserNode on the output. Neither the wall clock nor a frame's arrival
// time moves the mouth. Scheduling, the cap and `ahead` stay on the context's own time.
//
// **Bounded and in order.** Parts play gaplessly in (seq, part) order. A frame at or behind what
// was already accepted is dropped. When more than `MAX_AHEAD_S` of audio is waiting, the oldest
// line that has not started is dropped whole (every part of that seq) and the rest close up
// behind what is playing, so a stage that fell behind catches up instead of talking ever later.
// A line that has started is never cut, and the newest line is kept. A new run starts afresh.
//
// **Whose mouth.** A guest's utterance (`speaker: foil`, SEG-03) plays through the same FIFO, so
// the two voices never overlap, but it never moves Malakor's mouth: while it sounds his signal
// rests, and `foilAt` names the guest heard instead (the two-shot lights the guest's glow).
//
// **Told as it happens.** `PlayerEvents` hears each part as it is scheduled (and rescheduled when
// the cap closes a gap), as its audio starts, and each whole line the cap drops: the captions
// follow the first, and the daemon is told the other two (`report.ts`).

import { RESTING, type FaceSignal, type FaceSource } from "../avatar/types";
import type { Clock } from "../avatar/motion";
import { decodePcm16, visemeAt, type Utterance } from "./frame";
import { PlayoutClock } from "./playout";

/**
 * Audio waiting ahead of the playhead, at most (seconds). The brain paces a beat to a 260-character
 * speech budget (~14 s) plus a 1.5 s lead (`shadowrunr-brain` pace.rs), so one paced beat fits
 * whole; only a stage that has fallen a whole beat behind drops a line.
 */
export const MAX_AHEAD_S = 16;
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
  /** Seconds from the context's output to the speakers (`AudioContext.outputLatency`), if reported. */
  readonly outputLatency?: number | undefined;
  /** The context time at the speakers and the performance time (ms) it was there, if reported. */
  outputTimestamp?(): { contextTime?: number | undefined; performanceTime?: number | undefined } | undefined;
}

/** One part as the player placed it on the audio clock (seconds). */
export interface ScheduledPart {
  runId: string;
  seq: number;
  part: number;
  /** The part's sentence, as the daemon sent it. */
  text: string;
  /** The line's final part. */
  last: boolean;
  start: number;
  duration: number;
  /** The guest's foil id when the guest speaks it (absent: the host's line). */
  foil?: string;
}

/** What the player tells its owner as it happens. Every hook is optional. */
export interface PlayerEvents {
  /** A part was placed at `start`, or moved there when the cap closed a gap behind a dropped line. */
  scheduled?(p: ScheduledPart): void;
  /** A part's audio was heard; `lateS` is how far heard time had passed its start when seen. */
  started?(p: ScheduledPart, lateS: number): void;
  /** A whole line was dropped to keep the audio within MAX_AHEAD_S. */
  dropped?(runId: string, seq: number): void;
}

interface Entry {
  runId: string;
  seq: number;
  part: number;
  text: string;
  last: boolean;
  /** Its start has been told (`PlayerEvents.started`). */
  told: boolean;
  /** The guest's foil id for a guest utterance; null: the host's own voice. */
  foil: string | null;
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
  /** Utterances dropped for falling behind (since the last run change), whole lines at a time. */
  dropped = 0;
  /** Lines dropped for falling behind (since the last run change). */
  droppedLines = 0;

  constructor(
    private readonly audio: AudioLike,
    private readonly clock: Clock,
    private readonly events: PlayerEvents = {},
    /** When the audio is heard (default: a clock of its own on `audio`). */
    readonly playout: PlayoutClock = new PlayoutClock(audio),
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
    const e: Entry = {
      runId: u.run_id,
      seq: u.seq,
      part: u.part,
      text: u.text,
      last: u.last,
      told: false,
      foil: u.speaker === "foil" ? (u.foil ?? "foil") : null,
      generation: ++this.generation,
      buffer,
      visemes: u.visemes,
      start,
      source: null,
    };
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
    this.droppedLines = 0;
  }

  dispose(): void {
    this.reset();
    this.stopTick?.();
    this.stopTick = null;
    this.listeners.clear();
  }

  /** The utterance under heard time `now` plus the mouth's lead, if any. */
  private at(now: number): Entry | undefined {
    const t = now + VISEME_LEAD_MS / 1000;
    return this.queue.find((x) => x.start <= t && t < x.start + x.buffer.duration);
  }

  /** The guest heard at `now` (context seconds): its foil id while a guest utterance sounds, else null. */
  foilAt(now: number): string | null {
    return this.at(now)?.foil ?? null;
  }

  /** The face signal at heard time `now` (context seconds): the mouth at `now` plus its lead. A
   * guest's utterance is not his: his face rests under it. */
  signalAt(now: number): FaceSignal {
    const t = now + VISEME_LEAD_MS / 1000;
    const e = this.at(now);
    if (!e || e.foil !== null) return RESTING;
    const ms = Math.max(0, (t - e.start) * 1000);
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
    this.events.scheduled?.(part(e));
  }

  /** Tell each part whose start has been heard, once. */
  private tell(now: number) {
    for (const e of this.queue) {
      if (e.start > now) break;
      if (e.told) continue;
      e.told = true;
      this.events.started?.(part(e), now - e.start);
    }
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

  /** Tell what has been heard and forget what has been heard whole. */
  private prune() {
    const now = this.playout.heardNow();
    this.tell(now);
    while (this.queue.length && this.queue[0]!.start + this.queue[0]!.buffer.duration <= now) this.queue.shift();
  }

  /**
   * Over the cap: drop the oldest line none of whose parts has started, all of its parts, and
   * close the gap. A started line is never cut; the newest line is kept.
   */
  private bound() {
    const now = this.audio.currentTime;
    let changed = false;
    while (this.ahead > MAX_AHEAD_S) {
      const started = new Set(this.queue.filter((e) => e.start <= now).map((e) => e.seq));
      const newest = this.queue[this.queue.length - 1]!.seq;
      const i = this.queue.findIndex((e) => !started.has(e.seq) && e.seq !== newest);
      if (i < 0) break;
      const { seq, runId } = this.queue[i]!;
      let n = 0;
      while (this.queue[i]?.seq === seq) {
        this.silence(this.queue[i]!);
        this.queue.splice(i, 1);
        n++;
      }
      this.dropped += n;
      this.droppedLines++;
      this.events.dropped?.(runId, seq);
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
      const s = this.signalAt(this.playout.heardNow());
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

function part(e: Entry): ScheduledPart {
  const p: ScheduledPart = { runId: e.runId, seq: e.seq, part: e.part, text: e.text, last: e.last, start: e.start, duration: e.buffer.duration };
  if (e.foil !== null) p.foil = e.foil;
  return p;
}
