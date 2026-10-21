// What the On Air screen shows, derived from a stage-kit scene: pure functions, no clock. The
// "lives" (how long a card or a decree stays up) are the winner's stage.js timings in wall
// seconds; a replay at speed s covers s transcript-seconds per wall-second.
import type { ChatMsg, EvidenceGrade, Rank, SegmentId } from "@shadowrunr/contract";
import { foilInfo, pollBars, shownLane, type SceneState } from "@shadowrunr/stage-kit";

/** Wall-clock lives of the timed overlays, from the winner's `later(...)` calls. */
export const LIFE_MS = { slip: 8000, card: 7000, decree: 5500, moment: 3600, pollAfterClose: 5000 } as const;
/** The winner keeps the last six chat lines on screen. */
export const CHAT_ROWS = 6;

export interface Life {
  /** Still on screen at this position. */
  readonly live: boolean;
  /** A negative CSS animation delay (ms): an overlay mounted late starts part-way through. */
  readonly delayMs: number;
}

/** Whether an overlay that appeared at `at` (transcript ms) is still up at `position`. */
export function lifeOf(at: number, position: number, speed: number, lifeMs: number): Life {
  const elapsedWall = Math.max(0, (position - at) / Math.max(speed, 1e-6));
  return { live: elapsedWall < lifeMs, delayMs: -Math.round(elapsedWall) };
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
  /** Set once the poll closed: the panel clears after LIFE_MS.pollAfterClose. */
  readonly closing: Life | undefined;
}

export function voteView(scene: SceneState, position: number, speed: number): VoteView | null {
  const poll = scene.activePoll === null ? undefined : scene.polls[scene.activePoll];
  if (!poll) return null;
  const closing = poll.closedAt === undefined ? undefined : lifeOf(poll.closedAt, position, speed, LIFE_MS.pollAfterClose);
  if (closing && !closing.live) return null;
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

export function captionView(scene: SceneState, position: number, speed: number): CaptionView | null {
  const c = scene.caption;
  const f = scene.foilLine;
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
    talk: lifeOf(line.at, position, speed, talkSecs * 1000),
    parity: line.id % 2 === 0 ? "a" : "b",
  };
}

export interface Timed<T> {
  readonly key: string;
  readonly value: T;
  readonly life: Life;
}

export function cardView(scene: SceneState, position: number, speed: number): Timed<{ title: string; body: string }> | null {
  const c = scene.card;
  if (!c) return null;
  const life = lifeOf(c.at, position, speed, LIFE_MS.card);
  if (!life.live) return null;
  const body = c.body.length > 150 ? `${c.body.slice(0, 147)}…` : c.body;
  return { key: `${c.id}@${c.at}`, value: { title: c.title, body }, life };
}

/** The winner seals the first picked line of the latest pick as a decree. */
export function decreeView(scene: SceneState, position: number, speed: number): Timed<string> | null {
  const picked = scene.decrees.filter((d) => d.kind === "picked");
  if (picked.length === 0) return null;
  const at = Math.max(...picked.map((d) => d.at));
  const d = picked.find((x) => x.at === at);
  const m = d ? scene.chats.get(d.msgId) : undefined;
  if (!d || !m) return null;
  const life = lifeOf(d.at, position, speed, LIFE_MS.decree);
  if (!life.live) return null;
  return { key: d.key, value: m.text.slice(0, 110), life };
}

export function momentView(scene: SceneState, position: number, speed: number): Timed<string> | null {
  const m = scene.moments[scene.moments.length - 1];
  if (!m) return null;
  const life = lifeOf(m.at, position, speed, LIFE_MS.moment);
  if (!life.live) return null;
  return { key: `${m.at}:${m.label}`, value: `Moment marked: ${m.label}`, life };
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
