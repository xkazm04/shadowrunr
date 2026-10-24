// SEG-03, the Table Read's two-shot: the layout, whose mouth moves, and the fixture stretch drawn
// on the canvas (the two halves, the scene card, captions named per speaker, the ballot).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { StreamEvent } from "@shadowrunr/contract";
import { CANVAS_H, CANVAS_W, buildMap, captionAt, foldEvents, isTwoShot, type SceneState } from "@shadowrunr/stage-kit";
import { parseTranscript } from "@shadowrunr/stage-kit/parse";
import { GuestPortrait, guestPortraitFile } from "@shadowrunr/stage-kit/ui";
import { createClock } from "./avatar/motion";
import { RESTING, type FaceSignal } from "./avatar/types";
import { FACE_PX, REGIONS, SCENE_BAND, TWO_SHOT_GUEST, TWO_SHOT_HOST, layoutRects, type Rect, type Region } from "./layouts";
import { standingCommands } from "./OnAir";
import { Stage, chatTitle } from "./Stage";
import { SILENT, speakersNow } from "./twoShot";
import { VoiceCaptions, type SaidLine } from "./voice/captions";
import { learnLines, silentCaption, DeliveryFeed } from "./voice/delivery";
import { parseVoiceFrame, type Utterance } from "./voice/frame";
import { VoicePlayer, type AudioLike, type BufferLike, type SourceLike } from "./voice/player";
import { FaceMixer } from "./voice/useVoice";

const fixture = parseTranscript(readFileSync(join(__dirname, "..", "fixtures", "seg03-table-read.jsonl"), "utf8"));
const events = fixture.events;
const at = (t: number): SceneState => foldEvents(events, t);
const sayAt = (pred: (e: Extract<StreamEvent, { type: "say" }>) => boolean): number => {
  const e = events.find((x): x is Extract<StreamEvent, { type: "say" }> => x.type === "say" && pred(x));
  if (!e) throw new Error("no such line in the fixture");
  return e.ts_ms;
};
const firstSceneGuestLine = sayAt((e) => e.speaker === "foil" && e.ts_ms > 62_000);
const firstSceneHostLine = sayAt((e) => e.speaker === "host" && e.ts_ms > 62_000);

function render(scene: SceneState, now: number, extra: Record<string, unknown> = {}): string {
  return renderToStaticMarkup(
    createElement(Stage, { scene, now, map: buildMap(null, []), caseFile: null, sourceTag: "REPLAY", debug: false, broadcast: true, ...extra }),
  );
}

const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

describe("fixtures/seg03-table-read.jsonl", () => {
  it("parses with no errors and no warnings, and stays in SEG-03 on the two-shot", () => {
    expect(fixture.errors).toEqual([]);
    expect(fixture.warnings).toEqual([]);
    const s = at(firstSceneGuestLine);
    expect(s.segment).toBe("SEG-03");
    expect(isTwoShot(s)).toBe(true);
  });
});

describe("the two-shot layout", () => {
  for (const poll of [false, true]) {
    for (const scene of [false, true]) {
      for (const card of [false, true]) {
        it(`poll=${poll} scene=${scene} card=${card}: inside the canvas, nothing visible overlaps`, () => {
          const r = layoutRects("host", { poll, foil: true, twoShot: true, scene, card });
          const shown = REGIONS.filter((k) => !r[k].hidden && k !== "topbar" && k !== "ticker");
          for (const k of shown) {
            expect(r[k].x, k).toBeGreaterThanOrEqual(0);
            expect(r[k].y, k).toBeGreaterThanOrEqual(0);
            expect(r[k].x + r[k].w, k).toBeLessThanOrEqual(CANVAS_W);
            expect(r[k].y + r[k].h, k).toBeLessThanOrEqual(CANVAS_H);
            expect(r[k].h, k).toBeGreaterThan(40);
          }
          for (let i = 0; i < shown.length; i++) {
            for (let j = i + 1; j < shown.length; j++) expect(overlaps(r[shown[i]!], r[shown[j]!]), `${shown[i]} x ${shown[j]}`).toBe(false);
          }
          // the guest's half replaces the foil card, the decrees and the host room's card spot
          expect(r.foil.hidden).toBe(true);
          expect(r.decrees.hidden).toBe(true);
          expect(r.guest).toEqual(TWO_SHOT_GUEST);
          expect(r.avatar).toEqual(TWO_SHOT_HOST);
          expect(!!r.scene.hidden).toBe(!scene);
          expect(!!r.vote.hidden).toBe(!poll);
          expect(!!r.card.hidden).toBe(!card);
        });
      }
    }
  }

  it("sets the two halves side by side, his face near its full size, the band under both", () => {
    expect(TWO_SHOT_HOST.y).toBe(TWO_SHOT_GUEST.y);
    expect(TWO_SHOT_HOST.h).toBe(TWO_SHOT_GUEST.h);
    expect(TWO_SHOT_HOST.x + TWO_SHOT_HOST.w).toBeLessThan(TWO_SHOT_GUEST.x);
    expect(Math.min(TWO_SHOT_HOST.w, TWO_SHOT_HOST.h) / FACE_PX).toBeGreaterThan(0.95);
    expect(SCENE_BAND.y).toBeGreaterThanOrEqual(TWO_SHOT_HOST.y + TWO_SHOT_HOST.h);
    expect(SCENE_BAND.x + SCENE_BAND.w).toBe(TWO_SHOT_GUEST.x + TWO_SHOT_GUEST.w);
  });

  it("is only the Table Read's: other host rooms and the content screens keep the guest hidden", () => {
    for (const layout of ["boot", "host", "board", "hexmap", "browser", "desk"] as const) {
      const r = layoutRects(layout, { poll: true, foil: true });
      for (const k of ["guest", "scene"] as Region[]) expect(r[k].hidden, `${layout} ${k}`).toBe(true);
    }
    // the content screens ignore the flag
    expect(layoutRects("board", { poll: false, foil: false, twoShot: true }).guest.hidden).toBe(true);
  });
});

