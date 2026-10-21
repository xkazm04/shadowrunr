// Captions on the audio clock: words follow each part's real start and length, and a line that
// will never sound shows at once on the estimate, briefly, instead of waiting for audio.
import { describe, expect, it } from "vitest";

import { CAPTION_HOLD_MS, visibleWords, wordTimings } from "@shadowrunr/stage-kit";
import { FADE_MS, UNVOICED_HOLD_MS, VOICE_WAIT_S, VoiceCaptions, type SaidLine } from "./captions";
import type { ScheduledPart } from "./player";

const TEXT = "Kneel, mortal. The ledger is sealed.";
const line = (seq: number, text = TEXT, at = 0): SaidLine => ({ id: seq * 10, seq, text, at, replyingTo: [] });
const part = (seq: number, n: number, text: string, start: number, duration: number, last = false): ScheduledPart => ({
  runId: "r",
  seq,
  part: n,
  text,
  last,
  start,
  duration,
});

/** A line said at stream 0 whose two parts play from audio 10 s: 1 s, then 2 s. */
function voiced() {
  const c = new VoiceCaptions();
  const said = [line(1)];
  c.view("r", said, 5, 0);
  c.scheduled(part(1, 0, "Kneel, mortal.", 10, 1));
  c.scheduled(part(1, 1, "The ledger is sealed.", 11, 2, true));
  return { c, said, at: (now: number) => c.view("r", said, now, 1000) };
}

describe("captions follow the audio", () => {
  it("nothing shows before the line's audio starts, where the estimate would have shown it all", () => {
    const { at } = voiced();
    expect(at(9.9)).toBeNull();
    // the estimate, from when it was said, would be fully out by now
    const est = wordTimings(TEXT);
    expect(visibleWords(est, 9900)).toBe(6);
  });

  it("each part reveals its own words over its real duration", () => {
    const { at } = voiced();
    expect(at(10)?.shown).toBe(1);
    expect(at(10.5)?.shown).toBe(2);
    expect(at(11)?.shown).toBe(3);
    expect(at(12)?.shown).toBe(5);
    expect(at(12.99)?.shown).toBe(6);
    expect(at(12.5)).toMatchObject({ speaking: true, voiced: true, opacity: 1 });
  });

  it("holds the full hold after the audio ends, then fades", () => {
    const { at } = voiced();
    expect(at(13)).toMatchObject({ shown: 6, speaking: false, opacity: 1 });
    expect(at(13 + CAPTION_HOLD_MS / 1000)?.opacity).toBe(1);
    expect(at(13 + (CAPTION_HOLD_MS + FADE_MS / 2) / 1000)?.opacity).toBeCloseTo(0.5, 5);
    expect(at(13 + (CAPTION_HOLD_MS + FADE_MS) / 1000)).toBeNull();
  });

  it("a part moved by the stage's cap moves its words with it", () => {
    const { c, at } = voiced();
    c.scheduled(part(1, 1, "The ledger is sealed.", 12, 2, true));
    expect(at(11.5)?.shown).toBe(2);
    expect(at(12)?.shown).toBe(3);
  });

  it("a gap before the last part does not finish the line early", () => {
    const { c, at } = voiced();
    c.scheduled(part(1, 1, "The ledger is sealed.", 12, 2, true));
    expect(at(11.5)).toMatchObject({ shown: 2, speaking: true });
  });

  it("a line whose part texts do not split the caption evenly still ends whole", () => {
    const c = new VoiceCaptions();
    const said = [line(1, "One two three four five")];
    c.view("r", said, 0, 0);
    c.scheduled(part(1, 0, "One two", 1, 1));
    c.scheduled(part(1, 1, "three-four five", 2, 1, true));
    expect(c.view("r", said, 2.001, 0)?.shown).toBe(3);
    expect(c.view("r", said, 3, 0)?.shown).toBe(5);
  });
});

describe("the unvoiced-line rule", () => {
  const spoken = wordTimings(TEXT).spokenMs;

  it("a skipped line shows at once on the estimated timing, and holds only briefly", () => {
    const c = new VoiceCaptions();
    const said = [line(1)];
    c.view("r", said, 5, 0);
    c.unvoiced("r", 1, 6);
    expect(c.view("r", said, 5.9, 0)).toBeNull();
    expect(c.view("r", said, 6, 0)).toMatchObject({ shown: 1, voiced: false, speaking: true, opacity: 1 });
    const out = 6 + spoken / 1000;
    expect(c.view("r", said, out, 0)).toMatchObject({ shown: 6, speaking: false, opacity: 1 });
    expect(c.view("r", said, out + UNVOICED_HOLD_MS / 1000, 0)?.opacity).toBe(1);
    expect(c.view("r", said, out + (UNVOICED_HOLD_MS + FADE_MS) / 1000, 0)).toBeNull();
    expect(UNVOICED_HOLD_MS).toBeLessThan(CAPTION_HOLD_MS);
  });

  it("a line the stage dropped loses its waiting parts and shows as unvoiced", () => {
    const c = new VoiceCaptions();
    const said = [line(2)];
    c.view("r", said, 5, 0);
    c.scheduled(part(2, 0, TEXT, 12, 3, true));
    c.unvoiced("r", 2, 6);
    expect(c.view("r", said, 6.1, 0)?.voiced).toBe(false);
    // its parts never play: by the time they would have, the brief unvoiced caption is gone
    expect(c.view("r", said, 12.5, 0)).toBeNull();
  });

  it("no audio and no skip within VOICE_WAIT_S: the line counts as unvoiced", () => {
    const c = new VoiceCaptions();
    const said = [line(3)];
    expect(c.view("r", said, 5, 0)).toBeNull();
    expect(c.view("r", said, 5 + VOICE_WAIT_S - 0.01, 0)).toBeNull();
    expect(c.view("r", said, 5 + VOICE_WAIT_S, 0)).toMatchObject({ voiced: false, shown: 1 });
  });

  it("a line partly spoken and then skipped reveals the rest at once", () => {
    const { c, at } = voiced();
    c.unvoiced("r", 1, 10.5);
    expect(at(10.6)).toMatchObject({ shown: 6, voiced: true });
  });

  it("a sounding line keeps the slot; the unvoiced one said after it yields", () => {
    const { c, said, at } = voiced();
    said.push(line(2, "Ignored line."));
    c.view("r", said, 9, 1000);
    c.unvoiced("r", 2, 11);
    expect(at(11.5)?.caption.id).toBe(10);
    // once the voiced line has finished speaking, the newer unvoiced line may show
    expect(at(13.1)?.caption.id).toBe(20);
  });

  it("the backlog on joining a run is history: never shown, even if audio comes", () => {
    const c = new VoiceCaptions();
    const said = [line(1, TEXT, 0)];
    expect(c.view("r", said, 5, (VOICE_WAIT_S + 1) * 1000)).toBeNull();
    c.scheduled(part(1, 0, TEXT, 6, 2, true));
    expect(c.view("r", said, 7, (VOICE_WAIT_S + 3) * 1000)).toBeNull();
    expect(c.view("r", said, 60, (VOICE_WAIT_S + 60) * 1000)).toBeNull();
  });

  it("a new run forgets the old one", () => {
    const { c } = voiced();
    expect(c.view("s", [line(1)], 10.5, 0)).toBeNull();
  });
});
