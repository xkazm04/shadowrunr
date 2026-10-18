// The one door. Every byte the daemon sends enters the app here: `parseRpcResult` for a
// `POST /rpc` reply body, `parseEvent` for a `/events` frame. These are the only places that
// call `JSON.parse` or run a validator, and they never throw: they return a typed `Result`.

import { RESULT_VALIDATORS, type RpcMethod, type RpcResult } from "./generated/meta";
import type { ErrorBody, ErrorCode, ServerFrame, StreamEvent } from "./generated/types";
import { validateErrorEnvelope, validateServerFrame, type ValidateFn } from "./generated/validators.js";

export type Result<T, E> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly error: E };

/** One schema violation: where (`/result/0/state`) and what. */
export interface Issue {
  readonly path: string;
  readonly message: string;
}

export type DoorError =
  /** The text is not JSON. */
  | { readonly kind: "parse"; readonly message: string }
  /** JSON, but not what the contract says; `path` is the first violation. */
  | { readonly kind: "invalid"; readonly message: string; readonly path: string; readonly issues: readonly Issue[] }
  /** The method is not in the contract. */
  | { readonly kind: "unknown_method"; readonly message: string; readonly method: string }
  /** A well-formed error reply from the daemon. */
  | {
      readonly kind: "rpc";
      readonly message: string;
      readonly code: ErrorCode;
      readonly detail?: unknown;
    };

/** A decoded `/events` frame: an event (with its seq when enveloped) or an error frame. */
export type EventMessage =
  | { readonly kind: "event"; readonly event: StreamEvent; readonly seq?: number; readonly runId?: string }
  | { readonly kind: "error"; readonly error: ErrorBody };

const MAX_ISSUES = 10;

const ok = <T>(value: T): Result<T, never> => ({ ok: true, value });
const fail = <E>(error: E): Result<never, E> => ({ ok: false, error });

function decode(raw: unknown): Result<unknown, DoorError> {
  if (typeof raw !== "string") return ok(raw);
  try {
    return ok(JSON.parse(raw));
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    return fail({ kind: "parse", message: `not JSON: ${message}` });
  }
}

function invalid<V>(validate: ValidateFn<V>, prefix: string, what: string): DoorError {
  const issues: Issue[] = (validate.errors ?? []).slice(0, MAX_ISSUES).map((e) => ({
    path: `${prefix}${e.instancePath}`,
    message: e.message ?? e.keyword,
  }));
  const first = issues[0] ?? { path: prefix, message: "does not match the contract" };
  return {
    kind: "invalid",
    message: `${what}: ${first.path || "/"} ${first.message}`,
    path: first.path,
    issues,
  };
}

const isRecord = (v: unknown): v is Readonly<Record<string, unknown>> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

function isRpcMethod(method: string): method is RpcMethod {
  return Object.hasOwn(RESULT_VALIDATORS, method);
}

function rpcError(error: ErrorBody): DoorError {
  const base = { kind: "rpc", message: error.message, code: error.code } as const;
  return error.detail === undefined ? base : { ...base, detail: error.detail };
}

/**
 * Decode and validate a `POST /rpc` reply body (`{"result"}` or `{"error"}`), given as text
 * or as an already-decoded value.
 */
export function parseRpcResult<M extends string>(
  method: M,
  raw: unknown,
): Result<M extends RpcMethod ? RpcResult<M> : never, DoorError>;
export function parseRpcResult(method: string, raw: unknown): Result<unknown, DoorError> {
  if (!isRpcMethod(method)) {
    return fail({ kind: "unknown_method", message: `unknown RPC method ${JSON.stringify(method)}`, method });
  }
  const decoded = decode(raw);
  if (!decoded.ok) return decoded;
  const body = decoded.value;
  if (isRecord(body) && "error" in body) {
    return validateErrorEnvelope(body)
      ? fail(rpcError(body.error))
      : fail(invalid(validateErrorEnvelope, "", `${method} error reply`));
  }
  if (!isRecord(body) || !("result" in body)) {
    return fail({
      kind: "invalid",
      message: `${method} reply: expected {"result"} or {"error"}`,
      path: "",
      issues: [{ path: "", message: 'must have property "result" or "error"' }],
    });
  }
  const validate: ValidateFn<unknown> = RESULT_VALIDATORS[method];
  const result = body["result"];
  return validate(result) ? ok(result) : fail(invalid(validate, "/result", `${method} result`));
}

function eventMessage(frame: ServerFrame): EventMessage {
  if ("error" in frame) return { kind: "error", error: frame.error };
  if ("seq" in frame) return { kind: "event", event: frame.event, seq: frame.seq, runId: frame.run_id };
  return { kind: "event", event: frame };
}

/** Decode and validate one `/events` frame, given as text or as an already-decoded value. */
export function parseEvent(raw: unknown): Result<EventMessage, DoorError> {
  const decoded = decode(raw);
  if (!decoded.ok) return decoded;
  const frame = decoded.value;
  return validateServerFrame(frame)
    ? ok(eventMessage(frame))
    : fail(invalid(validateServerFrame, "", "event frame"));
}
