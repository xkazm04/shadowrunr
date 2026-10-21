// The event → scene reducer. Pure: `reduce(state, event)` returns a new state and never reads a
// clock. Everything time-based in the view is derived from event timestamps (`at`) and the
// playback clock, so seeking is just "fold the events up to t".

import type { ChatMsg, ScreenId, SegmentId, ShownRecord, StreamEvent } from "@shadowrunr/contract";
import type { ChatMemory } from "./chatMemory";
import { RANK_WEIGHT } from "./lore";
import {
  BROWSER_TABS_MAX,
  CAPTION_QUEUE_MAX,
  CHAT_MEMORY_MAX,
  DECREE_MAX,
  JOB_MAX,
  LANE_MAX,
  MOMENT_MAX,
  QUEUED_HOLD_MS,
  RECORD_MAX,
  REDACTION_MAX,
  defaultScreen,
  initialScene,
  type Decree,
  type Caption,
  type Hint,
  type Job,
  type Poll,
  type Redaction,
  type SceneState,
  type ScheduledCaption,
  type Seal,
  type Transition,
} from "./sceneModel";
import { wordTimings } from "./timing";
import { RANKS, type TsMs } from "./vocab";

function setScreen(
  s: SceneState,
  to: ScreenId,
  at: TsMs,
  cause: Transition["cause"],
  segment?: SegmentId,
  reason?: string,
): SceneState {
  if (s.layout === to) return s.screen === to ? s : { ...s, screen: to };
  const seq = s.seq + 1;
  const transition: Transition = { id: seq, from: s.layout, to, at, cause };
  if (segment !== undefined) transition.segment = segment;
  if (reason !== undefined) transition.reason = reason;
  return { ...s, seq, screen: to, layout: to, transition };
}

/** Bring a received record to the front of the browser window. */
function openRecord(s: SceneState, id: string): SceneState {
  const tabs = [id, ...s.tabs.filter((t) => t !== id)].slice(0, BROWSER_TABS_MAX);
  return { ...s, tabs, activeRecord: id };
}

/** Remember a record, forgetting the oldest beyond RECORD_MAX (never an open tab). */
function keepRecord(s: SceneState, record: ShownRecord): Record<string, ShownRecord> {
  const records: Record<string, ShownRecord> = { ...s.records };
  delete records[record.id];
  records[record.id] = record;
  let excess = Object.keys(records).length - RECORD_MAX;
  for (const id of Object.keys(records)) {
    if (excess <= 0) break;
    if (id === record.id || s.tabs.includes(id)) continue;
    delete records[id];
    excess--;
  }
  return records;
}

/**
 * `show_screen`. `host` and `hexmap` switch at once. `browser` and `desk` need a record: the one
 * named, else the active one, else the latest received; until it has arrived the screen waits on
 * hexmap and the record's `record_shown` completes the switch. A `focus` zooms the map on `hexmap`
 * and `desk`, and is ignored elsewhere; without one the map is the overview.
 */
