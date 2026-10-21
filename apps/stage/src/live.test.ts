// The live feed: resume with `since`, the reset on a new stream, the bounded store, the acks
// and the watchdog. A fake socket stands in for the daemon; nothing here opens a network socket.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { StreamEvent } from "@shadowrunr/contract";
import { foldEvents } from "@shadowrunr/stage-kit";
import {
  ACK_EVERY_MS,
  DISCONNECTED_SLATE_MS,
  EventRing,
  LiveSession,
  LiveStore,
  RING_MAX,
  SAID_MAX,
  STALLED_MS,
} from "./live";
import type { SocketLike } from "./source";
import { readJson } from "./test-json";

const start = (ts: number, id = "s-1"): StreamEvent => ({
  type: "stream_start",
  ts_ms: ts,
  stream_id: id,
  character: "malakor",
  tone_variant: "theatrical",
  core_tier: "haiku",
  audience_size: 30,
});
const hint = (ts: number, n: number): StreamEvent => ({
  type: "chat",
  ts_ms: ts,
  msg: {
    id: `m${n}`,
    ts_ms: ts,
    viewer: { id: `v${n}`, name: `viewer${n}`, platform: "sim", rank: "thrall", months: 0, gifted: 0, is_mod: false },
    text: "!hint HEX-01 follow the wallet",
    command: { cmd: "hint", hex: "HEX-01", text: "follow the wallet" },
  },
});
const end = (ts: number): StreamEvent => ({ type: "stream_end", ts_ms: ts, reason: "sim_complete" });

/** start, then hints 1..n-1: the hint count equals the events applied after the start. */
function stream(n: number): StreamEvent[] {
  const evs = [start(0)];
  for (let i = 1; i < n; i++) evs.push(hint(i * 100, i));
  return evs;
}

class FakeSocket implements SocketLike {
  onopen: (() => void) | null = null;
  onmessage: ((m: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  readyState = 0;
  sent: unknown[] = [];
  send(data: string): void {
    this.sent.push(readJson(data));
  }
  close(): void {
    this.drop();
  }
  open(): void {
    this.readyState = 1;
    this.onopen?.();
  }
  drop(): void {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.onclose?.();
  }
  frame(seq: number, ev: StreamEvent, runId = "run-a"): void {
    this.onmessage?.({ data: JSON.stringify({ seq, run_id: runId, event: ev }) });
  }
  raw(v: unknown): void {
    this.onmessage?.({ data: JSON.stringify(v) });
  }
  subscribes(): Record<string, unknown>[] {
    return this.sent.filter((f): f is Record<string, unknown> => typeof f === "object" && f !== null && "subscribe" in f);
  }
  acks(): { seq: number; gaps: number; stalled_ms: number }[] {
    return this.sent
      .filter((f): f is { ack: { seq: number; gaps: number; stalled_ms: number } } => typeof f === "object" && f !== null && "ack" in f)
      .map((f) => f.ack);
  }
}

function session() {
  const sockets: FakeSocket[] = [];
  const s = new LiveSession({
    url: "ws://daemon.test/events",
    run: "live",
    speed: 1,
    now: () => Date.now(),
    socket: () => {
      const f = new FakeSocket();
      sockets.push(f);
      return f;
    },
  });
  s.start();
  return { s, sockets, last: () => sockets[sockets.length - 1]! };
}

const hintCount = (store: LiveStore) => store.scene.hints["HEX-01"]?.count ?? 0;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(0);
});
afterEach(() => {
  vi.useRealTimers();
});

