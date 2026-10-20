// Shared reading of transcript events and evidence refs, and the number formats every page
// prints. One copy: the Brain's viewer, the simulator's quote checks, On Air's rundown,
// Characters' situations and Reconcile's evidence all read events and refs through here.
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

/**
 * One-line body of a transcript event: the words it carries (what a judge quotes, so quote
 * verification searches it) plus the facts that identify it.
 */
export function eventText(ev: StreamEvent): string {
  switch (ev.type) {
    case "stream_start":
      return `${ev.stream_id} · ${ev.character} · ${ev.tone_variant} · ${ev.core_tier} · ${ev.audience_size} viewers`;
    case "segment_start":
      return ev.segment;
    case "segment_end":
      return `${ev.segment}${typeof ev.ses === "number" ? ` · SES ${ev.ses.toFixed(2)}` : ""}`;
    case "case_open":
      return `${ev.case.title || ev.case.id}${ev.case.fictional ? " · FICTIONAL" : ""} · ${ev.case.hexes.length} hexes`;
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
    case "beat_plan":
      return `${ev.mode.replace(/_/g, " ")}${ev.bit_id ? ` · ${ev.bit_id}` : ""} · ${ev.picks} picked · ${ev.reason}`;
    case "health":
      return `engine ${ev.state}${ev.detail ? ` · ${ev.detail}` : ""}`;
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

/** An evidence ref `<run_id>#<line>`: run ids may be tournament-qualified with `/`. */
export interface TurnRef {
  readonly runId: string;
  /** 1-based transcript line. */
  readonly line: number;
}

/** `<run_id>#<line>` → its parts; undefined when it is not a transcript ref (no line, line 0). */
export function parseRef(ref: string): TurnRef | undefined {
  const at = ref.lastIndexOf("#");
  if (at <= 0) return undefined;
  const tail = ref.slice(at + 1);
  if (!/^\d+$/.test(tail)) return undefined;
  const line = Number(tail);
  return line >= 1 ? { runId: ref.slice(0, at), line } : undefined;
}

/** The run a ref names: its run id, or the whole ref when it carries no line. */
export const refRun = (ref: string): string => parseRef(ref)?.runId ?? ref;

/** The short chip text for a ref: line first, then the run's tail. */
export function refLabel(ref: string): string {
  const p = parseRef(ref);
  if (!p) return ref;
  const run = p.runId.length > 28 ? `…${p.runId.slice(-26)}` : p.runId;
  return `L${p.line} · ${run}`;
}

/** Stream clock `mm:ss` from ms since the stream started. */
export function clock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/** A span `m:ss`, or `h:mm:ss` past the hour. */
export function span(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** A duration: ms below ten seconds, seconds with one decimal above. */
export function msText(ms: number | undefined): string {
  if (ms === undefined) return "–";
  return ms >= 10_000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms)} ms`;
}

/** Dollars: four decimals under a dollar, two above. */
export function usd(x: number): string {
  return x >= 1 ? `$${x.toFixed(2)}` : `$${x.toFixed(4)}`;
}

/** A count with thousands separators. */
export const int = (n: number): string => n.toLocaleString("en-US");

/** A share 0..1 as a percentage (clamped); `–` when there is none. */
export function pct(x: number | undefined, digits = 0): string {
  if (x === undefined) return "–";
  return `${(Math.min(1, Math.max(0, x)) * 100).toFixed(digits)}%`;
}
