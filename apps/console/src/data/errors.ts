// The one error taxonomy (F2 mirrors it from shadowrunr-types) and how each code shows in the UI.
// Transport failures are folded into the same codes: an unreachable daemon or a timeout is
// `unavailable`, a payload the door rejects is `internal`. `detail.reason` says which.

export const ERROR_CODES = [
  "invalid_params",
  "not_found",
  "method_not_found",
  "preflight_red",
  "busy",
  "engine",
  "io",
  "unavailable",
  "conflict",
  "internal",
] as const;
export type ErrorCode = (typeof ERROR_CODES)[number];

/** Reasons the console itself assigns; daemon details may carry their own (`panic`, ...). */
export type TransportReason = "offline" | "timeout" | "aborted" | "http" | "invalid_payload" | "gap";

export interface ErrorDetail {
  readonly reason?: TransportReason | (string & {});
  /** JSON path of the first invalid field when the door rejects a payload. */
  readonly path?: string;
  readonly status?: number;
  readonly method?: string;
  readonly [key: string]: unknown;
}

/** The body of the one envelope: `{error: {code, message, detail?}}`. */
export interface DataError {
  readonly code: ErrorCode;
  readonly message: string;
  readonly detail?: ErrorDetail;
}

export function isErrorCode(s: unknown): s is ErrorCode {
  return typeof s === "string" && (ERROR_CODES as readonly string[]).includes(s);
}

export function dataError(code: ErrorCode, message: string, detail?: ErrorDetail): DataError {
  return detail === undefined ? { code, message } : { code, message, detail };
}

/** The page states every surface can be in besides "ready". */
export type UiState = "offline" | "degraded" | "empty" | "blocked" | "invalid";

export const UI_STATE: Readonly<Record<ErrorCode, UiState>> = {
  unavailable: "offline",
  busy: "degraded",
  engine: "degraded",
  io: "degraded",
  internal: "degraded",
  not_found: "empty",
  preflight_red: "blocked",
  invalid_params: "invalid",
  method_not_found: "invalid",
  conflict: "invalid",
};

const TITLES: Readonly<Record<ErrorCode, string>> = {
  unavailable: "The daemon is not reachable",
  busy: "The daemon is busy",
  engine: "The engine failed",
  io: "The daemon could not read or write a file",
  internal: "Something went wrong inside the console or daemon",
  not_found: "Nothing here yet",
  preflight_red: "Pre-flight is red",
  invalid_params: "The request was rejected",
  method_not_found: "The daemon does not know this request",
  conflict: "Someone changed this first",
};

const HINTS: Readonly<Record<ErrorCode, string>> = {
  unavailable: "Start it with `shadowrunr daemon --port 7717`; the console reconnects on its own.",
  busy: "Try again in a moment.",
  engine: "The run is kept; open the ledger for the failing call.",
  io: "Check the data directory and its permissions.",
  internal: "Retry; if it repeats, the payload did not match the contract.",
  not_found: "It may not have been created yet.",
  preflight_red: "Open pre-flight and clear the red checks first.",
  invalid_params: "The console and daemon may be on different contract versions.",
  method_not_found: "Update the daemon to the console's contract version.",
  conflict: "Reload to see the latest state.",
};

export interface ErrorView {
  readonly state: UiState;
  readonly title: string;
  readonly hint: string;
  readonly code: ErrorCode;
  readonly message: string;
}

export function viewOfError(e: DataError): ErrorView {
  const timedOut = e.code === "unavailable" && e.detail?.reason === "timeout";
  return {
    state: timedOut ? "degraded" : UI_STATE[e.code],
    title: timedOut ? "The daemon did not answer in time" : TITLES[e.code],
    hint: HINTS[e.code],
    code: e.code,
    message: e.message,
  };
}
