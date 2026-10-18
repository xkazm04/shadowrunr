// The event → scene reducer. Pure: `reduce(state, event)` returns a new state and never reads a
// clock. Everything time-based in the view is derived from event timestamps (`at`) and the
// playback clock, so seeking is just "fold the events up to t".

import type {
  ChatMsg,
  EndReason,
  EvidenceGrade,
  ModelTier,
  Rank,
  SegmentId,
  StreamEvent,
  TsMs,
  VoteTally,
} from "./contract";
import { RANKS } from "./contract";
import { RANK_WEIGHT } from "./lore";

export type LayoutId = "boot" | "improv" | "hexmap" | "descoped";

export function layoutFor(segment: SegmentId): LayoutId {
  switch (segment) {
    case "SEG-01":
      return "improv";
    case "SEG-02A":
      return "hexmap";
    default:
      return "descoped";
  }
}

export interface Transition {
  id: number;
  from: LayoutId;
  to: LayoutId;
  at: TsMs;
  /** What triggered it: a handover (with Malakor's reason) or a plain segment start. */
  cause: "handover" | "segment_start";
  reason?: string;
}

export interface Caption {
  id: number;
  text: string;
  at: TsMs;
  /** Display names of the chatters this line answers. */
  replyingTo: string[];
}

export interface FoilLine {
  id: number;
  foil: string;
  text: string;
  at: TsMs;
}

export interface Decree {
  key: string;
  msgId: string;
  kind: "highlight" | "picked";
  at: TsMs;
  reason?: string;
}

export interface Poll {
  id: string;
  question: string;
  options: string[];
  minRank: Rank;
  secs: number;
  openedAt: TsMs;
  closedAt?: TsMs;
  /** Authoritative tallies from the latest `vote` event (empty until one arrives). */
  tallies: VoteTally[];
  /** Rank-weighted votes observed in chat (`!vote` / `!summon`), per option, per rank. */
  observed: Record<string, Partial<Record<Rank, number>>>;
  /** viewer id → option, so a changed vote moves rather than doubles. */
  ballots: Record<string, { option: string; rank: Rank }>;
}

export interface HexState {
  grade: EvidenceGrade;
  revealedAt: TsMs;
}

export interface Hint {
  count: number;
  lastAt: TsMs;
  lastText: string;
  lastBy: string;
}

export interface Job {
  id: string;
  brief?: string;
  status: string;
  summary?: string;
  at: TsMs;
  updatedAt: TsMs;
}

export interface Card {
  id: string;
  title: string;
  body: string;
  at: TsMs;
}

export interface Moment {
  label: string;
  at: TsMs;
}

export interface Redaction {
  key: string;
  at: TsMs;
  rule: string;
  action: string;
}

export interface Seal {
  segment: SegmentId;
  ses?: number;
  at: TsMs;
}

export interface StreamMeta {
  streamId: string;
  character: string;
  toneVariant: string;
  coreTier: ModelTier;
  audienceSize: number;
  at: TsMs;
}

export interface SceneState {
  t0: TsMs | null;
  lastTs: TsMs | null;
  /** Count of events applied (handy for tests and the debug overlay). */
  applied: number;
  meta: StreamMeta | null;
  segment: SegmentId | null;
  layout: LayoutId;
  transition: Transition | null;
  pendingSwitch: { to: SegmentId; at: TsMs } | null;
  caption: Caption | null;
  foil: { id: string; since: TsMs } | null;
  foilLine: FoilLine | null;
  chats: Record<string, ChatMsg>;
  /** Most recent chat message ids, newest last. */
  lane: string[];
  heard: Record<string, TsMs>;
  purged: Record<string, string>;
  decrees: Decree[];
  polls: Record<string, Poll>;
  activePoll: string | null;
  hexes: Record<string, HexState>;
  revealOrder: string[];
  hints: Record<string, Hint>;
  /** Community `!grade` counts per hex. */
  gradeVotes: Record<string, Partial<Record<EvidenceGrade, number>>>;
  jobs: Job[];
  card: Card | null;
  moments: Moment[];
  redactions: Redaction[];
  seals: Seal[];
  outro: { reason: string; at: TsMs } | null;
  ended: { reason: EndReason; at: TsMs } | null;
  seq: number;
}

