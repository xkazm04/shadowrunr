import type { ChatMsg, Command, Rank } from "@shadowrunr/contract";
import { Sigil } from "../art/Sigil";
import { ballotOption, matchOption } from "../ballots";
import { RANK_COLOR, RANK_TITLE } from "../lore";
import { shownLane } from "../scene";
import type { Poll, SceneState } from "../sceneModel";
import type { TsMs } from "../vocab";
import { optionLabel } from "./VotePanel";

export function commandLabel(c: Command): string {
  switch (c.cmd) {
    case "summon":
      return `SUMMON ${c.foil}`;
    case "hint":
      return `HINT ${c.hex}`;
    case "grade":
      return `GRADE ${c.hex} ${c.grade}`;
    case "propose":
      return "PROPOSAL";
    case "vote":
      return `VOTE ${c.option}`;
  }
}

/** What became of a `!vote` line: counted for an option (with its 1-based key), or why not. */
export type VoteOutcome =
  | { kind: "counted"; key: number; option: string }
  | { kind: "not_an_option" }
  | { kind: "below_rank"; minRank: Rank }
  | { kind: "no_open_poll" };

/** The poll open at `ts` (opened at or before it, not yet closed), the latest if several. */
export function pollOpenAt(scene: SceneState, ts: TsMs): Poll | null {
  let best: Poll | null = null;
  for (const p of Object.values(scene.polls)) {
    if (p.openedAt > ts || (p.closedAt !== undefined && ts >= p.closedAt)) continue;
    if (best === null || p.openedAt >= best.openedAt) best = p;
  }
  return best;
}

/**
 * The outcome of a `!vote` line against the poll open at the line's time, decided by the
 * runtime-pinned ballot rule (`ballots.ts`): the chip never counts a ballot the runtime drops.
 * Null for a line that is not a `!vote`.
 */
export function voteOutcome(scene: SceneState, m: ChatMsg): VoteOutcome | null {
  const c = m.command;
  if (c?.cmd !== "vote") return null;
  const poll = pollOpenAt(scene, m.ts_ms);
  if (poll === null) return { kind: "no_open_poll" };
  // Judged as at the line's time, when the poll was still open, even if it has closed since.
  const open: Poll = { ...poll };
  delete open.closedAt;
  const option = ballotOption(open, { id: m.viewer.id, rank: m.viewer.rank }, c);
  if (option !== null) return { kind: "counted", key: poll.options.indexOf(option) + 1, option };
  if (matchOption(poll.options, c.option) === null) return { kind: "not_an_option" };
  return { kind: "below_rank", minRank: poll.minRank };
}

/** The chip's tone: counted, refused by a rule of the poll, or with no poll to count it. */
const VOTE_TONE: Record<VoteOutcome["kind"], string> = {
  counted: "vote-counted",
  not_an_option: "vote-refused",
  below_rank: "vote-refused",
  no_open_poll: "vote-idle",
};

/** The chip a vote line wears: what the runtime made of it, in the voter's own words. */
export function voteLabel(o: VoteOutcome): string {
  switch (o.kind) {
    case "counted":
      return `VOTE ${o.key} · ${optionLabel(o.option)}`;
    case "not_an_option":
      return "VOTE · not an option";
    case "below_rank":
      return `VOTE · below ${RANK_TITLE[o.minRank]}`;
    case "no_open_poll":
      return "VOTE · no open poll";
  }
}

/** Command text without the `!cmd args` prefix the chip already shows. */
function bodyText(m: ChatMsg): string {
  const c = m.command;
  if (!c) return m.text;
  switch (c.cmd) {
    case "hint":
      return c.text;
    case "propose":
      return c.text;
    default:
      return "";
  }
}

export function ChatLane({ scene, now, title = "The Minions" }: { scene: SceneState; now: number; title?: string }) {
  // Newest at the bottom; column-reverse clips the oldest at the top without measuring. Only
  // lines the input filter has judged: the chat lane is a path to broadcast like the caption.
  const lines = shownLane(scene).slice(-16).reverse();
  return (
    <div className="panel chat-lane">
      <div className="panel-title">
        <span>{title}</span>
        <span className="panel-sub">{scene.chats.size} lines in the ledger</span>
      </div>
      <div className="chat-rows" data-clip>
        {lines.map(({ id, shownAt }) => {
          const m = scene.chats.get(id);
          if (!m) return null;
          const purged = scene.purged[id];
          const heardAt = scene.heard.get(id);
          const isDecree = scene.decrees.some((d) => d.msgId === id);
          const fresh = now - shownAt < 700;
          const text = bodyText(m);
          const outcome = voteOutcome(scene, m);
          return (
            <div
              key={id}
              className={`chat-row${fresh ? " fresh" : ""}${heardAt !== undefined ? " heard" : ""}${isDecree ? " decreed" : ""}${purged ? " purged" : ""}`}
            >
              <Sigil rank={m.viewer.rank} size={28} />
              <div className="chat-body">
                <span className="chat-name" style={{ color: RANK_COLOR[m.viewer.rank] }}>
                  {m.viewer.name}
                </span>
                {purged ? (
                  <span className="chat-text purged-text">purged by the Warden</span>
                ) : (
                  <>
                    {m.command && (
                      <span className={`cmd-chip cmd-${m.command.cmd}${outcome ? ` ${VOTE_TONE[outcome.kind]}` : ""}`}>
                        {outcome ? voteLabel(outcome) : commandLabel(m.command)}
                      </span>
                    )}
                    {text && <span className="chat-text">{text}</span>}
                  </>
                )}
              </div>
              {heardAt !== undefined && !purged && <span className="heard-tag">HEARD</span>}
            </div>
          );
        })}
      </div>
    </div>
  );
}
