// The playout clock: heard time (the context time minus the device's output latency, monotone)
// and the wall time a sample is heard (from the browser's output timestamp), and the captions
// read on it.
import { describe, expect, it, vi } from "vitest";

import { VoiceCaptions } from "./captions";
import type { AudioLike, BufferLike, SourceLike } from "./player";
import { PlayoutClock, playoutOf } from "./playout";
import { heardCaptions } from "./useVoice";

class FakeAudio implements AudioLike {
  currentTime = 12.05;
  outputLatency: number | undefined = 0.1;
  stamp: { contextTime: number; performanceTime: number } | undefined = undefined;
  readonly output = {};
  createBuffer(_c: number, len: number, rate: number): BufferLike {
    const d = new Float32Array(len);
    return { duration: len / rate, getChannelData: () => d };
  }
  createBufferSource(): SourceLike {
    return { buffer: null, connect: () => undefined, start: () => {}, stop: () => {}, disconnect: () => {} };
  }
  level() {
    return 0;
  }
  outputTimestamp() {
    return this.stamp;
  }
}

describe("heard time", () => {
  it("is the context time minus the output latency; with no latency reported, the context time", () => {
    const audio = new FakeAudio();
    expect(new PlayoutClock(audio).heardNow()).toBeCloseTo(11.95, 9);
    audio.outputLatency = undefined;
    expect(new PlayoutClock(audio).heardNow()).toBeCloseTo(12.05, 9);
    const bare: AudioLike = { currentTime: 12.05, output: {}, createBuffer: audio.createBuffer, createBufferSource: audio.createBufferSource, level: () => 0 };
    expect(new PlayoutClock(bare).heardNow()).toBe(12.05);
    expect(new PlayoutClock(bare).latency()).toBe(0);
  });

  it("never goes backwards when the latency rises (a device swap), and the next wall time uses the new latency", () => {
    const audio = new FakeAudio();
    let wall = 2_000_000;
    const clock = new PlayoutClock(audio, { wall: () => wall });
    audio.currentTime = 12.0;
    audio.outputLatency = 0.02;
    expect(clock.heardNow()).toBeCloseTo(11.98, 9);
    audio.currentTime = 12.01;
    audio.outputLatency = 0.2;
    expect(clock.heardNow()).toBeCloseTo(11.98, 9);
    audio.currentTime = 12.3;
    expect(clock.heardNow()).toBeCloseTo(12.1, 9);
    // no output timestamp: the wall now, plus how far the sample is from the heard time now
    wall = 2_000_500;
    expect(clock.wallOf(12.0)).toBeCloseTo(2_000_500 - 100, 6);
  });

  it("one clock per context, so the clamp is shared by every reader", () => {
    const audio = new FakeAudio();
    expect(playoutOf(audio)).toBe(playoutOf(audio));
    expect(playoutOf(new FakeAudio())).not.toBe(playoutOf(audio));
  });
});

describe("the wall time a sample is heard", () => {
  it("comes from the output timestamp, which already includes the latency", () => {
    const audio = new FakeAudio();
    audio.stamp = { contextTime: 11.9, performanceTime: 500 };
    const clock = new PlayoutClock(audio, { wall: () => 9_999_999, perfWall: (p) => 999_500 + p });
    expect(clock.wallOf(12.0)).toBeCloseTo(1_000_100, 6);
  });

  it("an empty timestamp (a context not yet running reports zeros) falls back to the wall", () => {
    const audio = new FakeAudio();
    audio.stamp = { contextTime: 0, performanceTime: 0 };
    audio.outputLatency = 0;
    const clock = new PlayoutClock(audio, { wall: () => 5_000, perfWall: (p) => 999_500 + p });
    expect(clock.wallOf(12.05)).toBeCloseTo(5_000, 6);
  });
});

describe("the captions read heard time", () => {
  const said = [{ id: 1, seq: 3, text: "Kneel before the crown.", at: 0, replyingTo: [] }];
  const part = { runId: "r", seq: 3, part: 0, text: "Kneel before the crown.", last: true, start: 12.0, duration: 1.5 };

  it("a part at context 12.0 is not yet heard at context 12.05 with 0.1 s of output latency", () => {
    const audio = new FakeAudio();
    const captions = new VoiceCaptions();
    captions.scheduled(part);
    // on the context's own clock the first word would already show
    expect(captions.view("r", said, 12.05, 0)?.shown).toBe(1);
    const fresh = new VoiceCaptions();
    fresh.scheduled(part);
    const h = heardCaptions(fresh, new PlayoutClock(audio));
    expect(h.view("r", said, 0)?.shown ?? 0).toBe(0);
    audio.currentTime = 12.1;
    expect(h.view("r", said, 0)?.shown).toBe(1);
  });

  it("an unvoiced line is stamped on the same heard clock", () => {
    const audio = new FakeAudio();
    const captions = new VoiceCaptions();
    const spy = vi.spyOn(captions, "unvoiced");
    heardCaptions(captions, new PlayoutClock(audio)).unvoiced("r", 3);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0]![2]).toBeCloseTo(11.95, 9);
  });
});
