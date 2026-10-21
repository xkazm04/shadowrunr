// The cut-out's look: the springs' ease, the look mapping, the eyes leading the head, and the
// driver's motion layered on top.
import { describe, expect, it } from "vitest";

import { createDriver } from "../driver";
import { RESTING } from "../types";
import type { Look } from "../variants";
import { EYES_T90, HEAD_T90, headAngles, LOOK_PITCH, LOOK_YAW, lookTarget, PITCH_MAX, springStep, withLook, YAW_MAX } from "./look";

const FRAME = 1 / 60;

/** Seconds a spring takes to cover `share` of a unit step, stepped at `dt`. */
function timeTo(share: number, t90: number, dt = FRAME): number {
  const s = { x: 0, v: 0 };
  let t = 0;
  while (s.x < share && t < 5) {
    springStep(s, 1, dt, t90);
    t += dt;
  }
  return t;
}

describe("the spring", () => {
  it("reaches 90% of a step in its t90, for the eyes and the head", () => {
    expect(timeTo(0.9, EYES_T90)).toBeCloseTo(EYES_T90, 1);
    expect(timeTo(0.9, HEAD_T90)).toBeGreaterThan(HEAD_T90 - 2 * FRAME);
    expect(timeTo(0.9, HEAD_T90)).toBeLessThan(HEAD_T90 + 2 * FRAME);
  });

  it("never overshoots and starts with no jump in speed", () => {
    const s = { x: 0, v: 0 };
    springStep(s, 1, FRAME, HEAD_T90);
    // eased in: the first frame covers much less than a linear 1/24 of the way
    expect(s.x).toBeLessThan(0.02);
    for (let i = 0; i < 300; i++) {
      springStep(s, 1, FRAME, HEAD_T90);
      expect(s.x).toBeLessThanOrEqual(1 + 1e-9);
    }
    expect(s.x).toBeCloseTo(1, 6);
  });

  it("does not depend on the frame rate", () => {
    const a = { x: 0, v: 0 };
    const b = { x: 0, v: 0 };
    for (let i = 0; i < 30; i++) springStep(a, 1, 1 / 60, HEAD_T90);
    for (let i = 0; i < 15; i++) springStep(b, 1, 1 / 30, HEAD_T90);
    expect(a.x).toBeCloseTo(b.x, 9);
  });
});

describe("the look mapping", () => {
  it("clamps a look into -1..1 and makes junk straight ahead", () => {
    expect(lookTarget({ x: 3, y: -2 })).toEqual({ x: 1, y: -1 });
    expect(lookTarget({ x: Number.NaN, y: Infinity })).toEqual({ x: 0, y: 0 });
    expect(lookTarget(null)).toEqual({ x: 0, y: 0 });
  });

  it("turns right for a look right and down for a look down, inside the range", () => {
    expect(headAngles({ x: 1, y: 0 }, 0, 0)).toEqual({ yaw: LOOK_YAW, pitch: 0 });
    expect(headAngles({ x: 0, y: 1 }, 0, 0)).toEqual({ yaw: 0, pitch: LOOK_PITCH });
    expect(headAngles({ x: -1, y: -1 }, 0, 0)).toEqual({ yaw: -LOOK_YAW, pitch: -LOOK_PITCH });
    // the driver's own motion adds on top, held at about +-12 by +-6
    expect(headAngles({ x: 1, y: 1 }, 5, 5)).toEqual({ yaw: YAW_MAX, pitch: PITCH_MAX });
    expect(headAngles({ x: 0, y: 0 }, 1.5, -0.7)).toEqual({ yaw: 1.5, pitch: -0.7 });
  });
});

describe("withLook", () => {
  const speaking = { ...RESTING, phase: "speaking" as const, level: 0.5 };

  it("with no look the head is the driver's, unchanged", () => {
    const plain = createDriver({ seed: 5 });
    const looked = withLook(createDriver({ seed: 5 }));
    const ahead = withLook(createDriver({ seed: 5 }), { get: () => ({ x: 0, y: 0 }) });
    for (let i = 0; i < 240; i++) {
      plain.setSignal(speaking);
      looked.setSignal(speaking);
      ahead.setSignal(speaking);
      const p = { ...plain.step(FRAME) };
      const q = looked.step(FRAME);
      expect(q.yaw).toBe(p.yaw);
      expect(q.pitch).toBe(p.pitch);
      expect(q.open).toBe(p.open);
      expect(ahead.step(FRAME).yaw).toBe(p.yaw);
    }
  });

  it("the eyes lead and the head follows", () => {
    let look: Look = { x: 0, y: 0 };
    const d = withLook(createDriver({ seed: 2, idleLinger: 60 }), { get: () => look });
    d.step(FRAME);
    look = { x: 1, y: 0 };
    let t = 0;
    let eyes90 = -1;
    let head90 = -1;
    while (t < 2) {
      d.step(FRAME);
      t += FRAME;
      if (eyes90 < 0 && d.eyes.x >= 0.9) eyes90 = t;
      if (head90 < 0 && d.head.x >= 0.9) head90 = t;
    }
    expect(eyes90).toBeGreaterThan(0.08);
    expect(eyes90).toBeLessThan(0.16);
    expect(head90).toBeGreaterThan(0.35);
    expect(head90).toBeLessThan(0.45);
    // turned right by the look, with the idle drift still on top
    expect(d.pose.yaw).toBeGreaterThan(LOOK_YAW - 3);
    expect(d.pose.gazeX).toBeGreaterThan(0.8);
  });

  it("does not settle while the look is moving, settles once it has arrived, and a new look is news", () => {
    let look: Look = { x: 0, y: 0 };
    const d = withLook(createDriver({ seed: 1, idleLinger: 0.2 }), { get: () => look });
    for (let i = 0; i < 400; i++) d.step(FRAME);
    expect(d.settled).toBe(true);
    expect(d.poll()).toBe(false);
    look = { x: -0.5, y: 0.5 };
    expect(d.poll()).toBe(true);
    expect(d.settled).toBe(false);
    d.step(FRAME);
    expect(d.settled).toBe(false);
    for (let i = 0; i < 120; i++) d.step(FRAME);
    expect(d.settled).toBe(true);
    // settled at the look: the still frame looks where it was told
    expect(d.pose.yaw).toBeCloseTo(-0.5 * LOOK_YAW, 6);
    expect(d.pose.pitch).toBeCloseTo(0.5 * LOOK_PITCH, 6);
  });

  it("rest jumps straight to the look (reduced motion's still frame)", () => {
    const d = withLook(createDriver({ seed: 1 }), { get: () => ({ x: 1, y: -1 }) });
    const p = d.rest();
    expect(p.yaw).toBe(LOOK_YAW);
    expect(p.pitch).toBe(-LOOK_PITCH);
    expect(p.gazeX).toBe(1);
    expect(d.eyes).toEqual({ x: 1, y: -1 });
  });
});
