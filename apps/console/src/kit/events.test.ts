import { describe, expect, it } from "vitest";
import type { StreamEvent } from "@shadowrunr/contract";
import { eventText, eventWho } from "./events";

describe("session events", () => {
  it("reads a warm-session failure with its detail", () => {
    const ev: StreamEvent = { type: "session", ts_ms: 5_000, kind: "open_failed", detail: "core call unavailable" };
    expect(eventText(ev)).toBe("warm session open failed · core call unavailable");
    expect(eventWho(ev)).toBe("session");
  });

  it("reads one without a detail", () => {
    expect(eventText({ type: "session", ts_ms: 1, kind: "died" })).toBe("warm session died");
  });
});
