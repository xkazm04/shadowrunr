// The performance feed: a host line's delivery, learned by its `say` seq, becomes the face's cue
// only when that line's part 0 starts playing on the voice player (or, with no voice, when its
// caption shows).
import { describe, expect, it } from "vitest";

import type { StreamEvent } from "@shadowrunr/contract";
import { initialScene, reduce } from "@shadowrunr/stage-kit";

import { createClock } from "../avatar/motion";
import { NEUTRAL_CUE, performanceBox, type PerformanceCue } from "../avatar/performance";
import { LiveStore } from "../live";
import { DeliveryFeed, eventsOf, findSay, learnLines, silentCaption } from "./delivery";
import type { Utterance } from "./frame";
import { VoicePlayer, type AudioLike, type BufferLike, type SourceLike } from "./player";

const start: StreamEvent = { type: "stream_start", ts_ms: 0, stream_id: "s-1", character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 30 };

/** An AudioContext that only keeps time: sources start and are never heard. */
class ClockAudio implements AudioLike {
  currentTime = 10;
  readonly output = {};
  createBuffer(_ch: number, length: number, rate: number): BufferLike {
    const data = new Float32Array(length);
    return { duration: length / rate, getChannelData: () => data };
  }
  createBufferSource(): SourceLike {
    return { buffer: null, connect: () => undefined, start: () => undefined, stop: () => undefined, disconnect: () => undefined };
  }
  level() {
    return 0.5;
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

function utt(seq: number, part: number, ms: number): Utterance {
  let bin = "";
  for (let i = 0; i < ms; i++) bin += String.fromCharCode(0xe8, 0x03);
  return { type: "utterance", id: `r:${seq}:${part}`, run_id: "r", seq, part, last: part === 1, text: "x", sample_rate: 1000, duration_ms: ms, pcm: btoa(bin), visemes: [] };
}

describe("the performance feed", () => {
  it("cues menace 0.8 for say seq 5 only when utterance seq 5 part 0 starts", () => {
    const box = performanceBox();
    const cues: PerformanceCue[] = [];
    box.subscribe((c) => cues.push(c));
    const feed = new DeliveryFeed(box);

    // the live feed: a stream start, three quiet events, then the host's line at seq 5 and a foil's at 6
    const store = new LiveStore(0);
    const events: StreamEvent[] = [
      start,
      { type: "say", ts_ms: 100, speaker: "host", text: "Kneel." },
      { type: "say", ts_ms: 200, speaker: "foil", foil: "f", text: "Never." },
      { type: "say", ts_ms: 300, speaker: "host", text: "Rise." },
      { type: "say", ts_ms: 400, speaker: "host", text: "You will all kneel.", delivery: { emotion: "menace", intensity: 0.8, pace: "slow" } },
      { type: "say", ts_ms: 500, speaker: "foil", foil: "f", text: "Make me.", delivery: { emotion: "outrage", intensity: 1, pace: "fast" } },
    ];
    events.forEach((e, i) => store.accept({ kind: "event", event: e, seq: i + 1, runId: "r" }, 0));
    learnLines(feed, store);
    expect(feed.knows("r", 5)).toBe(true);
    expect(feed.knows("r", 6)).toBe(false); // a foil's line is not his face's
    expect(cues).toEqual([]); // the text arriving is not the line being heard

    // the voice: seq 5 in two parts, then the foil's seq 6, on a player whose audio clock we move
    const audio = new ClockAudio();
    const h = handClock();
    const player = new VoicePlayer(audio, h.clock, { started: (p) => feed.started(p) });
    player.accept(utt(5, 0, 400));
    player.accept(utt(5, 1, 300));
    player.accept(utt(6, 0, 300));
    h.tick();
    expect(cues).toEqual([]); // scheduled, not yet heard

    audio.currentTime = 10.1; // part 0 of seq 5 is heard
    h.tick();
    expect(cues).toEqual([{ id: 1, emotion: "menace", intensity: 0.8, gesture: "glare" }]);

    audio.currentTime = 10.5; // part 1 of seq 5: no second cue
    h.tick();
    audio.currentTime = 10.8; // the foil's seq 6: none either
    h.tick();
    expect(cues).toHaveLength(1);
  });

  it("a host line with no delivery cues neutral at 0, so the last emotion does not linger", () => {
    const box = performanceBox();
    const feed = new DeliveryFeed(box);
    feed.said("r", 3, undefined);
    expect(feed.started({ runId: "r", seq: 3, part: 0 })).toBe(true);
    expect(box.get()).toEqual({ id: 1, emotion: "neutral", intensity: 0 });
    // another run's seq 3 is not this one
    expect(feed.started({ runId: "q", seq: 3, part: 0 })).toBe(false);
  });

  it("with no voice, a caption cues its line's delivery once as it goes on screen", () => {
    const box = performanceBox();
    const feed = new DeliveryFeed(box);
    const events: StreamEvent[] = [start, { type: "say", ts_ms: 1000, speaker: "host", text: "Behold my glory.", delivery: { emotion: "triumph", intensity: 0.9, pace: "normal" } }];
    const scene = events.reduce(reduce, initialScene());
    expect(silentCaption(scene, 500, undefined)).toBeNull();
    const shown = silentCaption(scene, 1200, undefined)!;
    expect(shown.saidAt).toBe(1000);
    const said = findSay(eventsOf(events), shown.saidAt!, shown.text);
    expect(feed.captionShown(shown.key, said?.delivery)).toBe(true);
    expect(feed.captionShown(silentCaption(scene, 1400, undefined)!.key, said?.delivery)).toBe(false);
    expect(box.get()).toEqual({ id: 1, emotion: "triumph", intensity: 0.9, gesture: "flare" });
    expect(box.get()).not.toBe(NEUTRAL_CUE);
  });
});
