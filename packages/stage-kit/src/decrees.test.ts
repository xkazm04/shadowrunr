// The decree slot's schedule: one card at a time, a burst queued in order, never two at once.
// (What it renders is tested with the stage: apps/stage onair.test.ts.)
import { describe, expect, it } from "vitest";

import { DECREE_MIN_SHOW_MS, DECREE_MS, decreeAt, decreeTurns } from "./decrees";
import type { Decree } from "./sceneModel";

const picked = (key: string, at: number): Decree => ({ key, msgId: `m-${key}`, kind: "picked", at });

describe("decree turns", () => {
  it("a lone pick shows its full time, then is held", () => {
    const d = [picked("a", 1000)];
    expect(decreeTurns(d)).toEqual([{ decree: d[0], start: 1000, end: 1000 + DECREE_MS.picked }]);
    expect(decreeAt(d, 999)).toBeNull();
    expect(decreeAt(d, 1000)).toMatchObject({ held: false });
    expect(decreeAt(d, 1000 + DECREE_MS.picked)).toMatchObject({ held: true, turn: { decree: d[0] } });
  });

  it("a burst of three within 2 s queues: each waits its turn and the one before gives way", () => {
    const d = [picked("a", 0), picked("b", 800), picked("c", 1900)];
    const t = decreeTurns(d);
    expect(t.map((x) => [x.decree.key, x.start, x.end])).toEqual([
      ["a", 0, DECREE_MIN_SHOW_MS],
      ["b", DECREE_MIN_SHOW_MS, 2 * DECREE_MIN_SHOW_MS],
      ["c", 2 * DECREE_MIN_SHOW_MS, 2 * DECREE_MIN_SHOW_MS + DECREE_MS.picked],
    ]);
    // turns never overlap, so exactly one card is current at any time from the first pick on
    for (let now = 0; now < t[2]!.end; now += 50) {
      const current = t.filter((x) => now >= x.start && now < x.end);
      expect(current, `at ${now}`).toHaveLength(1);
    }
    expect(decreeAt(d, t[2]!.end + 1)).toMatchObject({ held: true, turn: { decree: { key: "c" } } });
  });

  it("a pick after the last one's time ran out does not wait; the gap holds the last line", () => {
    const d = [picked("a", 0), picked("b", 20_000)];
    expect(decreeTurns(d)[1]!.start).toBe(20_000);
    expect(decreeAt(d, 10_000)).toMatchObject({ held: true, turn: { decree: { key: "a" } } });
    expect(decreeAt(d, 20_000)).toMatchObject({ held: false, turn: { decree: { key: "b" } } });
  });

  it("order is by time even if the list is not", () => {
    expect(decreeTurns([picked("b", 500), picked("a", 0)]).map((x) => x.decree.key)).toEqual(["a", "b"]);
  });
});