describe("whose mouth moves", () => {
  it("a guest caption lights the guest, never the host; a host caption the reverse", () => {
    const g = captionAt(at(firstSceneGuestLine + 200), firstSceneGuestLine + 200);
    expect(g?.foil).toBe("the-painter");
    expect(speakersNow(g, undefined, firstSceneGuestLine + 200)).toEqual({ host: false, guest: "the-painter" });
    const h = captionAt(at(firstSceneHostLine + 200), firstSceneHostLine + 200);
    expect(speakersNow(h, undefined, firstSceneHostLine + 200)).toEqual({ host: true, guest: null });
    expect(speakersNow(null, undefined, 0)).toBe(SILENT);
    expect(speakersNow(g, null, firstSceneGuestLine + 200)).toBe(SILENT);
  });

  it("a guest utterance does not drive Malakor's mouth, and lights the guest's speaking glow", () => {
    const audio = new FakeAudio();
    const clock = handClock();
    const captions = new VoiceCaptions();
    const player = new VoicePlayer(audio, clock.clock, { scheduled: (p) => captions.scheduled(p) });
    const mixer = new FaceMixer();
    mixer.setVoice(player);
    const faces: FaceSignal[] = [];
    mixer.subscribe((s) => faces.push(s));

    // the frame guard keeps who speaks
    const frame = parseVoiceFrame(JSON.stringify(utt(1, 1000, { speaker: "foil", foil: "the-painter", text: "They are not empty. They are waiting." })));
    expect(frame.ok && frame.value.type === "utterance" && [frame.value.speaker, frame.value.foil]).toEqual(["foil", "the-painter"]);
    const guestUtt = frame.ok && frame.value.type === "utterance" ? frame.value : null;
    expect(player.accept(guestUtt!)).toBe("scheduled");
    expect(player.accept(utt(2, 1000, { text: "So never. Got it." }))).toBe("scheduled");
    const [g, h] = player.scheduled();

    // mid-way through the guest's audio
    audio.currentTime = g!.start + 0.5;
    clock.tick();
    expect(player.signalAt(audio.currentTime)).toBe(RESTING);
    expect(player.foilAt(audio.currentTime)).toBe("the-painter");
    expect(mixer.current()).toBe(RESTING);
    expect(faces.every((f) => f.phase === "idle")).toBe(true);
    const said: SaidLine[] = [
      { id: 1, seq: 1, text: "They are not empty. They are waiting.", at: 0, replyingTo: [], foil: "the-painter" },
      { id: 2, seq: 2, text: "So never. Got it.", at: 0, replyingTo: [] },
    ];
    const guestView = captions.view("r", said, audio.currentTime, 0);
    expect(guestView).toMatchObject({ voiced: true, speaking: true, caption: { foil: "the-painter" } });
    expect(speakersNow(null, guestView, 0)).toEqual({ host: false, guest: "the-painter" });
    // on the canvas: the guest's glow is lit, his plate is not, the caption names the guest
    const scene = at(firstSceneGuestLine);
    const html = render(scene, firstSceneGuestLine, { voiceCaption: guestView });
    expect(html).toMatch(/class="guest-shot speaking"/);
    expect(html).toMatch(/class="host-plate"/);
    expect(html).toContain('data-speaker="the-painter"');
    expect(html).toContain("THE PAINTER");

    // then his own line: his mouth moves, the guest rests
    audio.currentTime = h!.start + 0.5;
    clock.tick();
    expect(player.foilAt(audio.currentTime)).toBeNull();
    expect(mixer.current().phase).toBe("speaking");
    const hostView = captions.view("r", said, audio.currentTime, 0);
    expect(speakersNow(null, hostView, 0)).toEqual({ host: true, guest: null });
    const html2 = render(scene, firstSceneGuestLine, { voiceCaption: hostView });
    expect(html2).toMatch(/class="guest-shot"/);
    expect(html2).toMatch(/class="host-plate speaking"/);
    expect(html2).toContain('data-speaker="host"');
  });

  it("a guest's line cues nothing on his face", () => {
    const cues: unknown[] = [];
    const feed = new DeliveryFeed({ cue: (...a: unknown[]) => void cues.push(a) });
    learnLines(feed, { runId: "r", said: [{ id: 1, seq: 7, text: "x", at: 0, replyingTo: [], foil: "the-painter" }], ring: { first: 0, end: 0, at: () => undefined } });
    expect(feed.knows("r", 7)).toBe(false);
    expect(feed.started({ runId: "r", seq: 7, part: 0 })).toBe(false);
    expect(silentCaption(at(firstSceneGuestLine + 200), firstSceneGuestLine + 200, undefined)).toBeNull();
    expect(silentCaption(at(firstSceneHostLine + 200), firstSceneHostLine + 200, undefined)).not.toBeNull();
    expect(cues).toEqual([]);
  });
});

