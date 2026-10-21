// The screen's ballot rule: which chat command counts in which poll, for which option. It is a
// copy of the runtime's poll rules (`Poll::option_index` and `ingest_chat` in
// shadowrunr-segments), pinned to them by vectors the runtime itself generates
// (`generated/ballot-vectors.json`, checked by ballots.test.ts). The screen may not count a
// ballot the runtime refuses, or it crowns and bands an option the host never announces.

import type { Command, VoteTally } from "@shadowrunr/contract";
import { RANK_WEIGHT } from "./lore";
import type { PendingBallot, Poll, Voter } from "./sceneModel";
import { RANKS } from "./vocab";

/**
 * Screen-side gaps from the runtime's rule the project accepts, each declared in the vectors
 * against the scenario that shows it. Every other scenario must agree exactly.
 * - `command_from_text`: a chat line counts only by its parsed `command`; the runtime also
 *   parses the text of a line the adapter left without one.
 * - `poll_kind_from_id`: the stream does not carry a poll's kind, so a foil vote is recognised
 *   by its runtime-minted `foil-vote-` id.
 */
export const BALLOT_DEVIATIONS: readonly string[] = ["command_from_text", "poll_kind_from_id"];

/** The id prefix the runtime mints for the foil vote a `!summon` opens. */
export const FOIL_VOTE_PREFIX = "foil-vote-";

/** ASCII-only lower case: the runtime folds case with `eq_ignore_ascii_case`. */
function asciiLower(s: string): string {
  return s.replace(/[A-Z]/g, (c) => String.fromCharCode(c.charCodeAt(0) + 32));
}

/**
 * Match a chat vote to a poll option by the runtime's rule: a number (Rust's `usize` syntax, an
 * optional `+`) within 1..=options is a 1-based index; otherwise the trimmed text must equal an
 * option's trimmed text, ASCII case folded. No prefixes.
 */
export function matchOption(options: readonly string[], raw: string): string | null {
  const want = raw.trim();
  if (/^\+?\d+$/.test(want)) {
    const n = Number(want);
    if (n >= 1 && n <= options.length) return options[n - 1]!;
  }
  const folded = asciiLower(want);
  return options.find((o) => asciiLower(o.trim()) === folded) ?? null;
}

/**
 * The option a chat command votes for in `poll`, or null when the runtime would not count it:
 * the poll is closed, the option is unknown, a `!vote` is below the poll's min rank, or a
 * `!summon` lands outside a foil vote.
 */
export function ballotOption(poll: Poll, voter: Voter, cmd: Command): string | null {
  if (poll.closedAt !== undefined) return null;
  switch (cmd.cmd) {
    case "vote": {
      const option = matchOption(poll.options, cmd.option);
      if (option === null) return null;
      return RANKS.indexOf(voter.rank) < RANKS.indexOf(poll.minRank) ? null : option;
    }
    case "summon":
      return poll.id.startsWith(FOIL_VOTE_PREFIX) ? matchOption(poll.options, cmd.foil) : null;
    default:
      return null;
  }
}

/** Cast `cmd` into `poll` when it is a ballot there; a viewer's later ballot replaces the earlier. */
export function castBallot(poll: Poll, voter: Voter, cmd: Command): Poll {
  const option = ballotOption(poll, voter, cmd);
  if (option === null) return poll;
  const observed: Poll["observed"] = {};
  for (const [k, v] of Object.entries(poll.observed)) observed[k] = { ...v };
  const prev = poll.ballots[voter.id];
  if (prev) {
    const bucket = observed[prev.option] ?? {};
    bucket[prev.rank] = Math.max(0, (bucket[prev.rank] ?? 0) - RANK_WEIGHT[prev.rank]);
    observed[prev.option] = bucket;
  }
  const bucket = observed[option] ?? {};
  bucket[voter.rank] = (bucket[voter.rank] ?? 0) + RANK_WEIGHT[voter.rank];
  observed[option] = bucket;
  return {
    ...poll,
    observed,
    ballots: { ...poll.ballots, [voter.id]: { option, rank: voter.rank } },
  };
}

/**
 * A foil vote the runtime just opened, with the ballots of the chat batch that opened it. The
 * runtime opens it at the batch's first `!summon` naming a known foil and counts that line and
 * every ballot after it in the batch; lines before it found no poll and were dropped.
 */
export function openFoilVote(poll: Poll, pending: readonly PendingBallot[]): Poll {
  if (!poll.id.startsWith(FOIL_VOTE_PREFIX)) return poll;
  const opener = pending.findIndex((b) => b.cmd.cmd === "summon" && matchOption(poll.options, b.cmd.foil) !== null);
  if (opener === -1) return poll;
  let out = poll;
  for (const b of pending.slice(opener)) out = castBallot(out, b.voter, b.cmd);
  return out;
}

/** The tally of the ballots the screen saw in chat, in the runtime's `VoteTally` shape. */
export function observedTally(poll: Poll): VoteTally[] {
  const voters = new Map<string, number>();
  for (const b of Object.values(poll.ballots)) voters.set(b.option, (voters.get(b.option) ?? 0) + 1);
  return poll.options.map((option) => {
    const obs = poll.observed[option] ?? {};
    const weight = RANKS.reduce((a, r) => a + (obs[r] ?? 0), 0);
    return { option, voters: voters.get(option) ?? 0, weight };
  });
}
