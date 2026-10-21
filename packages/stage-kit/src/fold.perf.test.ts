// The ballot and grade books fold in linear time (stage-kit-scene-engine B). A poll's ballots
// and a hex's grades used to be plain objects copied whole on every ballot, so folding a big
// vote was quadratic: measured at 829eb0a on the dev box, 1000 / 2000 / 4000 / 8000 distinct
// `!vote` lines took 47 / 409 / 2,187 / 10,084 ms, and as many `!grade` lines 35 / 515 / 2,496 /
// 9,444 ms. The live OBS stage reduces once per event on the render thread and a backward scrub
// refolds from event 0, so that cost stalled the canvas.
//
// Bounds are generous on purpose (a loaded CI box must not flake): the linear fold of 8000
// ballots takes tens of milliseconds, the bound is two seconds. The ratio case is the real
// shape check: a linear fold grows ~4x from 2000 to 8000 ballots, a quadratic one ~16x+.
// Each fold bails out once it has spent its whole budget, so the red case fails in seconds.

import { describe, expect, it } from "vitest";

import type { ChatMsg, Command, Rank, StreamEvent } from "@shadowrunr/contract";
import { reduce } from "./scene";
import { initialScene } from "./sceneModel";

const RANKS: readonly Rank[] = ["thrall", "cultist", "acolyte", "warlock", "lich", "dread_council"];

function line(i: number, command: Command): StreamEvent {
  const msg: ChatMsg = {
    id: `m${i}`,
    ts_ms: i + 2,
    viewer: { id: `v${i}`, name: `n${i}`, platform: "sim", rank: RANKS[i % RANKS.length]!, months: 0, gifted: 0, is_mod: false },
    text: "x",
    command,
  };
  return { type: "chat", ts_ms: i + 2, msg };
}

function votes(n: number): StreamEvent[] {
  const evs: StreamEvent[] = [{ type: "op", ts_ms: 1, op: { op: "start_poll", poll_id: "p", question: "?", options: ["A", "B", "C"], min_rank: "thrall", secs: 30 } }];
  for (let i = 0; i < n; i++) evs.push(line(i, { cmd: "vote", option: ["A", "B", "C"][i % 3]! }));
  return evs;
}

function grades(n: number): StreamEvent[] {
  const evs: StreamEvent[] = [];
  for (let i = 0; i < n; i++) evs.push(line(i, { cmd: "grade", hex: "HEX-04", grade: (["DOC", "REP", "CLM", "SPC"] as const)[i % 4]! }));
  return evs;
}

/** Fold `events` through reduce; the elapsed ms, or Infinity once it spends `budgetMs`. */
function timedFold(events: readonly StreamEvent[], budgetMs: number): number {
  let s = initialScene();
  const t0 = performance.now();
  for (let i = 0; i < events.length; i++) {
    s = reduce(s, events[i]!);
    if (i % 100 === 0 && performance.now() - t0 > budgetMs) return Number.POSITIVE_INFINITY;
  }
  const ms = performance.now() - t0;
  // Keep the fold observable so nothing is optimised away.
  expect(s.applied).toBe(events.length);
  return ms;
}

/** The best of `runs` folds (the least disturbed by the machine's other load). */
function best(events: readonly StreamEvent[], budgetMs: number, runs = 3): number {
  let out = Number.POSITIVE_INFINITY;
  for (let r = 0; r < runs; r++) out = Math.min(out, timedFold(events, budgetMs));
  return out;
}

describe("ballot and grade folds are linear", () => {
  it("folds a start_poll and 8000 distinct `!vote` lines in under 2 s", () => {
    const evs = votes(8000);
    timedFold(votes(500), 10_000); // warm the JIT
    expect(best(evs, 2_000, 1)).toBeLessThan(2_000);
  }, 60_000);

  it("folds 8000 distinct `!grade` lines on one hex in under 1 s", () => {
    const evs = grades(8000);
    timedFold(grades(500), 10_000);
    expect(best(evs, 1_000, 1)).toBeLessThan(1_000);
  }, 60_000);

  it("grows ~linearly: 8000 ballots cost under 8x what 2000 do", () => {
    const small = votes(2000);
    const big = votes(8000);
    timedFold(votes(500), 10_000);
    // A floor keeps a sub-millisecond small fold from turning timer noise into a ratio.
    const tSmall = Math.max(best(small, 30_000), 10);
    const tBig = best(big, 8 * tSmall);
    expect(tBig / tSmall).toBeLessThan(8);
  }, 120_000);
});
