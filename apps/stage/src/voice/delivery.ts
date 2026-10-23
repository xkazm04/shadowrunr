// The performance feed: each of Malakor's lines carries a `delivery` (its emotion and intensity) on
// its `say` event, and the face performs it when the line is heard, not when its text arrives.
//
// The stage learns each host line's delivery by the `say` event's seq (foils' lines are not his
// face's), and when the voice player starts that seq's part 0 the feed sets the performance box:
// `box.cue(...cueFor(delivery))`. With no voice under a line (captions only, or a line the voice
// will not sound) the cue is set when its caption shows. A host line without a delivery is neutral
// at intensity 0, so the last line's emotion does not linger on the next.

import { useEffect } from "react";

import type { Delivery, StreamEvent } from "@shadowrunr/contract";
import { captionAt, type SceneState } from "@shadowrunr/stage-kit";

import { cueFor, type PerformanceBox } from "../avatar/performance";
import type { SaidLine, VoiceCaptionView } from "./captions";

type Say = Extract<StreamEvent, { type: "say" }>;

/** Events by absolute index, as the live store's ring and a plain array both give them. */
export interface EventsAt {
  readonly first: number;
  readonly end: number;
  at(i: number): StreamEvent | undefined;
}

export const eventsOf = (events: readonly StreamEvent[]): EventsAt => ({ first: 0, end: events.length, at: (i) => events[i] });

/** The host's `say` said at `at` (ms) with `text`, newest first; null when it is not held. */
export function findSay(events: EventsAt, at: number, text: string): Say | null {
  for (let i = events.end - 1; i >= events.first; i--) {
    const e = events.at(i);
    if (e?.type === "say" && e.speaker === "host" && e.ts_ms === at && e.text === text) return e;
  }
  return null;
}

/** Lines remembered per run, at most (the voice never lags further behind than this). */
const LINES_MAX = 64;

export class DeliveryFeed {
  private runId: string | null = null;
  /** Host lines by `say` seq: their delivery, `null` when the line gave none. */
  private lines = new Map<number, Delivery | null>();
  private captionKey: string | null = null;

  constructor(private readonly box: Pick<PerformanceBox, "cue">) {}

  /** Whether the host line `seq` of `runId` is known. */
  knows(runId: string, seq: number): boolean {
    return runId === this.runId && this.lines.has(seq);
  }

  /** A host line the stage saw, by its `say` seq, with its delivery (none: neutral). */
  said(runId: string, seq: number, delivery: Delivery | undefined): void {
    if (runId !== this.runId) {
      this.runId = runId;
      this.lines.clear();
    }
    this.lines.set(seq, delivery ?? null);
    if (this.lines.size > LINES_MAX) this.lines.delete(Math.min(...this.lines.keys()));
  }

  /** The delivery of a known host line (undefined: unknown, or none given). */
  deliveryOf(runId: string, seq: number): Delivery | undefined {
    return runId === this.runId ? (this.lines.get(seq) ?? undefined) : undefined;
  }

  /** A voice part started playing: part 0 of a known host line sets its cue. Returns whether it did. */
  started(p: { runId: string; seq: number; part: number }): boolean {
    if (p.part !== 0 || !this.knows(p.runId, p.seq)) return false;
    this.cue(this.lines.get(p.seq) ?? undefined);
    return true;
  }

  /** A caption with no voice under it went on screen (`key` names it): its cue, once. */
  captionShown(key: string, delivery: Delivery | undefined): boolean {
    if (key === this.captionKey) return false;
    this.captionKey = key;
    this.cue(delivery);
    return true;
  }

  private cue(delivery: Delivery | undefined) {
    const c = cueFor(delivery);
    this.box.cue(c.emotion, c.intensity, c.gesture ? { gesture: c.gesture } : undefined);
  }
}


/** What the feed reads from the live store: the run, its host lines by seq, its event ring. */
export interface LiveLines {
  runId: string;
  said: readonly SaidLine[];
  ring: EventsAt;
}

/** Learn every live host line's delivery by its seq that the feed does not know yet. */
export function learnLines(feed: DeliveryFeed, live: LiveLines): void {
  for (const line of live.said) if (!feed.knows(live.runId, line.seq)) feed.said(live.runId, line.seq, findSay(live.ring, line.at, line.text)?.delivery);
}

/**
 * The caption on screen with no voice under it, if any: a plain caption (no voice following the
 * audio; it is cued as it goes on screen), or a line the voice will not sound while it reveals.
 */
export function silentCaption(
  scene: SceneState,
  now: number,
  voiceCaption: VoiceCaptionView | null | undefined,
): { key: string; id: number; saidAt?: number; text: string } | null {
  if (voiceCaption === undefined) {
    const c = captionAt(scene, now);
    if (!c) return null;
    return { key: `c:${c.id}:${c.saidAt}`, id: c.id, saidAt: c.saidAt, text: c.text };
  }
  if (voiceCaption && !voiceCaption.voiced && voiceCaption.speaking) return { key: `v:${voiceCaption.caption.id}`, id: voiceCaption.caption.id, text: voiceCaption.caption.text };
  return null;
}

/**
 * Feed the performance box from the stage: learn each live host line's delivery by its seq (the
 * voice's `started` then cues it, through `feed.started`), and cue a caption that shows with no
 * voice under it. `live` reads the live store (null in a replay, whose `events` are searched).
 */
export function usePerformanceFeed(opts: {
  feed: DeliveryFeed;
  live: () => LiveLines | null;
  events: readonly StreamEvent[];
  scene: SceneState;
  now: number;
  voiceCaption: VoiceCaptionView | null | undefined;
}): void {
  const { feed, live, events, scene, now, voiceCaption } = opts;

  // every render (a live stage renders as its events arrive): only new lines cost anything
  useEffect(() => {
    const l = live();
    if (l) learnLines(feed, l);
  });

  const shown = silentCaption(scene, now, voiceCaption);
  const key = shown?.key ?? null;
  const id = shown?.id;
  const saidAt = shown?.saidAt;
  const text = shown?.text;
  useEffect(() => {
    if (key === null || id === undefined || text === undefined) return;
    const l = live();
    let delivery: Delivery | undefined;
    if (saidAt !== undefined) {
      delivery = findSay(l ? l.ring : eventsOf(events), saidAt, text)?.delivery;
    } else if (l) {
      const line = l.said.find((s) => s.id === id);
      delivery = line ? feed.deliveryOf(l.runId, line.seq) : undefined;
    }
    feed.captionShown(key, delivery);
  }, [feed, live, events, key, id, saidAt, text]);
}
