import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { parseEvent, parseRpcResult } from "./door";
import { ERROR_CODES, RPC_METHODS } from "./generated/meta";

const FIXTURE = new URL(
  "../../../crates/shadowrunr-daemon/tests/fixtures/sim/fixture-t/fixture-run/transcript.jsonl",
  import.meta.url,
);

const report = { check: "PF-01", state: "green", detail: "scripted engine" };

describe("parseRpcResult", () => {
  it("accepts a valid reply and returns the typed result", () => {
    const r = parseRpcResult("preflight.run", JSON.stringify({ result: [report] }));
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.value[0]?.state).toBe("green");
  });

  it("accepts an already-decoded body", () => {
    const r = parseRpcResult("runs.page", { result: { items: [] } });
    expect(r).toEqual({ ok: true, value: { items: [] } });
  });

  it("rejects an invalid payload with the path of the violation", () => {
    const r = parseRpcResult("preflight.run", { result: [{ ...report, state: "purple" }] });
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.error.kind).toBe("invalid");
      if (r.error.kind === "invalid") expect(r.error.path).toBe("/result/0/state");
    }
  });

  it("rejects null in an optional field (optional fields are omitted, never null)", () => {
    const r = parseRpcResult("runs.page", { result: { items: [], next_cursor: null } });
    expect(r.ok).toBe(false);
    if (!r.ok && r.error.kind === "invalid") expect(r.error.path).toBe("/result/next_cursor");
  });

  it("rejects an unknown method without looking at the body", () => {
    const r = parseRpcResult("no.such", { result: [] });
    expect(r).toEqual({
      ok: false,
      error: { kind: "unknown_method", message: 'unknown RPC method "no.such"', method: "no.such" },
    });
  });

  it("returns a daemon error reply as an rpc error with its code", () => {
    const body = { error: { code: "not_found", message: "run x", detail: { run_id: "x" } } };
    const r = parseRpcResult("run.verdict", body);
    expect(r).toEqual({
      ok: false,
      error: { kind: "rpc", code: "not_found", message: "run x", detail: { run_id: "x" } },
    });
  });

  it("rejects an error code outside the taxonomy", () => {
    const r = parseRpcResult("run.verdict", { error: { code: "sim_failed", message: "x" } });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.kind).toBe("invalid");
  });

  it("reports text that is not JSON as a parse error", () => {
    const r = parseRpcResult("preflight.run", "{not json");
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error.kind).toBe("parse");
  });

  it("rejects a body that is neither a result nor an error", () => {
    const r = parseRpcResult("preflight.run", { results: [] });
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
