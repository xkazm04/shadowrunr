// Transcript parser: StreamEvent JSONL (contract § transcript) → typed events, through the
// contract's one door (`@shadowrunr/contract` parseEvent). The door does the JSON decoding and
// the schema validation, so a malformed line is skipped and reported, never half-rendered.
// This entry (`@shadowrunr/stage-kit/parse`) carries the compiled validators; import it only
// where raw text enters (the OBS stage), not where replies were already validated.

import { parseEvent, type Case, type DoorError, type StreamEvent } from "@shadowrunr/contract";

export interface ParseIssue {
  /** 1-based line number in the JSONL text (0 when not line-based). */
  line: number;
  message: string;
}

export interface ParseResult {
  events: StreamEvent[];
  errors: ParseIssue[];
  warnings: ParseIssue[];
}

export type Parsed<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly message: string };

const depth = (path: string) => path.split("/").filter(Boolean).length;

/** The most specific issue of a door error (the StreamEvent union reports every branch). */
function deepest(e: DoorError): { path: string; message: string; depth: number } {
  if (e.kind !== "invalid") return { path: "", message: e.message, depth: -1 };
  let best = { path: e.path, message: e.message, depth: -1 };
  for (const i of e.issues) {
    const d = depth(i.path);
    if (d > best.depth) best = { path: i.path, message: `${i.path || "/"} ${i.message}`, depth: d };
  }
  return best;
}

function unwrap(raw: unknown): Parsed<StreamEvent> | DoorError {
  const r = parseEvent(raw);
  if (!r.ok) return r.error;
  const msg = r.value;
  if (msg.kind === "error") return { ok: false, message: `error frame ${msg.error.code}: ${msg.error.message}` };
  return { ok: true, value: msg.event };
}

const isParsed = (v: Parsed<StreamEvent> | DoorError): v is Parsed<StreamEvent> => "ok" in v;

/**
 * One event from raw text or an already-decoded value: a bare StreamEvent or an enveloped
 * `/events` frame (`{seq, run_id, event}`). An error frame is a failure carrying its message.
 * On a schema failure the frame is re-checked as an envelope's `event`, so the message names
 * the offending field (`/event/segment ...`) rather than the first branch of the frame union.
 */
export function parseStreamEvent(raw: unknown): Parsed<StreamEvent> {
  const first = unwrap(raw);
  if (isParsed(first)) return first;
  if (first.kind !== "invalid") return { ok: false, message: first.message };
  const wrapped = unwrap(typeof raw === "string" ? `{"seq":0,"run_id":"","event":${raw}}` : { seq: 0, run_id: "", event: raw });
  const a = deepest(first);
  const b = isParsed(wrapped) ? a : deepest(wrapped);
  const best = b.depth > a.depth ? b : a;
  return { ok: false, message: best.depth <= 1 ? `not a valid StreamEvent: ${best.message}` : best.message };
}

/** One `/events` frame: an event with its envelope position (when enveloped), or an error frame. */
export type EventsFrame =
  | { readonly kind: "event"; readonly event: StreamEvent; readonly seq?: number; readonly runId?: string }
  | { readonly kind: "error"; readonly code: string; readonly message: string; readonly detail?: unknown };

/**
 * One `/events` frame from raw text, keeping what `parseStreamEvent` drops: the envelope's `seq`
 * and `run_id`, and an error frame's code and detail (a `busy` frame's `resume_since`).
 */
export function parseFrame(raw: unknown): Parsed<EventsFrame> {
  const r = parseEvent(raw);
  if (!r.ok) {
    const p = parseStreamEvent(raw);
    return { ok: false, message: p.ok ? r.error.message : p.message };
  }
  const m = r.value;
  if (m.kind === "error") {
    const { code, message, detail } = m.error;
    return { ok: true, value: detail === undefined ? { kind: "error", code, message } : { kind: "error", code, message, detail } };
  }
  const value: { kind: "event"; event: StreamEvent; seq?: number; runId?: string } = { kind: "event", event: m.event };
  if (m.seq !== undefined) value.seq = m.seq;
  if (m.runId !== undefined) value.runId = m.runId;
  return { ok: true, value };
}

/** Parse a whole JSONL transcript. Blank lines are ignored; bad lines are reported and skipped. */
export function parseTranscript(text: string): ParseResult {
  const events: StreamEvent[] = [];
  const errors: ParseIssue[] = [];
  const warnings: ParseIssue[] = [];
  let lastTs = Number.NEGATIVE_INFINITY;
  text.split(/\r?\n/).forEach((raw, i) => {
    const line = i + 1;
    const trimmed = raw.trim();
    if (trimmed === "") return;
    const r = parseStreamEvent(trimmed);
    if (!r.ok) {
      errors.push({ line, message: r.message });
      return;
    }
    const ev = r.value;
    if (ev.ts_ms < lastTs) warnings.push({ line, message: `ts_ms goes backwards (${ev.ts_ms} < ${lastTs})` });
    lastTs = Math.max(lastTs, ev.ts_ms);
    events.push(ev);
  });
  return { events, errors, warnings };
}

/**
 * Validate a SEG-02A `Case` (a standalone case file: `?case=`, sidecar, dropped) by passing it
 * through the door as the payload of a `case_open` event, so it meets exactly the schema a
 * transcript's own case does. `value` is a decoded value or the case file's JSON text (a case is
 * never a string, so text is decoded by the door, not here).
 */
export function checkCase(value: unknown): Parsed<Case> {
  const r = parseStreamEvent(
    typeof value === "string" ? `{"type":"case_open","ts_ms":0,"case":${value}}` : { type: "case_open", ts_ms: 0, case: value },
  );
  if (!r.ok) return { ok: false, message: `case: ${r.message.replace(/^\/event\/case/, "")}` };
  if (r.value.type !== "case_open") return { ok: false, message: "case: not a case" };
  return { ok: true, value: r.value.case };
}

/** `checkCase` that throws on an invalid case (for built-in fixtures, which must be valid). */
export function parseCase(value: unknown): Case {
  const r = checkCase(value);
  if (!r.ok) throw new Error(r.message);
  return r.value;
}
