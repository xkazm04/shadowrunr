// JSON-RPC-ish client for the daemon's `POST /rpc` (docs/spec/contract.md § Daemon protocol).
// Every failure is normalised into one RpcError with a `kind`, so views can tell "the daemon
// is not running" (offline) from "the daemon said no" (rpc).

import type { RpcErrorBody, RpcMethod, RpcParams, RpcResult } from "../contract";

export const DEFAULT_DAEMON_URL = "http://127.0.0.1:7717";
export const DAEMON_START_COMMAND = "shadowrunr daemon --port 7717";

export type RpcErrorKind =
  /** No connection: the daemon is not running or the port is wrong. */
  | "offline"
  /** The request took longer than the client timeout. */
  | "timeout"
  /** The daemon answered with a non-2xx status and no error body. */
  | "http"
  /** The body was not JSON, or neither `result` nor `error`. */
  | "protocol"
  /** The daemon returned `{"error": {code, message}}`. */
  | "rpc";

export class RpcError extends Error {
  readonly kind: RpcErrorKind;
  readonly method: string;
  readonly code: string | undefined;
  readonly status: number | undefined;

  constructor(
    kind: RpcErrorKind,
    method: string,
    message: string,
    opts: { code?: string; status?: number } = {},
  ) {
    super(message);
    this.name = "RpcError";
    this.kind = kind;
    this.method = method;
    this.code = opts.code;
    this.status = opts.status;
  }
}

export function isOffline(err: unknown): boolean {
  return err instanceof RpcError && err.kind === "offline";
}

/** Anything that can answer an RPC call: the HTTP daemon or the fixture mock. */
export interface Transport {
  call(method: string, params: unknown, signal?: AbortSignal): Promise<unknown>;
}

export type ConnectionState = "unknown" | "online" | "offline" | "mock";

export interface HttpTransportOptions {
  baseUrl?: string;
  timeoutMs?: number;
  fetch?: typeof fetch;
}

function isErrorBody(v: unknown): v is RpcErrorBody {
  return (
    typeof v === "object" &&
    v !== null &&
    typeof (v as RpcErrorBody).code === "string" &&
    typeof (v as RpcErrorBody).message === "string"
  );
}

export function httpTransport(opts: HttpTransportOptions = {}): Transport {
  const baseUrl = (opts.baseUrl ?? DEFAULT_DAEMON_URL).replace(/\/+$/, "");
  const timeoutMs = opts.timeoutMs ?? 30_000;
  const doFetch = opts.fetch ?? ((...a: Parameters<typeof fetch>) => fetch(...a));

  return {
    async call(method, params, signal) {
      const ctrl = new AbortController();
      let timedOut = false;
      const timer = setTimeout(() => {
        timedOut = true;
        ctrl.abort();
      }, timeoutMs);
      const onAbort = () => ctrl.abort();
      signal?.addEventListener("abort", onAbort);

      let res: Response;
      try {
        res = await doFetch(`${baseUrl}/rpc`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ method, params: params ?? {} }),
          signal: ctrl.signal,
        });
      } catch (e) {
        if (timedOut) {
          throw new RpcError("timeout", method, `${method} timed out after ${timeoutMs} ms`);
        }
        if (signal?.aborted) throw e; // caller cancelled: not an error to show
        throw new RpcError(
          "offline",
          method,
          `Cannot reach the daemon at ${baseUrl} (${e instanceof Error ? e.message : String(e)})`,
        );
      } finally {
        clearTimeout(timer);
        signal?.removeEventListener("abort", onAbort);
      }

      let text: string;
      try {
        text = await res.text();
      } catch (e) {
        throw new RpcError("protocol", method, `Could not read the response body: ${String(e)}`, {
          status: res.status,
        });
      }

      let body: unknown;
      try {
        body = text.length ? JSON.parse(text) : undefined;
      } catch {
        if (!res.ok) {
          throw new RpcError("http", method, `HTTP ${res.status} ${res.statusText}`.trim(), {
            status: res.status,
          });
        }
        throw new RpcError("protocol", method, "The daemon replied with non-JSON", {
          status: res.status,
        });
      }

      if (typeof body === "object" && body !== null) {
        const b = body as Record<string, unknown>;
        if ("error" in b) {
          if (isErrorBody(b.error)) {
            throw new RpcError("rpc", method, b.error.message, {
              code: b.error.code,
              status: res.status,
            });
          }
          throw new RpcError("protocol", method, "Malformed error body", { status: res.status });
        }
        if (!res.ok) {
          throw new RpcError("http", method, `HTTP ${res.status} ${res.statusText}`.trim(), {
            status: res.status,
          });
        }
        if ("result" in b) return b.result;
      } else if (!res.ok) {
        throw new RpcError("http", method, `HTTP ${res.status} ${res.statusText}`.trim(), {
          status: res.status,
        });
      }
      throw new RpcError("protocol", method, "Response has neither `result` nor `error`", {
        status: res.status,
      });
    },
  };
}

/** Typed facade over a transport, reporting connection state to a listener. */
export class RpcClient {
  readonly transport: Transport;
  readonly mock: boolean;
  private readonly onState: ((s: ConnectionState) => void) | undefined;

  constructor(transport: Transport, opts: { mock?: boolean; onState?: (s: ConnectionState) => void } = {}) {
    this.transport = transport;
    this.mock = opts.mock ?? false;
    this.onState = opts.onState;
  }

  async call<M extends RpcMethod>(
    method: M,
    params: RpcParams<M>,
    signal?: AbortSignal,
  ): Promise<RpcResult<M>> {
    try {
      const r = (await this.transport.call(method, params, signal)) as RpcResult<M>;
      this.onState?.(this.mock ? "mock" : "online");
      return r;
    } catch (e) {
      if (e instanceof RpcError) {
        // An rpc/http/protocol error still proves the daemon is up; a timeout proves nothing.
        if (e.kind === "offline") this.onState?.("offline");
        else if (e.kind !== "timeout") this.onState?.(this.mock ? "mock" : "online");
      }
      throw e;
    }
  }
}
