// Display data the stage needs from other packages. These are COPIES with their source named;
// `lore.test.ts` checks them against the source files so drift fails the tests.

import { GRADE_COLORS, RANK_COLORS } from "@shadowrunr/tokens";

import type { EvidenceGrade, Rank, SegmentId } from "@shadowrunr/contract";

/** Vote weights from `crates/shadowrunr-segments/data/ranks.toml` (assumptions there, too). */
export const RANK_WEIGHT: Record<Rank, number> = {
  thrall: 1.0,
  acolyte: 1.4,
  cultist: 1.7,
  warlock: 2.0,
  lich: 2.2,
  dread_council: 2.4,
};

/** Rank titles from `characters/malakor/character.toml` `[ranks]`. */
export const RANK_TITLE: Record<Rank, string> = {
  thrall: "Thrall",
  acolyte: "Acolyte",
  cultist: "Cultist",
  warlock: "Warlock",
  lich: "Lesser Lich",
  dread_council: "Dread Council",
};

/** Rank colours: the single source is `@shadowrunr/tokens` (the Poster Wall's fidelity palette). */
export const RANK_COLOR: Record<Rank, string> = RANK_COLORS;

export interface GradeInfo {
  code: EvidenceGrade;
  name: string;
  color: string;
}

/** Evidence grades; colours from `@shadowrunr/tokens`: DOC verdigris, REP stone, CLM amber, SPC flare. */
export const GRADE: Record<EvidenceGrade, GradeInfo> = {
  DOC: { code: "DOC", name: "Documented primary", color: GRADE_COLORS.DOC },
  REP: { code: "REP", name: "Reported by press", color: GRADE_COLORS.REP },
  CLM: { code: "CLM", name: "Community claim", color: GRADE_COLORS.CLM },
  SPC: { code: "SPC", name: "Overlord speculation", color: GRADE_COLORS.SPC },
};

/** Anything below DOC is never stated as fact: the stage stamps it ALLEGED. */
export function isAlleged(grade: EvidenceGrade): boolean {
  return grade !== "DOC";
}

export const SEGMENT_TITLE: Record<SegmentId, string> = {
  "SEG-01": "The Improvised Room",
  "SEG-02A": "The Investigation",
  "SEG-02B-I": "Cartography Legion (descoped)",
  "SEG-02B-II": "Dependency Doctrine (descoped)",
};

export interface FoilInfo {
  id: string;
  name: string;
  tagline: string;
  color: string;
}

/** Foils from `characters/malakor/foils/*.md` (original parody archetypes, never real people). */
export const FOILS: Record<string, FoilInfo> = {
  "the-disruptor": {
    id: "the-disruptor",
    name: "The Disruptor",
    tagline: "Wants to disrupt world domination with an app and a rocket.",
    color: "#5fb4ff",
  },
  "the-life-coach": {
    id: "the-life-coach",
    name: "The Life Coach",
    tagline: "Treats world domination as a growth journey.",
    color: "#ff9fc8",
  },
  "the-coin-prophet": {
    id: "the-coin-prophet",
    name: "The Coin Prophet",
    tagline: "Always one airdrop away from glory. Phylactery Token: down 99%.",
    color: "#ffd25e",
  },
  "the-has-been-hero": {
    id: "the-has-been-hero",
    name: "The Has-Been Hero",
    tagline: "Saved the world nine times. Now does conventions and knee rehab.",
    color: "#ff7a59",
  },
  "the-inspector": {
    id: "the-inspector",
    name: "The Inspector",
    tagline: "Solves every case instantly, and wrongly.",
    color: "#b8c4a0",
  },
};

export function foilInfo(id: string): FoilInfo {
  return (
    FOILS[id] ?? {
      id,
      name: id
        .split(/[-_ ]+/)
        .filter(Boolean)
        .map((w) => w[0]!.toUpperCase() + w.slice(1))
        .join(" "),
      tagline: "An original guest foil.",
      color: "#c9c2b0",
    }
  );
}

/** The on-screen disclosure. Malakor speaks in text only until a voice is configured. */
export const DISCLOSURE = "AI character · text only";
export const VOICE_DISCLOSURE = "AI character · synthetic voice";

export function disclosureText(voice: boolean): string {
  return voice ? VOICE_DISCLOSURE : DISCLOSURE;
}
