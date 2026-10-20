// The Review page's view of a session: a pure function of `review.session` (the reader's shape,
// per timeline-derivation), recomputed on every load and never stored. Event i is transcript
// line i + 1, the same number as the judge's `<run>#<line>` refs and the `/events` seq.
import type { ChatMsg, LineFeedback, LineTag, ReviewSession, StreamEvent } from "@shadowrunr/contract";
import type { ShownKind } from "./names";

export interface SayLine {
  readonly line: number;
  /** Stream seconds. */
  readonly t: number;
  readonly text: string;
  readonly kind: ShownKind;
  readonly tag: LineTag | undefined;
  readonly replyingTo: readonly string[];
}

export interface Derived {
  /** Malakor's lines in transcript order. */
  readonly says: readonly SayLine[];
  readonly sayByLine: ReadonlyMap<number, SayLine>;
  /** Chat messages by message id, with their line. */
  readonly chatById: ReadonlyMap<string, { readonly line: number; readonly t: number; readonly msg: ChatMsg }>;
  /** Judged rule ids that cite each line. */
  readonly failLines: ReadonlyMap<number, readonly string[]>;
  /** Quality facts per sampled line: [dimension, value]. */
  readonly facts: ReadonlyMap<number, readonly (readonly [string, boolean])[]>;
  /** Malakor lines per shown kind (kinds with no line omitted). */
  readonly mix: Readonly<Partial<Record<ShownKind, number>>>;
  readonly lengthS: number;
}

export const lineOf = (ref: string): number | undefined => {
  const n = Number(ref.slice(ref.lastIndexOf("#") + 1));
  return Number.isInteger(n) && n > 0 ? n : undefined;
};
export const tsOf = (e: StreamEvent): number => e.ts_ms / 1000;

/** The kind the page shows: the tag's kind, `stall` for a line the engine record marks as a stall line, `untagged` without a tag. */
export function derive(rs: ReviewSession): Derived {
  const ev = rs.events;
  const tags = new Map<number, LineTag>((rs.tags?.lines ?? []).map((t) => [t.line, t]));
  // A stall line is the Malakor say just before `moderation output engine_stall` (within 3 events).
  const stall = new Set<number>();
  ev.forEach((e, i) => {
    if (e.type !== "moderation" || e.stage !== "output" || e.rule !== "engine_stall") return;
    for (let j = i - 1; j >= 0 && j >= i - 3; j--) {
      const p = ev[j];
      if (p?.type === "say") { stall.add(j + 1); break; }
    }
  });
  const says: SayLine[] = [];
  const chatById = new Map<string, { line: number; t: number; msg: ChatMsg }>();
  let last = 0;
  ev.forEach((e, i) => {
    const line = i + 1;
    last = Math.max(last, tsOf(e));
    if (e.type === "chat") chatById.set(e.msg.id, { line, t: tsOf(e), msg: e.msg });
    if (e.type !== "say" || e.speaker !== "malakor") return;
    const tag = tags.get(line);
    const kind: ShownKind = stall.has(line) ? "stall" : tag ? tag.kind : "untagged";
    says.push({ line, t: tsOf(e), text: e.text, kind, tag, replyingTo: e.replying_to ?? [] });
  });
  const failLines = new Map<number, string[]>();
  const facts = new Map<number, [string, boolean][]>();
  for (const r of rs.verdict?.rules ?? []) {
    if (r.state !== "fail") continue;
    for (const ref of r.evidence ?? []) { const l = lineOf(ref); if (l) failLines.set(l, [...(failLines.get(l) ?? []), r.rule]); }
  }
  for (const q of rs.verdict?.quality ?? []) { const l = lineOf(q.turn_ref); if (l) facts.set(l, [...(facts.get(l) ?? []), [q.dimension, q.value]]); }
  const mix: Partial<Record<ShownKind, number>> = {};
  for (const s of says) mix[s.kind] = (mix[s.kind] ?? 0) + 1;
  return { says, sayByLine: new Map(says.map((s) => [s.line, s])), chatById, failLines, facts, mix, lengthS: last };
}

/** Feedback by line, folded (the reply is already folded; local optimistic rows override it). */
export function feedbackByLine(rows: readonly LineFeedback[]): Map<number, LineFeedback> {
  const m = new Map<number, LineFeedback>();
  for (const r of rows) {
    if (r.tags.length === 0 && !r.note) m.delete(r.line);
    else m.set(r.line, r);
  }
  return m;
}

/** The segment a stream second falls in. */
export function segmentAt(rs: ReviewSession, t: number): string | undefined {
  return rs.segments.find((g) => t >= g.from_s - 0.01 && t <= g.to_s + 0.01)?.segment;
}
