// Pure helpers for the rundown: a run's beats (A/1's floor: key ops, handovers, segment starts
// and the end), its counts, and the labels the tables print.
import type { HandoverInfo, RunRow, StreamEvent } from "@shadowrunr/contract";

const KEY_OPS = new Set(["start_poll", "close_poll", "summon_foil", "dismiss_foil", "reveal_hex", "dispatch_research", "show_card", "switch_segment", "sign_off"]);

export interface Beat {
  /** 1-based transcript line, for the deep link into Brain › Transcripts. */
  readonly line: number;
  readonly ts: number;
  readonly kind: string;
  readonly detail: string;
  readonly op: boolean;
}

function opDetail(e: Extract<StreamEvent, { type: "op" }>): string {
  const o = e.op;
  switch (o.op) {
    case "start_poll":
      return `${o.question} (${o.options.join(" / ")})`;
    case "close_poll":
      return o.poll_id;
    case "summon_foil":
      return o.foil;
    case "dismiss_foil":
      return "";
    case "reveal_hex":
      return `${o.hex} · ${o.grade}`;
    case "dispatch_research":
      return o.brief;
    case "show_card":
      return `${o.title}: ${o.body}`;
    case "switch_segment":
      return `to ${o.to}`;
    case "sign_off":
      return o.reason;
    case "highlight":
      return o.msg_id;
    case "mark_moment":
      return o.label;
  }
}

/** The beats of a run, deduplicated by time and text (A/1 dropped repeated beats). */
export function beatsOf(events: readonly StreamEvent[]): Beat[] {
  const seen = new Set<string>();
  const out: Beat[] = [];
  events.forEach((e, i) => {
    let kind: string;
    let detail: string;
    let op = false;
    if (e.type === "op" && KEY_OPS.has(e.op.op)) {
      kind = e.op.op.replace(/_/g, " ");
      detail = opDetail(e);
      op = true;
    } else if (e.type === "handover") {
      kind = "handover";
      detail = `${e.from} → ${e.to} · ${e.reason}`;
    } else if (e.type === "segment_start") {
      kind = "segment start";
      detail = e.segment;
    } else if (e.type === "stream_end") {
      kind = "stream end";
      detail = e.reason.replace(/_/g, " ");
    } else return;
    const sig = `${e.ts_ms}|${kind}|${detail}`;
    if (seen.has(sig)) return;
    seen.add(sig);
    out.push({ line: i + 1, ts: e.ts_ms, kind, detail: detail.slice(0, 220), op });
  });
  return out;
}

export interface RunCounts {
  readonly events: number;
  readonly spoken: number;
  readonly chat: number;
  readonly picked: number;
  readonly start: number;
  readonly end: number;
}

export function countsOf(events: readonly StreamEvent[]): RunCounts {
  let spoken = 0;
  let chat = 0;
  let picked = 0;
  let start = Number.POSITIVE_INFINITY;
  let end = Number.NEGATIVE_INFINITY;
  for (const e of events) {
    if (e.type === "say" && e.speaker === "malakor") spoken++;
    else if (e.type === "chat") chat++;
    else if (e.type === "picked") picked += e.msg_ids.length;
    start = Math.min(start, e.ts_ms);
    end = Math.max(end, e.ts_ms);
  }
  if (events.length === 0) start = end = 0;
  return { events: events.length, spoken, chat, picked, start, end };
}

/** `m:ss` (or `h:mm:ss`) for a span in ms. */
export function clock(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

export function runLabel(r: RunRow): string {
  return `${r.spec.tone_variant.replace(/-/g, " ")} · ${r.spec.core_tier} core · ${r.spec.audience_size} viewers`;
}

export function handoverText(h: HandoverInfo): string {
  return `ccv ≥ ${h.ccv_min} held ${h.sustain_s} s, or ${Math.round(h.max_elapsed_s / 60)} min elapsed; ${h.grace_s} s grace`;
}
