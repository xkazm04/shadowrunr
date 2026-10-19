// Clip mode's input and its pure view model (ClipStage renders from these).
import { describe, expect, it } from "vitest";

import fixture from "../fixtures/clip-demo.json";
import { captionAt, headerModel, parseClip, posterMs, readClipParams, shortSource, speakerLabel } from "./clip";

const ok = (raw: unknown) => {
  const r = parseClip(raw);
  if (!r.ok) throw new Error(r.message);
  return r;
};

describe("parseClip", () => {
  it("reads the fixture: size, captions, events, case and header", () => {
    const { clip, dropped } = ok(fixture);
    expect(dropped).toEqual([]);
    expect([clip.width, clip.height, clip.duration_ms]).toEqual([1080, 1920, 16000]);
    expect(clip.captions).toHaveLength(4);
    expect(clip.events.map((e) => e.type)).toEqual(["segment_start", "case_open", "op", "op", "op"]);
    expect(clip.case?.fictional).toBe(true);
    expect(clip.header).toMatchObject({ alleged: true, grade: "REP", fictional: true });
  });

  it("accepts the JSON text as well as the decoded value", () => {
    expect(ok(JSON.stringify(fixture)).clip.id).toBe("clip-00000000de000001");
  });

  it("refuses what cannot be drawn: not JSON, a landscape size, no duration, no captions", () => {
    expect(parseClip("{nope").ok).toBe(false);
    expect(parseClip({ ...fixture, width: 1920, height: 1080 }).ok).toBe(false);
    expect(parseClip({ ...fixture, duration_ms: 0 }).ok).toBe(false);
    expect(parseClip({ ...fixture, captions: undefined }).ok).toBe(false);
    expect(parseClip([]).ok).toBe(false);
  });

  it("drops a malformed caption or event, and any chat, with a note", () => {
    const chat = {
      type: "chat",
      ts_ms: 1,
      msg: { id: "m1", ts_ms: 1, viewer: { id: "v", name: "someone", platform: "sim", rank: "thrall" }, text: "hi" },
    };
    const r = ok({
      ...fixture,
      captions: [...fixture.captions, { start_ms: 5, end_ms: 1, text: "backwards" }, { text: 3 }],
      events: [...fixture.events, { type: "nonsense" }, chat],
    });
    expect(r.clip.captions).toHaveLength(4);
    expect(r.clip.events.some((e) => e.type === "chat")).toBe(false);
    expect(r.dropped).toEqual(["caption 4", "caption 5", "event 5", "event 6"]);
  });

  it("keeps an unknown grade as UNGRADED and at most three citations", () => {
    const r = ok({ ...fixture, header: { ...fixture.header, grade: "XYZ", citation: ["a", "b", "c", "d"] } });
    expect(r.clip.header?.grade).toBe("UNGRADED");
    expect(r.clip.header?.citation).toEqual(["a", "b", "c"]);
  });
});

describe("captions on the clip clock", () => {
  const { clip } = ok(fixture);

  it("shows nothing before the first word and between cues", () => {
    expect(captionAt(clip.captions, -1)).toBeNull();
    expect(captionAt(clip.captions, 2700)).toBeNull();
  });

  it("reveals the exact text word by word and fully by the cue's end", () => {
    const early = captionAt(clip.captions, 2900)!;
    expect(early.caption.text).toBe("Four percent a week, minion, is not a yield. It is a confession with a rounding error.");
    expect(early.words).toBe(1);
    const late = captionAt(clip.captions, 8300)!;
    expect(late.words).toBe(late.total);
  });

  it("labels the speakers", () => {
    expect(speakerLabel("malakor")).toBe("Arch-Lich Malakor");
    expect(speakerLabel("viewer")).toBe("A minion asks");
    expect(speakerLabel("the-inspector")).toBe("The Inspector");
  });

  it("puts the poster frame mid-way through the longest Malakor line", () => {
    expect(posterMs(clip)).toBe((8550 + 13600) / 2);
  });
});

describe("header model", () => {
  const { clip } = ok(fixture);

  it("is the ALLEGED block with grade and fiction flag for a claim-bearing clip", () => {
    expect(headerModel(clip, null)).toEqual({
      kind: "alleged",
      grade: "REP",
      caseTitle: "The Glimmerquay Lunar Staking Affair (FICTIONAL DEMO CASE)",
      fictional: true,
    });
  });

  it("is the segment title and the foil without a header", () => {
    const seg01 = { ...clip, header: null, segment: "SEG-01" };
    expect(headerModel(seg01, "the-life-coach")).toEqual({ kind: "segment", title: "The Improvised Room", foil: "The Life Coach" });
    expect(headerModel({ ...seg01, segment: "SEG-99", title: "A title" }, null)).toEqual({ kind: "segment", title: "A title", foil: null });
  });
});

describe("params and sources", () => {
  it("reads ?clip= and record", () => {
    expect(readClipParams("?clip=/x.json&record=1")).toEqual({ clip: "/x.json", record: true });
    expect(readClipParams("?clip=/x.json&record=0")).toEqual({ clip: "/x.json", record: false });
    expect(readClipParams("")).toEqual({ clip: null, record: false });
  });

  it("shortens a source for the strip", () => {
    expect(shortSource("https://news.example/glimmerquay-yield-promise")).toBe("news.example/glimmerquay-yield-promise");
    expect(shortSource(`https://a.example/${"x".repeat(80)}`)).toHaveLength(56);
  });
});

describe("captionPage", () => {
  const words = "one two three four five six seven eight nine ten".split(" ");
  it("pages a long line so every word is shown in turn", async () => {
    const { captionPage } = await import("./clip");
    expect(captionPage(words, 0, 13)).toEqual({ from: 0, to: 3 });
    expect(captionPage(words, 3, 13)).toEqual({ from: 3, to: 6 });
    expect(captionPage(words, 9, 13)).toEqual({ from: 8, to: 10 });
    expect(captionPage(words, 0, 500)).toEqual({ from: 0, to: 10 });
    const seen = new Set<number>();
    for (let at = 0; at < words.length; at++) {
      const p = captionPage(words, at, 13);
      expect(at >= p.from && at < p.to).toBe(true);
      for (let i = p.from; i < p.to; i++) seen.add(i);
    }
    expect(seen.size).toBe(words.length);
  });
});