describe("resume with since", () => {
  it("subscribes enveloped from the start, then resumes after the last applied seq on a reconnect", () => {
    const { s, last } = session();
    const evs = stream(8);
    last().open();
    expect(last().subscribes()).toEqual([{ subscribe: "live", speed: 1, since: 0, run_id: "" }]);
    evs.slice(0, 5).forEach((e, i) => last().frame(i + 1, e));
    expect(hintCount(s.store)).toBe(4);

    last().drop();
    vi.advanceTimersByTime(1000);
    last().open();
    expect(last().subscribes()).toEqual([{ subscribe: "live", speed: 1, since: 5, run_id: "run-a" }]);
    // A daemon that replays from the start anyway: nothing is applied twice.
    evs.forEach((e, i) => last().frame(i + 1, e));
    expect(hintCount(s.store)).toBe(7);
    expect(s.store.scene.applied).toBe(8);
    expect(s.store.lastSeq).toBe(8);
    expect(s.store.gaps).toBe(0);
    s.stop();
  });

  it("counts a seq jump as missing seqs, holds later frames, and resubscribes once", () => {
    const { s, last } = session();
    const evs = stream(10);
    last().open();
    [1, 2, 3].forEach((q) => last().frame(q, evs[q - 1]!));
    last().frame(6, evs[5]!);
    last().frame(7, evs[6]!);
    expect(s.store.gaps).toBe(2);
    expect(s.store.lastSeq).toBe(3);
    expect(last().subscribes().slice(1)).toEqual([{ subscribe: "live", speed: 1, since: 3, run_id: "run-a" }]);
    // The backfill from since=3 heals it without another count.
    for (let q = 4; q <= 10; q++) last().frame(q, evs[q - 1]!);
    expect(s.store.lastSeq).toBe(10);
    expect(s.store.gaps).toBe(2);
    expect(hintCount(s.store)).toBe(9);
    s.stop();
  });

  it("resubscribes from the last applied seq on a busy frame and counts the skipped events", () => {
    const { s, last } = session();
    const evs = stream(6);
    last().open();
    [1, 2].forEach((q) => last().frame(q, evs[q - 1]!));
    last().raw({ error: { code: "busy", message: "slow consumer", detail: { skipped: 3, resume_since: 2 } } });
    expect(s.store.busyFrames).toBe(1);
    expect(s.store.gaps).toBe(3);
    expect(last().subscribes().slice(1)).toEqual([{ subscribe: "live", speed: 1, since: 2, run_id: "run-a" }]);
    // Frames still in flight from the old subscription are held, not counted again.
    last().frame(6, evs[5]!);
    expect(s.store.gaps).toBe(3);
    for (let q = 3; q <= 6; q++) last().frame(q, evs[q - 1]!);
    expect(s.store.lastSeq).toBe(6);
    expect(hintCount(s.store)).toBe(5);
    s.stop();
  });
});

describe("a new stream resets the scene", () => {
  it("resets on a new stream_start", () => {
    const store = new LiveStore(0);
    stream(5).forEach((e) => store.accept({ kind: "event", event: e }, 0));
    expect(hintCount(store)).toBe(4);
    store.accept({ kind: "event", event: start(9000, "s-2") }, 0);
    expect(hintCount(store)).toBe(0);
    expect(store.scene.applied).toBe(1);
    expect(store.scene.meta?.streamId).toBe("s-2");
    expect(store.events()).toHaveLength(1);
  });

  it("resets on a different run id and restarts seq tracking", () => {
    const store = new LiveStore(0);
    stream(5).forEach((e, i) => store.accept({ kind: "event", event: e, seq: i + 1, runId: "run-a" }, 0));
    expect(store.accept({ kind: "event", event: hint(50, 99), seq: 1, runId: "run-b" }, 0)).toBe("applied");
    expect(store.runId).toBe("run-b");
    expect(store.lastSeq).toBe(1);
    expect(store.gaps).toBe(0);
    expect(store.scene.applied).toBe(1);
    expect(hintCount(store)).toBe(1);
  });
});

describe("Malakor's lines and their seqs (for the captions that follow the voice)", () => {
  const say = (ts: number, text: string, foil?: string): StreamEvent =>
    foil ? { type: "say", ts_ms: ts, speaker: "foil", foil, text } : { type: "say", ts_ms: ts, speaker: "host", text };

  it("keeps each of his lines with the say event's seq, not a foil's, and resets with the run", () => {
    const store = new LiveStore(0);
    [start(0), say(100, "Kneel."), say(200, "Not me.", "the-inspector"), say(300, "Rise.")].forEach((e, i) =>
      store.accept({ kind: "event", event: e, seq: i + 1, runId: "r" }, 0),
    );
    expect(store.said.map((l) => [l.seq, l.text, l.at])).toEqual([
      [2, "Kneel.", 100],
      [4, "Rise.", 300],
    ]);
    expect(store.said[1]!.id).toBe(store.scene.caption!.id);
    store.accept({ kind: "event", event: start(9000, "s-2"), seq: 1, runId: "r2" }, 0);
    expect(store.said).toEqual([]);
  });

  it("holds at most SAID_MAX lines", () => {
    const store = new LiveStore(0);
    store.accept({ kind: "event", event: start(0), seq: 1, runId: "r" }, 0);
    for (let i = 0; i < SAID_MAX + 5; i++) store.accept({ kind: "event", event: say(i * 10 + 10, `Line ${i}.`), seq: i + 2, runId: "r" }, 0);
    expect(store.said).toHaveLength(SAID_MAX);
    expect(store.said[SAID_MAX - 1]!.text).toBe(`Line ${SAID_MAX + 4}.`);
  });
});

