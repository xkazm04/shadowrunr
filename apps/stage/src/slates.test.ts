// The broadcast canvas in trouble and at its end: what renders with `?broadcast=1`.
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { StreamEvent } from "@shadowrunr/contract";
import { buildMap, foldEvents } from "@shadowrunr/stage-kit";
import { EndCard } from "@shadowrunr/stage-kit/ui";
import { SignalLostSlate, StageErrorBoundary, StalledMark, fmtClock, streamElapsed } from "./Slates";
import { Stage } from "./Stage";

const start: StreamEvent = {
  type: "stream_start",
  ts_ms: 0,
  stream_id: "s-1",
  character: "malakor",
  tone_variant: "theatrical",
  core_tier: "haiku",
  audience_size: 30,
};
const seg: StreamEvent = { type: "segment_start", ts_ms: 600, segment: "SEG-01" };
const end: StreamEvent = { type: "stream_end", ts_ms: 300_000, reason: "fleet_lost" };
const signOff: StreamEvent = { type: "op", ts_ms: 299_000, op: { op: "sign_off", reason: "The Lord recalls me." } };

function render(events: StreamEvent[], now: number, extra: Record<string, unknown> = {}): string {
  const scene = foldEvents(events);
  return renderToStaticMarkup(
    createElement(Stage, {
      scene,
      now,
      map: buildMap(null, []),
      caseFile: null,
      sourceTag: "LIVE",
      debug: false,
      broadcast: true,
      ...extra,
    }),
  );
}

describe("the end of a stream", () => {
  it("shows an end card on stream_end without a sign_off", () => {
    const html = render([start, seg, end], 301_000);
    expect(html).toContain("end-card");
    expect(html).toContain("THE SPIRE GOES DARK");
    expect(html).toContain("transmission ended · fleet lost");
  });

  it("keeps the Outro when a sign_off came first", () => {
    const html = render([start, seg, signOff, end], 301_000);
    expect(html).not.toContain("end-card");
    expect(html).toContain("THE LEDGER IS SEALED");
    expect(html).toContain("The Lord recalls me.");
  });

  it("shows nothing before the end", () => {
    const scene = foldEvents([start, seg, end]);
    expect(renderToStaticMarkup(createElement(EndCard, { scene, now: 299_000 }))).toBe("");
  });
});

describe("broadcast-safe slates", () => {
  it("renders the signal-lost slate and the stalled mark on the canvas in broadcast mode", () => {
    const lost = render([start, seg], 10_000, { overlay: createElement(SignalLostSlate) });
    expect(lost).toContain("SIGNAL LOST · reconnecting");
    const stalled = render([start, seg], 30_000, { overlay: createElement(StalledMark, { ms: 21_400 }) });
    expect(stalled).toContain("signal stalled · 21 s");
  });

  it("shows the elapsed timer from the stream clock, frozen at the end", () => {
    const html = render([start, seg], 125_000, { clock: fmtClock(125_000) });
    expect(html).toContain('<span class="clock-pill">02:05</span>');
    expect(streamElapsed(foldEvents([start, seg, end]), 999_999)).toBe(300_000);
    expect(streamElapsed(foldEvents([]), 5)).toBeNull();
    expect(fmtClock(3_725_000)).toBe("62:05");
  });

  it("puts a render throw behind a calm fallback slate", () => {
    expect(StageErrorBoundary.getDerivedStateFromError()).toEqual({ failed: true });
    const b = new StageErrorBoundary({ voice: false, children: null });
    b.state = { failed: true };
    const html = renderToStaticMarkup(b.render() as never);
    expect(html).toContain("THE SPIRE IS RECOMPOSING");
    expect(html).toContain("AI character · text only");
  });
});

describe("text-only honesty", () => {
  it("discloses text only and drops the placeholder note in broadcast mode", () => {
    const html = render([start, seg], 5000);
    expect(html).toContain("AI character · text only");
    expect(html).not.toContain("synthetic voice");
    expect(html).not.toContain("avatar-note");
    const debugView = render([start, seg], 5000, { broadcast: false });
    expect(debugView).toContain("avatar slot · face rig or procedural placeholder");
    const voiced = render([start, seg], 5000, { voice: true });
    expect(voiced).toContain("AI character · synthetic voice");
    // and the small label beside the face says it too
    expect(voiced).toContain('<div class="voice-label">synthetic voice</div>');
  });
});

describe("captions on the canvas", () => {
  it("shows the caption at the head of the queue, not the latest line said", () => {
    const a = "The ledger opens, minions, and every line of it is mine.";
    const says: StreamEvent[] = [
      { type: "say", ts_ms: 1000, speaker: "malakor", text: a },
      { type: "say", ts_ms: 1100, speaker: "malakor", text: "A burst line." },
    ];
    const html = render([start, seg, ...says], 1200);
    expect(html).toContain("ledger");
    expect(html).not.toContain("burst");
  });
});
