// Quote verification (A/1 core.js:241-249): a judge quote is checked against the transcript
// line it cites and the two lines either side. Found on the cited line: verified. Found within
// ±2 lines: nearby. Otherwise: not found. The daemon verifies quotes mechanically too; this is
// the operator's own check, on the transcript the page loaded.
import type { StreamEvent } from "@shadowrunr/contract";

export interface TurnRef {
  readonly runId: string;
  /** 1-based transcript line. */
  readonly line: number;
}

/** `<run_id>#<line>` → its parts; undefined when it is not a transcript ref. */
export function parseRef(ref: string): TurnRef | undefined {
  const m = /^(.*)#(\d+)$/.exec(ref);
  if (!m || !m[1] || !m[2]) return undefined;
  const line = Number(m[2]);
  return line >= 1 ? { runId: m[1], line } : undefined;
}

/** The words an event carries, as the judge would quote them (A/1 `eventText`). */
export function eventText(e: StreamEvent): string {
  switch (e.type) {
    case "chat":
      return e.msg.text;
    case "say":
      return e.text;
    case "picked":
      return e.reason;
    case "op":
      return opText(e.op);
    case "vote":
      return `${e.poll_id} · ${e.tallies.map((t) => `${t.option} ${t.voters}/${t.weight}`).join(" · ")}${e.closed ? " · closed" : ""}`;
    case "moderation":
      return `${e.stage} · ${e.rule} · ${e.action}`;
    case "handover":
      return `${e.from} → ${e.to} (${e.reason})`;
    case "job":
      return `${e.job_id} ${e.status}${e.summary ? `: ${e.summary}` : ""}`;
    case "segment_start":
    case "segment_end":
      return e.segment;
    case "stream_start":
      return e.stream_id;
    case "stream_end":
      return e.reason;
    case "case_open":
      return e.case.title || e.case.id;
  }
}

function opText(o: Extract<StreamEvent, { type: "op" }>["op"]): string {
  switch (o.op) {
    case "mark_moment":
      return `${o.op}: ${o.label}`;
    case "start_poll":
      return `${o.op}: ${o.question}`;
    case "summon_foil":
      return `${o.op}: ${o.foil}`;
    case "show_card":
      return `${o.op}: ${o.title}`;
    case "reveal_hex":
      return `${o.op}: ${o.hex}`;
    case "dispatch_research":
      return `${o.op}: ${o.brief}`;
    case "switch_segment":
      return `${o.op}: ${o.to}`;
    default:
      return o.op;
  }
}

/** Case, curly quotes and whitespace folded, as A/1 does before matching. */
export function normalise(s: string): string {
  return s
    .toLowerCase()
    .replace(/[‘’]/g, "'")
    .replace(/[“”]/g, '"')
    .replace(/\s+/g, " ");
}

export type QuoteCheck =
  | { readonly state: "verified"; readonly line: number }
  | { readonly state: "nearby"; readonly line: number; readonly offset: number }
  | { readonly state: "not_found" };

/** Checks `quote` against `transcript` at `ref.line` ±2 (nearest line first). */
export function verifyQuote(transcript: readonly StreamEvent[], line: number, quote: string): QuoteCheck {
  const q = normalise(quote)
    .replace(/^\.\.\.|\.\.\.$/g, "")
    .trim();
  if (q.length === 0) return { state: "not_found" };
  for (let d = 0; d <= 2; d++) {
    for (const s of d === 0 ? [0] : [-1, 1]) {
      const at = line + d * s;
      const ev = transcript[at - 1];
      if (ev && normalise(eventText(ev)).includes(q)) {
        return d === 0 ? { state: "verified", line: at } : { state: "nearby", line: at, offset: d * s };
      }
    }
  }
  return { state: "not_found" };
}