function showScreen(s: SceneState, wire: ScreenId, recordId: string | undefined, focus: string | undefined, at: TsMs): SceneState {
  // Until the evidence board is drawn (appendix (screens and board)), the board shows as the map.
  const screen = wire === "board" ? "hexmap" : wire;
  s = { ...s, mapFocus: screen === "hexmap" || screen === "desk" ? (focus ?? null) : null };
  if (screen === "host" || screen === "hexmap") return setScreen({ ...s, pendingScreen: null }, screen, at, "screen");
  const latest = Object.keys(s.records).pop();
  const id = recordId ?? s.activeRecord ?? latest;
  if (id !== undefined && s.records[id]) return setScreen(openRecord({ ...s, pendingScreen: null }, id), screen, at, "screen");
  const pendingScreen: SceneState["pendingScreen"] = { screen, at };
  if (recordId !== undefined) pendingScreen.recordId = recordId;
  return setScreen({ ...s, pendingScreen }, "hexmap", at, "screen");
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

/** Input-filter actions that let a named message through: its text stays on screen. Every
 * other action (`dropped`, `bleeped`, `tagged`, `priority:safety`, anything new) withholds it. */
function letThrough(action: string): boolean {
  return action === "warned" || action === "ignored" || action.startsWith("exempt:") || action.startsWith("collapsed:");
}

/** A verdict of the input filter's beat at `at`: that beat is open until a later event arrives. */
function judgeTo(s: SceneState, at: TsMs): SceneState {
  const j = s.judged;
  if ((j.open !== null && at <= j.open) || (j.to !== null && at <= j.to)) return s;
  return { ...s, judged: { ...j, open: at } };
}

/** Close the open beat once an event later than it arrives: every verdict of that beat is in. */
function closeBeat(s: SceneState, at: TsMs): SceneState {
  const j = s.judged;
  if (j.open === null || at <= j.open) return s;
  return { ...s, judged: { to: j.open, prev: j.to, open: null, closedAt: at } };
}

export interface ShownLine {
  readonly id: string;
  /** When the line came on screen: when the beat that judged it closed (or the line's own time
   * for a line an earlier beat already released). */
  readonly shownAt: TsMs;
}

/** The lane lines the screen may draw, oldest first: those the runtime's input filter has judged.
 * A line waits for the beat that filters it, so a message the filter drops is never drawn with
 * its text, not even for the seconds between arriving and being dropped. */
export function shownLane(s: SceneState): ShownLine[] {
  const out: ShownLine[] = [];
  const j = s.judged;
  for (const id of s.lane) {
    const m = s.chats.get(id);
    if (!m) continue;
    const named = s.purged[id] !== undefined || s.heard.has(id);
    if ((j.to === null || m.ts_ms > j.to) && !named) continue;
    const shownAt = j.closedAt === null || (j.prev !== null && m.ts_ms <= j.prev) ? m.ts_ms : Math.max(m.ts_ms, j.closedAt);
    out.push({ id, shownAt });
  }
  return out;
}

/** Keep only the entries whose message is still remembered. */
function pruneRecord<V>(rec: Record<string, V>, chats: ChatMemory): Record<string, V> {
  const out: Record<string, V> = {};
  for (const [id, v] of Object.entries(rec)) if (chats.has(id)) out[id] = v;
  return out;
}

function applyChat(s: SceneState, msg: ChatMsg, at: TsMs): SceneState {
  let lane = [...s.lane.filter((id) => id !== msg.id), msg.id].slice(-LANE_MAX);
  const grew = s.chats.has(msg.id) ? 0 : 1;
  const chats = s.chats.with(msg, CHAT_MEMORY_MAX, () => new Set([...lane, ...s.decrees.map((d) => d.msgId)]));
  let heard = s.heard;
  let purged = s.purged;
  if (chats.size < s.chats.size + grew) {
    // The memory forgot its oldest lines: drop what pointed at them.
    lane = lane.filter((id) => chats.has(id));
    const h = new Map<string, TsMs>();
    for (const [id, t] of heard) if (chats.has(id)) h.set(id, t);
    heard = h;
    purged = pruneRecord(purged, chats);
  }
  let next: SceneState = { ...s, chats, lane, heard, purged };
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
      const votes = { ...s.gradeVotes[cmd.hex], [msg.viewer.id]: cmd.grade };
      return { ...next, gradeVotes: { ...s.gradeVotes, [cmd.hex]: votes } };
    }
    case "propose":
      return next;
  }
}

/**
 * Put a caption on the queue. What showed before `c.at` and has since been followed leaves; the
 * one showing at `c.at` stays; `c` waits behind those still queued, and when more than
 * CAPTION_QUEUE_MAX wait the oldest waiting is skipped (never the one showing). Each waiting
 * caption shows once the one before has revealed and held QUEUED_HOLD_MS, or when said if later.
 */
export function enqueueCaption(queue: readonly ScheduledCaption[], c: Caption): ScheduledCaption[] {
  let current = -1;
  for (let i = 0; i < queue.length; i++) if (queue[i]!.at <= c.at) current = i;
  const showing = current >= 0 ? [queue[current]!] : [];
  const waiting: ScheduledCaption[] = [
    ...queue.slice(current + 1),
    { ...c, saidAt: c.at, spokenMs: wordTimings(c.text).spokenMs },
  ];
  while (waiting.length > CAPTION_QUEUE_MAX) waiting.shift();
  const out = showing;
  for (const w of waiting) {
    const prev = out[out.length - 1];
    const at = prev ? Math.max(w.saidAt, prev.at + prev.spokenMs + QUEUED_HOLD_MS) : w.saidAt;
    out.push({ ...w, at });
  }
  return out;
}

