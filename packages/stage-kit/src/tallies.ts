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

const GRADE_ORDER: readonly EvidenceGrade[] = EVIDENCE_GRADES;

/**
 * The community-grade tally. A hex is DISPUTED when the community's majority grade differs from
 * Malakor's (the digest's "Malakor Confidence vs Minion Jury" divergence, simplified to grades).
 */
export function gradeTallies(s: SceneState): GradeTally[] {
  const hexIds = new Set([...Object.keys(s.gradeVotes), ...s.revealOrder]);
  const out: GradeTally[] = [];
  for (const hex of hexIds) {
    const votes = s.gradeVotes[hex] ?? {};
    const counts: Record<EvidenceGrade, number> = { DOC: votes.DOC ?? 0, REP: votes.REP ?? 0, CLM: votes.CLM ?? 0, SPC: votes.SPC ?? 0 };
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
