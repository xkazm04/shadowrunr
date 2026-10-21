// @shadowrunr/contract: the wire contract generated from crates/shadowrunr-types (JSON Schema
// -> TS types + compiled ajv validators), and the one door every daemon payload passes.
// Regenerate with `pnpm -C packages/contract generate`; `check` fails on drift.

export type * from "./generated/types";
export { ERROR_CODES, RPC_METHODS } from "./generated/meta";
export type { RpcMethod, RpcParams, RpcResult } from "./generated/meta";
export { parseEvent, parseRpcResult, preloadResultValidators } from "./door";
export type { DoorError, EventMessage, Issue, Result } from "./door";
