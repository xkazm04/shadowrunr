// Clip mode's input: `clip.json`, written by the Rust clip factory (crates/shadowrunr-clips,
// `ClipInput` in render.rs) for one candidate cut. The stage replays it on a vertical
// 1080x1920 canvas. The captions are the exact transcript lines (viewer handles already
// redacted by the factory), timed on the clip's own clock; `events` are the scene events of
// the cut on that clock (the state before the cut replayed at 0), never chat.
//
// The parser is tolerant where a bad item can be dropped (one malformed caption or event) and
// strict where the clip cannot be drawn without it (size, duration, the caption list).

import type { Case, StreamEvent } from "@shadowrunr/contract";
import { foilInfo, SEGMENT_TITLE, visibleWords, wordTimings } from "@shadowrunr/stage-kit";
import { checkCase, parseStreamEvent } from "@shadowrunr/stage-kit/parse";

export const CLIP_W = 1080;
export const CLIP_H = 1920;

export type ClipGrade = "DOC" | "REP" | "CLM" | "SPC" | "UNGRADED";
const GRADES: readonly ClipGrade[] = ["DOC", "REP", "CLM", "SPC", "UNGRADED"];

export interface ClipHeader {
  alleged: boolean;
  grade: ClipGrade;
  case_title: string;
  fictional: boolean;
  citation: string[];
}

export interface ClipCaption {
  start_ms: number;
  end_ms: number;
  /** `malakor`, `viewer`, or a foil id. */
  speaker: string;
  text: string;
}

export interface ClipInput {
  version: number;
  id: string;
  width: number;
  height: number;
  duration_ms: number;
  style: string;
  segment: string;
  title: string;
  header: ClipHeader | null;
  captions: ClipCaption[];
  events: StreamEvent[];
  case: Case | null;
  disclosure: string;
  provenance: string;
}

export type ClipParse = { ok: true; clip: ClipInput; dropped: string[] } | { ok: false; message: string };

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const str = (v: unknown, d = ""): string => (typeof v === "string" ? v : d);
const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

function parseHeader(v: unknown): ClipHeader | null {
  if (!isObj(v)) return null;
  const grade = GRADES.includes(v.grade as ClipGrade) ? (v.grade as ClipGrade) : "UNGRADED";
  return {
    alleged: v.alleged === true,
    grade,
    case_title: str(v.case_title),
    fictional: v.fictional === true,
    citation: Array.isArray(v.citation) ? v.citation.filter((c): c is string => typeof c === "string").slice(0, 3) : [],
  };
}

/** Parse `clip.json` (already-decoded JSON or its text). */
export function parseClip(raw: unknown): ClipParse {
  let v: unknown = raw;
  if (typeof raw === "string") {
    try {
      v = JSON.parse(raw);
    } catch (e) {
      return { ok: false, message: `clip.json is not JSON: ${(e as Error).message}` };
    }
  }
  if (!isObj(v)) return { ok: false, message: "clip.json is not an object" };
  const width = num(v.width);
  const height = num(v.height);
  const duration = num(v.duration_ms);
  if (width !== CLIP_W || height !== CLIP_H) return { ok: false, message: `clip is ${width}x${height}, not ${CLIP_W}x${CLIP_H}` };
  if (duration === null || duration <= 0) return { ok: false, message: "clip has no positive duration_ms" };
  if (!Array.isArray(v.captions)) return { ok: false, message: "clip has no captions array" };
  const dropped: string[] = [];
  const captions: ClipCaption[] = [];
  v.captions.forEach((c, i) => {
    const start = isObj(c) ? num(c.start_ms) : null;
    const end = isObj(c) ? num(c.end_ms) : null;
    if (!isObj(c) || start === null || end === null || end < start || typeof c.text !== "string") {
      dropped.push(`caption ${i}`);
      return;
    }
    captions.push({ start_ms: start, end_ms: end, speaker: str(c.speaker, "malakor"), text: c.text });
  });
  captions.sort((a, b) => a.start_ms - b.start_ms);
  const events: StreamEvent[] = [];
  (Array.isArray(v.events) ? v.events : []).forEach((e, i) => {
    const r = parseStreamEvent(e);
    // Chat never reaches a clip; drop it even if a hand-made file carries some.
    if (r.ok && r.value.type !== "chat") events.push(r.value);
    else dropped.push(`event ${i}`);
  });
  events.sort((a, b) => a.ts_ms - b.ts_ms);
  let kase: Case | null = null;
  if (v.case !== undefined && v.case !== null) {
    const c = checkCase(v.case);
    if (c.ok) kase = c.value;
    else dropped.push("case");
  }
  return {
    ok: true,
    dropped,
    clip: {
      version: num(v.version) ?? 1,
      id: str(v.id, "clip"),
      width: CLIP_W,
      height: CLIP_H,
      duration_ms: duration,
      style: str(v.style, "default"),
      segment: str(v.segment),
      title: str(v.title),
      header: parseHeader(v.header),
      captions,
      events,
      case: kase,
      disclosure: str(v.disclosure, "AI character · synthetic media"),
      provenance: str(v.provenance),
    },
  };
}

