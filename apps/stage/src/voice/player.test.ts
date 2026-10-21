// The voice player on a fake AudioContext: gapless order, the seq rules, the cap, and the face
// signal read off the audio clock.
import { describe, expect, it } from "vitest";

import { createClock } from "../avatar/motion";
import type { FaceSignal } from "../avatar/types";
import type { Utterance } from "./frame";
import { MAX_AHEAD_S, START_LEAD_S, VISEME_LEAD_MS, VoicePlayer, type AudioLike, type BufferLike, type SourceLike } from "./player";

const RATE = 1000;

/** PCM16LE samples as base64. */
function pcm(samples: readonly number[]): string {
  let bin = "";
  for (const v of samples) bin += String.fromCharCode(v & 0xff, (v >> 8) & 0xff);
  return btoa(bin);
}

class FakeAudio implements AudioLike {
  currentTime = 10;
  levelNow = 0.5;
  readonly output = {};
  readonly started: { at: number; buffer: BufferLike; stopped: boolean }[] = [];
  createBuffer(_ch: number, length: number, sampleRate: number): BufferLike {
    const data = new Float32Array(length);
    return { duration: length / sampleRate, getChannelData: () => data };
  }
  createBufferSource(): SourceLike {
    const rec = { at: NaN, buffer: null as unknown as BufferLike, stopped: false };
    const src: SourceLike = {
      buffer: null,
      connect: () => undefined,
      start: (when) => {
        rec.at = when;
        rec.buffer = src.buffer!;
        this.started.push(rec);
      },
      stop: () => {
        rec.stopped = true;
      },
      disconnect: () => undefined,
    };
    return src;
  }
  level() {
    return this.levelNow;
  }
  /** Sources that will still sound, in start order. */
  live() {
    return this.started.filter((s) => !s.stopped).sort((a, b) => a.at - b.at);
  }
}

function handClock() {
  let pending: ((t: number) => void) | null = null;
  let now = 0;
  const clock = createClock({ raf: (cb) => ((pending = cb), 1), caf: () => (pending = null), hidden: () => false, onVisible: () => {} });
  return {
    clock,
    tick() {
      const cb = pending;
      pending = null;
      now += 16;
      cb?.(now);
    },
  };
}

/** An utterance of `ms` milliseconds of audio at 1 kHz. */
function utt(seq: number, part: number, ms: number, extra: Partial<Utterance> = {}): Utterance {
  return {
    type: "utterance",
    id: `r:${seq}:${part}`,
    run_id: "r",
    seq,
    part,
    last: false,
    text: "x",
    sample_rate: RATE,
    duration_ms: ms,
    pcm: pcm(new Array<number>(ms).fill(1000)),
    ...extra,
  };
}

function rig() {
  const audio = new FakeAudio();
  const h = handClock();
  const player = new VoicePlayer(audio, h.clock);
  const signals: FaceSignal[] = [];
  player.subscribe((s) => signals.push(s));
  return { audio, h, player, signals };
}

