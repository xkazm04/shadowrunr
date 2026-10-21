// Test-only generators of contract-shaped Segments & ranks data (imported by *.test.ts[x] only,
// never by the page, so nothing here reaches a bundle). The base shape mirrors the compiled-in
// tables (crates/shadowrunr-segments/data); `scale` repeats it to stress the caps. SegmentId is
// a closed enum in the contract, so scaled segments reuse the four real ids.
import type { CommandInfo, RankRow, RankTableInfo, ReconcileReport, SegmentId, SegmentStatus, SegmentTableInfo, SegmentTables } from "@shadowrunr/contract";

const OP_TAGS = ["highlight", "start_poll", "close_poll", "summon_foil", "dismiss_foil", "reveal_hex", "dispatch_research", "show_card", "mark_moment", "switch_segment", "sign_off", "show_screen", "pin_link"];
const COMMANDS = ["summon", "hint", "grade", "propose", "vote"];

const seg01: SegmentTableInfo = {
  id: "SEG-01",
  title: "Improvised Room",
  status: "pilot",
  initial: "opening",
  states: ["opening", "foil_vote", "foil_live", "crowd_game", "handover_pending"],
  allowed_ops: ["highlight", "start_poll", "close_poll", "summon_foil", "dismiss_foil", "show_card", "mark_moment", "switch_segment", "sign_off"],
  screens: ["host"],
  commands: ["summon", "propose", "vote"],
  switch_to: ["SEG-02A"],
  rules_prompt: "SEGMENT SEG-01: THE IMPROVISED ROOM.\nYou open the show and warm the room.",
  clock: { foil_vote_secs: 60, poll_min_secs: 10, poll_max_secs: 600, max_poll_options: 6, handover: { to: "SEG-02A", ccv_min: 15, sustain_s: 180, max_elapsed_s: 1500, grace_s: 60 }, cold_open: { to: "opening", min_s: 60, max_s: 180, active_chatters: 5, window_s: 60, sustain_s: 20 } },
  state_rules: { cold_open: { allowed_ops: ["highlight", "show_card", "mark_moment", "sign_off"], prompt: "The room is still filling.", screens: ["host"], commands: ["propose"] } },
  transitions: [
    { from: "opening", on: "foil_vote_opened", to: "foil_vote" },
    { from: "foil_vote", on: "foil_vote_closed", guard: "foil_active", to: "foil_live" },
    { from: "foil_vote", on: "foil_vote_closed", guard: "no_foil", to: "opening" },
    { from: "*", on: "handover_due", to: "handover_pending" },
  ],
};

const seg02a: SegmentTableInfo = {
  id: "SEG-02A",
  title: "Hexmap Investigation",
  status: "pilot",
  initial: "briefing",
  states: ["briefing", "investigating", "jury_poll"],
  allowed_ops: ["highlight", "start_poll", "close_poll", "reveal_hex", "dispatch_research", "show_card", "mark_moment", "sign_off"],
  commands: ["hint", "grade", "vote"],
  switch_to: [],
  rules_prompt: "SEGMENT SEG-02A: THE HEXMAP INVESTIGATION.",
  clock: { foil_vote_secs: 60, poll_min_secs: 10, poll_max_secs: 600, max_poll_options: 6, max_searches_per_hex: 2, plebiscite_secs: 60 },
  arc: { min_planned_s: 3600, acts: ["Anomaly and filing map", "Shell and archive trail", "Dossier sealed"], seal_s: 600, reveal_pacing: true, reveal_gap_pct: 50, reveal_needs_work: true, recap_cooldown_s: 900 },
  transitions: [{ from: "briefing", on: "hex_revealed", to: "investigating" }],
};

const descopedSeg = (id: SegmentId, title: string): SegmentTableInfo => ({
  id,
  title,
  status: "descoped",
  initial: "descoped",
  states: ["descoped"],
  allowed_ops: [],
  commands: [],
  switch_to: [],
  rules_prompt: "",
  design_notes: `${title}: descoped from v0.1.`,
  clock: { foil_vote_secs: 60, poll_min_secs: 10, poll_max_secs: 600, max_poll_options: 6 },
  transitions: [],
});

