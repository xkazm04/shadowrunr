// Scoped, typed, coalesced invalidation: an event or a mutation refetches only the cached keys
// it can have changed, once per burst, and never fewer than the method-wide rule for a key whose
// scope includes the event's run.
import { describe, expect, it } from "vitest";
import { contractDoor } from "./contractDoor";
import type { Door } from "./door";
import { EVENT_RULES, MUTATION_RULES } from "./invalidation";
import { createDataLayer } from "./layer";
import { ok } from "./result";
import { fakeSockets, manualTimers, scriptedFetch } from "./testing";

const flush = async (n = 4) => {
  for (let i = 0; i < n; i += 1) await new Promise((r) => setTimeout(r, 0));
};

/** Every reply body sent, by its text: the test door answers with the value it was made from. */
const bodies = new Map<string, unknown>();

/** The contract's event parser, but any reply passes: these tests count calls, not shapes. */
const door: Door = {
  parseRpcResult: (_method, raw) => ok(bodies.get(raw)),
  parseEvent: (raw) => contractDoor.parseEvent(raw),
};

/** The string params these tests scope on, read from the sent request body. */
function paramsOf(body: string): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const k of ["id", "run_id", "tournament_id", "cursor"]) {
    const v = new RegExp(`"${k}":"([^"]*)"`).exec(body)?.[1];
    if (v !== undefined) out[k] = v;
  }
  return out;
}

interface Sent {
  readonly method: string;
  readonly params: Record<string, unknown>;
}

/** A daemon reply; `HANG` never answers. */
const HANG = Symbol("hang");
type Daemon = (method: string, params: Record<string, unknown>) => unknown;

function setup(daemon: Daemon = () => ({ items: [] })) {
  const sent: Sent[] = [];
  const fetch = scriptedFetch((method, body) => {
    const params = paramsOf(typeof body === "string" ? body : "");
    sent.push({ method, params });
    const result = daemon(method, params);
    if (result === HANG) return { hang: true };
    bodies.set(JSON.stringify({ result }), result);
    return { body: { result } };
  });
  const sockets = fakeSockets();
  const timers = manualTimers();
  const layer = createDataLayer({ baseUrl: "http://x", door, fetch, socket: sockets.factory, timers, backstopMs: 3_600_000 });
  layer.start();
  sockets.last().open();
  const seqs = new Map<string, number>();
  const push = (runId: string, event: Record<string, unknown>) => {
    const seq = (seqs.get(runId) ?? 0) + 1;
    seqs.set(runId, seq);
    sockets.last().push({ seq, run_id: runId, event });
  };
  /** A frame with no envelope: the event carries no run id. */
  const pushBare = (event: Record<string, unknown>) => sockets.last().push(event);
  const job = (runId: string) => push(runId, { type: "job", ts_ms: 1, job_id: "j1", status: "running" });
  const watch = (method: string, params: Record<string, unknown>) => layer.cache.watch(method, params, () => undefined);
  /** Calls of `method` sent since `mark()`. */
  let from = 0;
  const mark = () => (from = sent.length);
  const since = (method: string) => sent.slice(from).filter((s) => s.method === method);
  /** Lets every coalescing window close, then the replies land. */
  const settle = async () => {
    await flush();
    timers.advance(1_000);
    await flush();
  };
  return { layer, timers, job, push, pushBare, watch, mark, since, settle };
}

const row = (run_id: string, tournament_id: string) => ({ run_id, tournament_id });
const ledgerChain = (watch: (m: string, p: Record<string, unknown>) => unknown, tid: string, pages = 12) => {
  for (let i = 0; i < pages; i += 1) watch("ledger.page", { tournament_id: tid, limit: 500, ...(i === 0 ? {} : { cursor: String(i * 500) }) });
};

describe("scoped event invalidation", () => {
  it("case 1: a job for r1 refetches r1's ledger page only, r2's is untouched", async () => {
    const s = setup();
    s.watch("ledger.page", { run_id: "r1", limit: 500 });
    s.watch("ledger.page", { run_id: "r2", limit: 500 });
    await s.settle();
    s.mark();
    s.job("r1");
    await s.settle();
    expect(s.since("ledger.page").map((c) => c.params.run_id)).toEqual(["r1"]);
  });

  it("case 2: a complete runs listing for T without sbx-1 -> 0 fetches; a run of T -> the chain refetches", async () => {
    const s = setup((method) => (method === "runs.page" ? { items: [row("r1", "T"), row("r2", "T")] } : { items: [] }));
    s.watch("runs.page", { tournament_id: "T", limit: 500 });
    ledgerChain(s.watch, "T");
    await s.settle();
    s.mark();
    s.job("sbx-1");
    await s.settle();
    expect(s.since("ledger.page")).toHaveLength(0);
    s.job("r2");
    await s.settle();
    expect(s.since("ledger.page")).toHaveLength(12);
  });

  it("case 3 (guard): T's runs listing is partial and no cached row mentions the run -> the chain IS invalidated", async () => {
    const s = setup((method) => (method === "runs.page" ? { items: [row("r1", "T")], next_cursor: "1" } : { items: [] }));
    s.watch("runs.page", { tournament_id: "T", limit: 1 });
    ledgerChain(s.watch, "T");
    await s.settle();
    s.mark();
    s.job("sbx-1");
    await s.settle();
    expect(s.since("ledger.page")).toHaveLength(12);
  });

  it("case 4 (guard): the daemon engine ledger (no run, no tournament) refetches on every job event", async () => {
    const s = setup();
    s.watch("ledger.page", { limit: 500 });
    await s.settle();
    s.mark();
    for (const r of ["r1", "sbx-1", "anything"]) {
      s.job(r);
      await s.settle();
    }
    expect(s.since("ledger.page")).toHaveLength(3);
  });

  it("case 6: five job events for r1 inside one 250 ms window -> one fetch per watched key", async () => {
    const s = setup();
    s.watch("ledger.page", { run_id: "r1", limit: 500 });
    s.watch("ledger.summary", { run_id: "r1" });
    await s.settle();
    s.mark();
    for (let i = 0; i < 5; i += 1) {
      s.job("r1");
      s.timers.advance(40);
    }
    await s.settle();
    expect(s.since("ledger.page")).toHaveLength(1);
    expect(s.since("ledger.summary")).toHaveLength(1);
  });
});