/** `?clip=<url>&record=1` */
export function readClipParams(search: string): { clip: string | null; record: boolean } {
  const p = new URLSearchParams(search);
  const r = p.get("record");
  return { clip: p.get("clip"), record: r !== null && r !== "0" && r !== "false" };
}

/** The caption on screen at `t` (the latest one started), with how many words have landed. */
export function captionAt(captions: readonly ClipCaption[], t: number): { caption: ClipCaption; words: number; total: number } | null {
  let cur: ClipCaption | null = null;
  for (const c of captions) {
    if (c.start_ms <= t) cur = c;
    else break;
  }
  if (!cur || t > cur.end_ms) return null;
  const line = wordTimings(cur.text);
  // The factory sized the cue to the spoken line; never leave words unshown at its end.
  const words = t >= cur.end_ms ? line.words.length : visibleWords(line, t - cur.start_ms);
  return { caption: cur, words, total: line.words.length };
}

/** The label above a caption. */
export function speakerLabel(speaker: string): string {
  if (speaker === "malakor") return "Arch-Lich Malakor";
  if (speaker === "viewer") return "A minion asks";
  return foilInfo(speaker).name;
}

/** The header band's content: the ALLEGED block for claim-bearing clips, else the segment title. */
export function headerModel(clip: ClipInput, activeFoil: string | null):
  | { kind: "alleged"; grade: ClipGrade; caseTitle: string; fictional: boolean }
  | { kind: "segment"; title: string; foil: string | null } {
  if (clip.header?.alleged) {
    return { kind: "alleged", grade: clip.header.grade, caseTitle: clip.header.case_title, fictional: clip.header.fictional };
  }
  const title = (SEGMENT_TITLE as Record<string, string | undefined>)[clip.segment] ?? (clip.title || "Shadowrunr");
  return { kind: "segment", title, foil: activeFoil ? foilInfo(activeFoil).name : null };
}

/** The frame used for the poster: the midpoint of the longest Malakor caption, else the middle. */
export function posterMs(clip: ClipInput): number {
  let best: ClipCaption | null = null;
  for (const c of clip.captions) {
    if (c.speaker === "malakor" && (!best || c.text.length > best.text.length)) best = c;
  }
  const t = best ? (best.start_ms + best.end_ms) / 2 : clip.duration_ms / 2;
  return Math.max(0, Math.min(clip.duration_ms - 1, t));
}

/** Host part of a source URL plus its path, shortened for the citation strip. */
export function shortSource(url: string, max = 56): string {
  const s = url.replace(/^https?:\/\//, "");
  return s.length > max ? `${s.slice(0, max - 1)}…` : s;
}

/** Characters per caption page: five lines of the host's 60 px type in the 936 px band. */
export const PAGE_CHARS = 140;
/** A viewer's question is set smaller (48 px), so a page holds more. */
export const VIEWER_PAGE_CHARS = 190;

/**
 * Greedy pages of at most `max` characters over `words`; returns the `[from, to)` word range of
 * the page holding word `at`. A single word longer than `max` is a page of its own.
 */
export function captionPage(words: readonly string[], at: number, max: number): { from: number; to: number } {
  let from = 0;
  let len = 0;
  for (let i = 0; i < words.length; i++) {
    const add = (len === 0 ? 0 : 1) + words[i]!.length;
    if (len > 0 && len + add > max) {
      if (at < i) return { from, to: i };
      from = i;
      len = words[i]!.length;
    } else {
      len += add;
    }
  }
  return { from, to: words.length };
}
