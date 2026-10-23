// The performance channels: the emotion table, the driver's ease and gesture envelopes, and the
// wrapper that hangs a performance on a face driver's pose.
import { describe, expect, it } from "vitest";

import type { Emotion } from "@shadowrunr/contract";

import { createDriver } from "./driver";
import {
  cueFor,
  cueTarget,
  createPerformanceDriver,
  EMOTION_TABLE,
  GESTURE_MIN,
  GESTURES,
  NEUTRAL_CUE,
  PERFORMANCE_REST,
  performanceAt,
  performanceBox,
  performanceRelay,
  REDUCED_GESTURE,
  withPerformance,
  type Gesture,
  type PerformanceCue,
  type PerformedPose,
} from "./performance";

const EMOTIONS: Emotion[] = ["neutral", "menace", "glee", "deadpan", "hushed", "outrage", "sarcasm", "weary", "triumph", "tender"];
const cue = (id: number, emotion: Emotion, intensity: number, gesture?: Gesture): PerformanceCue => ({ id, emotion, intensity, ...(gesture ? { gesture } : {}) });

/** Step `d` at 60 fps for `s` seconds; the last pose (a copy). */
function run(d: ReturnType<typeof createPerformanceDriver>, s: number) {
  let p = d.pose;
  for (let i = 0; i < Math.round(s * 60); i++) p = d.step(1 / 60);
  return { ...p };
}

describe("the emotion table", () => {
  it("covers all ten emotions", () => {
    expect(Object.keys(EMOTION_TABLE).sort()).toEqual([...EMOTIONS].sort());
  });

  it("is the brief's map: eye light, brow and the strong line's gesture", () => {
    expect(EMOTION_TABLE.menace).toEqual({ eyeLight: 1.6, brow: -0.7, gesture: "glare" });
    expect(EMOTION_TABLE.outrage).toEqual({ eyeLight: 2.2, brow: -1, gesture: "recoil", then: "flare" });
    expect(EMOTION_TABLE.weary).toEqual({ eyeLight: 0.5, brow: -0.2, gesture: "nod" });
    expect(EMOTION_TABLE.neutral).toEqual({ eyeLight: 1, brow: 0 });
    expect(EMOTION_TABLE.deadpan.gesture).toBeUndefined();
  });

  it("intensity 0 is rest for every emotion; intensity scales the distance from rest", () => {
    for (const e of EMOTIONS) {
      expect(cueTarget(cue(1, e, 0))).toEqual({ eyeLight: 1, brow: 0 });
      const half = cueTarget(cue(1, e, 0.5));
      expect(half.eyeLight).toBeCloseTo(1 + (EMOTION_TABLE[e].eyeLight - 1) * 0.5, 12);
      expect(half.brow).toBeCloseTo(EMOTION_TABLE[e].brow * 0.5, 12);
    }
  });

  it("an explicit eye light overrides the emotion's", () => {
    expect(cueTarget({ ...cue(1, "menace", 1), eyeLight: 0.2 }).eyeLight).toBe(0.2);
  });

  it("cueFor: absent delivery is neutral 0; a gesture only from GESTURE_MIN; intensity clamped", () => {
    expect(cueFor(undefined)).toEqual({ emotion: "neutral", intensity: 0 });
    expect(cueFor({ emotion: "menace", intensity: 0.5, pace: "normal" })).toEqual({ emotion: "menace", intensity: 0.5 });
    expect(cueFor({ emotion: "menace", intensity: GESTURE_MIN, pace: "normal" })).toEqual({ emotion: "menace", intensity: GESTURE_MIN, gesture: "glare" });
    expect(cueFor({ emotion: "outrage", intensity: 3, pace: "fast" })).toEqual({ emotion: "outrage", intensity: 1, gesture: "recoil" });
    expect(cueFor({ emotion: "deadpan", intensity: 1, pace: "slow" })).toEqual({ emotion: "deadpan", intensity: 1 });
  });
});

