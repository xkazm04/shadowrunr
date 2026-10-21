// The real door: `@shadowrunr/contract`'s parseRpcResult / parseEvent, with its `DoorError`
// translated into the console's one error taxonomy. Import it lazily (see app/compose.ts).
import { parseEvent, parseRpcResult, type DoorError } from "@shadowrunr/contract";
import type { Door, EventEnvelope } from "./door";
import { dataError, type DataError, type ErrorDetail } from "./errors";
import { err, ok } from "./result";

function isRecord(v: unknown): v is Readonly<Record<string, unknown>> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function detailOf(d: unknown): ErrorDetail | undefined {
  if (d === undefined) return undefined;
  return isRecord(d) ? { ...d } : { value: d };
}

export function fromDoorError(e: DoorError, method?: string): DataError {
  const m = method === undefined ? {} : { method };
  switch (e.kind) {
    case "rpc": {
      const detail = detailOf(e.detail);
      return dataError(e.code, e.message, detail === undefined && method === undefined ? undefined : { ...detail, ...m });
    }
    case "unknown_method":
      return dataError("method_not_found", e.message, { method: e.method });
    case "unavailable_validator":
      return dataError("unavailable", e.message, { reason: "validator_unavailable", method: e.method });
    case "parse":
      return dataError("internal", e.message, { reason: "invalid_payload", path: "", ...m });
    case "invalid":
      return dataError("internal", e.message, { reason: "invalid_payload", path: e.path, ...m });
  }
}

export const contractDoor: Door = {
  async parseRpcResult(method, raw) {
    const r = await parseRpcResult(method, raw);
    return r.ok ? ok(r.value) : err(fromDoorError(r.error, method));
  },
  parseEvent(raw) {
    const r = parseEvent(raw);
    if (!r.ok) return err(fromDoorError(r.error));
    const msg = r.value;
    if (msg.kind === "error") {
      const detail = detailOf(msg.error.detail);
      return err(dataError(msg.error.code, msg.error.message, { ...detail, frame: "events" }));
    }
    const env: EventEnvelope = {
      event: msg.event,
      ...(msg.seq === undefined ? {} : { seq: msg.seq }),
      ...(msg.runId === undefined ? {} : { runId: msg.runId }),
    };
    return ok(env);
  },
};
