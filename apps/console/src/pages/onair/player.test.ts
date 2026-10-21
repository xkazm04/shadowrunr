// The replay player is event-driven: one timer per due event (coalesced), none between them,
// and pausing leaves nothing scheduled. Counts are asserted, never timings.
import type { StreamEvent } from "@shadowrunr/contract";
import { foldEvents } from "@shadowrunr/stage-kit";
import { describe, expect, it } from "vitest";
import { manualTimers } from "../../data/testing";
import { COALESCE_MS, LOOP_HOLD_MS, LiveFeed, ReplayPlayer } from "./player";
import { transcript } from "./testdata";

function rig(events: StreamEvent[], speed = 1, loop = true) {
  const timers = manualTimers();
  let wall = 0;
  let scheduled = 0;
  const counted = {
    setTimeout: (fn: () => void, ms: number) => {
      scheduled++;
      return timers.setTimeout(fn, ms);
    },
    clearTimeout: (h: number) => timers.clearTimeout(h),
  };
  const now = () => wall;
  const player = new ReplayPlayer(events, { timers: counted, now, speed, loop });
  const advance = (ms: number) => {
    // Move the wall clock with the manual timers, so the player's clock agrees with them.
    const step = 10;
    for (let t = 0; t < ms; t += step) {
      wall += step;
      timers.advance(step);
    }
  };
  return { player, timers, advance, scheduled: () => scheduled };
}

describe("ReplayPlayer", () => {
  it("shows the first event before playing and plays to the end on schedule", () => {
    const events = transcript(200);
    const { player, advance } = rig(events);
    expect(player.getSnapshot().scene.meta?.streamId).toBe("s1");
    expect(player.getSnapshot().playing).toBe(false);
    player.play();
    const last = events[events.length - 1]?.ts_ms ?? 0;
    advance(last + COALESCE_MS * 2);
    expect(player.getSnapshot().scene.applied).toBe(events.length);
  });

  it("schedules at most one timer per due event and nothing while paused", () => {
    const events = transcript(500);
    const { player, advance, scheduled, timers } = rig(events, 4);
    player.play();
    advance(2000);
    const during = scheduled();
    expect(during).toBeGreaterThan(0);
    expect(during).toBeLessThanOrEqual(events.length);
    player.pause();
    expect(timers.pending()).toEqual([]);
    const at = player.getSnapshot().scene.applied;
    advance(5000);
    expect(player.getSnapshot().scene.applied).toBe(at);
  });

  it("restarts from the first event and loops after the hold", () => {
    const events = transcript(50);
    const { player, advance } = rig(events, 10);
    player.play();
    const span = (events[events.length - 1]?.ts_ms ?? 0) / 10;
    advance(span + 200);
    expect(player.getSnapshot().scene.applied).toBe(events.length);
    const epoch = player.getSnapshot().epoch;
    let looped: number | undefined;
    player.subscribe(() => {
      if (looped === undefined && player.getSnapshot().epoch > epoch) looped = player.getSnapshot().scene.applied;
    });
    advance(LOOP_HOLD_MS + 20);
    expect(looped).toBe(1);
    player.restart();
    expect(player.getSnapshot().position).toBe(0);
  });

  it("stops at the end when it does not loop", () => {
    const events = transcript(30);
    const { player, advance, timers } = rig(events, 10, false);
    player.play();
    advance(2000);
    expect(player.getSnapshot().playing).toBe(false);
    expect(timers.pending()).toEqual([]);
  });

  it("notifies subscribers once per applied batch", () => {
    const { player, advance } = rig(transcript(100), 4);
    let calls = 0;
    const off = player.subscribe(() => calls++);
    player.play();
    advance(200);
    expect(calls).toBeGreaterThan(1);
    off();
    const before = calls;
    advance(200);
    expect(calls).toBe(before);
    player.dispose();
  });
});

describe("ReplayPlayer.seek", () => {
  it("paused at the start, seek puts the fold up to that moment on screen and schedules nothing", () => {
    const events = transcript(1000);
    const { player, timers } = rig(events, 4);
    player.seek(30_000);
    const snap = player.getSnapshot();
    expect(snap.position).toBe(30_000);
    expect(snap.scene).toEqual(foldEvents(events.filter((e) => e.ts_ms <= 30_000)));
    expect(snap.playing).toBe(false);
    expect(timers.pending()).toEqual([]);
  });

  it("playing, a backward seek rebuilds the scene, keeps playing on one timer and bumps the epoch", () => {
    const events = transcript(3000);
    const { player, timers, advance } = rig(events, 4);
    player.play();
    advance(90_000 / 4);
    expect(player.getSnapshot().position).toBeGreaterThanOrEqual(89_000);
    const epoch = player.getSnapshot().epoch;
    player.seek(10_000);
    const snap = player.getSnapshot();
    expect(snap.scene.applied).toBe(events.filter((e) => e.ts_ms <= 10_000).length);
    expect(snap.position).toBe(10_000);
    expect(snap.playing).toBe(true);
    expect(timers.pending()).toHaveLength(1);
    expect(snap.epoch).toBeGreaterThan(epoch);
  });

  it("clamps a seek to the run's span", () => {
    const events = transcript(100);
    const { player } = rig(events);
    player.seek(-5_000);
    expect(player.getSnapshot().position).toBe(player.getSnapshot().start);
    player.seek(10_000_000);
    expect(player.getSnapshot().position).toBe(player.getSnapshot().end);
    expect(player.getSnapshot().scene.applied).toBe(events.length);
  });
});

describe("LiveFeed", () => {
  it("folds events and starts a new scene for a new run", () => {
    const live = new LiveFeed();
    const ev = transcript(20);
    for (const e of ev) live.push(e, "r1");
    expect(live.getSnapshot().scene.applied).toBe(20);
    live.push({ type: "segment_start", ts_ms: 5, segment: "SEG-02A" }, "r2");
    expect(live.getSnapshot().scene.applied).toBe(1);
    expect(live.getSnapshot().scene.segment).toBe("SEG-02A");
  });

  it("reset() starts over from an empty scene, still live", () => {
    const live = new LiveFeed();
    for (const e of transcript(10)) live.push(e, "r1");
    let notified = 0;
    live.subscribe(() => notified++);
    live.reset();
    expect(live.getSnapshot().scene.applied).toBe(0);
    expect(live.getSnapshot().live).toBe(true);
    expect(notified).toBe(1);
    // The same run id after a reset is folded as a fresh run.
    live.push({ type: "segment_start", ts_ms: 5, segment: "SEG-02A" }, "r1", 1);
    expect(live.getSnapshot().scene.applied).toBe(1);
  });

  it("a gap holds until the next contiguous seq", () => {
    const live = new LiveFeed();
    const ev = transcript(5);
    ev.forEach((e, i) => live.push(e, "r1", i + 1));
    live.markGap();
    expect(live.getSnapshot().gap).toBe(true);
    const chat = ev[4];
    if (!chat) throw new Error("no event");
    live.push(chat, "r1", 9); // the frame past the hole: still catching up
    expect(live.getSnapshot().gap).toBe(true);
    live.push(chat, "r1", 10);
    expect(live.getSnapshot().gap).toBe(false);
  });

  it("seek is a no-op on the live feed", () => {
    const live = new LiveFeed();
    for (const e of transcript(10)) live.push(e, "r1");
    const before = live.getSnapshot();
    let notified = 0;
    live.subscribe(() => notified++);
    live.seek(0);
    expect(live.getSnapshot()).toBe(before);
    expect(notified).toBe(0);
  });
});
