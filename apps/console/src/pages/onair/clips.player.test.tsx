// One clip as a viewer sees it: the cut from the recording played from the daemon's media
// route (9:16 first, the 16:9 trim a toggle away), the lines on the voice's real timing with
// the unheard one marked, the programme level, and watched feedback saved by `clips.feedback`
// under its names, never its wire ids.
import type { ClipCandidate } from "@shadowrunr/contract";
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { mountPage, ok, role, settle } from "../../kit/testing";
import { aspects, cutFile, FEEDBACK_TAGS, footageOf, speakerName, timedLines } from "./clipModel";
import { Layer2 } from "./Layer2";

const ID = "clip-8c828a9ccd285161";
const W = { marker: 0.35, chat: 0.25, audio: 0.15, hook: 0.25 };

function recorded(extra: Partial<ClipCandidate> = {}): ClipCandidate {
  return {
    id: ID,
    created_ms: 1,
    status: "pending_owner_gate",
    score: 0.427,
    rank: 1,
    lane: "review",
    segment: "SEG-01",
    title: "I granted it out of sheer administrative respect.",
    source: { run_id: "sbx-20261021-085027-5m", stream_id: "sbx-20261021-085027-5m", transcript: "sandbox/sbx-20261021-085027-5m/transcript.jsonl", line_from: 36, line_to: 55 },
    window: { anchor_ms: 87_000, start_ms: 69_000, end_ms: 98_000, spoken_start_ms: 80_771, spoken_end_ms: 112_027, duration_s: 31.26, driver: "crowd", lag_ms: 5000, preroll_ms: 12_766, band_s: [25, 35], style: "seg01-split-stage", voice_timed: true },
    markers: [],
    signals: { marker: 0, chat: 1, chat_z: 5, audio: 0.469, audio_db: -26.1, audio_stream_db: -25.7, unmeasured: { hook: "no hook read for this window" } },
    scoring: { nominal: W, effective: { marker: 0.467, chat: 0.333, audio: 0.2, hook: 0 }, provenance: "unmeasured: design hypothesis", agreeing: ["chat"] },
    captions: [
      { start_ms: 12_800, end_ms: 15_000, speaker: "malakor", text: "I granted it out of sheer administrative respect." },
      { start_ms: 0, end_ms: 2_500, speaker: "malakor", text: "You're welcome here." },
      { start_ms: 16_000, end_ms: 18_000, speaker: "the-auditor", text: "Objection.", unvoiced: true },
    ],
    render: {
      kind: "video",
      footage: "recording",
      width: 1080,
      height: 1920,
      duration_s: 31.26,
      files: [
        { path: "cut-9x16.mp4", kind: "cut_9x16", bytes: 2_322_537 },
        { path: "cut-16x9.mp4", kind: "cut_16x9", bytes: 2_179_290 },
      ],
      overlay: { alleged_header: false, ai_disclosure: true, captions: "voice timing" },
      rendered_ms: 1,
    },
    gate: { requires_mandatory_gate: false, checks: [{ id: "ai_disclosure", state: "pass", detail: "burnt in" }], held: false, evaluated_ms: 1 },
    history: [],
    recording: { file: "recording.mp4", stream_offset_ms: -542, width: 1920, height: 1080, fps: 30, av_offset_ms: 200 },
    ...extra,
  };
}

let dispose: (() => void) | null = null;
afterEach(() => {
  dispose?.();
  dispose = null;
  cleanup();
});

describe("the clip model", () => {
  it("names the footage, the files and the lines", () => {
    const c = recorded();
    expect(footageOf(c)).toBe("recording");
    expect(aspects(c)).toEqual(["9x16", "16x9"]);
    expect(cutFile(c, "16x9")).toBe("cut-16x9.mp4");
    // A render from before the recorded cuts carries no footage: it was re-drawn.
    const old = recorded();
    if (old.render) delete old.render.footage;
    expect(footageOf(old)).toBe("redrawn");
    const lines = timedLines(c);
    expect(lines.map((l) => l.at)).toEqual([0, 12.8, 16]);
    expect(lines[0]?.streamMs).toBe(80_771);
    expect(lines[2]?.unheard).toBe(true);
    expect(speakerName("the-auditor")).toBe("Auditor");
    expect(new Set(FEEDBACK_TAGS.map(([id]) => id)).size).toBe(8);
  });
});