describe("the performance driver", () => {
  it("starts settled at rest, and a neutral cue at 0 keeps it there exactly", () => {
    const d = createPerformanceDriver();
    expect(d.settled).toBe(true);
    d.setCue(cue(1, "neutral", 0));
    expect(run(d, 0.5)).toEqual(PERFORMANCE_REST);
    expect(d.settled).toBe(true);
  });

  it("eases the eye light and brow in about 250 ms; the light flares up faster than it dies", () => {
    const d = createPerformanceDriver();
    d.setCue(cue(1, "outrage", 1));
    d.step(0.05);
    d.step(0.05);
    const up = d.pose.eyeLight;
    // 100 ms in: most of the way up already
    expect(up).toBeGreaterThan(1 + 0.8 * 1.2);
    run(d, 3);
    expect(d.settled).toBe(true);
    expect(d.pose.eyeLight).toBe(2.2);
    expect(d.pose.brow).toBe(-1);
    d.setCue(cue(2, "outrage", 0));
    d.step(0.05);
    d.step(0.05);
    // 100 ms after the fall began it is less than half way down
    expect(d.pose.eyeLight).toBeGreaterThan(1 + 1.2 * 0.5);
  });

  it.each(GESTURES)("%s plays once and returns to rest and settles", (g) => {
    const d = createPerformanceDriver();
    d.setCue(cue(1, "neutral", 1, g));
    expect(d.settled).toBe(false);
    let moved = false;
    for (let i = 0; i < 120; i++) {
      const p = d.step(1 / 60);
      if (p.gesturePitch || p.gestureRoll || p.lean || p.flare || p.eyeLight !== 1 || p.brow !== 0) moved = true;
    }
    expect(moved).toBe(true);
    run(d, 2);
    expect(d.settled).toBe(true);
    expect(d.pose).toEqual(PERFORMANCE_REST);
  });

  it("outrage's recoil is followed by a flare", () => {
    const d = createPerformanceDriver();
    d.setCue(cue(1, "outrage", 1, "recoil"));
    let lean = 0;
    let flare = 0;
    for (let i = 0; i < 60; i++) {
      const p = d.step(1 / 60);
      lean = Math.min(lean, p.lean);
      flare = Math.max(flare, p.flare);
    }
    expect(lean).toBeLessThan(-0.9);
    expect(flare).toBeGreaterThan(0.9);
  });

  it("the same dt sequence gives the same poses", () => {
    const dts = [0.016, 0.017, 0.033, 0.001, 0.05, 0.016, 0.2, 0.016];
    const trace = () => {
      const d = createPerformanceDriver();
      const out: unknown[] = [];
      dts.forEach((dt, i) => {
        if (i === 1) d.setCue(cue(1, "menace", 0.9, "glare"));
        if (i === 5) d.setCue(cue(2, "glee", 0.7, "tilt"));
        out.push({ ...d.step(dt) });
      });
      return out;
    };
    expect(trace()).toEqual(trace());
  });

  it("a repeated cue id does not retrigger its gesture; a new id does", () => {
    const d = createPerformanceDriver();
    const c = cue(1, "neutral", 1, "nod");
    d.setCue(c);
    run(d, 2);
    expect(d.settled).toBe(true);
    d.setCue(c);
    d.step(0.1);
    expect(d.pose.gesturePitch).toBe(0);
    expect(d.settled).toBe(true);
    d.setCue({ ...c, id: 2 });
    d.step(0.1);
    expect(d.pose.gesturePitch).toBeGreaterThan(0);
  });

  it("under reduced motion the head gestures shrink and the light does not", () => {
    const peak = (reduced: boolean, g: Gesture) => {
      const d = createPerformanceDriver({ reduced: () => reduced });
      d.setCue(cue(1, "neutral", 1, g));
      let roll = 0;
      let eye = 0;
      for (let i = 0; i < 90; i++) {
        const p = d.step(1 / 60);
        roll = Math.max(roll, Math.abs(p.gestureRoll));
        eye = Math.max(eye, p.eyeLight);
      }
      return { roll, eye };
    };
    expect(peak(true, "tilt").roll).toBeCloseTo(peak(false, "tilt").roll * REDUCED_GESTURE, 9);
    expect(peak(true, "flare").eye).toBe(peak(false, "flare").eye);
  });

  it("rest jumps to the cue's settled light with no gesture", () => {
    const d = createPerformanceDriver();
    const p = d.rest(cue(3, "hushed", 1, "lean_in"));
    expect(p).toEqual(performanceAt(cue(3, "hushed", 1)));
    expect(p.eyeLight).toBe(0.6);
    expect(d.settled).toBe(true);
  });
});