describe("the SEG-03 stretch on the canvas", () => {
  it("draws the two halves with their plates and the scene card while a scene plays", () => {
    const html = render(at(firstSceneGuestLine + 300), firstSceneGuestLine + 300);
    expect(html).toContain('data-role="guest-shot"');
    expect(html).toContain('data-guest="the-painter"');
    expect(html).toMatch(/class="guest-shot speaking"/);
    expect(html).toContain('data-role="host-plate"');
    expect(html).toContain('data-role="scene-card"');
    expect(html).toContain("The Gallery");
    expect(html).toMatch(/starring <b>The Painter<\/b>/);
    expect(html).toContain("THE PAINTER");
  });

  it("names the host under his own lines", () => {
    const html = render(at(firstSceneHostLine + 300), firstSceneHostLine + 300);
    expect(html).toContain('data-speaker="host"');
    expect(html).toContain("MALAKOR");
    expect(html).toMatch(/class="host-plate speaking"/);
    expect(html).not.toMatch(/guest-shot speaking/);
  });

  it("clears the scene card after scene_end, and keeps the guest for the free talk", () => {
    const end = events.find((e) => e.type === "scene_end")!.ts_ms;
    const html = render(at(end + 2000), end + 2000);
    expect(html).not.toContain('data-role="scene-card"');
    expect(html).toContain('data-guest="the-painter"');
  });

  it("draws the scene ballot in the vote panel, with the ballot keys", () => {
    const html = render(at(30_000), 30_000);
    expect(html).toContain("Which scene do we read first?");
    for (const t of ["The Gallery", "The Critic", "The Commission"]) expect(html).toContain(t);
    expect(html).toContain('class="vote-key">1<');
    expect(html).toContain("The audience");
  });

  it("offers !vote as the segment's standing command", () => {
    expect(standingCommands("SEG-03").map((c) => c.cmd)).toEqual(["!vote"]);
    expect(chatTitle({ segment: "SEG-03" })).toBe("The audience");
    const html = render(at(5000), 5000);
    expect(html).toContain("The Table Read");
    expect((html.match(/!vote/g) ?? []).length).toBe(1);
  });

  it("draws a raster portrait over the SVG only once it has loaded", () => {
    expect(guestPortraitFile("the-painter")).toBe("guest-the-painter.png");
    const html = renderToStaticMarkup(createElement(GuestPortrait, { id: "the-painter", src: "http://127.0.0.1:7717/avatar/guest-the-painter.png" }));
    expect(html).toContain('data-portrait="svg"');
    expect(html).toContain('class="guest-image"');
    expect(html).toContain("<svg");
    expect(renderToStaticMarkup(createElement(GuestPortrait, { id: "the-painter" }))).not.toContain("<img");
  });
});

// ── A fake AudioContext and frame clock (as voice/player.test.ts) ─────────────────────────────

class FakeAudio implements AudioLike {
  currentTime = 10;
  readonly output = {};
  createBuffer(_ch: number, length: number, sampleRate: number): BufferLike {
    const data = new Float32Array(length);
    return { duration: length / sampleRate, getChannelData: () => data };
  }
  createBufferSource(): SourceLike {
    return { buffer: null, connect: () => undefined, start: () => undefined, stop: () => undefined, disconnect: () => undefined };
  }
  level() {
    return 0.5;
  }
}

function handClock() {
  let pending: ((t: number) => void) | null = null;
  let now = 0;
  const clock = createClock({ raf: (cb) => ((pending = cb), 1), caf: () => (pending = null), hidden: () => false, onVisible: () => {} });
  return {
    clock,
    tick() {
      const cb = pending;
      pending = null;
      now += 16;
      cb?.(now);
    },
  };
}

function pcm(n: number): string {
  let bin = "";
  for (let i = 0; i < n; i++) bin += String.fromCharCode(1000 & 0xff, (1000 >> 8) & 0xff);
  return btoa(bin);
}

function utt(seq: number, ms: number, extra: Partial<Utterance> = {}): Utterance {
  return { type: "utterance", id: `r:${seq}:0`, run_id: "r", seq, part: 0, last: true, text: "x", sample_rate: 8000, duration_ms: ms, pcm: pcm(ms * 8), ...extra };
}
