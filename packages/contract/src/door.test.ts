import { readFileSync } from "node:fs";

import { afterEach, describe, expect, it, vi } from "vitest";

import { parseEvent, parseRpcResult } from "./door";
import { ERROR_CODES, RPC_METHODS } from "./generated/meta";

const FIXTURE = new URL(
  "../../../crates/shadowrunr-daemon/tests/fixtures/sim/fixture-t/fixture-run/transcript.jsonl",
  import.meta.url,
);

const report = { check: "PF-01", state: "green", detail: "scripted engine" };

describe("parseRpcResult", () => {
  it("accepts a valid reply and returns the typed result", async () => {
    const r = await parseRpcResult("preflight.run", JSON.stringify({ result: [report] }));
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.value[0]?.state).toBe("green");
  });

  it("accepts an already-decoded body", async () => {
    const r = await parseRpcResult("runs.page", { result: { items: [] } });
    expect(r).toEqual({ ok: true, value: { items: [] } });
  });

  it("rejects an invalid payload with the path of the violation", async () => {
    const r = await parseRpcResult("preflight.run", { result: [{ ...report, state: "purple" }] });
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.error.kind).toBe("invalid");
      if (r.error.kind === "invalid") expect(r.error.path).toBe("/result/0/state");
    }
  });

  it("rejects null in an optional field (optional fields are omitted, never null)", async () => {
    const r = await parseRpcResult("runs.page", { result: { items: [], next_cursor: null } });
    expect(r.ok).toBe(false);
    if (!r.ok && r.error.kind === "invalid") expect(r.error.path).toBe("/result/next_cursor");
  });

  it("rejects an unknown method without looking at the body", async () => {
    const r = await parseRpcResult("no.such", { result: [] });
    expect(r).toEqual({
      ok: false,
      error: { kind: "unknown_method", message: 'unknown RPC method "no.such"', method: "no.such" },
    });
  });

  it("returns a daemon error reply as an rpc error with its code", async () => {
    const body = { error: { code: "not_found", message: "run x", detail: { run_id: "x" } } };
    const r = await parseRpcResult("run.verdict", body);
    expect(r).toEqual({
      ok: false,
      error: { kind: "rpc", code: "not_found", message: "run x", detail: { run_id: "x" } },
    });
  });

  it("rejects an error code outside the taxonomy", async () => {
    const r = await parseRpcResult("run.verdict", { error: { code: "sim_failed", message: "x" } });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.kind).toBe("invalid");
  });

  it("reports text that is not JSON as a parse error", async () => {
    const r = await parseRpcResult("preflight.run", "{not json");
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.kind).toBe("parse");
  });

  it("rejects a body that is neither a result nor an error", async () => {
    const r = await parseRpcResult("preflight.run", { results: [] });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.kind).toBe("invalid");
  });
});

describe("parseEvent", () => {
  const start = {
    type: "stream_start",
    ts_ms: 0,
    stream_id: "s",
    character: "malakor",
    tone_variant: "theatrical",
    core_tier: "haiku",
    audience_size: 3,
  };

  it("accepts every line of a real stored transcript (legacy bare frames)", () => {
    const lines = readFileSync(FIXTURE, "utf8").split("\n").filter((l) => l.trim() !== "");
    expect(lines.length).toBeGreaterThan(5);
    for (const line of lines) {
      const r = parseEvent(line);
      expect(r.ok, line).toBe(true);
      if (r.ok) expect(r.value.kind).toBe("event");
    }
  });

  it("reads the v1 say speaker `malakor` as `host`, bare or enveloped", () => {
    const say = { type: "say", ts_ms: 1, speaker: "malakor", text: "Rise." };
    const host = { ...say, speaker: "host" };
    expect(parseEvent(JSON.stringify(say))).toEqual({ ok: true, value: { kind: "event", event: host } });
    expect(parseEvent({ seq: 2, run_id: "r1", event: say })).toEqual({
      ok: true,
      value: { kind: "event", event: host, seq: 2, runId: "r1" },
    });
    expect(parseEvent({ ...say, speaker: "narrator" }).ok).toBe(false);
  });

  it("unwraps an enveloped frame with its seq and run id", () => {
    const r = parseEvent(JSON.stringify({ seq: 7, run_id: "r1", event: start }));
    expect(r).toEqual({ ok: true, value: { kind: "event", event: start, seq: 7, runId: "r1" } });
  });

  it("decodes an error frame", () => {
    const r = parseEvent({ error: { code: "busy", message: "slow consumer", detail: { resume_since: 4 } } });
    expect(r.ok && r.value.kind === "error" && r.value.error.code).toBe("busy");
  });

  it("rejects an unknown event type", () => {
    const r = parseEvent({ type: "teleport", ts_ms: 1 });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.kind).toBe("invalid");
  });
});

