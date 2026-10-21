// The broadcast canvas on camera: no production detail, and no empty slots where the stage has
// something honest to show.
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { StreamEvent } from "@shadowrunr/contract";
import { buildMap, foldEvents } from "@shadowrunr/stage-kit";
import { DECREE_FADE_MS, DECREE_MIN_SHOW_MS } from "@shadowrunr/stage-kit";
import { CARD_MS, DECREE_MS, Decrees } from "@shadowrunr/stage-kit/ui";
import { TICKER_LINGER_MS, tickerActive } from "./OnAir";
import { Stage } from "./Stage";

const REASON = "deterministic top 1 of 2: m00015-2 mention+first_contact+mockable (4.20)";
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
const chat: StreamEvent = {
  type: "chat",
  ts_ms: 1500,
  msg: {
    id: "m00015",
    ts_ms: 1500,
    viewer: { id: "v-1", name: "MossyTombstone", platform: "sim", rank: "thrall", months: 0, gifted: 0, is_mod: false },
    text: "hi?? first time, what is this place",
  },
};
const picked: StreamEvent = { type: "picked", ts_ms: 6000, msg_ids: ["m00015"], reason: REASON };
const moderation: StreamEvent = { type: "moderation", ts_ms: 7000, stage: "output", rule: "foil-blocked-proper-noun", action: "line withheld before speech" };
const card: StreamEvent = { type: "op", ts_ms: 20_000, op: { op: "show_card", card_id: "c-1", title: "The Doom Funnel", body: "Hype -> Rocket -> Bankruptcy" } };
const job = (ts: number, status: string): StreamEvent => ({ type: "job", ts_ms: ts, job_id: "mem-consolidate-1", status, summary: "14 minion dossiers updated" });

function render(events: StreamEvent[], now: number, broadcast = true, extra: Record<string, unknown> = {}): string {
  return renderToStaticMarkup(
    createElement(Stage, {
      scene: foldEvents(events),
      now,
      map: buildMap(null, []),
      caseFile: null,
      sourceTag: "LIVE",
      debug: false,
      broadcast,
      ...extra,
    }),
  );
}

