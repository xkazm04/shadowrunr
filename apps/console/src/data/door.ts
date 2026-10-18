// The console's view of `@shadowrunr/contract`'s one door. The data layer never parses JSON or
// validates a payload itself: it hands raw text to a `Door` and gets a typed `Result` back.
// The implementation (`contractDoor.ts`) is loaded lazily: its compiled validators are most of
// its weight, and the shell paints without them.
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
  /** Parses and validates a `/rpc` reply body: the method's result, or the error envelope. */
  parseRpcResult(method: string, raw: string): Result<unknown, DataError>;
  /** Parses and validates one `/events` frame. A server error frame comes back as an error. */
  parseEvent(raw: string): Result<EventEnvelope, DataError>;
}

/** A door now, or one still loading. */
export type DoorSource = Door | Promise<Door>;

export function isDoorPromise(d: DoorSource): d is Promise<Door> {
  return d instanceof Promise;
}
