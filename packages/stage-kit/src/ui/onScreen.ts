// What the broadcast shows at transcript time `t`: one schedule for every timed overlay. The
// stage's components read their slot from here instead of keeping a window inline, and any other
// monitor of the broadcast (the console's On Air screen) asks the same question and gets the same
// answer, so the two agree by construction. Pure: nothing reads a clock.
//
// Each slot is `{ key, since, until, leaving, value }` or null: `since` is when it came on screen,
// `until` when it goes (Infinity while nothing ends it), `leaving` whether it is in its exit fade.

import type { ChatMsg } from "@shadowrunr/contract";
import { DECREE_FADE_MS, decreeAt, type DecreeTurn } from "../decrees";
import { captionAt } from "../scene";
import type {
  Card,
  FoilLine,
  Moment,
  Poll,
  Redaction,
  SceneState,
  ScheduledCaption,
  Seal,
  StreamMeta,
  Transition,
} from "../sceneModel";
import { CAPTION_HOLD_MS, wordTimings } from "../timing";
import { jobPhase } from "./JobTicker";

/** An explainer card stays up this long. */
export const CARD_MS = 16000;
/** A closed poll's panel stays up this long after the close, naming the winner. */
export const POLL_LINGER_MS = 9000;
/** A redaction notice covers the caption panel this long. */
export const REDACTION_MS = 3800;
export const TRANSITION_MS = 1900;
export const INTRO_MS = 4600;
export const MOMENT_FLASH_MS = 900;
export const MOMENT_CHIP_MS = 3600;
export const SEAL_MS = 4200;
/** The minion ticker stays this long after the last job came back (its summary is news). */
export const TICKER_LINGER_MS = 30_000;
/** A caption fades out over this long once it has held. */
export const CAPTION_FADE_MS = 600;

const CARD_FADE_MS = 600;
const INTRO_FADE_MS = 700;

export interface Shown<T> {
  readonly key: string;
  readonly since: number;
  readonly until: number;
  readonly leaving: boolean;
  readonly value: T;
}

export interface DecreeShown {
  readonly turn: DecreeTurn;
  readonly msg: ChatMsg;
  /** Its turn is over: the slot keeps the last line heard, quietly, until the next one. */
  readonly held: boolean;
}

export interface OnScreen {
  readonly card: Shown<Card> | null;
  readonly poll: Shown<Poll> | null;
  readonly decree: Shown<DecreeShown> | null;
  readonly caption: Shown<ScheduledCaption> | null;
  readonly foilLine: Shown<FoilLine> | null;
  readonly moment: Shown<Moment> | null;
  readonly seal: Shown<Seal> | null;
  readonly redaction: Shown<Redaction> | null;
  readonly intro: Shown<StreamMeta> | null;
  readonly wipe: Shown<Transition> | null;
  readonly ticker: Shown<null> | null;
}

/** A window [at, at + ms] (both ends shown), or null outside it. */
function windowed<T>(key: string, at: number, ms: number, t: number, fade: number, value: T): Shown<T> | null {
  const age = t - at;
  if (age < 0 || age > ms) return null;
  return { key, since: at, until: at + ms, leaving: ms - age < fade, value };
}

/** `show_card`: the explainer for CARD_MS. */
export function cardWindow(c: Card | null, t: number): Shown<Card> | null {
  return c ? windowed(c.id, c.at, CARD_MS, t, CARD_FADE_MS, c) : null;
}

export function cardOn(scene: SceneState, t: number): Shown<Card> | null {
  return cardWindow(scene.card, t);
}

/** The active poll while open, and for POLL_LINGER_MS after it closed. */
export function pollOn(scene: SceneState, t: number): Shown<Poll> | null {
  const p = scene.activePoll ? scene.polls[scene.activePoll] : undefined;
  if (!p) return null;
  const until = p.closedAt === undefined ? Infinity : p.closedAt + POLL_LINGER_MS;
  if (t > until) return null;
  return { key: p.id, since: p.openedAt, until, leaving: false, value: p };
}

/** The decree slot (decrees.ts): the turn showing now, else the last line heard, held. */
export function decreeOn(scene: SceneState, t: number): Shown<DecreeShown> | null {
  const at = decreeAt(scene.decrees, t);
  const msg = at ? scene.chats.get(at.turn.decree.msgId) : undefined;
  if (!at || !msg) return null;
  const { turn, held } = at;
  return {
    key: turn.decree.key,
    since: turn.start,
    until: held ? Infinity : turn.end,
    leaving: !held && turn.end - t < DECREE_FADE_MS,
    value: { turn, msg, held },
  };
}

