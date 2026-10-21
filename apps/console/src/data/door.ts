// The console's view of `@shadowrunr/contract`'s one door. The data layer never parses JSON or
// validates a payload itself: it hands raw text to a `Door` and gets a typed `Result` back.
// The implementation (`contractDoor.ts`) is loaded lazily, in parallel with the first paint: it
// carries only the eager ServerFrame + ErrorEnvelope validators. Each method's result validator
// is a further chunk the contract loads on that method's first reply.
import type { StreamEvent } from "@shadowrunr/contract";
import type { DataError } from "./errors";
import type { Result } from "./result";

/** One pushed event. `seq` and `runId` are present in enveloped mode (after a `since`). */
export interface EventEnvelope {
  readonly seq?: number;
  readonly runId?: string;
  readonly event: StreamEvent;
}

export interface Door {
  /** Parses and validates a `/rpc` reply body: the method's result, or the error envelope.
   *  May resolve later (the method's validator loads on first use); never rejects. */
  parseRpcResult(method: string, raw: string): Result<unknown, DataError> | Promise<Result<unknown, DataError>>;
  /** Parses and validates one `/events` frame. A server error frame comes back as an error. */
  parseEvent(raw: string): Result<EventEnvelope, DataError>;
}

/** A door now, or one still loading. */
export type DoorSource = Door | Promise<Door>;

export function isDoorPromise(d: DoorSource): d is Promise<Door> {
  return d instanceof Promise;
}