export const LANE_MAX = 40;
export const CHAT_MEMORY_MAX = 600;
export const DECREE_MAX = 3;
export const JOB_MAX = 12;
export const MOMENT_MAX = 200;
export const REDACTION_MAX = 20;

export function initialScene(): SceneState {
  return {
    t0: null,
    lastTs: null,
    applied: 0,
    meta: null,
    segment: null,
    layout: "boot",
    transition: null,
    pendingSwitch: null,
    caption: null,
    foil: null,
    foilLine: null,
    chats: {},
    lane: [],
    heard: {},
    purged: {},
    decrees: [],
    polls: {},
    activePoll: null,
    hexes: {},
    revealOrder: [],
    hints: {},
    gradeVotes: {},
    jobs: [],
    card: null,
    moments: [],
    redactions: [],
    seals: [],
    outro: null,
    ended: null,
    seq: 0,
  };
}

function switchLayout(
  s: SceneState,
  to: LayoutId,
  at: TsMs,
  cause: Transition["cause"],
  reason?: string,
): SceneState {
  if (s.layout === to) return s;
  const seq = s.seq + 1;
  const transition: Transition = { id: seq, from: s.layout, to, at, cause };
  if (reason !== undefined) transition.reason = reason;
  return { ...s, seq, layout: to, transition };
}

/** Match a chat vote to a poll option: exact (case-insensitive) text, or a 1-based index. */
export function matchOption(options: string[], raw: string): string | null {
  const want = raw.trim().toLowerCase();
  if (want === "") return null;
  const exact = options.find((o) => o.toLowerCase() === want);
  if (exact !== undefined) return exact;
  if (/^\d+$/.test(want)) {
    const idx = Number(want) - 1;
    return options[idx] ?? null;
  }
  const prefix = options.filter((o) => o.toLowerCase().startsWith(want));
  return prefix.length === 1 ? prefix[0]! : null;
}

function castBallot(poll: Poll, msg: ChatMsg, raw: string): Poll {
  if (poll.closedAt !== undefined) return poll;
  if (RANKS.indexOf(msg.viewer.rank) < RANKS.indexOf(poll.minRank)) return poll;
  const option = matchOption(poll.options, raw);
  if (option === null) return poll;
  const observed: Poll["observed"] = {};
  for (const [k, v] of Object.entries(poll.observed)) observed[k] = { ...v };
  const prev = poll.ballots[msg.viewer.id];
  if (prev) {
    const bucket = observed[prev.option] ?? {};
    bucket[prev.rank] = Math.max(0, (bucket[prev.rank] ?? 0) - RANK_WEIGHT[prev.rank]);
    observed[prev.option] = bucket;
  }
  const bucket = observed[option] ?? {};
  bucket[msg.viewer.rank] = (bucket[msg.viewer.rank] ?? 0) + RANK_WEIGHT[msg.viewer.rank];
  observed[option] = bucket;
  return {
    ...poll,
    observed,
    ballots: { ...poll.ballots, [msg.viewer.id]: { option, rank: msg.viewer.rank } },
  };
}

function addDecree(s: SceneState, d: Decree): Decree[] {
  const rest = s.decrees.filter((x) => x.msgId !== d.msgId);
  return [...rest, d].slice(-DECREE_MAX);
}

function upsertJob(jobs: Job[], id: string, at: TsMs, patch: Partial<Job>): Job[] {
  const i = jobs.findIndex((j) => j.id === id);
  if (i === -1) {
    const job: Job = { id, status: "dispatched", at, updatedAt: at, ...patch };
    return [...jobs, job].slice(-JOB_MAX);
  }
  const next = jobs.slice();
  next[i] = { ...jobs[i]!, ...patch, updatedAt: at };
  return next;
}