describe("the box", () => {
  it("stamps a growing id and tells subscribers", () => {
    const b = performanceBox();
    expect(b.get()).toBe(NEUTRAL_CUE);
    const heard: PerformanceCue[] = [];
    const off = b.subscribe((c) => heard.push(c));
    b.cue("menace", 0.8, { gesture: "glare" });
    b.cue("menace", 0.8, { gesture: "glare" });
    off();
    b.cue("glee", 0.5);
    expect(heard.map((c) => c.id)).toEqual([1, 2]);
    expect(b.get()).toEqual({ id: 3, emotion: "glee", intensity: 0.5 });
  });
});

describe("the relay", () => {
  it("forwards whichever source it holds, and tells its listeners on a swap and on each cue", () => {
    const r = performanceRelay();
    expect(r.get()).toBe(NEUTRAL_CUE);
    const heard: number[] = [];
    r.subscribe((c) => heard.push(c.id));
    const a = performanceBox();
    const b = performanceBox();
    r.set(a);
    a.cue("glee", 1);
    r.set(b);
    a.cue("glee", 1); // no longer heard
    b.cue("weary", 1);
    b.cue("weary", 1);
    r.set(undefined);
    expect(heard).toEqual([0, 1, 0, 1, 2, 0]);
    expect(r.get()).toBe(NEUTRAL_CUE);
  });
});

describe("withPerformance", () => {
  it("without a source returns the face driver itself", () => {
    const inner = createDriver({ seed: 7 });
    expect(withPerformance(inner, undefined)).toBe(inner);
  });

  it("hangs the performance on the pose, adds the gesture to the head, and waits for both to settle", () => {
    const box = performanceBox();
    const d = withPerformance(createDriver({ seed: 7, idleLinger: 0 }), box);
    // settle the face driver first
    for (let i = 0; i < 200 && !d.settled; i++) d.step(1 / 60);
    expect(d.settled).toBe(true);
    box.cue("neutral", 1, { gesture: "tilt" });
    expect(d.settled).toBe(true); // nothing read the cue yet: the face wakes on the event
    let p = d.step(1 / 60) as PerformedPose;
    for (let i = 0; i < 20; i++) p = d.step(1 / 60) as PerformedPose;
    expect(d.settled).toBe(false);
    expect(p.perf?.gestureRoll).toBeGreaterThan(0.1);
    expect(p.roll).toBeCloseTo((p.perf!.gestureRoll * 180) / Math.PI, 9);
    for (let i = 0; i < 200 && !d.settled; i++) d.step(1 / 60);
    expect(d.settled).toBe(true);
    expect((d.pose as PerformedPose).perf).toEqual(PERFORMANCE_REST);
  });

  it("rest draws the cue's light with the head still", () => {
    const box = performanceBox();
    const d = withPerformance(createDriver({ seed: 7 }), box);
    box.cue("triumph", 1, { gesture: "flare" });
    const p = d.rest() as PerformedPose;
    expect(p.perf?.eyeLight).toBe(2);
    expect(p.perf?.flare).toBe(0);
    expect([p.yaw, p.pitch, p.roll]).toEqual([0, 0, 0]);
  });
});
