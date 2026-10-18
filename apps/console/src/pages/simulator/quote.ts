// Quote verification (A/1 core.js:241-249): a judge quote is checked against the transcript
// line it cites and the two lines either side. Found on the cited line: verified. Found within
// ±2 lines: nearby. Otherwise: not found. The daemon verifies quotes mechanically too; this is
// the operator's own check, on the transcript the page loaded.
import type { StreamEvent } from "@shadowrunr/contract";
import { eventText } from "../../kit/events";

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