describe("tournament.results", () => {
  const usage = {
    calls: 0,
    errors: 0,
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_creation_tokens: 0,
    cost_usd: 0,
  };
  const key = { instrument_hash: "h", tone: "t", core_tier: "haiku", segment: "SEG-01", audience_size: 3 };
  const cell = {
    cell: key,
    runs: 1,
    judged: 1,
    void: 0,
    failed: 0,
    judge_tiers: ["sonnet"],
    rules: {},
    quality: {},
    core_latency: { n: 0 },
    usage,
    usage_by_role: {},
  };
  // A results.json written before tier decisions: no tier_decisions, no per-cell counts.
  const old = {
    tournament_id: "t",
    instrument_hashes: ["h"],
    calibration: [{ kind: "default_assumption" }],
    runs: 1,
    judged: 1,
    void: 0,
    failed: 0,
    cells: [cell],
    latency_by_tier: {},
    usage_total: usage,
    usage_by_role: {},
  };

  it("accepts a results.json written before tier decisions", async () => {
    expect((await parseRpcResult("tournament.results", { result: old })).ok).toBe(true);
  });

  it("accepts a tier decision and the per-cell run counts", async () => {
    const decided = {
      ...old,
      cells: [{ ...cell, runs_with_fail: 0, usable_runs: 1 }],
      tier_decisions: [
        {
          group: { instrument_hash: "h", tone: "t", segment: "SEG-01", audience_size: 3 },
          reference_judge: "sonnet",
          ranked: [{ core_tier: "haiku", judged: 1, usable_runs: 1, mean_quality: 0.8 }],
          ineligible: [],
          beside: [],
          tie_set: ["haiku"],
          winner: "haiku",
          provisional: true,
          decided_by: "only_eligible",
        },
      ],
    };
    const r = await parseRpcResult("tournament.results", { result: decided });
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.value.tier_decisions?.[0]?.winner).toBe("haiku");
  });

  it("rejects a decision reason outside the vocabulary", async () => {
    const bad = {
      ...old,
      tier_decisions: [
        {
          group: { instrument_hash: "h", tone: "t", segment: "SEG-01", audience_size: 3 },
          ranked: [],
          ineligible: [{ core_tier: "haiku", reason: "vibes", judged: 1, runs_with_fail: 0 }],
          beside: [],
          tie_set: [],
          provisional: false,
        },
      ],
    };
    expect((await parseRpcResult("tournament.results", { result: bad })).ok).toBe(false);
  });
});

describe("generated metadata", () => {
  it("keeps the error taxonomy at ten codes or fewer", () => {
    expect(ERROR_CODES.length).toBeLessThanOrEqual(10);
    expect(ERROR_CODES).toContain("internal");
  });

  it("lists the new paged and catalogue methods", () => {
    for (const m of ["character.list", "character.get", "segments.tables", "ranks.table"] as const) {
      expect(RPC_METHODS).toContain(m);
    }
    for (const m of ["ledger.page", "runs.page", "tournaments.page"] as const) {
      expect(RPC_METHODS).toContain(m);
    }
  });
});

describe("per-method result validators (lazy)", () => {
  // Fresh modules per case, so the door's memo and the loader map start empty.
  async function freshDoor() {
    vi.resetModules();
    const meta = await import("./generated/meta");
    const door = await import("./door");
    return { loaders: meta.RESULT_LOADERS, door };
  }

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("loads only the called method's validator module", async () => {
    const { loaders, door } = await freshDoor();
    const spies = Object.fromEntries(
      Object.keys(loaders).map((m) => [m, vi.spyOn(loaders, m as keyof typeof loaders)]),
    );
    const r = await door.parseRpcResult("runs.page", { result: { items: [] } });
    expect(r).toEqual({ ok: true, value: { items: [] } });
    const called = Object.entries(spies).filter(([, s]) => s.mock.calls.length > 0);
    expect(called.map(([m, s]) => [m, s.mock.calls.length])).toEqual([["runs.page", 1]]);
    // The second call reuses the loaded validator: no second load.
    await door.parseRpcResult("runs.page", { result: { items: [] } });
    expect(spies["runs.page"]?.mock.calls.length).toBe(1);
  });

  it("does not load a validator for an error reply or an unknown method", async () => {
    const { loaders, door } = await freshDoor();
    const spy = vi.spyOn(loaders, "run.verdict");
    const r = await door.parseRpcResult("run.verdict", { error: { code: "busy", message: "x" } });
    expect(r.ok).toBe(false);
    expect((await door.parseRpcResult("no.such", { result: 1 })).ok).toBe(false);
    expect(spy).not.toHaveBeenCalled();
  });

  it("resolves a failed validator load as unavailable_validator and never throws; the next call retries", async () => {
    const { loaders, door } = await freshDoor();
    const spy = vi.spyOn(loaders, "runs.page").mockRejectedValueOnce(new Error("Failed to fetch chunk"));
    const r = await door.parseRpcResult("runs.page", { result: { items: [] } });
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.error.kind).toBe("unavailable_validator");
      expect(r.error.message).toContain("runs.page");
      expect(r.error.message).toContain("Failed to fetch chunk");
    }
    expect(await door.parseRpcResult("runs.page", { result: { items: [] } })).toEqual({ ok: true, value: { items: [] } });
    expect(spy).toHaveBeenCalledTimes(2);
  });

  it("preloads validators ahead of a reply, and a preloaded method is not loaded again", async () => {
    const { loaders, door } = await freshDoor();
    const spy = vi.spyOn(loaders, "runs.page");
    await door.preloadResultValidators(["runs.page"]);
    expect(spy).toHaveBeenCalledTimes(1);
    expect((await door.parseRpcResult("runs.page", { result: { items: [] } })).ok).toBe(true);
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("has a loader for every method", async () => {
    const { loaders } = await freshDoor();
    expect(Object.keys(loaders).sort()).toEqual([...RPC_METHODS].sort());
  });
});
