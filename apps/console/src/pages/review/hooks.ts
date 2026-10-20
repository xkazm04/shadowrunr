// Data hooks for the Review page. A session is one `review.session` read; feedback writes are
// optimistic: the mark shows at once and rolls back if `review.feedback.set` fails, without
// refetching the whole session.
import { useCallback, useMemo, useState } from "react";
import type { FeedbackTag, LineFeedback, ReviewSession } from "@shadowrunr/contract";
import { useMutation, useRpc } from "../../data";
import { derive, feedbackByLine, type Derived } from "./derive";

const NONE = {} as const;

/** `review.sessions`: every sandbox session, newest first. */
export const useSessions = () => useRpc("review.sessions", NONE);

/** `review.session` for one id; skipped without one. */
export const useSession = (sessionId: string | undefined) => useRpc("review.session", sessionId === undefined ? null : { session_id: sessionId });

export interface Feedback {
  /** The current mark per line (server state with local writes on top). */
  readonly byLine: ReadonlyMap<number, LineFeedback>;
  /** Save a line's marks; empty tags and note clear it. Resolves to an error message, or undefined. */
  readonly save: (line: number, tags: readonly FeedbackTag[], note: string) => Promise<string | undefined>;
  readonly pending: boolean;
}

/** Feedback for one session: the reply's rows, overlaid by this view's own writes. */
export function useFeedback(rs: ReviewSession): Feedback {
  const set = useMutation("review.feedback.set");
  const [local, setLocal] = useState<ReadonlyMap<number, LineFeedback | null>>(new Map());
  const base = useMemo(() => feedbackByLine(rs.feedback), [rs.feedback]);
  const byLine = useMemo(() => {
    const m = new Map(base);
    for (const [line, f] of local) {
      if (f === null) m.delete(line);
      else m.set(line, f);
    }
    return m;
  }, [base, local]);
  const save = useCallback(
    async (line: number, tags: readonly FeedbackTag[], note: string) => {
      const sessionId = rs.row.session_id;
      const trimmed = note.trim();
      const before = local.has(line) ? local.get(line) : base.get(line) ?? null;
      const next: LineFeedback | null = tags.length === 0 && trimmed === "" ? null : { session_id: sessionId, line, tags: [...tags], ...(trimmed ? { note: trimmed } : {}), at_ms: Date.now() };
      setLocal((m) => new Map(m).set(line, next));
      const r = await set.run({ session_id: sessionId, line, tags: [...tags], ...(trimmed ? { note: trimmed } : {}) });
      if (r.ok) return undefined;
      setLocal((m) => new Map(m).set(line, before ?? null));
      return "Your mark was not saved. Check that the daemon is running, then try again.";
    },
    [rs.row.session_id, set, local, base],
  );
  return { byLine, save, pending: set.pending };
}

/** The derived view of a loaded session (pure; memoised per reply). */
export const useDerived = (rs: ReviewSession): Derived => useMemo(() => derive(rs), [rs]);
