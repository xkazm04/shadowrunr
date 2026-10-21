// The feedback write ledger, pure: per line, a reply settles only the write whose seq it carries,
// a failure falls back to the newest acknowledged state (never a snapshot), pending and failure
// are per line, and a server row at or after the ack prunes the line from the overlay.
import { describe, expect, it } from "vitest";
import type { FeedbackTag, LineFeedback } from "@shadowrunr/contract";
import { EMPTY_LEDGER, failedMarks, pruneLedger, rejectWrite, sendWrite, settleWrite, statusOf, viewOf, type Ledger } from "./ledger";

const S = "s1";
const row = (line: number, tags: FeedbackTag[], at_ms: number, note?: string): LineFeedback => ({ session_id: S, line, tags, at_ms, ...(note ? { note } : {}) });
const send = (l: Ledger, line: number, seq: number, tags: FeedbackTag[], note = "") => sendWrite(l, { session_id: S, line, seq, tags, note, at_ms: 0 });
const tagsOn = (l: Ledger, base: readonly LineFeedback[], line: number) => viewOf(l, base).get(line)?.tags;

describe("the feedback ledger", () => {
  it("a late failure of an older write never clobbers a newer success on the same line", () => {
    const L = 7;
    const base = [row(L, ["too_mean"], 5)];
    let l = send(EMPTY_LEDGER, L, 1, ["landed"]);
    l = send(l, L, 2, ["flat"]);
    l = settleWrite(l, L, 2, row(L, ["flat"], 20));
    l = rejectWrite(l, L, 1);
    expect(tagsOn(l, base, L)).toEqual(["flat"]);
    expect(statusOf(l, L)).toBe("saved");
    expect(failedMarks(l)).toEqual([]);
  });

  it("keeps status per line: a failed line keeps its payload for retry, a saved one keeps its mark", () => {
    let l = send(EMPTY_LEDGER, 1, 1, ["landed"], "nice");
    l = send(l, 2, 2, ["flat"]);
    expect(statusOf(l, 1)).toBe("sending");
    expect(statusOf(l, 2)).toBe("sending");
    l = rejectWrite(l, 1, 1);
    l = settleWrite(l, 2, 2, row(2, ["flat"], 30));
    expect(statusOf(l, 1)).toBe("failed");
    expect(failedMarks(l)).toEqual([{ line: 1, tags: ["landed"], note: "nice" }]);
    expect(statusOf(l, 2)).toBe("saved");
    expect(tagsOn(l, [], 2)).toEqual(["flat"]);
    // The failed line shows what the daemon holds (nothing), not the unsaved payload.
    expect(viewOf(l, []).has(1)).toBe(false);
    expect(statusOf(l, 3)).toBe("idle");
  });

  it("an optimistic write shows at once and a cleared mark hides the server row", () => {
    let l = send(EMPTY_LEDGER, 4, 1, ["great_roast"]);
    expect(tagsOn(l, [row(4, ["flat"], 1)], 4)).toEqual(["great_roast"]);
    l = send(l, 5, 2, []);
    expect(viewOf(l, [row(5, ["flat"], 1)]).has(5)).toBe(false);
  });

  it("a refetched server row at or after the ack prunes the line; an older row does not", () => {
    let l = send(EMPTY_LEDGER, 9, 1, ["landed"]);
    l = settleWrite(l, 9, 1, row(9, ["landed"], 100));
    expect(pruneLedger(l, [row(9, ["flat"], 99)]).has(9)).toBe(true);
    const pruned = pruneLedger(l, [row(9, ["repetitive"], 100)]);
    expect(pruned.has(9)).toBe(false);
    expect(tagsOn(pruned, [row(9, ["repetitive"], 100)], 9)).toEqual(["repetitive"]);
    // An in-flight write is never pruned, whatever the server says.
    const inFlight = send(l, 9, 2, ["flat"]);
    expect(pruneLedger(inFlight, [row(9, ["repetitive"], 200)]).has(9)).toBe(true);
  });

  it("pruning keeps a failed write for retry but drops its stale ack", () => {
    let l = send(EMPTY_LEDGER, 3, 1, ["landed"]);
    l = settleWrite(l, 3, 1, row(3, ["landed"], 10));
    l = send(l, 3, 2, ["flat"]);
    l = rejectWrite(l, 3, 2);
    expect(tagsOn(l, [], 3)).toEqual(["landed"]);
    l = pruneLedger(l, [row(3, ["off_tone"], 50)]);
    expect(statusOf(l, 3)).toBe("failed");
    expect(tagsOn(l, [row(3, ["off_tone"], 50)], 3)).toEqual(["off_tone"]);
  });
});
