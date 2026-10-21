// The live preview's DVR transport: follow the head, fall behind on a seek or a pause, play back
// at speed until the head is reached, and go live again. The broadcast view never builds any of it.
import { describe, expect, it } from "vitest";

import type { StreamEvent } from "@shadowrunr/contract";
import { FOLLOW, dvrFor, dvrReduce, liveView, type Dvr } from "./dvr";
import { LiveStore } from "./live";

const win = { start: 10_000, head: 100_000 };

const start = (ts: number): StreamEvent => ({
  type: "stream_start",
  ts_ms: ts,
  stream_id: "s-1",
  character: "malakor",
  tone_variant: "theatrical",
  core_tier: "haiku",
  audience_size: 30,
});
const say = (ts: number, text: string): StreamEvent => ({ type: "say", ts_ms: ts, speaker: "host", text });

function store(): LiveStore {
  const s = new LiveStore(0);
  [start(0), say(1000, "Kneel."), say(60_000, "Rise.")].forEach((e, i) => s.accept({ kind: "event", event: e, seq: i + 1, runId: "r" }, 0));
  return s;
}

describe("dvr transport", () => {
  it("follow --seek(t)--> behind(t), clamped to the held window", () => {
    expect(dvrReduce(FOLLOW, { type: "seek", t: 40_000, win })).toEqual({ mode: "behind", t: 40_000, playing: true });
    expect(dvrReduce(FOLLOW, { type: "seek", t: 0, win })).toEqual({ mode: "behind", t: 10_000, playing: true });
    // a seek at or past the head is following again
    expect(dvrReduce({ mode: "behind", t: 40_000, playing: false }, { type: "seek", t: 100_000, win })).toBe(FOLLOW);
    // a seek keeps a paused preview paused
    expect(dvrReduce({ mode: "behind", t: 40_000, playing: false }, { type: "seek", t: 20_000, win })).toEqual({ mode: "behind", t: 20_000, playing: false });
  });

  it("play advances at speed and returns to follow on reaching the head", () => {
    let d: Dvr = { mode: "behind", t: 90_000, playing: true };
    d = dvrReduce(d, { type: "tick", dt: 1000, speed: 2, win });
    expect(d).toEqual({ mode: "behind", t: 92_000, playing: true });
    d = dvrReduce(d, { type: "pause", win });
    expect(dvrReduce(d, { type: "tick", dt: 1000, speed: 2, win })).toBe(d);
    d = dvrReduce(d, { type: "play", win });
    d = dvrReduce(d, { type: "tick", dt: 5000, speed: 2, win });
    expect(d).toBe(FOLLOW);
    // following, a tick changes nothing (the head is the store's own scene)
    expect(dvrReduce(FOLLOW, { type: "tick", dt: 1000, speed: 1, win })).toBe(FOLLOW);
  });

  it("pause while following freezes the head moment; a paused moment that leaves the window clamps", () => {
    const d = dvrReduce(FOLLOW, { type: "pause", win });
    expect(d).toEqual({ mode: "behind", t: 100_000, playing: false });
    expect(dvrReduce(d, { type: "tick", dt: 16, speed: 1, win: { start: 120_000, head: 300_000 } })).toEqual({
      mode: "behind",
      t: 120_000,
      playing: false,
    });
    expect(dvrReduce(FOLLOW, { type: "toggle", win })).toEqual(d);
    // resumed once the head has moved on, it plays from the frozen moment; at the head it follows
    expect(dvrReduce(d, { type: "toggle", win: { start: 10_000, head: 130_000 } })).toEqual({ ...d, playing: true });
    expect(dvrReduce(d, { type: "toggle", win })).toBe(FOLLOW);
  });

  it("step moves from the head while following and from t while behind; goLive -> follow", () => {
    expect(dvrReduce(FOLLOW, { type: "step", by: -5000, win })).toEqual({ mode: "behind", t: 95_000, playing: true });
    expect(dvrReduce({ mode: "behind", t: 50_000, playing: false }, { type: "step", by: 5000, win })).toEqual({
      mode: "behind",
      t: 55_000,
      playing: false,
    });
    expect(dvrReduce({ mode: "behind", t: 50_000, playing: false }, { type: "goLive" })).toBe(FOLLOW);
  });

  it("behind, the preview shows the past scene at t with estimated captions, not the voice's", () => {
    const s = store();
    const followed = liveView(s, FOLLOW, 70_000);
    expect(followed).toEqual({ scene: s.scene, now: 70_000, behindMs: 0, followsVoice: true });
    expect(followed.scene).toBe(s.scene);
    const past = liveView(s, { mode: "behind", t: 30_000, playing: true }, 70_000);
    expect(past.followsVoice).toBe(false);
    expect(past.now).toBe(30_000);
    expect(past.behindMs).toBe(40_000);
    expect(past.scene.caption?.text).toBe("Kneel.");
    expect(s.scene.caption?.text).toBe("Rise.");
  });
});

describe("the broadcast view (guard)", () => {
  it("broadcast=1 has no DVR state, and the canvas always draws the head", () => {
    expect(dvrFor({ live: true, broadcast: true })).toBeNull();
    expect(dvrFor({ live: false, broadcast: false })).toBeNull();
    expect(dvrFor({ live: true, broadcast: false })).toBe(FOLLOW);
    const s = store();
    const v = liveView(s, null, 70_000);
    expect(v.scene).toBe(s.scene);
    expect(v.followsVoice).toBe(true);
    expect(v.now).toBe(70_000);
  });
});
