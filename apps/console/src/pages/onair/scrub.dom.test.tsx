// On Air's replay is an instrument, not a loop: the progress bar is a keyboard and pointer
// scrubber with the run's segment bands and moment ticks, and every beat or segment span of a
// run in the rundown can be put on screen at its own time. Counts and positions are asserted,
// never timings.
import type { RunRow, StreamEvent } from "@shadowrunr/contract";
import { clipMarkers, segmentSpans } from "@shadowrunr/stage-kit";
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { FetchReply } from "../../data/testing";
import { mountPage, settle } from "../../kit/testing";
import { LIVE, runSource, selectSource } from "./feed";
import { Layer1 } from "./Layer1";
import { Layer2 } from "./Layer2";
import { TABLES, runRow, transcript } from "./testdata";

/** The poll opens at 40 s; the room is quiet for a minute after it, so a playing replay holds. */
const POLL_AT = 40_000;
const SEG2_AT = 120_000;
const chat = (ts: number, i: number): StreamEvent => ({
  type: "chat",
  ts_ms: ts,
  msg: { id: `m${i}`, ts_ms: ts, text: `line ${i}`, viewer: { id: `v${i}`, name: `viewer_${i}`, platform: "sim", rank: "thrall" } },
});

function sparse(): StreamEvent[] {
  return [
    { type: "stream_start", ts_ms: 0, stream_id: "s1", character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 300 },
    { type: "segment_start", ts_ms: 10, segment: "SEG-01" },
    chat(5_000, 1),
    { type: "op", ts_ms: 6_000, op: { op: "mark_moment", label: "the ledger opens" } },
    chat(20_000, 2),
    { type: "op", ts_ms: POLL_AT, op: { op: "start_poll", poll_id: "p1", question: "Which foil?", options: ["Disruptor", "Life Coach"], min_rank: "thrall", secs: 30 } },
    chat(100_000, 3),
    { type: "segment_start", ts_ms: SEG2_AT, segment: "SEG-02A" },
    { type: "op", ts_ms: 150_000, op: { op: "mark_moment", label: "the map lifts" } },
    chat(240_000, 4),
  ];
}

let dispose: (() => void) | null = null;

function mount(ui: ReactNode, runs: RunRow[], events: StreamEvent[], hash = "#/onair") {
  const m = mountPage(
    ui,
    (method): FetchReply => {
      switch (method) {
        case "runs.page":
          return { body: { result: { items: runs } } };
        case "run.transcript":
          return { body: { result: events } };
        case "segments.tables":
          return { body: { result: TABLES } };
        default:
          return { status: 404, body: { error: { code: "method_not_found", message: method } } };
      }
    },
    hash,
  );
  dispose = m.dispose;
  return m;
}

function unmount() {
  cleanup();
  dispose?.();
  dispose = null;
}

afterEach(unmount);

const slider = () => screen.getByRole("slider", { name: /Replay position/ });
const now = () => Number(slider().getAttribute("aria-valuenow"));
const screenEl = () => document.querySelector("[data-role='onair-screen']");

describe("On Air scrubber", () => {
  it("is a slider in transcript time: arrows step 5 s, Home and End jump to the ends", async () => {
    const row = runRow(1);
    const events = transcript(3000);
    selectSource(runSource(row));
    mount(<Layer1 active />, [row], events);
    await screen.findByText(/Replay 4×/);
    fireEvent.click(await screen.findByRole("button", { name: "Pause" }));
    const s = slider();
    expect(s.getAttribute("aria-hidden")).toBeNull();
    expect(s.getAttribute("aria-valuemin")).toBe("0");
    expect(s.getAttribute("aria-valuemax")).toBe(String(events[events.length - 1]?.ts_ms));
    const p = now();
    fireEvent.keyDown(s, { key: "ArrowRight" });
    expect(now()).toBe(p + 5000);
    fireEvent.keyDown(s, { key: "ArrowLeft" });
    expect(now()).toBe(p);
    fireEvent.keyDown(s, { key: "End" });
    expect(now()).toBe(events[events.length - 1]?.ts_ms);
    fireEvent.keyDown(s, { key: "Home" });
    expect(now()).toBe(0);
  });

  it("draws one band per segment span and one tick per marked moment", async () => {
    const row = runRow(2);
    const events = sparse();
    selectSource(runSource(row));
    mount(<Layer1 active />, [row], events);
    await screen.findByText(/Replay 4×/);
    const bar = document.querySelector("[data-role='onair-progress']");
    expect(bar?.querySelectorAll("[data-seg]")).toHaveLength(segmentSpans(events).length);
    expect(bar?.querySelectorAll("[data-mark]")).toHaveLength(clipMarkers(events).length);
    expect(segmentSpans(events)).toHaveLength(2);
    expect(clipMarkers(events)).toHaveLength(2);
  });

  it("on the live feed it is disabled, and a key changes nothing", async () => {
    selectSource(LIVE);
    mount(<Layer1 active />, [], []);
    await settle();
    const s = slider();
    expect(s.getAttribute("aria-disabled")).toBe("true");
    const before = s.getAttribute("aria-valuenow");
    fireEvent.keyDown(s, { key: "ArrowRight" });
    expect(slider().getAttribute("aria-valuenow")).toBe(before);
  });
});

describe("Rundown run detail puts any moment on screen", () => {
  it("'Play from here' on a beat routes to On Air at that beat's time, with its poll up", async () => {
    const row = runRow(5);
    const events = sparse();
    mount(<Layer2 sub={["rundown", row.tournament_id, row.run_id]} />, [row], events, `#/onair/rundown/${row.tournament_id}/${row.run_id}`);
    await screen.findByText(/Beats ·/);
    const beat = [...document.querySelectorAll("[data-op]")].find((r) => r.querySelector("b")?.textContent === "start poll");
    const play = beat?.querySelector("button");
    expect(play?.getAttribute("aria-label")).toMatch(/^Play from here/);
    act(() => play?.click());
    expect(window.location.hash).toBe("#/onair");
    unmount();

    mount(<Layer1 active />, [row], events);
    await screen.findByText(/Replay 4×/);
    expect(now()).toBe(POLL_AT);
    expect(screenEl()?.hasAttribute("data-poll")).toBe(true);
  });

  it("a segment span in the timeline bar plays that run from the span's start", async () => {
    const row = runRow(6);
    const events = sparse();
    mount(<Layer2 sub={["rundown", row.tournament_id, row.run_id]} />, [row], events, `#/onair/rundown/${row.tournament_id}/${row.run_id}`);
    await screen.findByText(/Beats ·/);
    const span = document.querySelector("[data-role='onair-run-spans'] [data-seg='SEG-02A']");
    expect(span?.tagName).toBe("BUTTON");
    act(() => (span as HTMLButtonElement).click());
    expect(window.location.hash).toBe("#/onair");
    unmount();

    mount(<Layer1 active />, [runRow(0), row], events);
    await screen.findByText(/Replay 4×/);
    expect(now()).toBe(SEG2_AT);
    expect(screen.getByRole("button", { name: "Pause" })).toBeTruthy();
    expect(screenEl()?.getAttribute("data-seg")).toBe("SEG-02A");
  });
});
