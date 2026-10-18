// Transcript event text for the brain's viewer and memory curve. Ported from the pre-port
// console (`lib/format.ts` eventText/eventActor), which C2 deletes.
//
// Proposal for the Director: On Air (P1) and the simulator's quote verification (P5) need the
// same `eventText`; promote this file to `kit/` so there is one copy.
import type { StreamEvent } from "@shadowrunr/contract";

/** A/1 `isMemo`: a say line that lists memory candidates (not spoken on air). */
export function isMemo(ev: StreamEvent): boolean {
  return ev.type === "say" && ev.text.startsWith("Memory candidates");
}

/** A moment kept for memory: a `mark_moment` op or a memory-candidates line (A/1 core.js:278). */
export function isMemoryMoment(ev: StreamEvent): boolean {
  return (ev.type === "op" && ev.op.op === "mark_moment") || isMemo(ev);
}

function opText(ev: Extract<StreamEvent, { type: "op" }>): string {
  const o = ev.op;
  switch (o.op) {
    case "highlight":
      return `highlight ${o.msg_id}`;
    case "start_poll":
      return `poll ${o.poll_id}: ${o.question} [${o.options.join(" / ")}] ≥${o.min_rank} ${o.secs}s`;
    case "close_poll":
      return `close poll ${o.poll_id}`;
    case "summon_foil":
      return `summon ${o.foil}`;
    case "dismiss_foil":
      return "dismiss foil";
    case "reveal_hex":
      return `reveal ${o.hex} · ${o.grade}`;
    case "dispatch_research":
      return `research ${o.job_id}: ${o.brief}`;
    case "show_card":
      return `card “${o.title}”`;
    case "mark_moment":
      return `moment: ${o.label}`;
    case "switch_segment":
      return `switch to ${o.to}`;
    case "sign_off":
      return `sign off: ${o.reason}`;
  }
}

/** One-line body of a transcript event. */
export function eventText(ev: StreamEvent): string {
  switch (ev.type) {
    case "stream_start":
      return `${ev.character} · ${ev.tone_variant} · ${ev.core_tier} · ${ev.audience_size} viewers`;
    case "segment_start":
      return ev.segment;
    case "segment_end":
      return `${ev.segment}${typeof ev.ses === "number" ? ` · SES ${ev.ses.toFixed(2)}` : ""}`;
    case "case_open":
      return `${ev.case.title}${ev.case.fictional ? " · FICTIONAL" : ""} · ${ev.case.hexes.length} hexes`;
    case "chat":
      return ev.msg.text;
    case "picked":
      return `${ev.msg_ids.join(", ")} · ${ev.reason}`;
    case "say":
      return ev.text;
    case "op":
      return opText(ev);
    case "vote": {
      const top = [...ev.tallies].sort((a, b) => b.weight - a.weight)[0];
      return `${ev.poll_id}${ev.closed ? " closed" : ""} · ${top ? `${top.option} ${top.weight}w/${top.voters}` : "no votes"}`;
    }
    case "moderation":
      return `${ev.stage} · ${ev.rule} · ${ev.action}${ev.msg_id ? ` (${ev.msg_id})` : ""}`;
    case "handover":
      return `${ev.from} → ${ev.to} · ${ev.reason}`;
    case "job":
      return `${ev.job_id} ${ev.status}${ev.summary ? ` · ${ev.summary}` : ""}`;
    case "stream_end":
      return ev.reason;
  }
}

/** Who or what leads the line. */
export function eventWho(ev: StreamEvent): string {
  switch (ev.type) {
    case "chat":
      return ev.msg.viewer.name;
    case "say":
      return ev.speaker === "foil" ? (ev.foil ?? "foil") : "Malakor";
    case "op":
      return ev.op.op.replace(/_/g, " ");
    default:
      return ev.type.replace(/_/g, " ");
  }
}

/** Stream clock mm:ss from ms since the stream started. */
export function clock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

export function msText(ms: number | undefined): string {
  if (ms === undefined) return "–";
  return ms >= 10_000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms)} ms`;
}

export function usd(x: number): string {
  return x >= 1 ? `$${x.toFixed(2)}` : `$${x.toFixed(4)}`;
}

export const int = (n: number): string => n.toLocaleString("en-US");
