// On Air's live feed against a fake daemon that keeps the run in flight and backfills every new
// `live` subscription from its `since` (docs/spec/contract.md § Event resume). The screen must
// equal the fold of the run so far, whenever the poster was opened and however often it left.
import type { Case, StreamEvent } from "@shadowrunr/contract";
import { foldEvents } from "@shadowrunr/stage-kit";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useSyncExternalStore } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { contractDoor } from "../../data/contractDoor";
import { createDataLayer, DataProvider } from "../../data";
import { fakeSockets, manualTimers, scriptedFetch, type FakeSocket, type FetchReply } from "../../data/testing";
import { createHashRouter, RouterProvider } from "../../router";
import { LIVE, liveFeed, runSource, selectSource, sourceKey } from "./feed";
import { Layer1 } from "./Layer1";
import { TABLES, runRow, transcript } from "./testdata";

const HELLO = JSON.stringify({ subscribe: "live", speed: 1, since: 0, run_id: "" });
const CASE: Case = { id: "C-1", title: "The Case", fictional: true, hexes: [{ id: "HEX-01", q: 0, r: 0, kind: "entity", label: "x", fog: true }] };
const op = (ts: number, o: Extract<StreamEvent, { type: "op" }>["op"]): StreamEvent => ({ type: "op", ts_ms: ts, op: o });
const chat = (ts: number, i: number): StreamEvent => ({
  type: "chat",
  ts_ms: ts,
  msg: { id: `m${i}`, ts_ms: ts, text: `line ${i}`, viewer: { id: `v${i}`, name: `viewer_${i}`, platform: "sim", rank: "thrall" } },
});

/** Seq 1..50: the stream starts, a case opens, a poll opens and a foil is summoned, then chat. */
function firstFifty(): StreamEvent[] {
  const out: StreamEvent[] = [
    { type: "stream_start", ts_ms: 0, stream_id: "s1", character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 300 },
    { type: "segment_start", ts_ms: 10, segment: "SEG-01" },
    { type: "case_open", ts_ms: 11, case: CASE },
    op(20, { op: "start_poll", poll_id: "p1", question: "Which foil?", options: ["Summon the Disruptor", "Summon the Life Coach"], min_rank: "thrall", secs: 30 }),
    op(30, { op: "summon_foil", foil: "disruptor" }),
  ];
  for (let i = out.length + 1; i <= 50; i++) out.push(chat(40 + i * 100, i));
  return out;
}

/** Seq 51..80: the poll closes, the foil leaves, and the room talks on well past the panel's life. */
function nextThirty(): StreamEvent[] {
  const out: StreamEvent[] = [op(6000, { op: "close_poll", poll_id: "p1" }), op(6010, { op: "dismiss_foil" })];
  for (let i = 53; i <= 80; i++) out.push(chat(6100 + (i - 53) * 500, i));
  return out;
}

/** The daemon: one run in flight; a `live` hello with since/run_id backfills, then frames fan out. */
function fakeDaemon(sockets: { all: FakeSocket[] }) {
  const log: { runId: string; event: StreamEvent }[] = [];
  const cursor = new Map<FakeSocket, number>();
  const open = (s: FakeSocket) => s.closed === null && cursor.has(s);
  const frame = (i: number) => {
    const e = log[i];
    if (!e) throw new Error("no frame");
    const seq = log.slice(0, i + 1).filter((x) => x.runId === e.runId).length;
    return { seq, run_id: e.runId, event: e.event };
  };
  const pump = () => {
    for (const s of sockets.all) {
      if (s.closed !== null) continue;
      if (!cursor.has(s)) {
        s.open();
        const hello = s.sent[0] ?? "";
        const run = log.at(-1)?.runId;
        // A fresh subscribe (or one for another run) gets the run in flight from seq 1.
        const since = /"run_id":"([^"]*)"/.exec(hello)?.[1] === run ? Number(/"since":(\d+)/.exec(hello)?.[1] ?? "0") : 0;
        const first = log.findIndex((x) => x.runId === run);
        cursor.set(s, first < 0 ? log.length : first + since);
      }
      let at = cursor.get(s) ?? 0;
      while (at < log.length) s.push(frame(at++));
      cursor.set(s, at);
    }
  };
  return {
    pump,
    emit(events: readonly StreamEvent[], runId = "r1") {
      for (const event of events) log.push({ runId, event });
      act(() => pump());
    },
    /** A raw frame to every open subscription, bypassing the log (to fake a gap). */
    raw(f: unknown) {
      act(() => {
        for (const s of sockets.all) if (open(s)) s.push(f);
      });
    },
    openLive: () => sockets.all.filter((s) => s.closed === null),
  };
}

function rpc(method: string): FetchReply {
  switch (method) {
    case "runs.page":
      return { body: { result: { items: [runRow(0)] } } };
    case "run.transcript":
      return { body: { result: transcript(40) } };
    case "segments.tables":
      return { body: { result: TABLES } };
    default:
      return { status: 404, body: { error: { code: "method_not_found", message: method } } };
  }
}

/** Whether On Air is on screen (the poster is the active one). */
const shown = (() => {
  let v = false;
  const ls = new Set<() => void>();
  return {
    get: () => v,
    set(next: boolean) {
      v = next;
      for (const l of [...ls]) l();
    },
    subscribe(l: () => void) {
      ls.add(l);
      return () => ls.delete(l);
    },
  };
})();

function Host() {
  const on = useSyncExternalStore(shown.subscribe, shown.get, shown.get);
  return on ? <Layer1 active /> : null;
}

let disposeRouter: (() => void) | null = null;

