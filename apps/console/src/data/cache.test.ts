import { describe, expect, it } from "vitest";
import { QueryCache, queryKey } from "./cache";
import { contractDoor } from "./contractDoor";
import { createDataLayer } from "./layer";
import { createRpcClient } from "./rpc";
import { fakeSockets, manualTimers, scriptedFetch, type FetchReply } from "./testing";

const flush = () => new Promise((r) => setTimeout(r, 0));

/** Schema-valid replies for the methods these tests call. */
const RESULTS: Readonly<Record<string, unknown>> = {
  "runs.page": { items: [] },
  "directive.add": { id: "d1", kind: "try", text: "x", created_ms: 1, active: true },
};
const valid = (method: string): FetchReply => ({ body: { result: RESULTS[method] ?? [] } });

function setup(reply: (method: string) => FetchReply = valid) {
  const fetch = scriptedFetch(reply);
  const timers = manualTimers();
  let now = 0;
  const client = createRpcClient({ door: contractDoor, fetch });
  const cache = new QueryCache(client, { timers, backstopMs: 60_000, now: () => now });
  return { fetch, timers, cache, setNow: (t: number) => (now = t) };
}

describe("query cache", () => {
  it("keys params by content, not by object identity or key order", () => {
    expect(queryKey("runs.page", { limit: 5, cursor: "c" })).toBe(queryKey("runs.page", { cursor: "c", limit: 5 }));
    expect(queryKey("runs.page", { limit: 5, cursor: undefined })).toBe(queryKey("runs.page", { limit: 5 }));
  });

  it("fetches once for any number of watchers", async () => {
    const s = setup();
    const off1 = s.cache.watch("tournament.list", {}, () => undefined);
    const off2 = s.cache.watch("tournament.list", {}, () => undefined);
    await flush();
    expect(s.fetch.calls).toEqual(["tournament.list"]);
    expect(s.cache.getSnapshot(queryKey("tournament.list", {})).status).toBe("ready");
    off1();
    off2();
  });

  it("refetches only the methods an event invalidates", async () => {
    const s = setup();
    s.cache.watch("runs.page", { limit: 50 }, () => undefined);
    s.cache.watch("directive.list", {}, () => undefined);
    await flush();
    s.cache.onEvent("stream_end");
    await flush();
    expect(s.fetch.calls).toEqual(["runs.page", "directive.list", "runs.page"]);
    s.cache.onEvent("chat"); // chat invalidates nothing
    await flush();
    expect(s.fetch.calls).toHaveLength(3);
  });

  it("does not poll: only the slow backstop refreshes, and only stale watched entries", async () => {
    const s = setup();
    s.cache.watch("tournament.list", {}, () => undefined);
    s.cache.startBackstop();
    await flush();
    s.timers.advance(59_999);
    await flush();
    expect(s.fetch.calls).toHaveLength(1);
    s.setNow(60_000);
    s.timers.advance(1);
    await flush();
    expect(s.fetch.calls).toHaveLength(2);
    s.cache.dispose();
  });

  it("keeps the last good data when a refetch fails", async () => {
    let fail = false;
    const row = { tournament_id: "t-1", instrument_hash: "h", runs: 1, completed: 1 };
    const s = setup(() => (fail ? { network: true } : { body: { result: [row] } }));
    s.cache.watch("tournament.list", {}, () => undefined);
    await flush();
    fail = true;
    s.cache.invalidate(["tournament.list"]);
    await flush();
    const snap = s.cache.getSnapshot(queryKey("tournament.list", {}));
    expect(snap.status).toBe("error");
    expect(snap.data).toEqual([row]);
    expect(snap.error?.code).toBe("unavailable");
    fail = false;
    s.cache.retryFailed();
    await flush();
    expect(s.cache.getSnapshot(queryKey("tournament.list", {})).status).toBe("ready");
  });
});

describe("data layer", () => {
  it("drives invalidation and connection state from the live stream", async () => {
    const sockets = fakeSockets();
    const timers = manualTimers();
    const fetch = scriptedFetch(valid);
    const layer = createDataLayer({ baseUrl: "http://127.0.0.1:7717", door: contractDoor, fetch, socket: sockets.factory, timers });
    layer.start();
    expect(sockets.last().url).toBe("ws://127.0.0.1:7717/events");
    expect(layer.connection.getSnapshot().stream).toBe("connecting");
    layer.cache.watch("runs.page", { limit: 10 }, () => undefined);
    await flush();
    sockets.last().open();
    sockets.last().push({ seq: 1, run_id: "r1", event: { type: "stream_end", ts_ms: 1, reason: "sim_complete" } });
    await flush();
    expect(fetch.calls).toEqual(["runs.page", "runs.page"]);
    sockets.last().drop();
    expect(layer.connection.getSnapshot()).toMatchObject({ stream: "reconnecting", rpc: "online" });
    layer.stop();
  });

  it("revalidates watched ready entries once when the stream reopens after a gap, never unwatched ones", async () => {
    const sockets = fakeSockets();
    const timers = manualTimers();
    const fetch = scriptedFetch(valid);
    let now = 1_000;
    const layer = createDataLayer({ baseUrl: "http://x", door: contractDoor, fetch, socket: sockets.factory, timers, now: () => now });
    layer.start();
    layer.cache.watch("runs.page", { limit: 10 }, () => undefined);
    const off = layer.cache.watch("directive.list", {}, () => undefined);
    await flush();
    sockets.last().open();
    await flush();
    off(); // directive.list is no longer on screen
    expect(fetch.calls).toEqual(["runs.page", "directive.list"]);
    expect(layer.reopenedAt()).toBeUndefined(); // the first open is not a reopen
    sockets.last().drop(); // events missed from here on
    now = 5_000;
    timers.advance(30_000);
    sockets.last().open();
    await flush();
    expect(layer.reopenedAt()).toBe(5_000);
    expect(fetch.calls).toEqual(["runs.page", "directive.list", "runs.page"]);
    expect(layer.cache.getSnapshot(queryKey("runs.page", { limit: 10 })).updatedAt).toBe(5_000);
    layer.stop();
  });

  it("refetches what a mutation invalidates", async () => {
    const fetch = scriptedFetch(valid);
    const layer = createDataLayer({ baseUrl: "http://x", door: contractDoor, fetch, socket: fakeSockets().factory, timers: manualTimers() });
    layer.cache.watch("directive.list", {}, () => undefined);
    await flush();
    await layer.mutate("directive.add", { kind: "try", text: "x" });
    await flush();
    expect(fetch.calls).toEqual(["directive.list", "directive.add", "directive.list"]);
  });
});
