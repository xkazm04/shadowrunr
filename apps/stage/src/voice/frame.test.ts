// `/voice` frames: the typed guard, the PCM decode and the mouth shape at a playback time.
import { describe, expect, it } from "vitest";

import { decodePcm16, parseVoiceFrame, visemeAt } from "./frame";

/** PCM16LE samples as base64. */
function pcm(samples: readonly number[]): string {
  const bytes = new Uint8Array(samples.length * 2);
  const view = new DataView(bytes.buffer);
  samples.forEach((s, i) => view.setInt16(2 * i, s, true));
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

const utterance = {
  type: "utterance",
  id: "r:4:0",
  run_id: "r",
  seq: 4,
  part: 0,
  last: false,
  text: "Kneel.",
  sample_rate: 24000,
  duration_ms: 400,
  pcm: pcm([0, 1, -1]),
  visemes: [
    { v: "kk", start_ms: 0, end_ms: 80 },
    { v: "aa", start_ms: 100, end_ms: 300 },
  ],
};

describe("parseVoiceFrame", () => {
  it("reads the three frame kinds and omits absent optional keys", () => {
    expect(parseVoiceFrame('{"type":"status","state":"off","visemes":false}')).toEqual({
      ok: true,
      value: { type: "status", state: "off", visemes: false },
    });
    const s = parseVoiceFrame('{"type":"status","state":"degraded","reason":"engine missing","voice":"af_heart","visemes":true}');
    expect(s.ok && s.value).toEqual({ type: "status", state: "degraded", reason: "engine missing", voice: "af_heart", visemes: true });
    const u = parseVoiceFrame(JSON.stringify(utterance));
    expect(u.ok && u.value).toEqual(utterance);
    const k = parseVoiceFrame('{"type":"skipped","run_id":"r","seq":7,"part":0,"reason":"queue_full"}');
    expect(k.ok && k.value).toEqual({ type: "skipped", run_id: "r", seq: 7, part: 0, reason: "queue_full" });
  });

  it("an utterance with no mouth shapes has no visemes key", () => {
    const { visemes: _drop, ...bare } = utterance;
    const u = parseVoiceFrame(JSON.stringify(bare));
    expect(u.ok && "visemes" in u.value).toBe(false);
  });

  it.each([
    ["not JSON", "{", /not JSON/],
    ["an array", "[]", /not an object/],
    ["an unknown type", '{"type":"hello"}', /unknown voice frame type hello/],
    ["an unknown state", '{"type":"status","state":"sleepy","visemes":false}', /bad state/],
    ["a negative seq", JSON.stringify({ ...utterance, seq: -1 }), /bad seq/],
    ["a silly sample rate", JSON.stringify({ ...utterance, sample_rate: 12 }), /bad sample_rate/],
    ["an unknown viseme", JSON.stringify({ ...utterance, visemes: [{ v: "zz", start_ms: 0, end_ms: 1 }] }), /bad visemes/],
    ["an unknown skip reason", '{"type":"skipped","run_id":"r","seq":1,"part":0,"reason":"bored"}', /bad reason/],
  ])("refuses %s", (_what, text, why) => {
    const r = parseVoiceFrame(text);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.message).toMatch(why);
  });
});

describe("decodePcm16", () => {
  it("decodes little-endian signed samples into -1..1", () => {
    const out = decodePcm16(pcm([0, 16384, -16384, 32767, -32768]));
    expect([...out]).toEqual([0, 0.5, -0.5, 32767 / 32768, -1]);
  });

  it("ignores a trailing odd byte and decodes nothing to nothing", () => {
    expect(decodePcm16(btoa("\x00\x40\x01")).length).toBe(1);
    expect(decodePcm16("").length).toBe(0);
  });
});

describe("visemeAt", () => {
  const spans = utterance.visemes as Parameters<typeof visemeAt>[0];
  it("is the span covering the time, sil in a gap or past the ends", () => {
    expect(visemeAt(spans, 0)).toBe("kk");
    expect(visemeAt(spans, 79)).toBe("kk");
    expect(visemeAt(spans, 90)).toBe("sil");
    expect(visemeAt(spans, 100)).toBe("aa");
    expect(visemeAt(spans, 299)).toBe("aa");
    expect(visemeAt(spans, 300)).toBe("sil");
    expect(visemeAt(spans, -5)).toBe("sil");
  });

  it("is null when the utterance has no shapes (the mouth follows the level)", () => {
    expect(visemeAt(undefined, 50)).toBeNull();
    expect(visemeAt([], 50)).toBeNull();
  });
});
