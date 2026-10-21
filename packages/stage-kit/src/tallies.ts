// Derived views of a scene: poll bars (authoritative tallies, rank bands from chat) and the
// community-grade tallies of the hexmap.

import type { EvidenceGrade, Rank } from "@shadowrunr/contract";
import type { Poll, SceneState } from "./sceneModel";
import { EVIDENCE_GRADES, RANKS } from "./vocab";

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
  /** Ahead by the runtime's rule: more weight, then more voters. Every level option leads. */
  leading: boolean;
  /** Level with another option on weight and voters: a tie the host breaks, no winner. */
  tied: boolean;
}

/**
 * The options that lead a tally, by the runtime's rule (`outcome` in shadowrunr-segments):
 * highest weight wins, equal weight goes to more voters, options level on both tie. Options
 * nobody voted for never lead, whatever weight a row carries (`outcome` skips `voters == 0`). The screen reads this rather than its own rule, so the bar it
 * crowns is the result the host announces.
 */
export function leaders(rows: readonly { option: string; weight: number; voters: number }[]): string[] {
  let top: { option: string; weight: number; voters: number }[] = [];
  for (const t of rows) {
    if (t.voters <= 0) continue;
    const b = top[0];
    if (!b) {
      top = [t];
      continue;
    }
    const level = Math.abs(t.weight - b.weight) <= 1e-9;
    if (t.weight > b.weight + 1e-9 || (level && t.voters > b.voters)) top = [t];
    else if (level && t.voters === b.voters) top.push(t);
  }
  return top.map((t) => t.option);
}

/**
 * Bars for a poll. Lengths come from the authoritative tallies; the rank colour bands inside
 * each bar come from the rank-weighted ballots seen in chat, counted by the runtime's rule
 * (ballots.ts, pinned to runtime-generated vectors). When chat shows no votes for an option,
 * its bar is one neutral "unattributed" band.
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
      : (poll.observedVoters[option] ?? 0);
    const bands: RankBand[] =
      obsTotal > 0
        ? RANKS.filter((r) => (obs[r] ?? 0) > 0).map((r) => ({ rank: r, share: (obs[r] ?? 0) / obsTotal }))
        : [{ rank: "unattributed", share: 1 }];
    return { option, weight, voters, bands };
  });
  const max = Math.max(0, ...rows.map((r) => r.weight));
  const total = rows.reduce((a, r) => a + r.weight, 0);
  const lead = leaders(rows);
  return rows.map((r) => ({
    ...r,
    length: max > 0 ? r.weight / max : 0,
    pct: total > 0 ? r.weight / total : 0,
    leading: lead.includes(r.option),
    tied: lead.length > 1 && lead.includes(r.option),
  }));
}

export interface GradeTally {
  hex: string;
  counts: Record<EvidenceGrade, number>;
  total: number;
  /** The community's leading grade by the runtime's rule; null when the jury is level. */
  majority: EvidenceGrade | null;
  /** Malakor's grade for the hex, when revealed. */
  official: EvidenceGrade | null;
  disputed: boolean;
}

const GRADE_ORDER: readonly EvidenceGrade[] = EVIDENCE_GRADES;

/**
 * The community-grade tally. A hex is DISPUTED when the community's majority grade differs from
 * Malakor's (the digest's "Malakor Confidence vs Minion Jury" divergence, simplified to grades).
 * The runtime's jury tally decides once it arrives (one grade per viewer, rank-weighted, a level
 * jury leads with nothing); until then the grades seen in chat stand in, one per viewer.
 */
export function gradeTallies(s: SceneState): GradeTally[] {
  const hexIds = new Set([...Object.keys(s.gradeVotes), ...Object.keys(s.jury), ...s.revealOrder]);
  const out: GradeTally[] = [];
  for (const hex of hexIds) {
    const counts: Record<EvidenceGrade, number> = { DOC: 0, REP: 0, CLM: 0, SPC: 0 };
    const tallies = s.jury[hex];
    let rows: { option: string; weight: number; voters: number }[];
    if (tallies) {
      rows = GRADE_ORDER.map((g) => {
        const t = tallies.find((x) => x.option === g);
        return { option: g, weight: t?.weight ?? 0, voters: t?.voters ?? 0 };
      });
    } else {
      Object.assign(counts, s.gradeVotes[hex]?.counts);
      rows = GRADE_ORDER.map((g) => ({ option: g, weight: counts[g], voters: counts[g] }));
    }
    for (const r of rows) counts[r.option as EvidenceGrade] = r.voters;
    const total = GRADE_ORDER.reduce((a, g) => a + counts[g], 0);
    const lead = leaders(rows);
    const majority = lead.length === 1 ? (lead[0] as EvidenceGrade) : null;
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
