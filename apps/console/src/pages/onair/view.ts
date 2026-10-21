// What the On Air screen shows, derived from a stage-kit scene: pure functions, no clock. The
// screen is a screenprint of the broadcast, so WHETHER a timed overlay is up is the broadcast's
// own on-screen schedule (stage-kit `onScreen`), in transcript time, at any replay speed; this
// file keeps only the presentation (trims, bar scale, the CSS delay derived from `since`).
import type { ChatMsg, EvidenceGrade, Rank, SegmentId } from "@shadowrunr/contract";
import { foilInfo, pollBars, shownLane, type SceneState } from "@shadowrunr/stage-kit";
import { captionOn, cardOn, decreeOn, foilLineOn, momentOn, pollOn, type Shown } from "@shadowrunr/stage-kit/ui";

/** The winner keeps the last six chat lines on screen. */
export const CHAT_ROWS = 6;

export interface Life {
  /** Still on screen at this position. */
  readonly live: boolean;
  /** A negative CSS animation delay (ms, transcript time): an overlay mounted late starts part-way through. */
  readonly delayMs: number;
  /** How long it stays up from when it came on screen (Infinity while nothing ends it). */
  readonly lifeMs: number;
}

/** An overlay the broadcast has on screen, `position - since` (transcript ms) into its life. */
export function lifeOf(shown: Pick<Shown<unknown>, "since" | "until">, position: number): Life {
  return { live: position < shown.until, delayMs: -Math.round(Math.max(0, position - shown.since)), lifeMs: shown.until - shown.since };
}

export interface ChatRow {
  readonly id: string;
  readonly name: string;
  readonly rank: Rank;
  readonly text: string;
  readonly command: boolean;
  readonly picked: boolean;
  readonly redacted: boolean;
}

export function chatRows(scene: SceneState, n = CHAT_ROWS): ChatRow[] {
  const out: ChatRow[] = [];
  // What the broadcast draws: only the lines the runtime's input filter has judged.
  for (const { id } of shownLane(scene).slice(-n)) {
    const m: ChatMsg | undefined = scene.chats.get(id);
    if (!m) continue;
    out.push({
      id,
      name: m.viewer.name,
      rank: m.viewer.rank,
      text: m.text,
      command: /^!/.test(m.text),
      picked: scene.heard.has(id),
      redacted: scene.purged[id] !== undefined,
    });
  }
  return out;
}

export interface VoteRow {
  readonly name: string;
  /** `×2.4`, empty while the option has no weight. */
  readonly weight: string;
  /** Bar length in percent, on the winner's scale. */
  readonly pct: number;
  readonly win: boolean;
}

export interface VoteView {
  readonly key: string;
  readonly question: string;
  readonly rows: readonly VoteRow[];
  readonly closed: boolean;
  /** Set once the poll closed: the panel clears when the broadcast's does (POLL_LINGER_MS). */
  readonly closing: Life | undefined;
}

export function voteView(scene: SceneState, position: number, _speed = 1): VoteView | null {
  const shown = pollOn(scene, position);
  if (!shown) return null;
  const poll = shown.value;
  const closing = poll.closedAt === undefined ? undefined : lifeOf({ since: poll.closedAt, until: shown.until }, position);
  const bars = pollBars(poll);
  const weights = bars.map((b) => b.weight);
  // The winner's scale: the bars never hit 100% until one option holds most of the weight.
  const tot = Math.max(1, Math.max(0, ...weights), weights.reduce((a, w) => a + w, 0) * 0.55);
  // The runtime's result, not a rule of our own: a level tie crowns nobody.
  const win = poll.closedAt === undefined ? -1 : bars.findIndex((b) => b.leading && !b.tied);
  return {
    key: poll.id,
    question: poll.question,
    closed: poll.closedAt !== undefined,
    closing,
    rows: bars.map((b, i) => ({
      name: b.option.replace(/^Summon /, ""),
      weight: b.weight ? `×${Math.round(b.weight * 10) / 10}` : "",
      pct: Math.round(Math.min(100, (b.weight / tot) * 100)),
      win: i === win,
    })),
  };
}

export interface Word {
  readonly text: string;
  /** `*stage directions*` print softer. */
  readonly aside: boolean;
}

export interface CaptionView {
  readonly key: string;
  readonly foil: boolean;
  readonly name: string;
  readonly role: string;
  readonly words: readonly Word[];
  /** Seconds the jaw moves (the winner's min(5, hold), hold = max(3, length / 17)). */
  readonly talkSecs: number;
  readonly talk: Life;
  /** Alternates per line, so the jaw animation restarts on every new line. */
  readonly parity: "a" | "b";
}

