// Data hooks for the Review page. A session is one `review.session` read; feedback writes go
// through a per-line ledger (ledger.ts): the mark shows at once, a reply settles only its own
// write, a failure stays on screen for retry, and a refetched session prunes what it confirms.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { FeedbackTag, LineFeedback, ReviewSession } from "@shadowrunr/contract";
import { useMutation, useRpc } from "../../data";
import { derive, type Derived } from "./derive";
import { EMPTY_LEDGER, failedMarks, pruneLedger, rejectWrite, sendWrite, settleWrite, statusOf, viewOf, type FailedMark, type Ledger, type MarkStatus } from "./ledger";

const NONE = {} as const;

/** `review.sessions`: every sandbox session, newest first. */
export const useSessions = () => useRpc("review.sessions", NONE);

/** `review.session` for one id; skipped without one. */
export const useSession = (sessionId: string | undefined) => useRpc("review.session", sessionId === undefined ? null : { session_id: sessionId });

export interface Feedback {
  /** The current mark per line (server state with this view's writes on top). */
  readonly byLine: ReadonlyMap<number, LineFeedback>;
  /** Send a line's marks (empty tags and note clear it). Returns the write's seq, or undefined when the line already has a write in flight. */
  readonly save: (line: number, tags: readonly FeedbackTag[], note: string) => number | undefined;
  /** Where the line's newest write stands. */
  readonly statusOf: (line: number) => MarkStatus;
  /** Writes the daemon did not take, with what to resend. */
  readonly failed: readonly FailedMark[];
  /** Resend a failed line's write as it was. */
  readonly retry: (line: number) => void;
}

/** Feedback for one session: the reply's rows, overlaid by this view's write ledger. */
export function useFeedback(rs: ReviewSession): Feedback {
  const { run } = useMutation("review.feedback.set");
  // The ref is the ledger's truth (two keypresses in one tick both see the first write); the state renders it.
  const ledger = useRef<Ledger>(EMPTY_LEDGER);
  const [shown, setShown] = useState<Ledger>(EMPTY_LEDGER);
  const seq = useRef(0);
  const apply = useCallback((f: (l: Ledger) => Ledger) => {
    const next = f(ledger.current);
    if (next === ledger.current) return;
    ledger.current = next;
    setShown(next);
  }, []);
  useEffect(() => apply((l) => pruneLedger(l, rs.feedback)), [apply, rs.feedback]);
  const sessionId = rs.row.session_id;
  const save = useCallback(
    (line: number, tags: readonly FeedbackTag[], note: string) => {
      if (statusOf(ledger.current, line) === "sending") return undefined;
      const s = ++seq.current;
      const trimmed = note.trim();
      apply((l) => sendWrite(l, { session_id: sessionId, line, seq: s, tags: [...tags], note: trimmed, at_ms: Date.now() }));
      void run({ session_id: sessionId, line, tags: [...tags], ...(trimmed ? { note: trimmed } : {}) }).then((r) =>
        apply((l) => (r.ok ? settleWrite(l, line, s, r.value) : rejectWrite(l, line, s))),
      );
      return s;
    },
    [sessionId, run, apply],
  );
  const retry = useCallback(
    (line: number) => {
      const f = failedMarks(ledger.current).find((x) => x.line === line);
      if (f) save(line, f.tags, f.note);
    },
    [save],
  );
  const byLine = useMemo(() => viewOf(shown, rs.feedback), [shown, rs.feedback]);
  const failed = useMemo(() => failedMarks(shown), [shown]);
  const status = useCallback((line: number) => statusOf(shown, line), [shown]);
  return { byLine, save, statusOf: status, failed, retry };
}

/** The derived view of a loaded session (pure; memoised per reply). */
export const useDerived = (rs: ReviewSession): Derived => useMemo(() => derive(rs), [rs]);