describe("the bounded store", () => {
  it("holds the last 2,000 of 5,000 events with the scene reduced over all of them", () => {
    const store = new LiveStore(0);
    const evs = stream(5000);
    evs.forEach((e, i) => store.accept({ kind: "event", event: e, seq: i + 1, runId: "r" }, 0));
    const held = store.events();
    expect(held).toHaveLength(RING_MAX);
    expect(held[0]).toBe(evs[3000]);
    expect(held[RING_MAX - 1]).toBe(evs[4999]);
    const whole = foldEvents(evs);
    expect(store.scene.applied).toBe(5000);
    expect(hintCount(store)).toBe(4999);
    expect(store.scene.hints).toEqual(whole.hints);
    expect(store.scene.lane).toEqual(whole.lane);
    // The copy for the debug views is made once per change, not per read.
    expect(store.events()).toBe(held);
  });

  it("EventRing keeps insertion order across the wrap", () => {
    const r = new EventRing<number>(3);
    for (let i = 1; i <= 7; i++) r.push(i);
    expect(r.toArray()).toEqual([5, 6, 7]);
    expect(r.size).toBe(3);
    r.clear();
    expect(r.toArray()).toEqual([]);
  });
});

describe("acks", () => {
  it("sends {seq, gaps, stalled_ms} every 5 s while open, and at stream_end", () => {
    const { s, last } = session();
    const evs = stream(4);
    last().open();
    evs.forEach((e, i) => last().frame(i + 1, e));
    vi.advanceTimersByTime(ACK_EVERY_MS);
    expect(last().acks()).toEqual([{ seq: 4, gaps: 0, stalled_ms: ACK_EVERY_MS }]);
    vi.advanceTimersByTime(ACK_EVERY_MS);
    expect(last().acks()[1]).toEqual({ seq: 4, gaps: 0, stalled_ms: 2 * ACK_EVERY_MS });
    last().frame(5, end(5000));
    expect(last().acks()).toHaveLength(3);
    // Ended: the open socket with no events is not a stall.
    expect(last().acks()[2]).toEqual({ seq: 5, gaps: 0, stalled_ms: 0 });
    s.stop();
  });

  it("reports the gaps found and sends nothing while the socket is closed", () => {
    const { s, sockets, last } = session();
    last().open();
    last().frame(1, start(0));
    last().frame(3, hint(200, 2));
    last().drop();
    vi.advanceTimersByTime(ACK_EVERY_MS);
    expect(sockets.flatMap((f) => f.acks())).toEqual([]);
    last().open();
    vi.advanceTimersByTime(ACK_EVERY_MS);
    expect(last().acks()).toEqual([{ seq: 1, gaps: 1, stalled_ms: expect.any(Number) as number }]);
    s.stop();
  });
});

describe("watchdog", () => {
  it("shows the signal-lost slate after 3 s down, and the stalled mark after 20 s open with no event", () => {
    const store = new LiveStore(0);
    expect(store.watch(DISCONNECTED_SLATE_MS - 1).disconnected).toBe(false);
    expect(store.watch(DISCONNECTED_SLATE_MS).disconnected).toBe(true);
    store.status("open", 4000);
    expect(store.watch(4000).disconnected).toBe(false);
    store.accept({ kind: "event", event: start(0) }, 5000);
    expect(store.watch(5000 + STALLED_MS - 1).stalled).toBe(false);
    expect(store.watch(5000 + STALLED_MS)).toEqual({ disconnected: false, stalled: true, stalledMs: STALLED_MS });
    store.status("closed", 30_000);
    expect(store.watch(30_000).stalledMs).toBe(0);
    // A reconnect attempt does not restart the disconnected clock.
    store.status("connecting", 31_000);
    expect(store.watch(30_000 + DISCONNECTED_SLATE_MS).disconnected).toBe(true);
  });

  it("keeps the end card instead of the signal-lost slate once the stream has ended", () => {
    const store = new LiveStore(0);
    store.status("open", 0);
    store.accept({ kind: "event", event: start(0) }, 0);
    store.accept({ kind: "event", event: end(100) }, 100);
    store.status("closed", 200);
    expect(store.watch(60_000)).toEqual({ disconnected: false, stalled: false, stalledMs: 0 });
  });
});
