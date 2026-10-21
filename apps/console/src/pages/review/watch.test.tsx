// The player's watch pane: off until asked for, then the session's recording from
// `clips.recording`, seeked to the line on screen (its heard start, else its transcript time,
// moved by the recording's stream offset); a session without a recording says so.
import { act, cleanup, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ClipRunRecording, ReviewSession } from "@shadowrunr/contract";
import { mountPage, ok, settle } from "../../kit/testing";
import { derive } from "./derive";
import { Layer2 } from "./Layer2";
import { reviewSession, SESSION } from "./tabs.fixture";
import { fileSeconds, lineStartMs, mediaUrl } from "./watch";

const rec = (heard: ClipRunRecording["heard"]): ClipRunRecording => ({
  session_id: SESSION,
  media_path: `/media/runs/sandbox/${SESSION}/recording.mp4`,
  stream_offset_ms: -542,
  width: 1920,
  height: 1080,
  fps: 30,
  av_offset_ms: 200,
  heard,
});

let dispose: (() => void) | null = null;
afterEach(() => {
  cleanup();
  dispose?.();
  dispose = null;
});

describe("where a line sits in the recording", () => {
  it("uses the heard start, else the transcript time, on the file's clock", () => {
    const r = rec([{ line: 4, start_ms: 10_720, end_ms: 15_346 }]);
    expect(lineStartMs(r, 4, 8_990)).toEqual({ ms: 10_720, heard: true });
    expect(lineStartMs(r, 5, 8_990)).toEqual({ ms: 8_990, heard: false });
    // The guide's example: 10720 with an offset of -542 is 11.262 s in.
    expect(fileSeconds(r, 10_720)).toBeCloseTo(11.262, 6);
    expect(fileSeconds({ ...r, stream_offset_ms: 5_000 }, 1_000)).toBe(0);
    expect(mediaUrl("http://127.0.0.1:7833/", r)).toBe(`http://127.0.0.1:7833/media/runs/sandbox/${SESSION}/recording.mp4`);
  });
});

async function mount(rs: ReviewSession, recording: ClipRunRecording | null) {
  const m = mountPage(
    <Layer2 sub={[SESSION, "player", "roast"]} />,
    (method) => {
      if (method === "review.session") return ok(rs);
      if (method === "review.sessions" || method === "review.feedback.list") return ok([]);
      if (method === "clips.recording") return recording ? ok(recording) : { status: 200, body: { error: { code: "not_found", message: `session ${SESSION} has no recording.mp4` } } };
      return { status: 404, body: { error: { code: "method_not_found", message: method } } };
    },
    `#/review/${SESSION}/player/roast`,
  );
  dispose = m.dispose;
  await settle(8);
  return m;
}

const turnOn = async () => {
  await act(async () => {
    screen.getByRole("switch", { name: /^Watch this line in the recording/ }).click();
  });
  await settle(6);
};

describe("the watch pane", () => {
  it("loads nothing until asked, then plays the recording at the line's real start", async () => {
    const rs = reviewSession();
    const first = derive(rs).says.find((s) => s.kind === "roast");
    expect(first).toBeDefined();
    const line = first?.line ?? 0;
    const m = await mount(rs, rec([{ line, start_ms: 61_458, end_ms: 64_000 }]));
    expect(m.calls).not.toContain("clips.recording");
    expect(document.querySelector("[data-role='review-watch-video']")).toBeNull();
    await turnOn();
    expect(m.calls).toContain("clips.recording");
    const video = document.querySelector<HTMLVideoElement>("[data-role='review-watch-video']");
    expect(video?.getAttribute("src")).toBe(`http://127.0.0.1:7717/media/runs/sandbox/${SESSION}/recording.mp4`);
    expect(video?.getAttribute("data-at")).toBe("62.000");
    expect(document.querySelector("[data-role='review-watch-where']")?.textContent).toContain(`Line ${line} starts at 01:01`);
  });

  it("says when the session has no recording", async () => {
    await mount(reviewSession(), null);
    await turnOn();
    expect(document.querySelector("[data-role='review-watch']")?.textContent).toContain("This session has no recording");
    expect(document.querySelector("[data-role='review-watch-video']")).toBeNull();
  });
});