/** How long a line is up from when it shows: revealed, held, faded. */
function lineMs(text: string): { fadeStart: number; until: number } {
  const fadeStart = wordTimings(text).spokenMs + CAPTION_HOLD_MS;
  return { fadeStart, until: fadeStart + CAPTION_FADE_MS };
}

/** The host's caption: the queued line showing now (captionAt), until it has faded. */
export function captionOn(scene: SceneState, t: number): Shown<ScheduledCaption> | null {
  const c = captionAt(scene, t);
  if (!c) return null;
  const { fadeStart, until } = lineMs(c.text);
  const age = t - c.at;
  if (age >= until) return null;
  return { key: String(c.id), since: c.at, until: c.at + until, leaving: age > fadeStart, value: c };
}

/** The foil's latest line, until it has faded. */
export function foilLineOn(scene: SceneState, t: number): Shown<FoilLine> | null {
  const f = scene.foilLine;
  if (!f) return null;
  const { fadeStart, until } = lineMs(f.text);
  const age = t - f.at;
  if (age >= until) return null;
  return { key: String(f.id), since: f.at, until: f.at + until, leaving: age > fadeStart, value: f };
}

/** The latest marked moment's chip. */
export function momentOn(scene: SceneState, t: number): Shown<Moment> | null {
  const m = scene.moments[scene.moments.length - 1];
  return m ? windowed(`${m.at}:${m.label}`, m.at, MOMENT_CHIP_MS, t, 0, m) : null;
}

/** The latest segment seal's toast. */
export function sealOn(scene: SceneState, t: number): Shown<Seal> | null {
  const s = scene.seals[scene.seals.length - 1];
  return s ? windowed(String(s.at), s.at, SEAL_MS, t, 0, s) : null;
}

/** The redaction notice over the caption panel: the latest one still inside REDACTION_MS. */
export function redactionAt(redactions: readonly Redaction[], t: number): Shown<Redaction> | null {
  for (let i = redactions.length - 1; i >= 0; i--) {
    const r = redactions[i]!;
    if (t >= r.at && t - r.at < REDACTION_MS) return { key: r.key, since: r.at, until: r.at + REDACTION_MS, leaving: false, value: r };
  }
  return null;
}

/** The opening slate, from `stream_start`. */
export function introOn(scene: SceneState, t: number): Shown<StreamMeta> | null {
  const m = scene.meta;
  return m ? windowed("intro", m.at, INTRO_MS, t, INTRO_FADE_MS, m) : null;
}

/** The segment wipe: a segment change only (a screen switch inside a segment moves the regions). */
export function wipeOn(scene: SceneState, t: number): Shown<Transition> | null {
  const w = scene.transition;
  if (!w || w.from === "boot" || w.cause === "screen" || w.segment === undefined) return null;
  return windowed(String(w.id), w.at, TRANSITION_MS, t, 0, w);
}

/** The minion ticker: while a job is out, and for TICKER_LINGER_MS after the last came back. */
export function tickerOn(scene: SceneState, t: number): Shown<null> | null {
  let out = false;
  let last = -Infinity;
  let since = Infinity;
  for (const j of scene.jobs) {
    const phase = jobPhase(j.status);
    if (phase === "queued" || phase === "running") out = true;
    else last = Math.max(last, j.updatedAt);
    since = Math.min(since, j.at);
  }
  const until = out ? Infinity : last + TICKER_LINGER_MS;
  if (t >= until) return null;
  return { key: "ticker", since, until, leaving: false, value: null };
}

/** Everything timed that is on the broadcast at `t`. */
export function onScreen(scene: SceneState, t: number): OnScreen {
  return {
    card: cardOn(scene, t),
    poll: pollOn(scene, t),
    decree: decreeOn(scene, t),
    caption: captionOn(scene, t),
    foilLine: foilLineOn(scene, t),
    moment: momentOn(scene, t),
    seal: sealOn(scene, t),
    redaction: redactionAt(scene.redactions, t),
    intro: introOn(scene, t),
    wipe: wipeOn(scene, t),
    ticker: tickerOn(scene, t),
  };
}