function mountApp() {
  window.location.hash = "#/onair";
  const router = createHashRouter(window);
  disposeRouter = () => router.dispose();
  const sockets = fakeSockets();
  const layer = createDataLayer({ baseUrl: "http://127.0.0.1:7717", door: contractDoor, fetch: scriptedFetch(rpc), socket: sockets.factory, timers: manualTimers() });
  render(
    <DataProvider layer={layer}>
      <RouterProvider router={router}>
        <Host />
      </RouterProvider>
    </DataProvider>,
  );
  const daemon = fakeDaemon(sockets);
  act(() => daemon.pump());
  return { sockets, daemon };
}

const settle = async () => {
  for (let i = 0; i < 4; i++) await act(async () => void (await new Promise((r) => setTimeout(r, 0))));
};
const screenEl = () => document.querySelector("[data-role='onair-screen']");
const hud = () => document.querySelector("[data-role='onair-hud']")?.textContent ?? "";

beforeEach(() => {
  shown.set(false);
  selectSource(LIVE);
});

afterEach(() => {
  cleanup();
  disposeRouter?.();
  disposeRouter = null;
});

describe("On Air live: the screen is the fold of the run in flight", () => {
  it("opened mid-run, its own subscription backfills from seq 1 and the HUD names the stream", async () => {
    const { sockets, daemon } = mountApp();
    const run = firstFifty();
    daemon.emit(run); // reached only the app-wide stream: On Air is not on screen
    expect(sockets.all).toHaveLength(1);

    act(() => shown.set(true));
    act(() => daemon.pump());
    await settle();

    expect(sockets.all).toHaveLength(2);
    expect(sockets.all[1]?.sent[0]).toBe(HELLO);
    expect(hud()).toContain("Live · theatrical · haiku · 300 simulated viewers");
    expect(screenEl()?.hasAttribute("data-poll")).toBe(true);
    expect(screenEl()?.hasAttribute("data-foil")).toBe(true);
  });

  it("the live scene equals stage-kit's fold of events 1..50", async () => {
    const { daemon } = mountApp();
    const run = firstFifty();
    daemon.emit(run);
    act(() => shown.set(true));
    act(() => daemon.pump());
    await settle();

    const scene = liveFeed.getSnapshot().scene;
    expect(scene.applied).toBe(50);
    expect(scene.meta?.streamId).toBe("s1");
    expect(scene).toEqual(foldEvents(run));
  });

  it("leaving and coming back folds what happened while away: no stale poll, no stale foil", async () => {
    const { sockets, daemon } = mountApp();
    const a = firstFifty();
    daemon.emit(a);
    act(() => shown.set(true));
    act(() => daemon.pump());
    await settle();
    expect(screenEl()?.hasAttribute("data-poll")).toBe(true);

    act(() => shown.set(false)); // the operator opens another poster
    const b = nextThirty();
    daemon.emit(b);
    // No zombie: the subscription On Air owned closed with it.
    expect(daemon.openLive()).toHaveLength(1);

    act(() => shown.set(true));
    act(() => daemon.pump());
    await settle();

    expect(sockets.all.at(-1)?.sent[0]).toBe(HELLO);
    expect(screenEl()?.hasAttribute("data-poll")).toBe(false);
    expect(screenEl()?.hasAttribute("data-foil")).toBe(false);
    expect(liveFeed.getSnapshot().scene).toEqual(foldEvents([...a, ...b]));
  });

  it("a new run id resets the scene to that run (guard)", async () => {
    const { daemon } = mountApp();
    act(() => shown.set(true));
    act(() => daemon.pump());
    daemon.emit(firstFifty());
    await settle();
    expect(liveFeed.getSnapshot().scene.applied).toBe(50);

    daemon.emit([{ type: "segment_start", ts_ms: 5, segment: "SEG-02A" }], "r2");
    await settle();
    expect(liveFeed.getSnapshot().scene.applied).toBe(1);
    expect(liveFeed.getSnapshot().scene.segment).toBe("SEG-02A");
  });

  it("switching to a stored run closes On Air's live subscription; switching back re-folds from seq 1", async () => {
    const { sockets, daemon } = mountApp();
    const run = firstFifty();
    daemon.emit(run);
    act(() => shown.set(true));
    act(() => daemon.pump());
    await settle();
    expect(daemon.openLive()).toHaveLength(2);

    const pick = screen.getByLabelText(/Choose a real run/);
    act(() => void fireEvent.change(pick, { target: { value: sourceKey(runSource(runRow(0))) } }));
    await settle();
    expect(daemon.openLive()).toHaveLength(1); // only the app-wide stream is left

    const before = sockets.all.length;
    act(() => void fireEvent.change(pick, { target: { value: "live" } }));
    act(() => daemon.pump());
    await settle();
    expect(sockets.all.length).toBe(before + 1);
    expect(sockets.all.at(-1)?.sent[0]).toBe(HELLO);
    expect(daemon.openLive()).toHaveLength(2);
    expect(liveFeed.getSnapshot().scene).toEqual(foldEvents(run));
  });

  it("a seq gap says so in the HUD until the next contiguous frame", async () => {
    const { daemon } = mountApp();
    act(() => shown.set(true));
    act(() => daemon.pump());
    daemon.emit(firstFifty());
    await settle();
    expect(hud()).toContain("Live · theatrical");

    daemon.raw({ seq: 53, run_id: "r1", event: chat(9000, 53) }); // 51 and 52 never arrived
    await settle();
    expect(hud()).toContain("Live · missed events, catching up");

    daemon.raw({ seq: 54, run_id: "r1", event: chat(9100, 54) });
    await settle();
    expect(hud()).toContain("Live · theatrical · haiku · 300 simulated viewers");
  });
});
