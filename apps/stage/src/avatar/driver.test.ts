// The face driver: seeded procedural motion, the speech drive, and settling at rest.
import { describe, expect, it } from "vitest";

import { LINE_MS, lineAt } from "./demo";
import { createDriver, mulberry32, type Pose } from "./driver";
import { RESTING, type FaceSignal } from "./types";
import { levelOpen, visemeOpen } from "./visemes";

const FRAME = 1 / 60;
const LINE_S = LINE_MS / 1000;
const signal = (s: Partial<FaceSignal>): FaceSignal => ({ ...RESTING, ...s });
const snap = (p: Pose) => ({ yaw: p.yaw, pitch: p.pitch, roll: p.roll, blink: p.blinkL, gx: p.gazeX, gy: p.gazeY, open: p.open });
const speakingAt = (t: number) => lineAt(t * 1000);

/** Run `seconds` of frames, feeding `at(t)` as the signal each frame; returns every pose. */
function run(driver: ReturnType<typeof createDriver>, seconds: number, at: (t: number) => FaceSignal) {
  const out: ReturnType<typeof snap>[] = [];
  for (let t = 0; t < seconds; t += FRAME) {
    driver.setSignal(at(t));
    out.push(snap(driver.step(FRAME)));
  }
  return out;
}

describe("procedural motion is seeded", () => {
  it("the same seed and the same signals give the same poses", () => {
    const a = run(createDriver({ seed: 3 }), 8, speakingAt);
    const b = run(createDriver({ seed: 3 }), 8, speakingAt);
    expect(a).toEqual(b);
  });

  it("another seed moves differently", () => {
    const idle = () => signal({ phase: "idle" });
    expect(run(createDriver({ seed: 3 }), 4, idle)).not.toEqual(run(createDriver({ seed: 4 }), 4, idle));
  });

  it("the generator is mulberry32", () => {
    expect(mulberry32(1)()).toBeCloseTo(0.6270739405881613, 12);
  });

  it("an idle face blinks every few seconds and its head drifts", () => {
    const poses = run(createDriver({ seed: 11, idleLinger: 60 }), 20, () => signal({ phase: "idle" }));
    const closed = poses.filter((p, i) => p.blink > 0.9 && !((poses[i - 1]?.blink ?? 0) > 0.9)).length;
    expect(closed).toBeGreaterThanOrEqual(3); // 2-6 s apart over 20 s
    expect(closed).toBeLessThanOrEqual(12);
    const yaws = poses.map((p) => p.yaw);
    expect(Math.max(...yaws) - Math.min(...yaws)).toBeGreaterThan(0.5);
  });

  it("speaking sways the head more than idle", () => {
    const spread = (ps: ReturnType<typeof snap>[]) => {
      const ps2 = ps.slice(60).map((p) => p.pitch);
      return Math.max(...ps2) - Math.min(...ps2);
    };
    const idle = run(createDriver({ seed: 2, idleLinger: 60 }), 6, () => signal({ phase: "idle" }));
    const talk = run(createDriver({ seed: 2 }), 6, () => signal({ phase: "speaking" }));
    expect(spread(talk)).toBeGreaterThan(spread(idle));
  });
});

describe("speech opens the mouth", () => {
  it("a viseme's opening is scaled by the level and a closure dips to shut", () => {
    expect(visemeOpen("aa", 1)).toBeGreaterThan(visemeOpen("aa", 0.3));
    expect(visemeOpen("PP", 1)).toBe(0);
    expect(visemeOpen("sil", 1)).toBe(0);
    expect(visemeOpen("FF", 1)).toBeLessThan(visemeOpen("E", 1));
  });

  it("an open vowel followed by a bilabial closes within a few frames", () => {
    const d = createDriver({ seed: 1 });
    let p: Pose = d.pose;
    d.setSignal(signal({ phase: "speaking", level: 0.8, viseme: "aa" }));
    for (let i = 0; i < 10; i++) p = d.step(FRAME);
    const wide = p.open;
    expect(wide).toBeGreaterThan(0.7);
    d.setSignal(signal({ phase: "speaking", level: 0.8, viseme: "PP" }));
    for (let i = 0; i < 5; i++) p = d.step(FRAME);
    expect(p.open).toBeLessThan(wide * 0.2);
  });

  it("with no viseme the level alone opens it: fast attack, slower release", () => {
    expect(levelOpen(0.02)).toBe(0);
    const d = createDriver({ seed: 1 });
    d.setSignal(signal({ phase: "speaking", level: 0.7 }));
    let p: Pose = d.pose;
    for (let i = 0; i < 4; i++) p = d.step(FRAME); // 67 ms
    const attacked = p.open / levelOpen(0.7);
    d.setSignal(signal({ phase: "speaking", level: 0 }));
    const top = p.open;
    for (let i = 0; i < 4; i++) p = d.step(FRAME);
    const released = 1 - p.open / top;
    expect(attacked).toBeGreaterThan(0.85);
    expect(released).toBeLessThan(attacked);
    expect(released).toBeGreaterThan(0.2);
  });

  it("the level-less speaking phase (a caption, no voice) keeps the mouth shut", () => {
    const d = createDriver({ seed: 1 });
    d.setSignal(signal({ phase: "speaking", level: 0 }));
    let p: Pose = d.pose;
    for (let i = 0; i < 60; i++) p = d.step(FRAME);
    expect(p.open).toBe(0);
    expect(p.activity).toBeGreaterThan(0.9);
  });
});

describe("a face at rest settles", () => {
  it("an idle face lingers, then settles at the rest pose", () => {
    const d = createDriver({ seed: 9, idleLinger: 2 });
    d.setSignal(signal({ phase: "idle" }));
    let steps = 0;
    let p: Pose = d.pose;
    while (!d.settled && steps < 60 * 10) {
      p = d.step(FRAME);
      steps++;
    }
    expect(d.settled).toBe(true);
    expect(steps / 60).toBeGreaterThan(2);
    expect(steps / 60).toBeLessThan(4.5);
    expect([p.yaw, p.pitch, p.roll, p.blinkL, p.open, p.activity]).toEqual([0, 0, 0, 0, 0, 0]);
  });

  it("speaking never settles; the end of the line settles after its linger", () => {
    const d = createDriver({ seed: 9, idleLinger: 1 });
    for (let t = 0; t < LINE_S; t += FRAME) {
      d.setSignal(speakingAt(t));
      d.step(FRAME);
      expect(d.settled).toBe(false);
    }
    d.setSignal(RESTING);
    let steps = 0;
    while (!d.settled && steps < 600) {
      d.step(FRAME);
      steps++;
    }
    expect(d.settled).toBe(true);
  });

  it("an identical signal is not news; a changed one is", () => {
    const d = createDriver();
    expect(d.setSignal(signal({ phase: "idle" }))).toBe(false);
    expect(d.setSignal(signal({ phase: "speaking" }))).toBe(true);
    expect(d.setSignal(signal({ phase: "speaking" }))).toBe(false);
  });
});
