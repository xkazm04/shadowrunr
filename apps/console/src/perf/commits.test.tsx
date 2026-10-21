// Gated: React commits when one live event arrives at On Air (budgets/perf.json `commits`),
// counted by a <Profiler> around layer 1 on the live feed: one commit per event, none while
// idle over `idleTicks` ticks (no per-tick churn).
import { cleanup } from "@testing-library/react";
import { act } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { LIVE, selectSource } from "../pages/onair/feed";
import { Layer1 as OnAirL1 } from "../pages/onair/Layer1";
import { onairScript } from "./fixture";
import { DEFAULT_COALESCE_MS } from "../data/cache";
import { BUDGETS, frame, idle, line, mountPerf, quiet, type Mounted } from "./harness";

let mounted: Mounted | undefined;
afterEach(() => {
  mounted?.dispose();
  mounted = undefined;
  cleanup();
});

const chat = (i: number) => ({
  type: "chat",
  ts_ms: 1000 * i,
  msg: { id: `live-m${i}`, ts_ms: 1000 * i, text: `live chat ${i}`, viewer: { id: `v${i}`, name: `viewer_${i}`, platform: "sim", rank: "thrall" } },
});

describe("React commits per live event at On Air (10x fixture)", () => {
  it("onair: one commit per live event, none while idle", async () => {
    selectSource(LIVE);
    const m = (mounted = mountPerf(<OnAirL1 active />, onairScript(), "#/onair"));
    await quiet(m, 400);
    await act(async () => m.live.open());
    // The first event takes the screen off its "waiting for the stream" state; measure the next.
    await act(async () => m.live.push(frame(1, { type: "stream_start", ts_ms: 0, stream_id: "live-run", character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 300 })));
    // A chat line is drawn only once the input filter's beat has judged it: a line, its beat,
    // then the measured event, the next chat, which closes that beat and brings the line up (the
    // beat picks another line, so nothing but the close can release this one).
    await act(async () => m.live.push(frame(2, chat(2))));
    await act(async () => m.live.push(frame(3, { type: "picked", ts_ms: 2500, msg_ids: ["live-m1"], reason: "top" })));
    // stream_start's refetches land once the data layer's burst window closes, before measuring.
    await act(async () => m.timers.advance(DEFAULT_COALESCE_MS));
    await quiet(m, 50);
    const budget = line(BUDGETS.commits, "onair.liveEvent");

    let c = m.commits();
    await act(async () => m.live.push(frame(4, chat(3))));
    await quiet(m, 50);
    const perEvent = m.commits() - c;

    c = m.commits();
    await idle(m, BUDGETS.idleTicks);
    const idleCommits = m.commits() - c;
    console.info(`perf commits onair: ${perEvent} per live event, ${idleCommits} over ${BUDGETS.idleTicks} idle ticks`);
    expect(perEvent, "onair commits per live event").toBeLessThanOrEqual(budget.perChange);
    expect(document.querySelector("[data-role='onair-chat']")?.textContent, "onair: the live event reached the screen").toContain("live chat 2");
    expect(idleCommits, "onair commits while idle").toBeLessThanOrEqual(budget.idle);
  }, 60_000);
});