describe("On Air · one clip, watched", () => {
  it("plays the vertical cut, toggles to the broadcast trim and seeks from a line", async () => {
    const m = mountPage(<Layer2 sub={["clips", ID]} />, (method) => (method === "clips.get" ? ok(recorded()) : ok(null)), `#/onair/clips/${ID}`);
    dispose = m.dispose;
    await settle();
    const video = () => document.querySelector<HTMLVideoElement>("[data-role='onair-clip-video']");
    expect(video()?.getAttribute("src")).toBe(`http://127.0.0.1:7717/media/clips/${ID}/cut-9x16.mp4`);
    expect(role("onair-clip-player")?.closest("section")?.textContent).toContain("Cut from the recording");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "16:9 broadcast" }));
    });
    expect(video()?.getAttribute("src")).toBe(`http://127.0.0.1:7717/media/clips/${ID}/cut-16x9.mp4`);
    const lines = role("onair-clip-lines");
    expect(lines?.querySelectorAll("li")).toHaveLength(3);
    expect(lines?.textContent).toContain("1:20");
    expect(lines?.textContent).toContain("never heard");
    expect(lines?.textContent).not.toContain("the-auditor");
    await act(async () => {
      fireEvent.click(screen.getByTitle("play from 12.8 s"));
    });
    expect(video()?.currentTime).toBe(12.8);
    expect(role("onair-clip-level")?.textContent).toContain("-26.1 dBFS RMS against the recording’s -25.7 dBFS");
  });

  it("saves watched feedback under its names", async () => {
    let sent = "";
    const m = mountPage(
      <Layer2 sub={["clips", ID]} />,
      (method, body) => {
        if (method === "clips.feedback") {
          sent = body;
          return ok(recorded({ feedback: { tags: ["cut_too_early", "landed"], note: "opens late", at_ms: 1_792_572_900_000 } }));
        }
        return method === "clips.get" ? ok(recorded()) : ok(null);
      },
      `#/onair/clips/${ID}`,
    );
    dispose = m.dispose;
    await settle();
    const form = role("onair-clip-feedback");
    expect(form?.textContent).toContain("Cut too early");
    expect(form?.textContent).not.toContain("cut_too_early");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cut too early" }));
      fireEvent.click(screen.getByRole("button", { name: "Landed" }));
      fireEvent.change(screen.getByLabelText("Note"), { target: { value: "  opens late " } });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save feedback" }));
    });
    await settle();
    expect(m.calls).toContain("clips.feedback");
    expect(sent).toContain(`"id":"${ID}"`);
    expect(sent).toContain('"tags":["cut_too_early","landed"]');
    expect(sent).toContain('"note":"opens late"');
    expect(screen.getByRole("status").textContent).toBe("Saved");
  });

  it("says a clip without a recording is re-drawn", async () => {
    const base = recorded();
    const render = base.render && { ...base.render, footage: "redrawn" as const, files: [{ path: "draft.mp4", kind: "video", bytes: 10 }] };
    const redrawn = recorded(render ? { render } : {});
    delete redrawn.recording;
    const m = mountPage(<Layer2 sub={["clips", ID]} />, (method) => (method === "clips.get" ? ok(redrawn) : ok(null)), `#/onair/clips/${ID}`);
    dispose = m.dispose;
    await settle();
    expect(role("onair-clip-player")?.closest("section")?.textContent).toContain("Re-drawn (no recording)");
    expect(document.querySelector("[data-role='onair-clip-video']")?.getAttribute("src")).toContain("/draft.mp4");
    expect(screen.queryByRole("button", { name: "16:9 broadcast" })).toBeNull();
  });
});
