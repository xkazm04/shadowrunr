// SEG-03, the Table Read: the scene card (scene_start until its scene_end), the guest, the guest's
// lines as captions under its own name, and the ballot drawn as an ordinary poll.
import { describe, expect, it } from "vitest";

import type { StreamEvent } from "@shadowrunr/contract";
import { captionAt, foldEvents, reduce } from "./scene";
import { defaultScreen, guestOf, initialScene, isTwoShot } from "./sceneModel";
import { pollBars } from "./tallies";

const say = (ts: number, text: string, guest?: string): StreamEvent =>
  guest === undefined ? { type: "say", ts_ms: ts, speaker: "host", text } : { type: "say", ts_ms: ts, speaker: "foil", foil: guest, text };

/** A SEG-03 stretch: the ballot of three, the winning scene, alternating lines, the end. */
const STRETCH: StreamEvent[] = [
  { type: "segment_start", ts_ms: 0, segment: "SEG-03" },
  { type: "op", ts_ms: 100, op: { op: "start_poll", poll_id: "scene-1", question: "Which scene do we read?", options: ["The Gallery", "The Critic", "The Commission"], min_rank: "thrall", secs: 60 } },
  { type: "vote", ts_ms: 30_000, poll_id: "scene-1", closed: false, tallies: [{ option: "The Gallery", voters: 3, weight: 4.4 }, { option: "The Critic", voters: 1, weight: 1 }, { option: "The Commission", voters: 0, weight: 0 }] },
  { type: "op", ts_ms: 60_000, op: { op: "close_poll", poll_id: "scene-1" } },
  { type: "scene_start", ts_ms: 61_000, scene_id: "the-gallery", title: "The Gallery", guest: "the-painter" },
  say(62_000, "Welcome to my gallery opening. Mind the empty walls."),
  say(72_000, "Empty? They are pre-hung for my masterpieces.", "the-painter"),
  say(82_000, "Pre-hung. Like the jury that rejected you."),
  { type: "scene_end", ts_ms: 90_000, scene_id: "the-gallery" },
];

describe("SEG-03 Table Read", () => {
  it("opens on the host room, drawn as the two-shot", () => {
    expect(defaultScreen("SEG-03")).toBe("host");
    const s = foldEvents(STRETCH.slice(0, 1));
    expect(s.layout).toBe("host");
    expect(isTwoShot(s)).toBe(true);
    expect(isTwoShot(foldEvents([{ type: "segment_start", ts_ms: 0, segment: "SEG-01" }]))).toBe(false);
  });

  it("shows the scene's title card from scene_start and clears it on its scene_end", () => {
    const during = foldEvents(STRETCH, 61_500);
    expect(during.sceneCard).toEqual({ id: "the-gallery", title: "The Gallery", guest: "the-painter", at: 61_000 });
    expect(guestOf(during)).toBe("the-painter");
    const after = foldEvents(STRETCH);
    expect(after.sceneCard).toBeNull();
    // the guest stays for the free talk after the scene
    expect(guestOf(after)).toBe("the-painter");
  });

  it("keeps the card when a scene_end names another scene", () => {
    let s = foldEvents(STRETCH, 61_500);
    s = reduce(s, { type: "scene_end", ts_ms: 61_600, scene_id: "the-critic" });
    expect(s.sceneCard?.id).toBe("the-gallery");
  });

  it("captions the guest's lines under the guest, the host's under the host", () => {
    const guestLine = captionAt(foldEvents(STRETCH, 72_100), 72_100);
    expect(guestLine).toMatchObject({ text: "Empty? They are pre-hung for my masterpieces.", foil: "the-painter" });
    const hostLine = captionAt(foldEvents(STRETCH, 82_100), 82_100);
    expect(hostLine?.text).toBe("Pre-hung. Like the jury that rejected you.");
    expect(hostLine && "foil" in hostLine).toBe(false);
  });

  it("names the guest from its own line when no scene has started", () => {
    const s = foldEvents([STRETCH[0]!, say(10, "I demand a better easel.", "the-painter")]);
    expect(guestOf(s)).toBe("the-painter");
    expect(s.caption?.foil).toBe("the-painter");
  });

  it("leaves SEG-01's foil lines in the foil card, not the caption", () => {
    const s = foldEvents([{ type: "segment_start", ts_ms: 0, segment: "SEG-01" }, say(10, "Disrupt this.", "the-disruptor")]);
    expect(s.caption).toBeNull();
    expect(s.foilLine?.text).toBe("Disrupt this.");
    expect(s.guest).toBeNull();
    expect(guestOf(s)).toBeNull();
  });

  it("forgets the guest and any open scene when the segment changes", () => {
    let s = foldEvents(STRETCH, 61_500);
    s = reduce(s, { type: "handover", ts_ms: 62_000, from: "SEG-03", to: "SEG-01", reason: "back to the room" });
    expect(s.guest).toBeNull();
    expect(s.sceneCard).toBeNull();
    expect(isTwoShot(s)).toBe(false);
  });

  it("draws the scene ballot as an ordinary poll", () => {
    const s = foldEvents(STRETCH, 30_000);
    const poll = s.polls["scene-1"]!;
    expect(s.activePoll).toBe("scene-1");
    expect(poll.options).toEqual(["The Gallery", "The Critic", "The Commission"]);
    expect(pollBars(poll).map((b) => [b.option, b.leading])).toEqual([
      ["The Gallery", true],
      ["The Critic", false],
      ["The Commission", false],
    ]);
  });

  it("is a no-op for the initial scene's other fields", () => {
    expect(initialScene().guest).toBeNull();
    expect(initialScene().sceneCard).toBeNull();
  });
});
