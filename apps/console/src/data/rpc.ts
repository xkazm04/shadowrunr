// `POST /rpc` client. Every call resolves to a `Result`: it never throws, and every failure is a
// `DataError` from the one taxonomy. Each call has a timeout; the reply text goes to the door.
import type { DoorSource } from "./door";
import { dataError, type DataError } from "./errors";
import { DEFAULT_DAEMON_URL } from "./config";
import { err, type Result } from "./result";

export const DEFAULT_TIMEOUT_MS = 15_000;

export type FetchLike = (input: string, init: RequestInit) => Promise<Pick<Response, "ok" | "status" | "text">>;

export type Reachability = "unknown" | "online" | "offline";

export interface RpcClientOptions {
  readonly baseUrl?: string;
  /** The door, or a promise of it while its validators load. */
  readonly door: DoorSource;
  readonly fetch?: FetchLike;
  readonly timeoutMs?: number;
  /** Told after every call whether the daemon answered at all. */
  readonly onReachability?: (r: Reachability) => void;
}

export interface CallOptions {
  readonly signal?: AbortSignal;
  readonly timeoutMs?: number;
}

export interface RpcClient {
  call(method: string, params: unknown, opts?: CallOptions): Promise<Result<unknown, DataError>>;
}

export function createRpcClient(opts: RpcClientOptions): RpcClient {
  const baseUrl = (opts.baseUrl ?? DEFAULT_DAEMON_URL).replace(/\/+$/, "");
  const doFetch: FetchLike = opts.fetch ?? ((input, init) => fetch(input, init));
  const report = opts.onReachability ?? (() => undefined);

  return {
    async call(method, params, callOpts = {}) {
      const timeoutMs = callOpts.timeoutMs ?? opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;
      const ctrl = new AbortController();
      let timedOut = false;
      const timer = setTimeout(() => {
        timedOut = true;
        ctrl.abort();
      }, timeoutMs);
      const onAbort = () => ctrl.abort();
      callOpts.signal?.addEventListener("abort", onAbort);

      try {
        let text: string;
        let status: number;
        try {
          const res = await doFetch(`${baseUrl}/rpc`, {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ method, params: params ?? {} }),
            signal: ctrl.signal,
          });
          status = res.status;
          text = await res.text();
        } catch (e) {
          if (timedOut) {
            return err(dataError("unavailable", `${method} timed out after ${timeoutMs} ms`, { reason: "timeout", method }));
          }
          if (callOpts.signal?.aborted) {
            return err(dataError("unavailable", `${method} was cancelled`, { reason: "aborted", method }));
          }
          report("offline");
          const why = e instanceof Error ? e.message : String(e);
          return err(dataError("unavailable", `Cannot reach the daemon at ${baseUrl} (${why})`, { reason: "offline", method }));
        }

        report("online");
        if (text.length === 0) {
          return err(dataError("internal", `${method}: empty reply (HTTP ${status})`, { reason: "http", status, method }));
        }
        const door = await opts.door;
        return door.parseRpcResult(method, text);
      } finally {
        clearTimeout(timer);
        callOpts.signal?.removeEventListener("abort", onAbort);
      }
    },
  };
}
