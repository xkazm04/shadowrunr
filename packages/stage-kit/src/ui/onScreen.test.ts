// The on-screen schedule's windows. What the canvas draws against it is pinned in apps/stage
// (onScreen.pin.test.ts), where a DOM renderer is at hand.
import { describe, expect, it } from "vitest";

import type { StreamEvent } from "@shadowrunr/contract";
import { foldEvents } from "../scene";
import { CARD_MS, POLL_LINGER_MS, REDACTION_MS, TICKER_LINGER_MS, onScreen } from "./onScreen";

const viewer = { id: "v", name: "v", platform: "sim" as const, rank: "warlock" as const };

describe("onScreen", () => {
  it("keeps a card for CARD_MS, fading over its end", () => {
    const s = foldEvents([{ type: "op", ts_ms: 1000, op: { op: "show_card", card_id: "c", title: "T", body: "b" } }]);
    expect(onScreen(s, 999).card).toBeNull();
    expect(onScreen(s, 1000 + CARD_MS).card).toMatchObject({ key: "c", since: 1000, until: 1000 + CARD_MS, leaving: true });
    expect(onScreen(s, 1001 + CARD_MS).card).toBeNull();
  });

  it("keeps a poll while open, and POLL_LINGER_MS after it closed", () => {
    const open: StreamEvent = { type: "op", ts_ms: 0, op: { op: "start_poll", poll_id: "p", question: "?", options: ["a", "b"], min_rank: "thrall", secs: 10 } };
    expect(onScreen(foldEvents([open]), 1e9).poll?.until).toBe(Infinity);
    const s = foldEvents([open, { type: "op", ts_ms: 500, op: { op: "close_poll", poll_id: "p" } }]);
    expect(onScreen(s, 500 + POLL_LINGER_MS).poll?.key).toBe("p");
    expect(onScreen(s, 501 + POLL_LINGER_MS).poll).toBeNull();
  });

  it("holds the last decree after its turn", () => {
    const s = foldEvents([
      { type: "chat", ts_ms: 0, msg: { id: "m1", ts_ms: 0, text: "hail", viewer } },
      { type: "op", ts_ms: 0, op: { op: "highlight", msg_id: "m1" } },
    ]);
    expect(onScreen(s, 100).decree?.value.held).toBe(false);
    expect(onScreen(s, 60_000).decree).toMatchObject({ until: Infinity, value: { held: true } });
  });

  it("captions the queued line showing now, and lets a redaction cover REDACTION_MS", () => {
    const s = foldEvents([
      { type: "say", ts_ms: 0, speaker: "host", text: "One long opening line, said slowly." },
      { type: "say", ts_ms: 100, speaker: "host", text: "Two." },
      { type: "moderation", ts_ms: 200, stage: "output", rule: "r", action: "withheld" },
    ]);
    const now = onScreen(s, 300);
    expect(now.caption?.value.text).toMatch(/^One/);
    expect(now.redaction?.since).toBe(200);
    expect(onScreen(s, 200 + REDACTION_MS).redaction).toBeNull();
  });

  it("keeps the ticker while a job is out and TICKER_LINGER_MS after the last came back", () => {
    const job = (ts: number, status: string): StreamEvent => ({ type: "job", ts_ms: ts, job_id: "j", status });
    expect(onScreen(foldEvents([job(0, "running")]), 1e9).ticker).not.toBeNull();
    const done = foldEvents([job(0, "running"), job(1000, "done")]);
    expect(onScreen(done, 1000 + TICKER_LINGER_MS - 1).ticker).not.toBeNull();
    expect(onScreen(done, 1000 + TICKER_LINGER_MS).ticker).toBeNull();
  });
});