export const CAPTION_MAX = 190;

export function captionWords(text: string): Word[] {
  const t = text.length > CAPTION_MAX ? `${text.slice(0, CAPTION_MAX - 3)}…` : text;
  const out: Word[] = [];
  let aside = false;
  for (const raw of t.split(" ")) {
    if (raw === "") continue;
    const opens = raw.startsWith("*");
    const closes = raw.endsWith("*") && raw.length > (opens ? 1 : 0);
    out.push({ text: raw.replace(/\*/g, ""), aside: aside || opens });
    if (opens && !closes) aside = true;
    else if (closes) aside = false;
  }
  return out;
}

/** The lower third: the broadcast's queued caption showing now (or the foil's newer line). */
export function captionView(scene: SceneState, position: number, _speed = 1): CaptionView | null {
  const c = captionOn(scene, position)?.value ?? null;
  const f = foilLineOn(scene, position)?.value ?? null;
  const useFoil = f !== null && (c === null || f.id > c.id);
  const line = useFoil ? f : c;
  if (line === null) return null;
  const text = line.text;
  const talkSecs = Math.min(5, Math.max(3, text.length / 17));
  return {
    key: `${useFoil ? "f" : "m"}${line.id}`,
    foil: useFoil,
    name: useFoil && f ? foilInfo(f.foil).name : "Arch-Lich Malakor",
    role: useFoil ? "Guest foil" : "Lord’s envoy",
    words: captionWords(text),
    talkSecs,
    talk: lifeOf({ since: line.at, until: line.at + talkSecs * 1000 }, position),
    parity: line.id % 2 === 0 ? "a" : "b",
  };
}

export interface Timed<T> {
  readonly key: string;
  readonly value: T;
  readonly life: Life;
}

export function cardView(scene: SceneState, position: number, _speed = 1): Timed<{ title: string; body: string }> | null {
  const shown = cardOn(scene, position);
  if (!shown) return null;
  const c = shown.value;
  const body = c.body.length > 150 ? `${c.body.slice(0, 147)}…` : c.body;
  return { key: `${c.id}@${c.at}`, value: { title: c.title, body }, life: lifeOf(shown, position) };
}

export interface DecreeView extends Timed<string> {
  /** What the broadcast's slot says the line is: highlighted, picked, or held after its turn. */
  readonly label: string;
  readonly held: boolean;
}

/** The broadcast's decree slot: highlights and picks in queued turns, the last one held. */
export function decreeView(scene: SceneState, position: number, _speed = 1): DecreeView | null {
  const shown = decreeOn(scene, position);
  if (!shown) return null;
  const { turn, msg, held } = shown.value;
  const label = held ? "Last heard by the Lich" : turn.decree.kind === "highlight" ? "Decree of the Lich" : "Decree · picked from chat";
  return { key: shown.key, value: msg.text.slice(0, 110), life: lifeOf(shown, position), label, held };
}

export function momentView(scene: SceneState, position: number, _speed = 1): Timed<string> | null {
  const shown = momentOn(scene, position);
  if (!shown) return null;
  const m = shown.value;
  return { key: shown.key, value: `Moment marked: ${m.label}`, life: lifeOf(shown, position) };
}

/** The sign-off slate: Malakor's reason, or the end of the stream. */
export function slateView(scene: SceneState, live: boolean): string | null {
  if (scene.outro) return scene.outro.reason.slice(0, 120);
  if (scene.ended) return live ? "End of the stream." : "End of the replay. It loops.";
  return null;
}

export function foilView(scene: SceneState): { id: string; name: string } | null {
  return scene.foil ? { id: scene.foil.id, name: foilInfo(scene.foil.id).name } : null;
}

export function gradesOf(scene: SceneState): Readonly<Record<string, EvidenceGrade>> {
  const out: Record<string, EvidenceGrade> = {};
  for (const [id, h] of Object.entries(scene.hexes)) out[id] = h.grade;
  return out;
}

export function segmentOf(scene: SceneState): SegmentId {
  return scene.segment ?? "SEG-01";
}

/** The HUD's source line: what is on screen, from the stream's own meta. */
export function sourceLabel(scene: SceneState, live: boolean, speed: number): string {
  const m = scene.meta;
  const what = m ? `${m.toneVariant.replace(/-/g, " ")} · ${m.coreTier} · ${m.audienceSize} simulated viewers` : "waiting for the stream";
  return live ? `Live · ${what}` : `Replay ${speed}× · ${what}`;
}