/** The caption showing at `now` (its `at` is when it started to show), or null. */
export function captionAt(s: SceneState, now: TsMs): ScheduledCaption | null {
  let found: ScheduledCaption | null = null;
  for (const c of s.captions) {
    if (c.at > now) break;
    found = c;
  }
  return found;
}

/** Apply one event. Pure. */
export function reduce(prev: SceneState, ev: StreamEvent): SceneState {
  const at = ev.ts_ms;
  let s: SceneState = closeBeat(
    {
      ...prev,
      t0: prev.t0 ?? at,
      lastTs: prev.lastTs === null ? at : Math.max(prev.lastTs, at),
      applied: prev.applied + 1,
    },
    at,
  );
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
    case "segment_start": {
      // The segment_start that follows its handover keeps the screen the handover chose.
      const fresh = prev.segment !== ev.segment || prev.layout === "boot";
      s = { ...s, segment: ev.segment, pendingSwitch: null };
      if (!fresh) return s;
      return setScreen({ ...s, pendingScreen: null, mapFocus: null }, defaultScreen(ev.segment), at, "segment_start", ev.segment);
    }
    case "segment_end": {
      const seal: Seal = { segment: ev.segment, at };
      if (ev.ses !== undefined) seal.ses = ev.ses;
      return { ...s, seals: [...s.seals, seal] };
    }
    case "case_open":
      return { ...s, caseFile: ev.case };
    // The board's strings (appendix (screens and board)): drawn by the board package.
    case "link_pinned":
      return s;
    case "record_shown": {
      s = { ...s, records: keepRecord(s, ev.record) };
      const p = s.pendingScreen;
      if (!p || (p.recordId !== undefined && p.recordId !== ev.record.id)) return s;
      return setScreen(openRecord({ ...s, pendingScreen: null }, ev.record.id), p.screen, at, "screen");
    }
    case "handover":
      s = { ...s, segment: ev.to, pendingSwitch: null, pendingScreen: null, mapFocus: null };
      return setScreen(s, defaultScreen(ev.to), at, "handover", ev.to, ev.reason);
    case "chat":
      return applyChat(s, ev.msg, at);
    case "picked": {
      s = judgeTo(s, at);
      const heard = new Map(s.heard);
      let decrees = s.decrees;
      for (const id of ev.msg_ids) {
        heard.set(id, at);
        if (s.chats.has(id) && !s.purged[id]) {
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
        .map((id) => s.chats.get(id)?.viewer.name)
        .filter((n): n is string => n !== undefined);
      const caption: Caption = { id: seq, text: ev.text, at, replyingTo };
      return { ...s, seq, caption, captions: enqueueCaption(s.captions, caption) };
    }
    case "op":
      return applyOp(s, ev.op, at);
    case "vote": {
      // A jury tally is not a poll: it feeds the hex's grade tally and never takes the panel.
      if (ev.poll_id.startsWith("grade:")) {
        return { ...s, jury: { ...s.jury, [ev.poll_id.slice("grade:".length)]: ev.tallies } };
      }
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
      // Only a verdict on a message marks the filter's beat; a stall or name notice can fall between.
      if (ev.msg_id !== undefined) s = judgeTo(s, at);
      if (ev.msg_id !== undefined && !letThrough(ev.action)) {
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
    case "beat_plan":
    case "health":
      return s;
    case "stream_end":
      return { ...s, ended: { reason: ev.reason, at } };
  }
}

function applyOp(s: SceneState, op: Extract<StreamEvent, { type: "op" }>["op"], at: TsMs): SceneState {
  switch (op.op) {
    case "highlight": {
      if (!s.chats.has(op.msg_id) || s.purged[op.msg_id]) return s;
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
    case "show_screen":
      return showScreen(s, op.screen, op.record_id, op.focus, at);
    case "pin_link":
      return s;
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