describe("scheduling", () => {
  it("plays parts back to back with no gap, the first a little ahead of now", () => {
    const { audio, player } = rig();
    expect(player.accept(utt(4, 0, 500))).toBe("scheduled");
    expect(player.accept(utt(4, 1, 300))).toBe("scheduled");
    expect(player.accept(utt(6, 0, 200))).toBe("scheduled");
    const s = audio.live();
    expect(s[0]!.at).toBeCloseTo(10 + START_LEAD_S, 9);
    expect(s[1]!.at).toBeCloseTo(s[0]!.at + 0.5, 9);
    expect(s[2]!.at).toBeCloseTo(s[1]!.at + 0.3, 9);
    expect(player.scheduled().map((e) => [e.seq, e.part])).toEqual([
      [4, 0],
      [4, 1],
      [6, 0],
    ]);
  });

  it("after a silence the next line starts from now, not from where the last one ended", () => {
    const { audio, player } = rig();
    player.accept(utt(1, 0, 200));
    audio.currentTime = 20;
    player.accept(utt(2, 0, 200));
    expect(audio.live()[1]!.at).toBeCloseTo(20 + START_LEAD_S, 9);
  });

  it("drops a frame at or behind what was already accepted", () => {
    const { audio, player } = rig();
    player.accept(utt(5, 1, 100));
    expect(player.accept(utt(5, 1, 100))).toBe("behind");
    expect(player.accept(utt(5, 0, 100))).toBe("behind");
    expect(player.accept(utt(3, 4, 100))).toBe("behind");
    expect(player.accept(utt(5, 2, 100))).toBe("scheduled");
    expect(audio.live()).toHaveLength(2);
  });

  it("a new run starts afresh: the old run's audio stops and its seqs no longer count", () => {
    const { audio, player } = rig();
    player.accept(utt(9, 0, 400));
    expect(player.accept(utt(1, 0, 100, { run_id: "s" }))).toBe("scheduled");
    expect(audio.started[0]!.stopped).toBe(true);
    expect(player.scheduled().map((e) => e.seq)).toEqual([1]);
  });

  it("past the cap the oldest unstarted utterance goes and the rest close up", () => {
    const { audio, player } = rig();
    const n = Math.ceil(MAX_AHEAD_S / 1.5) + 2;
    for (let i = 1; i <= n; i++) player.accept(utt(i, 0, 1500));
    expect(player.ahead).toBeLessThanOrEqual(MAX_AHEAD_S + 1e-9);
    expect(player.dropped).toBeGreaterThan(0);
    const kept = player.scheduled();
    // the newest is always kept, the order holds, and there are no gaps
    expect(kept[kept.length - 1]!.seq).toBe(n);
    for (let i = 1; i < kept.length; i++) {
      expect(kept[i]!.seq).toBeGreaterThan(kept[i - 1]!.seq);
      expect(kept[i]!.start).toBeCloseTo(kept[i - 1]!.end, 9);
    }
    // what sounds is exactly what is kept
    expect(audio.live().map((s) => s.at)).toEqual(kept.map((k) => k.start));
  });

  it("past the cap whole lines go, the oldest unstarted first, never part of a started line", () => {
    const { audio, player } = rig();
    // Line 1 starts playing; its later parts arrive while it plays.
    player.accept(utt(1, 0, 1000));
    audio.currentTime = 10.5;
    player.accept(utt(1, 1, 2000));
    player.accept(utt(1, 2, 2000));
    // Line 2 (three parts) and line 3 (two parts) queue behind it: 12.5 s ahead in all.
    player.accept(utt(2, 0, 1500));
    player.accept(utt(2, 1, 1500));
    player.accept(utt(2, 2, 1500));
    player.accept(utt(3, 0, 1500));
    player.accept(utt(3, 1, 1500));
    // Line 2 went whole; line 1 (started) is intact and line 3 (the newest) is kept whole.
    expect(player.scheduled().map((e) => [e.seq, e.part])).toEqual([
      [1, 0],
      [1, 1],
      [1, 2],
      [3, 0],
      [3, 1],
    ]);
    expect(player.droppedLines).toBe(1);
    expect(player.dropped).toBe(3);
    const kept = player.scheduled();
    for (let i = 1; i < kept.length; i++) expect(kept[i]!.start).toBeCloseTo(kept[i - 1]!.end, 9);
    expect(audio.live().map((s) => s.at)).toEqual(kept.map((k) => k.start));
    // A part of the dropped line arriving late is behind what was accepted.
    expect(player.accept(utt(2, 3, 500))).toBe("behind");
  });

  it("a started line over the cap is kept whole, even alone", () => {
    const { audio, player } = rig();
    player.accept(utt(7, 0, 1000));
    audio.currentTime = 10.5;
    for (let p = 1; p <= 5; p++) player.accept(utt(7, p, 2000));
    expect(player.ahead).toBeGreaterThan(MAX_AHEAD_S);
    expect(player.scheduled()).toHaveLength(6);
    expect(player.droppedLines).toBe(0);
  });

  it("an empty or undecodable utterance is refused", () => {
    const { player } = rig();
    expect(player.accept(utt(1, 0, 0))).toBe("bad");
    expect(player.accept({ ...utt(2, 0, 10), pcm: "%%%" })).toBe("bad");
  });
});

describe("the face signal follows the audio clock", () => {
  const spans: Utterance["visemes"] = [
    { v: "kk", start_ms: 0, end_ms: 100 },
    { v: "aa", start_ms: 100, end_ms: 400 },
  ];

  it("reads the viseme at currentTime minus the utterance's start (with the lead), and the level", () => {
    const { audio, player } = rig();
    player.accept(utt(1, 0, 500, { visemes: spans }));
    const start = player.scheduled()[0]!.start;
    expect(player.signalAt(start - 0.01).phase).toBe("idle");
    const early = player.signalAt(start + 0.02);
    expect(early).toMatchObject({ phase: "speaking", viseme: "kk", level: 0.5 });
    // 70 ms in reads 105 ms with the lead: already the vowel
    expect(player.signalAt(start + (100 - VISEME_LEAD_MS + 5) / 1000).viseme).toBe("aa");
    expect(player.signalAt(start + 0.45).viseme).toBe("sil");
    audio.levelNow = 0.9;
    expect(player.signalAt(start + 0.2).level).toBe(0.9);
    expect(player.signalAt(start + 0.5).phase).toBe("idle");
  });

  it("without mouth shapes it speaks with no viseme, so the mouth follows the level", () => {
    const { player } = rig();
    player.accept(utt(1, 0, 300));
    const start = player.scheduled()[0]!.start;
    expect(player.signalAt(start + 0.1)).toMatchObject({ phase: "speaking", viseme: null, level: 0.5 });
  });

  it("emits per frame while audio is held, then one resting signal, then no frames", () => {
    const { audio, h, player, signals } = rig();
    player.accept(utt(1, 0, 300, { visemes: spans }));
    const start = player.scheduled()[0]!.start;
    audio.currentTime = start + 0.05;
    h.tick();
    audio.currentTime = start + 0.2;
    h.tick();
    audio.currentTime = start + 1;
    h.tick();
    expect(signals.map((s) => [s.phase, s.viseme])).toEqual([
      ["idle", null],
      ["speaking", "kk"],
      ["speaking", "aa"],
      ["idle", null],
    ]);
    expect(player.playing).toBe(false);
  });
});
