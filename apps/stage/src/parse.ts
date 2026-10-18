// Transcript parser: StreamEvent JSONL (contract § transcript) → typed events.
//
// Strict about required fields and enum values (a malformed line is skipped and reported, never
// half-rendered), lenient about what the contract allows: optional fields may be absent, and
// `#[serde(default)]` fields (viewer.months, gifted, is_mod) default to 0/false.

import {
  END_REASONS,
  EVIDENCE_GRADES,
  HEX_KINDS,
  MODEL_TIERS,
  PLATFORMS,
  RANKS,
  SEGMENT_IDS,
  type Case,
  type ChatMsg,
  type Command,
  type Hex,
  type Op,
  type StreamEvent,
  type Viewer,
  type VoteTally,
} from "./contract";

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

class FieldError extends Error {}

type Obj = Record<string, unknown>;

function isObj(v: unknown): v is Obj {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function fail(path: string, want: string, got: unknown): never {
  const shown = got === undefined ? "missing" : JSON.stringify(got);
  throw new FieldError(`${path}: expected ${want}, got ${shown?.slice(0, 60) ?? String(got)}`);
}

function obj(v: unknown, path: string): Obj {
  if (!isObj(v)) fail(path, "object", v);
  return v;
}

function str(o: Obj, k: string, path: string): string {
  const v = o[k];
  if (typeof v !== "string") fail(`${path}.${k}`, "string", v);
  return v;
}

function num(o: Obj, k: string, path: string): number {
  const v = o[k];
  if (typeof v !== "number" || !Number.isFinite(v)) fail(`${path}.${k}`, "number", v);
  return v;
}

function int(o: Obj, k: string, path: string): number {
  const v = num(o, k, path);
  if (!Number.isInteger(v)) fail(`${path}.${k}`, "integer", v);
  return v;
}

function uint(o: Obj, k: string, path: string): number {
  const v = int(o, k, path);
  if (v < 0) fail(`${path}.${k}`, "non-negative integer", v);
  return v;
}

function bool(o: Obj, k: string, path: string): boolean {
  const v = o[k];
  if (typeof v !== "boolean") fail(`${path}.${k}`, "boolean", v);
  return v;
}

function oneOf<T extends string>(o: Obj, k: string, allowed: readonly T[], path: string): T {
  const v = o[k];
  if (typeof v !== "string" || !(allowed as readonly string[]).includes(v)) {
    fail(`${path}.${k}`, `one of ${allowed.join("|")}`, v);
  }
  return v as T;
}

function strArray(o: Obj, k: string, path: string): string[] {
  const v = o[k];
  if (!Array.isArray(v) || v.some((x) => typeof x !== "string")) {
    fail(`${path}.${k}`, "array of strings", v);
  }
  return v as string[];
}

/** Optional fields: absent is fine; `null` is tolerated but reported (the contract omits). */
function present(o: Obj, k: string, path: string, warn: (m: string) => void): boolean {
  if (!(k in o)) return false;
  if (o[k] === null) {
    warn(`${path}.${k} is null; the contract omits absent fields`);
    return false;
  }
  return true;
}

function parseViewer(v: unknown, path: string, warn: (m: string) => void): Viewer {
  const o = obj(v, path);
  const viewer: Viewer = {
    id: str(o, "id", path),
    name: str(o, "name", path),
    platform: oneOf(o, "platform", PLATFORMS, path),
    rank: oneOf(o, "rank", RANKS, path),
    months: present(o, "months", path, warn) ? uint(o, "months", path) : 0,
    gifted: present(o, "gifted", path, warn) ? uint(o, "gifted", path) : 0,
    is_mod: present(o, "is_mod", path, warn) ? bool(o, "is_mod", path) : false,
  };
  if (present(o, "sub_tier", path, warn)) {
    const t = uint(o, "sub_tier", path);
    if (t < 1 || t > 3) fail(`${path}.sub_tier`, "1, 2 or 3", t);
    viewer.sub_tier = t;
  }
  return viewer;
}

function parseCommand(v: unknown, path: string): Command {
  const o = obj(v, path);
  const cmd = oneOf(o, "cmd", ["summon", "hint", "grade", "propose", "vote"] as const, path);
  switch (cmd) {
    case "summon":
      return { cmd, foil: str(o, "foil", path) };
    case "hint":
      return { cmd, hex: str(o, "hex", path), text: str(o, "text", path) };
    case "grade":
      return { cmd, hex: str(o, "hex", path), grade: oneOf(o, "grade", EVIDENCE_GRADES, path) };
    case "propose":
      return { cmd, text: str(o, "text", path) };
    case "vote":
      return { cmd, option: str(o, "option", path) };
  }
}

function parseChatMsg(v: unknown, path: string, warn: (m: string) => void): ChatMsg {
  const o = obj(v, path);
  const msg: ChatMsg = {
    id: str(o, "id", path),
    ts_ms: int(o, "ts_ms", path),
    viewer: parseViewer(o.viewer, `${path}.viewer`, warn),
    text: str(o, "text", path),
  };
  if (present(o, "command", path, warn)) msg.command = parseCommand(o.command, `${path}.command`);
  return msg;
}

export function parseOp(v: unknown, path = "op"): Op {
  const o = obj(v, path);
  const op = o.op;
  switch (op) {
    case "highlight":
      return { op, msg_id: str(o, "msg_id", path) };
    case "start_poll": {
      const options = strArray(o, "options", path);
      if (options.length === 0) fail(`${path}.options`, "at least one option", options);
      return {
        op,
        poll_id: str(o, "poll_id", path),
        question: str(o, "question", path),
        options,
        min_rank: oneOf(o, "min_rank", RANKS, path),
        secs: uint(o, "secs", path),
      };
    }
    case "close_poll":
      return { op, poll_id: str(o, "poll_id", path) };
    case "summon_foil":
      return { op, foil: str(o, "foil", path) };
    case "dismiss_foil":
      return { op };
    case "reveal_hex":
      return { op, hex: str(o, "hex", path), grade: oneOf(o, "grade", EVIDENCE_GRADES, path) };
    case "dispatch_research":
      return { op, job_id: str(o, "job_id", path), brief: str(o, "brief", path) };
    case "show_card":
      return {
        op,
        card_id: str(o, "card_id", path),
        title: str(o, "title", path),
        body: str(o, "body", path),
      };
    case "mark_moment":
      return { op, label: str(o, "label", path) };
    case "switch_segment":
      return { op, to: oneOf(o, "to", SEGMENT_IDS, path) };
    case "sign_off":
      return { op, reason: str(o, "reason", path) };
    default:
      return fail(`${path}.op`, "a known op", op);
  }
}

function parseTally(v: unknown, path: string): VoteTally {
  const o = obj(v, path);
  const weight = num(o, "weight", path);
  if (weight < 0) fail(`${path}.weight`, "non-negative number", weight);
  return { option: str(o, "option", path), voters: uint(o, "voters", path), weight };
}

/** Parse one decoded JSON value as a StreamEvent. Throws `Error` with a field path on mismatch. */
export function parseEvent(v: unknown, warn: (m: string) => void = () => {}): StreamEvent {
  const o = obj(v, "event");
  const type = o.type;
  const p = String(type);
  switch (type) {
    case "stream_start":
      return {
        type,
        ts_ms: int(o, "ts_ms", p),
        stream_id: str(o, "stream_id", p),
        character: str(o, "character", p),
        tone_variant: str(o, "tone_variant", p),
        core_tier: oneOf(o, "core_tier", MODEL_TIERS, p),
        audience_size: uint(o, "audience_size", p),
      };
    case "segment_start":
      return { type, ts_ms: int(o, "ts_ms", p), segment: oneOf(o, "segment", SEGMENT_IDS, p) };
    case "segment_end": {
      const ev: StreamEvent = {
        type,
        ts_ms: int(o, "ts_ms", p),
        segment: oneOf(o, "segment", SEGMENT_IDS, p),
      };
      if (present(o, "ses", p, warn)) ev.ses = num(o, "ses", p);
      return ev;
    }
    case "chat":
      return { type, ts_ms: int(o, "ts_ms", p), msg: parseChatMsg(o.msg, `${p}.msg`, warn) };
    case "picked":
      return {
        type,
        ts_ms: int(o, "ts_ms", p),
        msg_ids: strArray(o, "msg_ids", p),
        reason: str(o, "reason", p),
      };
    case "say": {
      const ev: Extract<StreamEvent, { type: "say" }> = {
        type,
        ts_ms: int(o, "ts_ms", p),
        speaker: oneOf(o, "speaker", ["malakor", "foil"] as const, p),
        text: str(o, "text", p),
      };
      if (present(o, "foil", p, warn)) ev.foil = str(o, "foil", p);
      if (present(o, "replying_to", p, warn)) ev.replying_to = strArray(o, "replying_to", p);
      if (present(o, "first_text_ms", p, warn)) ev.first_text_ms = uint(o, "first_text_ms", p);
      if (ev.speaker === "foil" && ev.foil === undefined) warn("say: speaker=foil without a foil id");
      return ev;
    }
    case "op":
      return { type, ts_ms: int(o, "ts_ms", p), op: parseOp(o.op, `${p}.op`) };
    case "vote": {
      const t = o.tallies;
      if (!Array.isArray(t)) fail(`${p}.tallies`, "array", t);
      return {
        type,
        ts_ms: int(o, "ts_ms", p),
        poll_id: str(o, "poll_id", p),
        tallies: t.map((x, i) => parseTally(x, `${p}.tallies[${i}]`)),
        closed: bool(o, "closed", p),
      };
    }
    case "moderation": {
      const ev: Extract<StreamEvent, { type: "moderation" }> = {
        type,
        ts_ms: int(o, "ts_ms", p),
        stage: oneOf(o, "stage", ["input", "output"] as const, p),
        rule: str(o, "rule", p),
        action: str(o, "action", p),
      };
      if (present(o, "msg_id", p, warn)) ev.msg_id = str(o, "msg_id", p);
      return ev;
    }
    case "handover":
      return {
        type,
        ts_ms: int(o, "ts_ms", p),
        from: oneOf(o, "from", SEGMENT_IDS, p),
        to: oneOf(o, "to", SEGMENT_IDS, p),
        reason: str(o, "reason", p),
      };
    case "job": {
      const ev: Extract<StreamEvent, { type: "job" }> = {
        type,
        ts_ms: int(o, "ts_ms", p),
        job_id: str(o, "job_id", p),
        status: str(o, "status", p),
      };
      if (present(o, "summary", p, warn)) ev.summary = str(o, "summary", p);
      return ev;
    }
    case "stream_end":
      return { type, ts_ms: int(o, "ts_ms", p), reason: oneOf(o, "reason", END_REASONS, p) };
    default:
      return fail("event.type", "a known StreamEvent type", type);
  }
}

/** Parse a whole JSONL transcript. Blank lines are ignored; bad lines are reported and skipped. */
export function parseTranscript(text: string): ParseResult {
  const events: StreamEvent[] = [];
  const errors: ParseIssue[] = [];
  const warnings: ParseIssue[] = [];
  const lines = text.split(/\r?\n/);
  let lastTs = Number.NEGATIVE_INFINITY;
  lines.forEach((raw, i) => {
    const line = i + 1;
    const trimmed = raw.trim();
    if (trimmed === "") return;
    let json: unknown;
    try {
      json = JSON.parse(trimmed);
    } catch (e) {
      errors.push({ line, message: `invalid JSON: ${(e as Error).message}` });
      return;
    }
    try {
      const ev = parseEvent(json, (message) => warnings.push({ line, message }));
      if (ev.ts_ms < lastTs) {
        warnings.push({ line, message: `ts_ms goes backwards (${ev.ts_ms} < ${lastTs})` });
      }
      lastTs = Math.max(lastTs, ev.ts_ms);
      events.push(ev);
    } catch (e) {
      errors.push({ line, message: (e as Error).message });
    }
  });
  return { events, errors, warnings };
}

function parseHex(v: unknown, path: string, warn: (m: string) => void): Hex {
  const o = obj(v, path);
  const hex: Hex = {
    id: str(o, "id", path),
    q: int(o, "q", path),
    r: int(o, "r", path),
    kind: oneOf(o, "kind", HEX_KINDS, path),
    label: str(o, "label", path),
    fog: bool(o, "fog", path),
  };
  if (present(o, "grade", path, warn)) hex.grade = oneOf(o, "grade", EVIDENCE_GRADES, path);
  if (present(o, "sources", path, warn)) hex.sources = strArray(o, "sources", path);
  return hex;
}

/** Parse a SEG-02A `Case` (the transcript does not carry one; see DESIGN.md § case source). */
export function parseCase(v: unknown, warn: (m: string) => void = () => {}): Case {
  const o = obj(v, "case");
  const hexes = o.hexes;
  if (!Array.isArray(hexes)) fail("case.hexes", "array", hexes);
  const c: Case = {
    id: str(o, "id", "case"),
    title: str(o, "title", "case"),
    fictional: bool(o, "fictional", "case"),
    hexes: hexes.map((h, i) => parseHex(h, `case.hexes[${i}]`, warn)),
  };
  if (present(o, "links", "case", warn)) {
    const links = o.links;
    if (
      !Array.isArray(links) ||
      links.some(
        (l) =>
          !Array.isArray(l) || l.length !== 2 || typeof l[0] !== "string" || typeof l[1] !== "string",
      )
    ) {
      fail("case.links", "array of [string, string]", links);
    }
    c.links = links as [string, string][];
  }
  return c;
}
