import { describe, expect, it, vi } from "vitest";
import { RpcClient, RpcError, httpTransport, isOffline, type ConnectionState } from "./rpc";

function respond(body: string, init: { status?: number; statusText?: string } = {}) {
  return vi.fn(async () => new Response(body, { status: init.status ?? 200, statusText: init.statusText ?? "" }));
}

async function failure(p: Promise<unknown>): Promise<RpcError> {
  try {
    await p;
  } catch (e) {
    expect(e).toBeInstanceOf(RpcError);
    return e as RpcError;
  }
  throw new Error("expected the call to fail");
}

describe("httpTransport", () => {
  it("POSTs {method, params} to <base>/rpc and returns `result`", async () => {
    const fetch = respond(JSON.stringify({ result: [{ check: "PF-01", state: "green", detail: "ok" }] }));
    const t = httpTransport({ baseUrl: "http://127.0.0.1:7717/", fetch });
    await expect(t.call("preflight.run", {})).resolves.toEqual([{ check: "PF-01", state: "green", detail: "ok" }]);
    const [url, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("http://127.0.0.1:7717/rpc");
    expect(init.method).toBe("POST");
    expect(JSON.parse(String(init.body))).toEqual({ method: "preflight.run", params: {} });
  });

  it("accepts a null result (e.g. a method that returns nothing)", async () => {
    const t = httpTransport({ fetch: respond(JSON.stringify({ result: null })) });
    await expect(t.call("x", {})).resolves.toBeNull();
  });

  it("maps a network failure to kind=offline", async () => {
    const fetch = vi.fn(async () => {
      throw new TypeError("Failed to fetch");
    });
    const e = await failure(httpTransport({ fetch }).call("tournament.list", {}));
    expect(e.kind).toBe("offline");
    expect(e.method).toBe("tournament.list");
    expect(e.message).toContain("127.0.0.1:7717");
    expect(isOffline(e)).toBe(true);
  });

  it("maps `{error: {code, message}}` to kind=rpc with the code, even on a 4xx/5xx", async () => {
    const body = JSON.stringify({ error: { code: "preflight_red", message: "PF-07 is red" } });
    for (const status of [200, 409, 500]) {
      const e = await failure(httpTransport({ fetch: respond(body, { status }) }).call("sim.start", {}));
      expect(e.kind).toBe("rpc");
      expect(e.code).toBe("preflight_red");
      expect(e.message).toBe("PF-07 is red");
      expect(e.status).toBe(status);
      expect(isOffline(e)).toBe(false);
    }
  });

  it("maps a non-2xx without an error body to kind=http", async () => {
    const e = await failure(httpTransport({ fetch: respond("Bad Gateway", { status: 502, statusText: "Bad Gateway" }) }).call("runs.list", {}));
    expect(e.kind).toBe("http");
    expect(e.status).toBe(502);
    const e2 = await failure(httpTransport({ fetch: respond(JSON.stringify({ result: 1 }), { status: 500 }) }).call("runs.list", {}));
    expect(e2.kind).toBe("http");
  });

  it("maps non-JSON 200 and bodies without result/error to kind=protocol", async () => {
    expect((await failure(httpTransport({ fetch: respond("<html>") }).call("m", {}))).kind).toBe("protocol");
    expect((await failure(httpTransport({ fetch: respond(JSON.stringify({ ok: true })) }).call("m", {}))).kind).toBe("protocol");
    expect((await failure(httpTransport({ fetch: respond("") }).call("m", {}))).kind).toBe("protocol");
    expect((await failure(httpTransport({ fetch: respond(JSON.stringify([1, 2])) }).call("m", {}))).kind).toBe("protocol");
  });

  it("rejects a malformed error body as kind=protocol", async () => {
    const e = await failure(httpTransport({ fetch: respond(JSON.stringify({ error: "nope" })) }).call("m", {}));
    expect(e.kind).toBe("protocol");
  });

  it("times out as kind=timeout", async () => {
    vi.useFakeTimers();
    try {
      const fetch = vi.fn(
        (_u: RequestInfo | URL, init?: RequestInit) =>
          new Promise<Response>((_, reject) => {
            init?.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
          }),
      );
      const p = httpTransport({ fetch, timeoutMs: 50 }).call("reconcile.run", {});
      const assertion = failure(p);
      await vi.advanceTimersByTimeAsync(60);
      const e = await assertion;
      expect(e.kind).toBe("timeout");
      expect(e.message).toContain("50 ms");
    } finally {
      vi.useRealTimers();
    }
  });

  it("re-throws a caller abort as-is instead of reporting the daemon offline", async () => {
    const ctrl = new AbortController();
    const fetch = vi.fn(
      (_u: RequestInfo | URL, init?: RequestInit) =>
        new Promise<Response>((_, reject) => {
          init?.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
        }),
    );
    const p = httpTransport({ fetch }).call("run.transcript", { run_id: "r" }, ctrl.signal);
    ctrl.abort();
    await expect(p).rejects.toMatchObject({ name: "AbortError" });
  });
});

describe("RpcClient connection state", () => {
  it("reports online on success and on daemon-side errors, offline on network failure", async () => {
    const states: ConnectionState[] = [];
    const ok = new RpcClient(httpTransport({ fetch: respond(JSON.stringify({ result: [] })) }), { onState: (s) => states.push(s) });
    await ok.call("directive.list", {});
    const rpcErr = new RpcClient(httpTransport({ fetch: respond(JSON.stringify({ error: { code: "not_found", message: "x" } })) }), {
      onState: (s) => states.push(s),
    });
    await expect(rpcErr.call("run.verdict", { run_id: "r" })).rejects.toBeInstanceOf(RpcError);
    const down = new RpcClient(
      httpTransport({
        fetch: vi.fn(async () => {
          throw new TypeError("connection refused");
        }),
      }),
      { onState: (s) => states.push(s) },
    );
    await expect(down.call("directive.list", {})).rejects.toMatchObject({ kind: "offline" });
    expect(states).toEqual(["online", "online", "offline"]);
  });
});
