import { describe, expect, it } from "vitest";

import { createDriver } from "../driver";
import { lookDriver } from "./index";
import { createLookFollower, HEAD_PITCH_MAX, HEAD_YAW_MAX, lookMoved } from "./look";

const DT = 1 / 120;

/** Step toward `look` until `t` seconds; returns the poses. */
function run(look: { x: number; y: number }, seconds: number) {
  const f = createLookFollower();
  const out: { t: number; yaw: number; pitch: number; eyeX: number }[] = [];
  for (let t = DT; t <= seconds + 1e-9; t += DT) {
    const p = f.step(DT, look);
    out.push({ t, yaw: p.yaw, pitch: p.pitch, eyeX: p.eyeX });
  }
  return { f, out };
}

const at = <T,>(out: T[], t: number): T => out[Math.round(t / DT) - 1]!;

describe("the look follower", () => {
  it("starts straight ahead and stays there with nowhere to look", () => {
    const { f, out } = run({ x: 0, y: 0 }, 1);
    expect(at(out, 1)).toMatchObject({ yaw: 0, pitch: 0, eyeX: 0 });
    expect(f.settledAt({ x: 0, y: 0 })).toBe(true);
  });

  it("leads with the eyes: they slide toward the target well before the head arrives", () => {
    const { out } = run({ x: 1, y: 0 }, 2);
    const early = at(out, 0.12);
    // by 120 ms the eyes have done most of their turn while the head is not half way
    expect(early.eyeX).toBeGreaterThan(6);
    expect(early.yaw).toBeLessThan(0.5 * HEAD_YAW_MAX);
  });

  it("brings the head about 95 % of the way in about 400 ms, without overshoot", () => {
    const { out, f } = run({ x: 1, y: 1 }, 2);
    expect(at(out, 0.4).yaw).toBeGreaterThan(0.9 * HEAD_YAW_MAX);
    expect(at(out, 0.4).yaw).toBeLessThan(0.99 * HEAD_YAW_MAX);
    expect(at(out, 0.4).pitch).toBeGreaterThan(0.9 * HEAD_PITCH_MAX);
    expect(Math.max(...out.map((p) => p.yaw))).toBeLessThanOrEqual(HEAD_YAW_MAX + 1e-6);
    expect(f.settledAt({ x: 1, y: 1 })).toBe(true);
  });

  it("re-centres the eyes as the head catches up, keeping a little lean toward the target", () => {
    const { out } = run({ x: 1, y: 0 }, 2);
    const peak = Math.max(...out.map((p) => p.eyeX));
    const end = at(out, 2).eyeX;
    expect(end).toBeLessThan(peak);
    expect(end).toBeGreaterThan(2);
  });

  it("clamps a look past the box and ignores a broken one", () => {
    const f = createLookFollower();
    expect(f.snap({ x: 5, y: -5 })).toMatchObject({ yaw: HEAD_YAW_MAX, pitch: -HEAD_PITCH_MAX });
    expect(f.snap({ x: Number.NaN, y: 0 }).yaw).toBe(0);
  });

  it("tells a moved look from a jitter", () => {
    expect(lookMoved({ x: 0, y: 0 }, { x: 0.0001, y: 0 })).toBe(false);
    expect(lookMoved({ x: 0, y: 0 }, { x: 0.2, y: 0 })).toBe(true);
  });
});

describe("the look on the face driver", () => {
  it("keeps the face loop running until the head has arrived, and snaps at rest", () => {
    let look = { x: 0, y: 0 };
    const d = lookDriver(createDriver({ seed: 7, idleLinger: 0 }), createLookFollower(), () => look);
    for (let i = 0; i < 400; i++) d.step(1 / 60);
    expect(d.settled).toBe(true);
    look = { x: 1, y: 0 };
    expect(d.settled).toBe(false);
    for (let i = 0; i < 6; i++) d.step(1 / 60);
    expect(d.settled).toBe(false);
    expect(d.look.yaw).toBeGreaterThan(0);
    for (let i = 0; i < 120; i++) d.step(1 / 60);
    expect(d.settled).toBe(true);
    look = { x: -1, y: 0 };
    d.rest();
    expect(d.look.yaw).toBe(-HEAD_YAW_MAX);
  });
});
