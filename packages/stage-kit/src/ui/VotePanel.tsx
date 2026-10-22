import { Sigil } from "../art/Sigil";
import { RANKS } from "../vocab";
import { RANK_COLOR, RANK_TITLE, foilInfo } from "../lore";
import type { Poll, SceneState } from "../sceneModel";
import { pollBars } from "../tallies";
import { pollOn } from "./onScreen";

/** The poll that should be on screen right now, if any. */
export function visiblePoll(scene: SceneState, now: number): Poll | null {
  return pollOn(scene, now)?.value ?? null;
}

/** An option as the screen names it: a foil by its display name, anything else as given. */
export function optionLabel(o: string): string {
  return o.startsWith("the-") ? foilInfo(o).name : o;
}

export function VotePanel({ poll, now }: { poll: Poll; now: number }) {
  const bars = pollBars(poll);
  const closed = poll.closedAt !== undefined && now >= poll.closedAt;
  const left = Math.max(0, Math.ceil(poll.secs - (now - poll.openedAt) / 1000));
  const frac = poll.secs > 0 ? Math.min(1, (now - poll.openedAt) / (poll.secs * 1000)) : 1;
  const totalVoters = bars.reduce((a, b) => a + b.voters, 0);
  // While the poll is open each option shows the 1-based key the runtime counts (`!vote 2`): a
  // foil's display name is not a ballot, its id and its key are.
  const keys = closed ? null : new Map(poll.options.map((o, i) => [o, i + 1]));
  const n = poll.options.length;
  return (
    <div className={`panel vote-panel${closed ? " closed" : ""}`}>
      <div className="panel-title">
        <span>{closed ? "The vote is sealed" : "The minions vote"}</span>
        <span className="vote-gate">
          <Sigil rank={poll.minRank} size={22} />
          {RANK_TITLE[poll.minRank]}
          {poll.minRank !== "dread_council" ? "+" : ""}
        </span>
      </div>
      <div className="vote-question">{poll.question}</div>
      <div className="vote-bars">
        {bars.map((b) => (
          <div key={b.option} className={`vote-row${b.leading ? " leading" : ""}${closed && b.leading && !b.tied ? " winner" : ""}`}>
            <div className="vote-label">
              {keys?.has(b.option) && <span className="vote-key">{keys.get(b.option)}</span>}
              <span className="vote-option">{optionLabel(b.option)}</span>
              <span className="vote-num">
                {b.weight.toFixed(1)} <small>· {Math.round(b.pct * 100)}%</small>
              </span>
            </div>
            <div className="vote-track">
              <div className="vote-fill" style={{ width: `${Math.max(b.length * 100, b.weight > 0 ? 2 : 0)}%` }}>
                {b.bands.map((band) => (
                  <span
                    key={band.rank}
                    className="vote-band"
                    style={{
                      flexGrow: band.share,
                      background: band.rank === "unattributed" ? "#5b5f5a" : RANK_COLOR[band.rank],
                    }}
                  />
                ))}
              </div>
            </div>
          </div>
        ))}
      </div>
      <div className="vote-foot">
        <div className="vote-legend">
          {RANKS.map((r) => (
            <span key={r} className="legend-dot" style={{ background: RANK_COLOR[r] }} title={RANK_TITLE[r]} />
          ))}
          <span>rank-weighted · {totalVoters} voters</span>
        </div>
        {!closed && n > 0 && <span className="vote-call">{n === 1 ? "!vote 1" : `!vote 1-${n}`}</span>}
        {!closed && poll.secs > 0 && (
          <div className="vote-timer">
            <svg viewBox="0 0 36 36" width="30" height="30">
              <circle cx="18" cy="18" r="15" fill="none" stroke="#2a312e" strokeWidth="4" />
              <circle
                cx="18"
                cy="18"
                r="15"
                fill="none"
                stroke="#5fe0bf"
                strokeWidth="4"
                strokeDasharray={`${(1 - frac) * 94.2} 94.2`}
                transform="rotate(-90 18 18)"
              />
            </svg>
            <span>{left}s</span>
          </div>
        )}
      </div>
    </div>
  );
}
