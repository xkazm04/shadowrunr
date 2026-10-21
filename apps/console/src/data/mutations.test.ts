import { describe, expect, it } from "vitest";
import { contractDoor } from "./contractDoor";
import { createDataLayer } from "./layer";
import type { FetchLike } from "./rpc";
import { fakeSockets, manualTimers } from "./testing";

type Reply = { status?: number; body: unknown };

/** A fetch whose replies the test releases by hand: every call hangs until `answer(i, reply)`. */
function deferredFetch() {
  const calls: { method: string; sent: string; answer: (r: Reply) => void }[] = [];
  const f: FetchLike = (_url, init) => {
    const sent = typeof init.body === "string" ? init.body : "";
    const method = /"method":"([^"]+)"/.exec(sent)?.[1] ?? "?";
    return new Promise((resolve) => {
      calls.push({
        method,
        sent,
        answer: (r) => {
          const status = r.status ?? 200;
          const text = JSON.stringify(r.body);
          resolve({ ok: status < 400, status, text: () => Promise.resolve(text) });
        },
      });
    });
  };
  return Object.assign(f, { calls, methods: () => calls.map((c) => c.method) });
}

const tick = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

function layerWith(now = () => 1_000) {
  const fetch = deferredFetch();
  const layer = createDataLayer({ baseUrl: "http://x", door: contractDoor, fetch, socket: fakeSockets().factory, timers: manualTimers(), now });
  return { layer, fetch };
}

const T1 = { source: { tournament_id: "t1" } };
const T2 = { source: { tournament_id: "t2" } };

describe("the layer owns in-flight mutations", () => {
  it("joins a second call for the same key to the flight in progress: one request, one answer", async () => {
    const { layer, fetch } = layerWith();
    const a = layer.mutate("reconcile.run", T1);
    const b = layer.mutate("reconcile.run", T1);
    await tick();
    expect(fetch.methods().filter((m) => m === "reconcile.run")).toHaveLength(1);
    fetch.calls[0]?.answer({ body: { result: { report_id: "rec-1" } } });
    const [ra, rb] = await Promise.all([a, b]);
    expect(ra.ok && (ra.value as { report_id: string }).report_id).toBe("rec-1");
    expect(rb.ok && (rb.value as { report_id: string }).report_id).toBe("rec-1");
  });

  it("guards per key, never per method: t1 and t2 both run", async () => {
    const { layer, fetch } = layerWith();
    void layer.mutate("reconcile.run", T1);
    void layer.mutate("reconcile.run", T2);
    await tick();
    expect(fetch.methods()).toEqual(["reconcile.run", "reconcile.run"]);
  });

  it("guards only while in flight: the same call after it settles is sent again", async () => {
    const { layer, fetch } = layerWith();
    const p = { kind: "try", text: "new" };
    const D = { id: "d1", kind: "try", text: "new", created_ms: 1, active: true };
    const first = layer.mutate("directive.add", p);
    await tick();
    fetch.calls[0]?.answer({ body: { result: D } });
    await first;
    void layer.mutate("directive.add", p);
    await tick();
    expect(fetch.methods().filter((m) => m === "directive.add")).toHaveLength(2);
  });

  it("keeps a failure nobody was mounted to read, until it is acknowledged", async () => {
    const { layer, fetch } = layerWith();
    void layer.mutate("reconcile.run", T1);
    await tick();
    expect(layer.mutations.inflight("reconcile.run")).toHaveLength(1);
    fetch.calls[0]?.answer({ status: 503, body: { error: { code: "busy", message: "the reconciler is busy" } } });
    await tick();
    expect(layer.mutations.inflight("reconcile.run")).toHaveLength(0);
    const missed = layer.mutations.unacked("reconcile.run");
    expect(missed?.result.ok).toBe(false);
    expect(missed && !missed.result.ok && missed.result.error.code).toBe("busy");
    layer.mutations.ack(missed?.id ?? -1);
    expect(layer.mutations.unacked("reconcile.run")).toBeUndefined();
  });

  it("keeps the newest 32 settled outcomes and never evicts a call in flight", async () => {
    const { layer, fetch } = layerWith();
    void layer.mutate("reconcile.run", T1); // stays in flight throughout
    for (let i = 0; i < 41; i++) void layer.mutate("directive.deactivate", { id: `d${i}` });
    await tick();
    for (const c of fetch.calls) if (c.method === "directive.deactivate") c.answer({ body: { result: { id: /"id":"([^"]+)"/.exec(c.sent)?.[1], kind: "try", text: "x", created_ms: 1, active: false } } });
    await tick();
    const kept = layer.mutations.settled();
    expect(kept).toHaveLength(32);
    expect((kept[0]?.params as { id: string }).id).toBe("d9");
    expect((kept[31]?.params as { id: string }).id).toBe("d40");
    expect(layer.mutations.inflight("reconcile.run")).toHaveLength(1);
  });
});
