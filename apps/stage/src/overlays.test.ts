// The vote panel names a ballot the runtime accepts, and the chat lane says what became of each
// `!vote` line: counted for which option, or why the runtime dropped it. The chip decides with the
// runtime-pinned ballot rule (stage-kit `ballots.ts`), never a looser rule of its own.
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { Command, Rank, StreamEvent } from "@shadowrunr/contract";
import { foldEvents } from "@shadowrunr/stage-kit";
import { ChatLane, VotePanel } from "@shadowrunr/stage-kit/ui";

const FOILS = ["the-disruptor", "the-life-coach"];
const start: StreamEvent = {
  type: "stream_start",
  ts_ms: 0,
  stream_id: "s-1",
  character: "malakor",
  tone_variant: "theatrical",
  core_tier: "haiku",
  audience_size: 30,
};
const seg: StreamEvent = { type: "segment_start", ts_ms: 100, segment: "SEG-01" };
const poll = (ts: number, minRank: Rank = "thrall"): StreamEvent => ({
  type: "op",
  ts_ms: ts,
  op: { op: "start_poll", poll_id: "foil-vote-1", question: "Which foil shall the Host summon?", options: FOILS, min_rank: minRank, secs: 60 },
});
const close = (ts: number): StreamEvent => ({ type: "op", ts_ms: ts, op: { op: "close_poll", poll_id: "foil-vote-1" } });

let seq = 0;
/** A `!vote` line, judged by the input filter in its own beat so the lane may draw it. */
function vote(ts: number, option: string, rank: Rank = "thrall"): StreamEvent[] {
  seq += 1;
  const id = `m${seq}`;
  const command: Command = { cmd: "vote", option };
  return [
    {
      type: "chat",
      ts_ms: ts,
      msg: {
        id,
        ts_ms: ts,
        viewer: { id: `v${seq}`, name: `viewer${seq}`, platform: "sim", rank, months: 0, gifted: 0, is_mod: false },
        text: `!vote ${option}`,
        command,
      },
    },
    { type: "moderation", ts_ms: ts, stage: "input", rule: "rate", msg_id: id, action: "warned" },
  ];
}
/** A later event that closes the filter's last beat. */
const later = (ts: number): StreamEvent => ({ type: "segment_start", ts_ms: ts, segment: "SEG-01" });

function lane(events: StreamEvent[], now: number): string {
  return renderToStaticMarkup(createElement(ChatLane, { scene: foldEvents(events), now }));
}
function chips(html: string): string[] {
  return [...html.matchAll(/class="cmd-chip[^"]*">([^<]*)</g)].map((m) => m[1]!.replace(/&#x27;/g, "'"));
}
function panel(events: StreamEvent[], now: number): string {
  const scene = foldEvents(events);
  return renderToStaticMarkup(createElement(VotePanel, { poll: scene.polls["foil-vote-1"]!, now }));
}

describe("the vote panel names a ballot the runtime accepts", () => {
  it("an open poll shows each option's key and the command to type", () => {
    const html = panel([start, seg, poll(1000)], 2000);
    expect(html).toMatch(/<span class="vote-key">1<\/span><span class="vote-option">The Disruptor</);
    expect(html).toMatch(/<span class="vote-key">2<\/span><span class="vote-option">The Life Coach</);
    expect(html).toContain("!vote 1-2");
  });

  it("guard: a closed poll drops the keys and the command; the winner row stays as it was", () => {
    const html = panel([start, seg, poll(1000), ...vote(1500, "2"), close(5000)], 6000);
    expect(html).not.toContain("vote-key");
    expect(html).not.toContain("!vote");
    expect(html).toContain('class="vote-row leading winner"><div class="vote-label"><span class="vote-option">The Life Coach</span>');
  });
});

describe("the chat lane says what became of a vote line", () => {
  it("a display name the runtime does not accept reads 'not an option', never as counted", () => {
    expect(chips(lane([start, seg, poll(1000), ...vote(2000, "The Life Coach"), later(3000)], 3000))).toEqual(["VOTE · not an option"]);
  });

  it("an index or a case-folded option id is counted, and the chip names the option", () => {
    const html = lane([start, seg, poll(1000), ...vote(2000, "2"), ...vote(2500, "THE-LIFE-COACH"), later(3000)], 3000);
    expect(chips(html)).toEqual(["VOTE 2 · The Life Coach", "VOTE 2 · The Life Coach"]);
  });

  it("a voter below the poll's rank reads 'below Warlock'", () => {
    expect(chips(lane([start, seg, poll(1000, "warlock"), ...vote(2000, "1", "thrall"), later(3000)], 3000))).toEqual(["VOTE · below Warlock"]);
  });

  it("a vote with no poll open at its time reads 'no open poll', before and after the poll", () => {
    const html = lane([start, seg, ...vote(500, "1"), poll(1000), close(2000), ...vote(2500, "1"), later(3000)], 3000);
    expect(chips(html)).toEqual(["VOTE · no open poll", "VOTE · no open poll"]);
  });

  it("a vote counted while the poll was open still reads counted after it closes", () => {
    const html = lane([start, seg, poll(1000), ...vote(1500, "1"), close(2000), later(3000)], 3000);
    expect(chips(html)).toEqual(["VOTE 1 · The Disruptor"]);
  });
});