describe("the caption panel", () => {
  const say: StreamEvent = { type: "say", ts_ms: 1000, speaker: "malakor", text: "Kneel, mortal. The ledger is sealed." };
  const revealed = (html: string) => (html.match(/class="w in/g) ?? []).length;

  it("with the voice off it reveals on the estimate, as before", () => {
    expect(revealed(render([start, seg, say], 1000))).toBe(1);
    expect(revealed(render([start, seg, say], 4500))).toBe(6);
  });

  it("following the voice it shows what the audio clock gives, and nothing while the audio waits", () => {
    const view = { caption: { id: 3, text: "Kneel, mortal. The ledger is sealed.", replyingTo: [] }, shown: 2, opacity: 1, speaking: true, voiced: true };
    expect(revealed(render([start, seg, say], 9000, true, { voiceCaption: view }))).toBe(2);
    const waiting = render([start, seg, say], 9000, true, { voiceCaption: null });
    expect(waiting).not.toContain("Kneel");
  });
});

describe("no production detail on air", () => {
  it("a picked line's card never shows the picker's reason in broadcast", () => {
    const html = render([start, seg, chat, picked], 6500);
    expect(html).toContain("HEARD BY THE LICH");
    expect(html).toContain("hi?? first time");
    expect(html).not.toContain("decree-reason");
    expect(html).not.toContain("deterministic");
    expect(html).not.toContain("m00015");
  });

  it("the operator's own view keeps the reason", () => {
    expect(render([start, seg, chat, picked], 6500, false)).toContain(REASON);
  });

  it("a redaction shows that it happened, not the rule's id", () => {
    const html = render([start, seg, moderation], 7500);
    expect(html).toContain("REDACTED BY THE LORD");
    expect(html).not.toContain("foil-blocked-proper-noun");
  });

  it("job ids never print on air; the operator view keeps them", () => {
    const events = [start, seg, job(10_000, "running")];
    expect(render(events, 11_000)).not.toContain("mem-consolidate-1");
    expect(render(events, 11_000)).toContain("14 minion dossiers updated");
    expect(render(events, 11_000, false)).toContain("mem-consolidate-1");
  });

  it("the top bar names the segment by its title, without its id, on air", () => {
    const html = render([start, seg], 1000);
    expect(html).toContain("The Improvised Room");
    expect(html).not.toContain("SEG-01");
    expect(render([start, seg], 1000, false)).toContain("SEG-01 · The Improvised Room");
  });

  it("the seal toast names the segment's title and leaves out the score on air", () => {
    const sealed: StreamEvent = { type: "segment_end", ts_ms: 2000, segment: "SEG-01", ses: 0.42 };
    const html = render([start, seg, sealed], 2500);
    expect(html).toMatch(/seal-seg">The Improvised Room<\/span> sealed/);
    expect(html).not.toContain("SEG-01");
    expect(html).not.toContain("SES");
    expect(render([start, seg, sealed], 2500, false)).toContain("SES");
  });
});

describe("no empty slots", () => {
  it("between explainer cards the card slot shows the segment and what the room can do", () => {
    const idle = render([start, seg], 10_000);
    expect(idle).toContain("The Improvised Room");
    expect(idle).toContain("!propose");
    expect(idle).toContain("!summon");
    expect(idle).not.toContain("!vote");
    const shown = render([start, seg, card], 21_000);
    expect(shown).toContain("THE LICH EXPLAINS");
    expect(shown).not.toContain("standing");
    expect(render([start, seg, card], 20_000 + CARD_MS + 1)).toContain("standing");
  });

  it("!vote joins the standing card only while a poll is open", () => {
    const poll: StreamEvent = {
      type: "op",
      ts_ms: 2000,
      op: { op: "start_poll", poll_id: "p-1", question: "Which fool?", options: ["the-disruptor", "the-inspector"], min_rank: "thrall", secs: 40 },
    };
    const close: StreamEvent = { type: "op", ts_ms: 42_000, op: { op: "close_poll", poll_id: "p-1" } };
    expect(render([start, seg, poll], 3000)).toContain("!vote");
    // closed: the result may linger on the vote panel, but no vote counts any more
    expect(render([start, seg, poll, close], 42_500)).not.toContain("!vote");
  });

  it("before the first segment there is no standing card", () => {
    expect(render([start], 500)).not.toContain("standing");
  });

  it("between picks the decree slot keeps the last line heard, quietly", () => {
    const events = [start, seg, chat, picked];
    expect(render(events, 6500)).not.toContain("LAST HEARD");
    const later = render(events, 6000 + DECREE_MS.picked + 1);
    expect(later).toContain("LAST HEARD BY THE LICH");
    expect(later).toContain("decree-held");
    expect(later).toContain("hi?? first time");
    expect(render([start, seg, chat], 9000)).not.toContain("decree-held");
  });

  it("the ticker shows while a minion is out and a while after, then hides", () => {
    expect(tickerActive(foldEvents([start, seg]), 5000)).toBe(false);
    const running = foldEvents([start, seg, job(10_000, "running")]);
    expect(tickerActive(running, 600_000)).toBe(true);
    const done = foldEvents([start, seg, job(10_000, "running"), job(20_000, "done")]);
    expect(tickerActive(done, 20_000 + TICKER_LINGER_MS - 1)).toBe(true);
    expect(tickerActive(done, 20_000 + TICKER_LINGER_MS)).toBe(false);
    expect(render([start, seg], 5000)).toMatch(/region region-ticker hidden/);
    expect(render([start, seg, job(10_000, "running")], 11_000)).not.toMatch(/region region-ticker hidden/);
  });
});

describe("the decree slot shows one card at a time", () => {
  const line = (n: number): StreamEvent => ({
    type: "chat",
    ts_ms: 100 + n,
    msg: { id: `m${n}`, ts_ms: 100 + n, viewer: { id: `v${n}`, name: `viewer${n}`, platform: "sim", rank: "thrall", months: 0, gifted: 0, is_mod: false }, text: `line ${n}` },
  });
  const pick = (n: number, ts: number): StreamEvent => ({ type: "picked", ts_ms: ts, msg_ids: [`m${n}`], reason: "deterministic top 1 of 2" });
  const scene = foldEvents([start, line(1), line(2), line(3), pick(1, 1000), pick(2, 1700), pick(3, 2900)]);
  const html = (now: number, onAir = true) => renderToStaticMarkup(createElement(Decrees, { scene, now, onAir }));
  const cards = (h: string) => (h.match(/class="decree /g) ?? []).length;

  it("through a burst and after it, never more than one card", () => {
    for (let now = 0; now < 20_000; now += 100) expect(cards(html(now)), `at ${now}`).toBeLessThanOrEqual(1);
    expect(html(1500)).toContain("line 1");
    expect(html(1000 + DECREE_MIN_SHOW_MS + 10)).toContain("line 2");
    expect(html(1000 + 2 * DECREE_MIN_SHOW_MS + 10)).toContain("line 3");
  });

  it("the card fades out before the next enters, which lifts in", () => {
    const handover = 1000 + DECREE_MIN_SHOW_MS;
    expect(html(handover - DECREE_FADE_MS / 2)).toMatch(/decree decree-picked leaving/);
    expect(html(handover)).toMatch(/decree decree-picked lifting/);
  });

  it("after the last turn, the last line is held; the reason shows only off air", () => {
    expect(html(30_000)).toContain("LAST HEARD BY THE LICH");
    expect(html(30_000)).toContain("line 3");
    expect(html(1500)).not.toContain("deterministic");
    expect(html(1500, false)).toContain("deterministic");
    expect(html(30_000, false)).not.toContain("deterministic");
  });
});