function applyChat(s: SceneState, msg: ChatMsg, at: TsMs): SceneState {
  let chats = { ...s.chats, [msg.id]: msg };
  let lane = [...s.lane.filter((id) => id !== msg.id), msg.id].slice(-LANE_MAX);
  const ids = Object.keys(chats);
  if (ids.length > CHAT_MEMORY_MAX) {
    // Forget the oldest messages that nothing on screen still points at.
    const keep = new Set([...lane, ...s.decrees.map((d) => d.msgId)]);
    chats = {};
    for (const id of ids.slice(-CHAT_MEMORY_MAX)) chats[id] = s.chats[id] ?? msg;
    for (const id of keep) if (s.chats[id]) chats[id] = s.chats[id]!;
    chats[msg.id] = msg;
    lane = lane.filter((id) => id in chats);
  }
  let next: SceneState = { ...s, chats, lane };
  const cmd = msg.command;
  if (!cmd) return next;
  switch (cmd.cmd) {
    case "vote":
    case "summon": {
      const pollId = s.activePoll;
      const poll = pollId ? s.polls[pollId] : undefined;
      if (poll && pollId) {
        const raw = cmd.cmd === "vote" ? cmd.option : cmd.foil;
        next = { ...next, polls: { ...s.polls, [pollId]: castBallot(poll, msg, raw) } };
      }
      return next;
    }
    case "hint": {
      const prev = s.hints[cmd.hex];
      const hint: Hint = {
        count: (prev?.count ?? 0) + 1,
        lastAt: at,
        lastText: cmd.text,
        lastBy: msg.viewer.name,
      };
      return { ...next, hints: { ...s.hints, [cmd.hex]: hint } };
    }
    case "grade": {
      const prev = s.gradeVotes[cmd.hex] ?? {};
      const votes = { ...prev, [cmd.grade]: (prev[cmd.grade] ?? 0) + 1 };
      return { ...next, gradeVotes: { ...s.gradeVotes, [cmd.hex]: votes } };
    }
    case "propose":
      return next;
  }
}

/** Apply one event. Pure. */
export function reduce(prev: SceneState, ev: StreamEvent): SceneState {
  const at = ev.ts_ms;
  let s: SceneState = {
    ...prev,
    t0: prev.t0 ?? at,
    lastTs: prev.lastTs === null ? at : Math.max(prev.lastTs, at),
    applied: prev.applied + 1,
  };
  switch (ev.type) {
    case "stream_start":
      return {
        ...s,
        meta: {
          streamId: ev.stream_id,
          character: ev.character,
          toneVariant: ev.tone_variant,
          coreTier: ev.core_tier,
          audienceSize: ev.audience_size,
          at,
        },
      };
    case "segment_start":
      s = { ...s, segment: ev.segment, pendingSwitch: null };
      return switchLayout(s, layoutFor(ev.segment), at, "segment_start");
    case "segment_end": {
      const seal: Seal = { segment: ev.segment, at };
      if (ev.ses !== undefined) seal.ses = ev.ses;
      return { ...s, seals: [...s.seals, seal] };
    }
    case "handover":
      s = { ...s, segment: ev.to, pendingSwitch: null };
      return switchLayout(s, layoutFor(ev.to), at, "handover", ev.reason);
    case "chat":
      return applyChat(s, ev.msg, at);
    case "picked": {
      const heard = { ...s.heard };
      let decrees = s.decrees;
      for (const id of ev.msg_ids) {
        heard[id] = at;
        if (s.chats[id] && !s.purged[id]) {
          decrees = addDecree(
            { ...s, decrees },
            { key: `p${s.seq + 1}-${id}`, msgId: id, kind: "picked", at, reason: ev.reason },
          );
        }
      }
      return { ...s, heard, decrees, seq: s.seq + 1 };
    }
    case "say": {
      const seq = s.seq + 1;
      if (ev.speaker === "foil") {
        const foil = ev.foil ?? s.foil?.id ?? "foil";
        return { ...s, seq, foilLine: { id: seq, foil, text: ev.text, at } };
      }
      const replyingTo = (ev.replying_to ?? [])
        .map((id) => s.chats[id]?.viewer.name)
        .filter((n): n is string => n !== undefined);
      return { ...s, seq, caption: { id: seq, text: ev.text, at, replyingTo } };
    }
    case "op":
      return applyOp(s, ev.op, at);
    case "vote": {
      const poll = s.polls[ev.poll_id];
      if (!poll) {
        // A tally for a poll we never saw open (a mid-stream join): show it with what we know.
        const fresh: Poll = {
          id: ev.poll_id,
          question: "The minions are voting",
          options: ev.tallies.map((t) => t.option),
          minRank: "thrall",
          secs: 0,
          openedAt: at,
          tallies: ev.tallies,
          observed: {},
          ballots: {},
        };
        if (ev.closed) fresh.closedAt = at;
        return { ...s, polls: { ...s.polls, [ev.poll_id]: fresh }, activePoll: ev.poll_id };
      }
      const next: Poll = { ...poll, tallies: ev.tallies };
      if (ev.closed && next.closedAt === undefined) next.closedAt = at;
      return { ...s, polls: { ...s.polls, [ev.poll_id]: next } };
    }
    case "moderation": {
      if (ev.stage === "output") {
        const seq = s.seq + 1;
        const red: Redaction = { key: `r${seq}`, at, rule: ev.rule, action: ev.action };
        return { ...s, seq, redactions: [...s.redactions, red].slice(-REDACTION_MAX) };
      }
      if (ev.msg_id !== undefined) {
        const id = ev.msg_id;
        return {
          ...s,
          purged: { ...s.purged, [id]: ev.rule },
          decrees: s.decrees.filter((d) => d.msgId !== id),
        };
      }
      return s;
    }
    case "job": {
      const patch: Partial<Job> = { status: ev.status };
      if (ev.summary !== undefined) patch.summary = ev.summary;
      return { ...s, jobs: upsertJob(s.jobs, ev.job_id, at, patch) };
    }
    case "stream_end":
      return { ...s, ended: { reason: ev.reason, at } };
  }
}

