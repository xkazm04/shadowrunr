// What fills the stage's quiet moments on camera, from data the stage already has, and the caption
// panel that follows the voice.
//
// - The card slot shows the explainer for CARD_MS per `show_card`; between cards it shows the
//   segment's standing card: the segment's name and the chat commands the room can use in it (the
//   daemon's command table, `shadowrunr-segments` COMMAND_SPECS, copied: the stage has no RPC).
//   `!vote` is listed only while a poll is open.
// - The decree slot is stage-kit's `Decrees`: one card at a time, and the last line heard held
//   quietly between picks (styled here, `.decree-held`).
// - The minion ticker shows while a job is out, and for TICKER_LINGER_MS after the last one came
//   back; otherwise there is nothing abroad to show and it hides.

import { useMemo } from "react";

import type { SegmentId } from "@shadowrunr/contract";
import { SEGMENT_TITLE, foilInfo, wordTimings, type Redaction, type SceneState } from "@shadowrunr/stage-kit";
import { captionSize, captionSpeaker, cardOn, redactionAt, tickerOn } from "@shadowrunr/stage-kit/ui";
import type { VoiceCaptionView } from "./voice/captions";

export { TICKER_LINGER_MS } from "@shadowrunr/stage-kit/ui";

/** Whether the minion ticker has anything to show at `now` (stage-kit's on-screen schedule). */
export const tickerActive = (scene: SceneState, now: number): boolean => tickerOn(scene, now) !== null;

/** Whether an explainer card is on screen at `now` (stage-kit's on-screen schedule). */
export const cardShowing = (scene: SceneState, now: number): boolean => cardOn(scene, now) !== null;

interface CommandRow {
  cmd: string;
  args: string;
  what: string;
}

const PROPOSE: CommandRow = { cmd: "!propose", args: "<scheme>", what: "Propose a scheme. Rank-gated." };
const SUMMON: CommandRow = { cmd: "!summon", args: "<foil>", what: "Call a guest foil into the room. The weighted vote decides." };
const VOTE: CommandRow = { cmd: "!vote", args: "<option>", what: "Vote in the open poll. Weight rises with rank." };
const HINT: CommandRow = { cmd: "!hint", args: "<hex> <text>", what: "Nominate a lead on the board." };
const GRADE: CommandRow = { cmd: "!grade", args: "<hex> <DOC|REP|CLM|SPC>", what: "Grade a hex. Jury advice, never proof." };
const BALLOT: CommandRow = { cmd: "!vote", args: "<number>", what: "Pick the next scene when the ballot opens. Weight rises with rank." };

/** The commands the room can use, per segment (none in a descoped one); `!vote` joins while a poll
 * is open. SEG-03's one command is the scene ballot's `!vote`. */
export function standingCommands(segment: SegmentId | null): CommandRow[] {
  switch (segment) {
    case "SEG-01":
      return [PROPOSE, SUMMON];
    case "SEG-02A":
      return [HINT, GRADE];
    case "SEG-03":
      return [BALLOT];
    case "SEG-02B-I":
    case "SEG-02B-II":
    case null:
      return [];
  }
}

/** The card slot between explainer cards: the segment and what the room can do in it now. */
export function StandingCard({ scene, poll }: { scene: SceneState; poll: boolean }) {
  const base = scene.layout === "boot" ? [] : standingCommands(scene.segment);
  const rows = base.length && poll && !base.some((r) => r.cmd === VOTE.cmd) ? [...base, VOTE] : base;
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
  speaker = "Host",
}: {
  view: VoiceCaptionView | null;
  redactions: Redaction[];
  now: number;
  compact: boolean;
  onAir: boolean;
  /** Who says a host line (the stream's host name); shown upper-cased. A guest line names its guest. */
  speaker?: string;
}) {
  const text = view?.caption.text;
  const foil = view?.caption.foil;
  const line = useMemo(() => (text === undefined ? null : wordTimings(text)), [text]);
  const redaction = redactionAt(redactions, now)?.value;
  return (
    <div className={`caption-panel${compact ? " compact" : ""}`}>
      {view && line && view.opacity > 0 && (
        <div
          className={`caption${foil === undefined ? "" : " guest"}`}
          key={view.caption.id}
          style={{ opacity: view.opacity, fontSize: captionSize(view.caption.text, compact), ...(foil === undefined ? {} : { ["--foil" as string]: foilInfo(foil).color }) }}
          data-speaker={foil ?? "host"}
        >
          <div className="caption-head">
            <span className="speaker">{captionSpeaker(view.caption, speaker).toUpperCase()}</span>
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
