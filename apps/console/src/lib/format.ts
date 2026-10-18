import type { Bound, StreamEvent } from "../contract";

export function pct(x: number | undefined, digits = 0): string {
  return typeof x === "number" ? `${(x * 100).toFixed(digits)}%` : "–";
}

export function boundStr(b: Bound | undefined): string {
  if (!b) return "–";
  const m = b.method === "rule_of_three" ? "3/n" : "Wilson";
  return `${pct(b.lo)}–${pct(b.hi)} (${m})`;
}

export function msStr(x: number | undefined): string {
  if (typeof x !== "number") return "–";
  return x >= 10_000 ? `${(x / 1000).toFixed(1)} s` : `${Math.round(x)} ms`;
}

/** Stream clock mm:ss from ms since stream start. */
export function clock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

export function dateTime(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export const shortHash = (h: string) => h.slice(0, 8);

/** `<run_id>#<line>` -> parts. Run ids never contain '#'; the line is 1-based. */
export function parseRef(ref: string): { runId: string; line: number } | undefined {
  const i = ref.lastIndexOf("#");
  if (i <= 0) return undefined;
  const line = Number(ref.slice(i + 1));
  if (!Number.isInteger(line) || line < 1) return undefined;
  return { runId: ref.slice(0, i), line };
}

/** One-line body of a transcript event for the indicative layer. */
export function eventText(ev: StreamEvent): string {
  switch (ev.type) {
    case "stream_start":
      return `${ev.character} · ${ev.tone_variant} · ${ev.core_tier} · ${ev.audience_size} viewers`;
    case "segment_start":
      return ev.segment;
    case "segment_end":
      return `${ev.segment}${typeof ev.ses === "number" ? ` · SES ${ev.ses.toFixed(2)}` : ""}`;
    case "chat":
      return ev.msg.text;
    case "picked":
      return `${ev.msg_ids.join(", ")} · ${ev.reason}`;
    case "say":
      return ev.text;
    case "op": {
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
      return "";
    }
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

/** Who/what for the event's lead column. */
export function eventActor(ev: StreamEvent): string {
  switch (ev.type) {
    case "chat":
      return ev.msg.viewer.name;
    case "say":
      return ev.speaker === "foil" ? (ev.foil ?? "foil") : "malakor";
    case "op":
      return ev.op.op;
    default:
      return ev.type;
  }
}
