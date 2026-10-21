// The Review page's feedback write ledger, pure. Per line it holds the newest write (sending,
// saved or failed, with its payload kept for retry) and the newest write the daemon
// acknowledged. A reply settles only the write whose seq it carries, so a late failure of an
// older write never undoes a newer success; a failure shows the newest acknowledged mark, not a
// snapshot taken when the write began; a server row at or after the ack prunes the line, so the
// overlay never outranks later server state. The operator's mark is attention spent once: it is
// either shown as the daemon holds it or shown as not saved, never silently lost.
import type { FeedbackTag, LineFeedback } from "@shadowrunr/contract";

export type MarkStatus = "idle" | "sending" | "saved" | "failed";

/** One write as the operator made it. */
export interface MarkWrite {
  readonly session_id: string;
  readonly line: number;
  readonly seq: number;
  readonly tags: readonly FeedbackTag[];
  /** Trimmed; empty is no note. */
  readonly note: string;
  /** The local clock when it was made (the optimistic row's at_ms). */
  readonly at_ms: number;
}

interface Head {
  readonly write: MarkWrite;
  readonly state: Exclude<MarkStatus, "idle">;
}

interface Ack {
  readonly seq: number;
  /** The daemon's row; null when the write cleared the line. */
  readonly mark: LineFeedback | null;
  /** The daemon's at_ms on its reply. */
  readonly atMs: number;
}

export interface LineEntry {
  readonly head: Head;
  readonly acked?: Ack;
}

export type Ledger = ReadonlyMap<number, LineEntry>;

export const EMPTY_LEDGER: Ledger = new Map();

/** A row with no tags and no note clears the line. */
const markOf = (r: LineFeedback): LineFeedback | null => (r.tags.length === 0 && !r.note ? null : r);

const optimistic = (w: MarkWrite): LineFeedback | null =>
  w.tags.length === 0 && w.note === "" ? null : { session_id: w.session_id, line: w.line, tags: [...w.tags], ...(w.note ? { note: w.note } : {}), at_ms: w.at_ms };

const put = (l: Ledger, line: number, e: LineEntry | undefined): Ledger => {
  const m = new Map(l);
  if (e) m.set(line, e);
  else m.delete(line);
  return m;
};

/** A write leaves: it becomes the line's head, shown at once. */
export function sendWrite(l: Ledger, w: MarkWrite): Ledger {
  const prev = l.get(w.line);
  return put(l, w.line, { head: { write: w, state: "sending" }, ...(prev?.acked ? { acked: prev.acked } : {}) });
}

/** The daemon accepted write `seq`. The newest acknowledgement wins (by daemon clock, then seq). */
export function settleWrite(l: Ledger, line: number, seq: number, reply: LineFeedback): Ledger {
  const e = l.get(line);
  if (!e) return l;
  const a = e.acked;
  const newer = !a || reply.at_ms > a.atMs || (reply.at_ms === a.atMs && seq > a.seq);
  const acked = newer ? { seq, mark: markOf(reply), atMs: reply.at_ms } : a;
  const head = e.head.write.seq === seq ? { ...e.head, state: "saved" as const } : e.head;
  return put(l, line, { head, ...(acked ? { acked } : {}) });
}

/** Write `seq` failed. Only the line's newest write can fail it; an older one is superseded. */
export function rejectWrite(l: Ledger, line: number, seq: number): Ledger {
  const e = l.get(line);
  if (!e || e.head.write.seq !== seq || e.head.state !== "sending") return l;
  return put(l, line, { ...e, head: { ...e.head, state: "failed" } });
}

/** A refetched session's rows: a line whose server row is at or after its ack leaves the overlay (a failed write stays, for retry). */
export function pruneLedger(l: Ledger, rows: readonly LineFeedback[]): Ledger {
  const newest = new Map<number, number>();
  for (const r of rows) newest.set(r.line, Math.max(newest.get(r.line) ?? -Infinity, r.at_ms));
  let out = l;
  for (const [line, e] of l) {
    const at = newest.get(line);
    if (!e.acked || at === undefined || at < e.acked.atMs) continue;
    if (e.head.state === "saved") out = put(out, line, undefined);
    else if (e.head.state === "failed") out = put(out, line, { head: e.head });
  }
  return out;
}

/** The mark each line shows: the server's rows with the ledger on top. */
export function viewOf(l: Ledger, rows: readonly LineFeedback[]): Map<number, LineFeedback> {
  const m = new Map<number, LineFeedback>();
  for (const r of rows) {
    const k = markOf(r);
    if (k) m.set(r.line, k);
    else m.delete(r.line);
  }
  for (const [line, e] of l) {
    let shown: LineFeedback | null | undefined;
    if (e.head.state === "sending") shown = optimistic(e.head.write);
    else shown = e.acked?.mark;
    if (shown === undefined) continue;
    if (shown) m.set(line, shown);
    else m.delete(line);
  }
  return m;
}

export const statusOf = (l: Ledger, line: number): MarkStatus => l.get(line)?.head.state ?? "idle";

export interface FailedMark {
  readonly line: number;
  readonly tags: readonly FeedbackTag[];
  readonly note: string;
}

/** Writes the daemon did not take, in line order, with what to resend. */
export function failedMarks(l: Ledger): FailedMark[] {
  const out: FailedMark[] = [];
  for (const [line, e] of l) if (e.head.state === "failed") out.push({ line, tags: e.head.write.tags, note: e.head.write.note });
  return out.sort((a, b) => a.line - b.line);
}
