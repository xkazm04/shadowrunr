// What fills the stage's quiet moments on camera, from data the stage already has, and the caption
// panel that follows the voice.
//
// - The card slot shows the explainer for CARD_MS per `show_card`; between cards it shows the
//   segment's standing card: the segment's name and the chat commands the room can use in it (the
//   daemon's command table, `shadowrunr-segments` COMMAND_SPECS, copied: the stage has no RPC).
//   `!vote` is listed only while a poll is open.
// - The decree slot shows a picked line while it is fresh; between picks it keeps the last one,
//   quieter, as "last heard".
// - The minion ticker shows while a job is out, and for TICKER_LINGER_MS after the last one came
//   back; otherwise there is nothing abroad to show and it hides.

import { useMemo } from "react";

import { RANK_COLOR, RANK_TITLE, SEGMENT_TITLE, wordTimings, type LayoutId, type Redaction, type SceneState } from "@shadowrunr/stage-kit";
import { CARD_MS, DECREE_MS, REDACTION_MS, Sigil, captionSize, jobPhase } from "@shadowrunr/stage-kit/ui";
import type { VoiceCaptionView } from "./voice/captions";

/** The ticker stays this long after the last job came back (its summary is news). */
export const TICKER_LINGER_MS = 30_000;

/** Whether the minion ticker has anything to show at `now`. */
export function tickerActive(scene: SceneState, now: number): boolean {
  return scene.jobs.some((j) => {
    const phase = jobPhase(j.status);
    return phase === "queued" || phase === "running" || now - j.updatedAt < TICKER_LINGER_MS;
  });
}

/** Whether an explainer card is on screen at `now` (see stage-kit ExplainerCard). */
export function cardShowing(scene: SceneState, now: number): boolean {
  const c = scene.card;
  return c !== null && now - c.at >= 0 && now - c.at <= CARD_MS;
}

interface CommandRow {
  cmd: string;
  args: string;
  what: string;
}

const PROPOSE: CommandRow = { cmd: "!propose", args: "<scheme>", what: "Propose a scheme. Rank-gated." };
const SUMMON: CommandRow = { cmd: "!summon", args: "<foil>", what: "Call a guest foil into the room. The weighted vote decides." };
const VOTE: CommandRow = { cmd: "!vote", args: "<option>", what: "Vote in the open poll. Weight rises with rank." };
const HINT: CommandRow = { cmd: "!hint", args: "<hex> <text>", what: "Nominate a lead on the hexmap." };
const GRADE: CommandRow = { cmd: "!grade", args: "<hex> <DOC|REP|CLM|SPC>", what: "Grade a hex. Jury advice, never proof." };

/** The commands the room can use, per layout (none on the boot or a descoped segment); `!vote` joins while a poll is open. */
export const STANDING_COMMANDS: Record<LayoutId, CommandRow[]> = {
  boot: [],
  descoped: [],
  improv: [PROPOSE, SUMMON],
  hexmap: [HINT, GRADE],
};

/** The card slot between explainer cards: the segment and what the room can do in it now. */
export function StandingCard({ scene, poll }: { scene: SceneState; poll: boolean }) {
  const base = STANDING_COMMANDS[scene.layout];
  const rows = base.length && poll ? [...base, VOTE] : base;
  if (!scene.segment || rows.length === 0) return null;
  return (
    <div className="standing">
      <div className="standing-kicker">{SEGMENT_TITLE[scene.segment]}</div>
      <div className="standing-title">The minions may</div>
      <div className="standing-rows">
        {rows.map((r) => (
          <div key={r.cmd} className="standing-row">
            <span className="standing-cmd">
              {r.cmd} <span className="standing-args">{r.args}</span>
            </span>
            <span className="standing-what">{r.what}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/** The decree slot between picks: the last line he heard, held quietly until the next. */
export function LastHeard({ scene, now }: { scene: SceneState; now: number }) {
  const fresh = scene.decrees.some((d) => now >= d.at && now - d.at < DECREE_MS[d.kind]);
  if (fresh) return null;
  const last = [...scene.decrees].reverse().find((d) => d.at <= now && scene.chats.get(d.msgId));
  const m = last ? scene.chats.get(last.msgId) : undefined;
  if (!last || !m) return null;
  const color = RANK_COLOR[m.viewer.rank];
  return (
    <div className="decrees decrees-held">
      <div className="decree decree-held" key={last.key} style={{ ["--rank" as string]: color }}>
        <div className="decree-head">
          <span className="decree-label">LAST HEARD BY THE LICH</span>
        </div>
        <div className="decree-main">
          <Sigil rank={m.viewer.rank} size={64} />
          <div className="decree-text">
            <div className="decree-who">
              <span style={{ color }}>{m.viewer.name}</span>
              <span className="decree-rank">{RANK_TITLE[m.viewer.rank]}</span>
            </div>
            <div className="decree-quote">“{m.text}”</div>
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * The caption panel while captions follow the voice: stage-kit's CaptionPanel markup, with the
 * words revealed and the opacity given by `view` (voice/captions.ts) instead of the estimate.
 */
export function VoiceCaptionPanel({
  view,
  redactions,
  now,
  compact,
  onAir,
}: {
  view: VoiceCaptionView | null;
  redactions: Redaction[];
  now: number;
  compact: boolean;
  onAir: boolean;
}) {
  const line = useMemo(() => (view ? wordTimings(view.caption.text) : null), [view?.caption.text]);
  const redaction = [...redactions].reverse().find((r) => now >= r.at && now - r.at < REDACTION_MS);
  return (
    <div className={`caption-panel${compact ? " compact" : ""}`}>
      {view && line && view.opacity > 0 && (
        <div className="caption" key={view.caption.id} style={{ opacity: view.opacity, fontSize: captionSize(view.caption.text, compact) }}>
          <div className="caption-head">
            <span className="speaker">MALAKOR</span>
            {view.caption.replyingTo.slice(0, 3).map((n) => (
              <span className="reply-chip" key={n}>
                ↳ {n}
              </span>
            ))}
          </div>
          <span className="caption-text">
            {line.words.map((w, i) => (
              <span key={i} className={`w${i < view.shown ? " in" : ""}${w.emphasis ? " em" : ""}`}>
                {w.word}{" "}
              </span>
            ))}
          </span>
        </div>
      )}
      {redaction && (
        <div className="redaction" key={redaction.key}>
          <div className="redaction-bar">
            <span className="redaction-title">REDACTED BY THE LORD</span>
            {!onAir && <span className="redaction-rule">{redaction.rule}</span>}
          </div>
        </div>
      )}
    </div>
  );
}
