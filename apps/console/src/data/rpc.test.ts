import { afterEach, describe, expect, it, vi } from "vitest";
import { contractDoor } from "./contractDoor";
import { ERROR_CODES, UI_STATE, viewOfError, type DataError } from "./errors";
import type { Result } from "./result";
import { createRpcClient } from "./rpc";
import { scriptedFetch, type FetchReply } from "./testing";

function client(reply: (method: string) => FetchReply, opts: { timeoutMs?: number } = {}) {
  const fetch = scriptedFetch(reply);
  const seen: string[] = [];
  const c = createRpcClient({
    baseUrl: "http://127.0.0.1:7717/",
    door: contractDoor,
    fetch,
    ...(opts.timeoutMs === undefined ? {} : { timeoutMs: opts.timeoutMs }),
    onReachability: (r) => seen.push(r),
  });
  return { c, fetch, seen };
}

async function failure(p: Promise<Result<unknown, DataError>>): Promise<DataError> {
  const r = await p;
  if (r.ok) throw new Error("expected a failure");
  return r.error;
}

afterEach(() => vi.useRealTimers());

describe("rpc client", () => {
  it("returns the validated result", async () => {
    const report = [{ check: "PF-01", state: "green", detail: "ok" }];
    const { c, seen } = client(() => ({ body: { result: report } }));
    await expect(c.call("preflight.run", {})).resolves.toEqual({ ok: true, value: report });
    expect(seen).toEqual(["online"]);
  });

  it("maps every daemon error code to a typed error and a UI state", async () => {
    expect(ERROR_CODES).toHaveLength(10);
    for (const code of ERROR_CODES) {
      const { c } = client(() => ({ status: 400, body: { error: { code, message: `boom ${code}`, detail: { run_id: "r-1" } } } }));
      const e = await failure(c.call("run.verdict", { run_id: "r-1" }));
      expect(e).toEqual({ code, message: `boom ${code}`, detail: { run_id: "r-1", method: "run.verdict" } });
      expect(viewOfError(e).state).toBe(UI_STATE[code]);
    }
  });

  it("is offline (unavailable) when the daemon cannot be reached", async () => {
    const { c, seen } = client(() => ({ network: true }));
    const e = await failure(c.call("tournament.list", {}));
    expect(e.code).toBe("unavailable");
    expect(e.detail?.reason).toBe("offline");
    expect(viewOfError(e).state).toBe("offline");
    expect(seen).toEqual(["offline"]);
  });

  it("times out a hanging request", async () => {
    vi.useFakeTimers();
    const { c, seen } = client(() => ({ hang: true }), { timeoutMs: 2_000 });
    const pending = c.call("reconcile.run", { source: { run_ids: [] } });
    await vi.advanceTimersByTimeAsync(2_000);
    const e = await failure(pending);
    expect(e).toMatchObject({ code: "unavailable", detail: { reason: "timeout", method: "reconcile.run" } });
    expect(viewOfError(e)).toMatchObject({ state: "degraded", title: "The daemon did not answer in time" });
    expect(seen).toEqual([]); // a timeout proves nothing about reachability
  });

  it("reports a cancelled call without blaming the daemon", async () => {
    const { c, seen } = client(() => ({ hang: true }));
    const ctrl = new AbortController();
    const pending = c.call("runs.page", { limit: 50 }, { signal: ctrl.signal });
    ctrl.abort();
    expect((await failure(pending)).detail?.reason).toBe("aborted");
    expect(seen).toEqual([]);
  });

  it("rejects replies that break the contract as internal errors with a path", async () => {
    const cases: [unknown, string | RegExp][] = [
      ["<html>502</html>", ""],
      [{ neither: true }, ""],
      [{ result: [{ check: 1, state: "green", detail: "ok" }] }, /^\/result\/0/],
      [{ error: { code: "busy" } }, /error|^$/],
    ];
    for (const [body, path] of cases) {
      const { c } = client(() => ({ body }));
      const e = await failure(c.call("preflight.run", {}));
      expect(e.code).toBe("internal");
      expect(e.detail?.reason).toBe("invalid_payload");
      if (typeof path === "string") expect(e.detail?.path).toBe(path);
      else expect(String(e.detail?.path)).toMatch(path);
    }
  });

  it("rejects a code outside the taxonomy at the door", async () => {
    const { c } = client(() => ({ status: 500, body: { error: { code: "sim_failed", message: "judge crashed" } } }));
    const e = await failure(c.call("sim.start", {}));
    expect(e).toMatchObject({ code: "internal", detail: { reason: "invalid_payload" } });
  });

  it("knows a method the contract lacks before it hits the network", async () => {
    const { c } = client(() => ({ body: { result: null } }));
    expect((await failure(c.call("no.such.method", {}))).code).toBe("method_not_found");
  });

  it("posts to /rpc once per call", async () => {
    const { c, fetch } = client(() => ({ body: { result: [] } }));
    await c.call("directive.list", {});
    expect(fetch.calls).toEqual(["directive.list"]);
  });
});
