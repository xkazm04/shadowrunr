import { foldEvents } from "@shadowrunr/stage-kit";
import type { StreamEvent } from "@shadowrunr/contract";
import { describe, expect, it } from "vitest";
import { CHAT_ROWS, LIFE_MS, captionView, captionWords, cardView, chatRows, decreeView, lifeOf, slateView, voteView } from "./view";
import { beatsOf, clock, countsOf } from "./beats";
import { transcript } from "./testdata";

const viewer = (name: string) => ({ id: name, name, platform: "sim" as const, rank: "warlock" as const });

const POLL: StreamEvent[] = [
  { type: "op", ts_ms: 0, op: { op: "start_poll", poll_id: "p1", question: "Which foil?", options: ["Summon the Disruptor", "Summon the Life Coach"], min_rank: "thrall", secs: 24 } },
  { type: "vote", ts_ms: 100, poll_id: "p1", closed: false, tallies: [{ option: "Summon the Disruptor", voters: 2, weight: 2 }, { option: "Summon the Life Coach", voters: 2, weight: 4 }] },
];

describe("lifeOf", () => {
  it("is live within its wall-clock life, scaled by the replay speed", () => {
    expect(lifeOf(0, 6999 * 4, 4, LIFE_MS.card).live).toBe(true);
    expect(lifeOf(0, 7000 * 4, 4, LIFE_MS.card).live).toBe(false);
    expect(lifeOf(1000, 3000, 1, 7000).delayMs).toBe(-2000);
  });
});

describe("voteView", () => {
  it("uses the winner's scale and strips 'Summon '", () => {
    const v = voteView(foldEvents(POLL), 100, 1);
    expect(v?.rows.map((r) => r.name)).toEqual(["the Disruptor", "the Life Coach"]);
    expect(v?.rows.map((r) => r.weight)).toEqual(["×2", "×4"]);
    // tot = max(1, 4, 6 * .55 = 3.3) = 4
    expect(v?.rows.map((r) => r.pct)).toEqual([50, 100]);
    expect(v?.rows.some((r) => r.win)).toBe(false);
  });

  it("names the winner once closed, and clears five seconds later", () => {
    const closed = foldEvents([...POLL, { type: "op", ts_ms: 200, op: { op: "close_poll", poll_id: "p1" } }]);
    expect(voteView(closed, 200, 1)?.rows.map((r) => r.win)).toEqual([false, true]);
    expect(voteView(closed, 200 + LIFE_MS.pollAfterClose, 1)).toBeNull();
  });
});

describe("captions", () => {
  it("truncates at 190 characters and marks stage directions", () => {
    expect(captionWords("x".repeat(200)).map((w) => w.text).join("")).toHaveLength(188);
    expect(captionWords("Fine. *deep sigh* Moving on.").map((w) => w.aside)).toEqual([false, true, true, false, false]);
  });

  it("prefers the newer of Malakor's line and the foil's", () => {
    const s = foldEvents([
      { type: "say", ts_ms: 0, speaker: "malakor", text: "Welcome." },
      { type: "say", ts_ms: 10, speaker: "foil", foil: "the-life-coach", text: "Growth!" },
    ]);
    const c = captionView(s, 10, 1);
    expect(c?.foil).toBe(true);
    expect(c?.name).toBe("The Life Coach");
    expect(c?.talkSecs).toBe(3);
  });
});

describe("chat, decree, card and slate", () => {
  it("keeps the last six lines, marks picked and redacted ones", () => {
    const s = foldEvents([
      ...Array.from({ length: 9 }, (_, i): StreamEvent => ({ type: "chat", ts_ms: i, msg: { id: `m${i}`, ts_ms: i, text: i === 7 ? "!vote 1" : `hi ${i}`, viewer: viewer(`v${i}`) } })),
      { type: "picked", ts_ms: 10, msg_ids: ["m8"], reason: "top" },
      { type: "moderation", ts_ms: 11, stage: "input", rule: "prompt_injection", msg_id: "m6", action: "tagged" },
    ]);
    const rows = chatRows(s);
    expect(rows).toHaveLength(CHAT_ROWS);
    expect(rows.find((r) => r.id === "m8")?.picked).toBe(true);
    expect(rows.find((r) => r.id === "m6")?.redacted).toBe(true);
    expect(rows.find((r) => r.id === "m7")?.command).toBe(true);
    expect(decreeView(s, 10, 1)?.value).toBe("hi 8");
    expect(decreeView(s, 10 + LIFE_MS.decree, 1)).toBeNull();
  });

  it("shows a card for seven seconds and the sign-off slate after", () => {
    const s = foldEvents([
      { type: "op", ts_ms: 0, op: { op: "show_card", card_id: "c", title: "T", body: "b".repeat(200) } },
      { type: "op", ts_ms: 5, op: { op: "sign_off", reason: "End of night." } },
    ]);
    expect(cardView(s, 0, 1)?.value.body).toHaveLength(148);
    expect(cardView(s, 7000, 1)).toBeNull();
    expect(slateView(s, false)).toBe("End of night.");
  });
});

describe("beats", () => {
  it("lists key ops, handovers and segment starts with their transcript line", () => {
    const ev = transcript(2000);
    const beats = beatsOf(ev);
    expect(beats[0]).toMatchObject({ line: 2, kind: "segment start", detail: "SEG-01" });
    expect(beats.some((b) => b.kind === "show card")).toBe(true);
    expect(beats.every((b) => b.kind !== "mark moment")).toBe(true);
    expect(countsOf(ev).events).toBe(2000);
    expect(clock(61_000)).toBe("1:01");
    expect(clock(3_661_000)).toBe("1:01:01");
  });
});