function applyOp(s: SceneState, op: Extract<StreamEvent, { type: "op" }>["op"], at: TsMs): SceneState {
  switch (op.op) {
    case "highlight": {
      if (!s.chats[op.msg_id] || s.purged[op.msg_id]) return s;
      const seq = s.seq + 1;
      return {
        ...s,
        seq,
        decrees: addDecree(s, { key: `h${seq}`, msgId: op.msg_id, kind: "highlight", at }),
      };
    }
    case "start_poll": {
      const poll: Poll = {
        id: op.poll_id,
        question: op.question,
        options: op.options,
        minRank: op.min_rank,
        secs: op.secs,
        openedAt: at,
        tallies: op.options.map((option) => ({ option, voters: 0, weight: 0 })),
        observed: {},
        ballots: {},
      };
      return { ...s, polls: { ...s.polls, [op.poll_id]: poll }, activePoll: op.poll_id };
    }
    case "close_poll": {
      const poll = s.polls[op.poll_id];
      if (!poll) return s;
      if (poll.closedAt !== undefined) return s;
      return { ...s, polls: { ...s.polls, [op.poll_id]: { ...poll, closedAt: at } } };
    }
    case "summon_foil":
      return { ...s, foil: { id: op.foil, since: at }, foilLine: null };
    case "dismiss_foil":
      return { ...s, foil: null, foilLine: null };
    case "reveal_hex": {
      const hexes = { ...s.hexes, [op.hex]: { grade: op.grade, revealedAt: at } };
      const revealOrder = [...s.revealOrder.filter((h) => h !== op.hex), op.hex];
      return { ...s, hexes, revealOrder };
    }
    case "dispatch_research":
      return {
        ...s,
        jobs: upsertJob(s.jobs, op.job_id, at, { brief: op.brief, status: "dispatched" }),
      };
    case "show_card":
      return { ...s, card: { id: op.card_id, title: op.title, body: op.body, at } };
    case "mark_moment":
      return { ...s, moments: [...s.moments, { label: op.label, at }].slice(-MOMENT_MAX) };
    case "switch_segment":
      return { ...s, pendingSwitch: { to: op.to, at } };
    case "sign_off":
      return { ...s, outro: { reason: op.reason, at } };
  }
}