describe("guard: a scoped rule never misses a refetch the method-wide rule made for an affected run", () => {
  it("refetches every entry whose scope holds the run: its run, its tournament, the engine ledger", async () => {
    const s = setup((method) => (method === "runs.page" ? { items: [row("r1", "T"), row("x9", "U")] } : { items: [] }));
    s.watch("runs.page", { limit: 500 });
    s.watch("ledger.page", { run_id: "r1", limit: 500 });
    s.watch("ledger.page", { tournament_id: "T", limit: 500 });
    s.watch("ledger.summary", { tournament_id: "T" });
    s.watch("ledger.page", { limit: 500 });
    await s.settle();
    for (const type of ["job", "segment_end"]) {
      s.mark();
      s.push("r1", type === "job" ? { type, ts_ms: 1, job_id: "j", status: "done" } : { type, ts_ms: 1, segment: "SEG-01" });
      await s.settle();
      expect(s.since("ledger.page").length, type).toBe(3);
      expect(s.since("ledger.summary").length, type).toBe(1);
    }
  });

  it("an event with no run id invalidates as broadly as before", async () => {
    const s = setup((method) => (method === "runs.page" ? { items: [row("r1", "T")] } : { items: [] }));
    s.watch("runs.page", { tournament_id: "T", limit: 500 });
    s.watch("ledger.page", { run_id: "r1", limit: 500 });
    s.watch("ledger.page", { tournament_id: "T", limit: 500 });
    await s.settle();
    s.mark();
    s.pushBare({ type: "job", ts_ms: 1, job_id: "j", status: "done" });
    await s.settle();
    expect(s.since("ledger.page")).toHaveLength(2);
  });

  it("a complete listing that is refetching (a run just started) proves nothing: the chain still refetches", async () => {
    let rows = [row("r1", "T")];
    let hang = false;
    const s = setup((method) => (method === "runs.page" ? (hang ? HANG : { items: rows }) : { items: [] }));
    s.watch("runs.page", { tournament_id: "T", limit: 500 });
    ledgerChain(s.watch, "T", 2);
    await s.settle();
    // r9 starts in T: the listing is invalidated and its refetch has not answered yet.
    rows = [row("r1", "T"), row("r9", "T")];
    hang = true;
    s.layer.cache.invalidate(["runs.page"]);
    await flush();
    s.mark();
    s.job("r9");
    await s.settle();
    expect(s.since("ledger.page")).toHaveLength(2);
  });

  it("a listing from a filtered or partial chain is never taken as complete", async () => {
    const s = setup((method, p) =>
      method === "runs.page" ? (p.cursor === undefined ? { items: [row("r1", "T")], next_cursor: "a" } : { items: [row("r2", "T")], next_cursor: "b" }) : { items: [] },
    );
    s.watch("runs.page", { tournament_id: "T", limit: 1 });
    s.watch("runs.page", { tournament_id: "T", limit: 1, cursor: "a" });
    s.watch("runs.page", { tournament_id: "T", limit: 500, filter: { has_verdict: true } });
    ledgerChain(s.watch, "T", 1);
    await s.settle();
    s.mark();
    s.job("r7");
    await s.settle();
    expect(s.since("ledger.page")).toHaveLength(1);
  });
});

describe("scoped mutation invalidation", () => {
  it("case 5: clips.approve c1 refetches clips.get c1 plus clips.list, not clips.get c2", async () => {
    const s = setup((method) => (method === "clips.list" ? { items: [] } : { id: "c" }));
    s.watch("clips.get", { id: "c1" });
    s.watch("clips.get", { id: "c2" });
    s.watch("clips.list", { limit: 50 });
    await s.settle();
    s.mark();
    const r = await s.layer.mutate("clips.approve", { id: "c1", signature: "s" });
    expect(r.ok).toBe(true);
    await s.settle();
    expect(s.since("clips.get").map((c) => c.params.id)).toEqual(["c1"]);
    expect(s.since("clips.list")).toHaveLength(1);
  });
});

describe("rule tables are typed against the contract", () => {
  it("case 7: a misspelt method or event type is a tsc error", () => {
    const tables: (typeof EVENT_RULES | typeof MUTATION_RULES)[] = [
      // @ts-expect-error -- not a StreamEvent type
      { jobb: ["ledger.page"] } satisfies typeof EVENT_RULES,
      // @ts-expect-error -- not an RpcMethod
      { job: ["ledger.pages"] } satisfies typeof EVENT_RULES,
      // @ts-expect-error -- not an RpcMethod
      { "clips.aprove": ["clips.list"] } satisfies typeof MUTATION_RULES,
    ];
    expect(tables).toHaveLength(3);
  });
});
