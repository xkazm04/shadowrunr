// Generated, contract-shaped data for On Air's tests (imported by *.test.* only, never by the
// page). Sizes follow the 10× fixture shape: 48k runs, 100k transcript lines.
import type { Rank, RunRow, SegmentId, SegmentTables, StreamEvent } from "@shadowrunr/contract";

const RANKS: readonly Rank[] = ["thrall", "acolyte", "cultist", "warlock", "lich", "dread_council"];

export function runRow(i: number, tournament = `t-${Math.floor(i / 120)}`): RunRow {
  const segment: SegmentId = i % 2 === 0 ? "SEG-01" : "SEG-02A";
  const run_id = `run-${i}`;
  return {
    run_id,
    tournament_id: tournament,
    has_verdict: i % 3 === 0,
    spec: {
      run_id,
      audience_size: [3, 30, 300][i % 3] ?? 30,
      character: "malakor",
      core_effort: "medium",
      core_tier: i % 2 === 0 ? "haiku" : "sonnet",
      duration_s: 240 + (i % 7) * 30,
      population: "default",
      seed: i,
      segment,
      tone_variant: i % 4 === 0 ? "weary-middle-manager" : "theatrical",
    },
  };
}

/** A transcript of `n` events: a stream start, both segments, and a chatty room with ops. */
export function transcript(n: number): StreamEvent[] {
  const out: StreamEvent[] = [
    { type: "stream_start", ts_ms: 0, stream_id: "s1", character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 300 },
    { type: "segment_start", ts_ms: 10, segment: "SEG-01" },
  ];
  let t = 20;
  for (let i = out.length; i < n; i++, t += 40) {
    const k = i % 10;
    if (k === 3) out.push({ type: "picked", ts_ms: t, msg_ids: [`m${i - 1}`], reason: "top 1" });
    else if (k === 5) out.push({ type: "say", ts_ms: t, speaker: "malakor", text: `Line ${i}: per my last decree, *sigh*, the ledger is open.` });
    else if (k === 7) out.push({ type: "op", ts_ms: t, op: { op: "mark_moment", label: `beat ${i}` } });
    else if (k === 8 && i % 1000 === 8) out.push({ type: "op", ts_ms: t, op: { op: "show_card", card_id: `c${i}`, title: "The Audit", body: "Promised: 4%; Delivered: 0%" } });
    else if (k === 9 && i % 500 === 9) out.push({ type: "op", ts_ms: t, op: { op: "reveal_hex", hex: `HEX-${(i % 16) + 1}`, grade: "REP" } });
    else {
      const rank = RANKS[i % RANKS.length] ?? "thrall";
      out.push({ type: "chat", ts_ms: t, msg: { id: `m${i}`, ts_ms: t, text: `chat line ${i}`, viewer: { id: `v${i % 300}`, name: `viewer_${i % 300}`, platform: "sim", rank } } });
    }
  }
  return out;
}

export const TABLES: SegmentTables = {
  command_names: ["vote", "summon", "hint", "grade", "propose"],
  commands: [{ name: "vote", args: "<option>", what: "Vote in the open poll." }],
  op_tags: ["start_poll", "close_poll", "reveal_hex"],
  segments: [
    {
      id: "SEG-01",
      title: "Improvised Room",
      status: "pilot",
      initial: "open",
      states: ["open", "polling", "foil_active"],
      allowed_ops: ["start_poll", "close_poll", "summon_foil"],
      commands: ["vote", "summon"],
      switch_to: ["SEG-02A"],
      rules_prompt: "",
      transitions: [{ from: "open", on: "poll_opened", to: "polling" }],
      clock: { foil_vote_secs: 30, max_poll_options: 4, poll_max_secs: 60, poll_min_secs: 15, handover: { ccv_min: 15, grace_s: 30, max_elapsed_s: 1200, sustain_s: 60, to: "SEG-02A" } },
    },
    {
      id: "SEG-02A",
      title: "Hexmap Investigation",
      status: "pilot",
      initial: "fog",
      states: ["fog", "lifting"],
      allowed_ops: ["reveal_hex"],
      commands: ["hint", "grade"],
      switch_to: [],
      rules_prompt: "",
      transitions: [],
      clock: { foil_vote_secs: 30, max_poll_options: 4, poll_max_secs: 60, poll_min_secs: 15 },
    },
    { id: "SEG-02B-I", title: "Cartography Legion", status: "descoped", initial: "x", states: ["x"], allowed_ops: [], commands: [], switch_to: [], rules_prompt: "", transitions: [], clock: { foil_vote_secs: 0, max_poll_options: 0, poll_max_secs: 0, poll_min_secs: 0 } },
    { id: "SEG-02B-II", title: "Dependency Doctrine", status: "descoped", initial: "x", states: ["x"], allowed_ops: [], commands: [], switch_to: [], rules_prompt: "", transitions: [], clock: { foil_vote_secs: 0, max_poll_options: 0, poll_max_secs: 0, poll_min_secs: 0 } },
  ],
};