/** Fold a list of events. */
export function foldEvents(events: readonly StreamEvent[], upTo = Number.POSITIVE_INFINITY): SceneState {
  let s = initialScene();
  for (const ev of events) {
    if (ev.ts_ms > upTo) break;
    s = reduce(s, ev);
  }
  return s;
}

export interface RankBand {
  rank: Rank | "unattributed";
  /** Share of this option's bar, 0..1. */
  share: number;
}

export interface PollBar {
  option: string;
  weight: number;
  voters: number;
  /** Share of the leading option's weight, 0..1 (bar length). */
  length: number;
  /** Share of all weight, 0..1. */
  pct: number;
  bands: RankBand[];
  leading: boolean;
}

/**
 * Bars for a poll. Lengths come from the authoritative tallies; the rank colour bands inside
 * each bar come from the rank-weighted `!vote` messages seen in chat. When chat shows no votes
 * for an option, its bar is one neutral "unattributed" band.
 */
export function pollBars(poll: Poll): PollBar[] {
  const authoritative = poll.tallies.some((t) => t.weight > 0 || t.voters > 0);
  const rows = poll.options.map((option) => {
    const obs = poll.observed[option] ?? {};
    const obsTotal = RANKS.reduce((a, r) => a + (obs[r] ?? 0), 0);
    const tally = poll.tallies.find((t) => t.option === option);
    const weight = authoritative ? (tally?.weight ?? 0) : Math.round(obsTotal * 100) / 100;
    const voters = authoritative
      ? (tally?.voters ?? 0)
      : Object.values(poll.ballots).filter((b) => b.option === option).length;
    const bands: RankBand[] =
      obsTotal > 0
        ? RANKS.filter((r) => (obs[r] ?? 0) > 0).map((r) => ({ rank: r, share: (obs[r] ?? 0) / obsTotal }))
        : [{ rank: "unattributed", share: 1 }];
    return { option, weight, voters, bands };
  });
  const max = Math.max(0, ...rows.map((r) => r.weight));
  const total = rows.reduce((a, r) => a + r.weight, 0);
  return rows.map((r) => ({
    ...r,
    length: max > 0 ? r.weight / max : 0,
    pct: total > 0 ? r.weight / total : 0,
    leading: max > 0 && r.weight === max,
  }));
}

export interface GradeTally {
  hex: string;
  counts: Record<EvidenceGrade, number>;
  total: number;
  /** The community's most-voted grade (ties broken toward the stricter grade). */
  majority: EvidenceGrade | null;
  /** Malakor's grade for the hex, when revealed. */
  official: EvidenceGrade | null;
  disputed: boolean;
}

const GRADE_ORDER: EvidenceGrade[] = ["DOC", "REP", "CLM", "SPC"];

/**
 * The community-grade tally. A hex is DISPUTED when the community's majority grade differs from
 * Malakor's (the digest's "Malakor Confidence vs Minion Jury" divergence, simplified to grades).
 */
export function gradeTallies(s: SceneState): GradeTally[] {
  const hexIds = new Set([...Object.keys(s.gradeVotes), ...s.revealOrder]);
  const out: GradeTally[] = [];
  for (const hex of hexIds) {
    const votes = s.gradeVotes[hex] ?? {};
    const counts = { DOC: 0, REP: 0, CLM: 0, SPC: 0, ...votes } as Record<EvidenceGrade, number>;
    const total = GRADE_ORDER.reduce((a, g) => a + counts[g], 0);
    let majority: EvidenceGrade | null = null;
    for (const g of GRADE_ORDER) if (counts[g] > 0 && (majority === null || counts[g] > counts[majority])) majority = g;
    const official = s.hexes[hex]?.grade ?? null;
    out.push({
      hex,
      counts,
      total,
      majority,
      official,
      disputed: official !== null && majority !== null && total >= 3 && majority !== official,
    });
  }
  return out;
}
