import { describe, expect, it } from "vitest";
import { contractDoor } from "./contractDoor";
import type { Door, EventEnvelope } from "./door";
import type { DataError } from "./errors";
import { backoffDelay, openEventStream, type StreamStatus } from "./socket";
import { fakeSockets, manualTimers } from "./testing";

const POLICY = { baseMs: 500, capMs: 30_000 };

function harness(target = "live") {
  const sockets = fakeSockets();
  const timers = manualTimers();
  const events: EventEnvelope[] = [];
  const statuses: { s: StreamStatus; delayMs?: number }[] = [];
  const issues: DataError[] = [];
  const stream = openEventStream({
    url: "ws://127.0.0.1:7717/events",
    target,
    door: contractDoor,
    socket: sockets.factory,
    timers,
    random: () => 0.5,
    backoff: POLICY,
    onEvent: (e) => events.push(e),
    onStatus: (s, info) => statuses.push(info.delayMs === undefined ? { s } : { s, delayMs: info.delayMs }),
    onIssue: (i) => issues.push(i),
  });
  return { sockets, timers, events, statuses, issues, stream };
}

/** An enveloped frame, valid against the contract schema. */
const ev = (seq: number, run = "r1") => ({ seq, run_id: run, event: { type: "segment_start", ts_ms: seq, segment: "SEG-01" } });
const hello = (since: number, runId?: string) =>
  JSON.stringify({ subscribe: "live", speed: 1, since, ...(runId === undefined ? {} : { run_id: runId }) });

describe("backoff", () => {
  it("doubles per attempt with equal jitter, capped", () => {
    expect([0, 1, 2, 3].map((a) => backoffDelay(a, POLICY, () => 0))).toEqual([250, 500, 1000, 2000]);
    expect([0, 1, 2, 3].map((a) => backoffDelay(a, POLICY, () => 1))).toEqual([500, 1000, 2000, 4000]);
    expect(backoffDelay(20, POLICY, () => 1)).toBe(30_000);
    expect(backoffDelay(20, POLICY, () => 0)).toBe(15_000);
  });
});

describe("event stream: reconnect and resume", () => {
  it("subscribes enveloped from the start of the in-flight run", () => {
    const h = harness();
    h.sockets.last().open();
    expect(h.sockets.last().sent).toEqual([hello(0, "")]);
    expect(h.stream.status()).toBe("open");
  });

  it("subscribes a replay by run id, without a run_id field", () => {
    const h = harness("t1/r7");
    h.sockets.last().open();
    expect(h.sockets.last().sent).toEqual([JSON.stringify({ subscribe: "t1/r7", speed: 1, since: 0 })]);
  });

  it("reconnects with growing delays and resumes from the last (run, seq)", () => {
    const h = harness();
    const first = h.sockets.last();
    first.open();
    first.push(ev(1));
    first.push(ev(2));
    first.drop();
    expect(h.statuses.at(-1)).toEqual({ s: "reconnecting", delayMs: 375 });

    h.timers.advance(374);
    expect(h.sockets.all).toHaveLength(1);
    h.timers.advance(1);
    expect(h.sockets.all).toHaveLength(2);
    h.sockets.last().drop(); // fails again before opening: the delay grows
    expect(h.statuses.at(-1)).toEqual({ s: "reconnecting", delayMs: 750 });
    h.timers.advance(750);

    const third = h.sockets.last();
    third.open();
    expect(third.sent).toEqual([hello(2, "r1")]);
    // The daemon resumes after the cursor; a repeat is dropped, new events flow.
    third.push(ev(2));
    third.push(ev(3));
    expect(h.events.map((e) => e.seq)).toEqual([1, 2, 3]);
    expect(h.stream.lastSeq()).toBe(3);
    expect(h.issues).toEqual([]);
  });

  it("starts a new cursor when the next run begins", () => {
    const h = harness();
    h.sockets.last().open();
    h.sockets.last().push(ev(1, "r1"));
    h.sockets.last().push(ev(2, "r1"));
    h.sockets.last().push(ev(1, "r2"));
    expect(h.events.map((e) => `${e.runId}#${e.seq}`)).toEqual(["r1#1", "r1#2", "r2#1"]);
    expect(h.stream.lastRunId()).toBe("r2");
  });

  it("resets the backoff once a connection opens", () => {
    const h = harness();
    h.sockets.last().drop();
    h.timers.advance(10_000);
    h.sockets.last().drop();
    h.timers.advance(10_000);
    h.sockets.last().open();
    h.sockets.last().drop();
    expect(h.statuses.at(-1)).toEqual({ s: "reconnecting", delayMs: 375 });
  });

  it("reports a gap in seq as an issue but still delivers", () => {
    const h = harness();
    h.sockets.last().open();
    h.sockets.last().push(ev(1));
    h.sockets.last().push(ev(5));
    expect(h.events.map((e) => e.seq)).toEqual([1, 5]);
    expect(h.issues[0]).toMatchObject({ code: "unavailable", detail: { reason: "gap" } });
  });

  it("resubscribes from resume_since when the daemon drops a slow consumer's events", () => {
    const h = harness();
    const s = h.sockets.last();
    s.open();
    s.push(ev(1));
    s.push({ error: { code: "busy", message: "slow consumer", detail: { skipped: 40, resume_since: 1 } } });
    expect(s.sent).toEqual([hello(0, ""), hello(1, "r1")]);
    expect(h.issues[0]).toMatchObject({ code: "busy", detail: { resume_since: 1, frame: "events" } });
  });

  it("surfaces a server error frame and a bad frame as typed issues", () => {
    const h = harness();
    h.sockets.last().open();
    h.sockets.last().push({ error: { code: "not_found", message: "no run r-9" } });
    h.sockets.last().push("{not json");
    h.sockets.last().push({ seq: 1, run_id: "r1", event: { type: "segment_start" } });
    expect(h.issues.map((i) => i.code)).toEqual(["not_found", "internal", "internal"]);
    expect(h.issues[2]?.detail?.reason).toBe("invalid_payload");
    expect(h.events).toEqual([]);
  });

  it("accepts a legacy daemon's bare events (no seq)", () => {
    const h = harness();
    h.sockets.last().open();
    h.sockets.last().push({ type: "stream_end", ts_ms: 9, reason: "sim_complete" });
    expect(h.events).toEqual([{ event: { type: "stream_end", ts_ms: 9, reason: "sim_complete" } }]);
    expect(h.stream.lastSeq()).toBeUndefined();
  });

  it("waits for a loading door before the first connection", async () => {
    const sockets = fakeSockets();
    let resolve: (d: Door) => void = () => undefined;
    const door = new Promise<Door>((r) => (resolve = r));
    const stream = openEventStream({ url: "ws://x/events", target: "live", door, socket: sockets.factory, timers: manualTimers(), onEvent: () => undefined });
    expect(sockets.all).toHaveLength(0);
    expect(stream.status()).toBe("connecting");
    resolve(contractDoor);
    await door;
    await Promise.resolve();
    expect(sockets.all).toHaveLength(1);
  });

  it("stops for good when closed by the client", () => {
    const h = harness();
    h.sockets.last().open();
    h.stream.close();
    expect(h.sockets.last().closed).toEqual({ code: 1000, reason: "client closed" });
    h.timers.advance(60_000);
    expect(h.sockets.all).toHaveLength(1);
    expect(h.stream.status()).toBe("closed");
  });
});
