// `/voice` frames (contract appendix (voice)): a typed guard over what the daemon pushes, and the
// PCM and mouth-shape helpers the player reads them with. `@shadowrunr/contract` exports the
// `VoiceFrame` type but no compiled validator for it, so this guard checks each field the stage
// reads; a frame that fails is reported and dropped, never half-played.

import type { Viseme, VisemeSpan, VoiceFrame } from "@shadowrunr/contract";

import { VISEMES } from "../avatar/types";

export type Parsed<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly message: string };

export type Utterance = Extract<VoiceFrame, { type: "utterance" }>;

const STATES: ReadonlySet<string> = new Set(["off", "starting", "ready", "degraded"]);
const SKIPS: ReadonlySet<string> = new Set(["late", "queue_full", "error"]);
const KNOWN: ReadonlySet<string> = new Set<string>(VISEMES);

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isUint = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v) && v >= 0;
const optStr = (v: unknown) => v === undefined || typeof v === "string";

function spans(v: unknown): VisemeSpan[] | null {
  if (v === undefined) return [];
  if (!Array.isArray(v)) return null;
  const out: VisemeSpan[] = [];
  for (const s of v) {
    if (!isObj(s) || typeof s.v !== "string" || !KNOWN.has(s.v) || !isUint(s.start_ms) || !isUint(s.end_ms)) return null;
    out.push({ v: s.v as Viseme, start_ms: s.start_ms, end_ms: s.end_ms });
  }
  return out;
}

/** Decode and check one `/voice` text frame. */
export function parseVoiceFrame(text: string): Parsed<VoiceFrame> {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return { ok: false, message: "voice frame is not JSON" };
  }
  if (!isObj(raw)) return { ok: false, message: "voice frame is not an object" };
  const bad = (field: string): Parsed<VoiceFrame> => ({ ok: false, message: `voice ${String(raw.type)} frame: bad ${field}` });
  switch (raw.type) {
    case "status": {
      if (typeof raw.state !== "string" || !STATES.has(raw.state)) return bad("state");
      if (typeof raw.visemes !== "boolean") return bad("visemes");
      if (!optStr(raw.reason)) return bad("reason");
      if (!optStr(raw.voice)) return bad("voice");
      const f: Extract<VoiceFrame, { type: "status" }> = { type: "status", state: raw.state as "off", visemes: raw.visemes };
      if (typeof raw.reason === "string") f.reason = raw.reason;
      if (typeof raw.voice === "string") f.voice = raw.voice;
      return { ok: true, value: f };
    }
    case "utterance": {
      for (const k of ["id", "run_id", "text", "pcm"] as const) if (typeof raw[k] !== "string") return bad(k);
      for (const k of ["seq", "part", "duration_ms"] as const) if (!isUint(raw[k])) return bad(k);
      if (!isUint(raw.sample_rate) || raw.sample_rate < 8000 || raw.sample_rate > 96000) return bad("sample_rate");
      if (typeof raw.last !== "boolean") return bad("last");
      const v = spans(raw.visemes);
      if (!v) return bad("visemes");
      const f: Utterance = {
        type: "utterance",
        id: raw.id as string,
        run_id: raw.run_id as string,
        seq: raw.seq as number,
        part: raw.part as number,
        last: raw.last,
        text: raw.text as string,
        sample_rate: raw.sample_rate,
        duration_ms: raw.duration_ms as number,
        pcm: raw.pcm as string,
      };
      if (v.length) f.visemes = v;
      return { ok: true, value: f };
    }
    case "skipped": {
      if (typeof raw.run_id !== "string") return bad("run_id");
      if (!isUint(raw.seq) || !isUint(raw.part)) return bad("seq");
      if (typeof raw.reason !== "string" || !SKIPS.has(raw.reason)) return bad("reason");
      return { ok: true, value: { type: "skipped", run_id: raw.run_id, seq: raw.seq, part: raw.part, reason: raw.reason as "late" } };
    }
    default:
      return { ok: false, message: `unknown voice frame type ${String(raw.type)}` };
  }
}

/** Mono PCM16 little-endian in base64 → samples in -1..1. A trailing odd byte is ignored. */
export function decodePcm16(b64: string): Float32Array {
  const bin = atob(b64);
  const n = bin.length >> 1;
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const lo = bin.charCodeAt(2 * i);
    const hi = bin.charCodeAt(2 * i + 1);
    let v = (hi << 8) | lo;
    if (v >= 0x8000) v -= 0x10000;
    out[i] = v / 0x8000;
  }
  return out;
}

/**
 * The mouth shape at `ms` into an utterance: the span covering it, `sil` in a gap or past the
 * ends, and `null` when the utterance has no shapes at all (the mouth follows the level then).
 */
export function visemeAt(list: readonly VisemeSpan[] | undefined, ms: number): Viseme | null {
  if (!list || list.length === 0) return null;
  let lo = 0;
  let hi = list.length - 1;
  if (ms < list[0]!.start_ms) return "sil";
  // the last span that started by `ms`
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (list[mid]!.start_ms <= ms) lo = mid;
    else hi = mid - 1;
  }
  const s = list[lo]!;
  return ms < s.end_ms ? s.v : "sil";
}