/** The daemon's command table (crates/shadowrunr-segments/src/command.rs COMMAND_SPECS). */
export const COMMAND_INFO: CommandInfo[] = [
  { name: "summon", args: "<foil>", what: "Call a guest foil into the room. The weighted vote decides." },
  { name: "hint", args: "<hex> <text>", what: "Nominate a lead on the hexmap." },
  { name: "grade", args: "<hex> <DOC|REP|CLM|SPC>", what: "Grade a hex. Jury advice, never proof." },
  { name: "propose", args: "<text>", what: "Propose a scheme. Rank-gated.", min_rank: "warlock" },
  { name: "vote", args: "<option>", what: "Vote in the open poll. Weight rises with rank." },
];

export const BASE_SEGMENTS: readonly SegmentTableInfo[] = [seg01, seg02a, descopedSeg("SEG-02B-I", "Cartography Legion"), descopedSeg("SEG-02B-II", "Dependency Doctrine")];

const STATUSES: readonly SegmentStatus[] = ["pilot", "trial", "regular", "flagship", "retired", "descoped"];

/** The compiled-in tables; `scale` > 0 appends that many more segments across every status. */
export function genTables(scale = 0): SegmentTables {
  const extra = Array.from({ length: scale }, (_, i): SegmentTableInfo => {
    const base = BASE_SEGMENTS[i % 2] ?? seg01;
    return { ...base, title: `${base.title} ${i}`, status: STATUSES[i % STATUSES.length] ?? "pilot" };
  });
  return { segments: [...BASE_SEGMENTS, ...extra], op_tags: OP_TAGS, command_names: COMMANDS, commands: COMMAND_INFO };
}

/** `ranks.table` for Malakor (crates/shadowrunr-segments/data/ranks.toml + character titles). */
export function genRanks(): RankTableInfo {
  return {
    character: "malakor",
    propose_min_rank: "warlock",
    ranks: [
      { rank: "thrall", title: "Thrall", weight: 1 },
      { rank: "acolyte", title: "Acolyte", weight: 1.4, tiers: [{ tier: 1, min_months: 0 }] },
      { rank: "cultist", title: "Cultist", weight: 1.7, tiers: [{ tier: 1, min_months: 3 }, { tier: 2, min_months: 0 }] },
      { rank: "warlock", title: "Warlock", weight: 2, min_gifted: 5, tiers: [{ tier: 1, min_months: 12 }, { tier: 2, min_months: 6 }, { tier: 3, min_months: 0 }] },
      { rank: "lich", title: "Lesser Lich", weight: 2.2, min_gifted: 20, tiers: [{ tier: 2, min_months: 12 }, { tier: 3, min_months: 6 }] },
      { rank: "dread_council", title: "Dread Council", weight: 2.4, min_gifted: 50, tiers: [{ tier: 3, min_months: 12 }] },
    ],
  };
}

const bound = (lo: number, hi: number) => ({ lo, hi, method: "wilson" as const });

export const BY_RANK: readonly RankRow[] = [
  { rank: "thrall", viewers: 41, messages: 212, picked: 64, answered: 51, pick_rate: { rate: 0.3, n: 212, bound: bound(0.24, 0.37) }, answer_rate: { rate: 0.24, n: 212, bound: bound(0.19, 0.3) } },
  { rank: "warlock", viewers: 6, messages: 30, picked: 9, answered: 6, pick_rate: { rate: 0.3, n: 30, bound: bound(0.17, 0.48) }, answer_rate: { rate: 0.2, n: 30, bound: bound(0.1, 0.37) } },
  { rank: "dread_council", viewers: 2, messages: 14, picked: 1, answered: 0 },
];

export function genReport(byRank: readonly RankRow[] = BY_RANK): ReconcileReport {
  return {
    report_id: "rec-1792284840627",
    created_ms: 1792284840627,
    source: { tournament_id: "pilot-d1ef98d3a6" },
    summary: "My Lord, the room held.",
    segments: [],
    tone: [],
    ranks: { by_rank: [...byRank] },
    red_lines: [],
    latency: { errors: 0, fallbacks: 0, ledger_rows: 0, say_n: 0 },
  };
}
