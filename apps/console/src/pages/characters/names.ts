// Every label the package's Parameters tab shows. Wire ids (ranks, segments, provenance, face
// renderers, tuning keys) stay on the wire and never reach the screen: they are named here.
import type { Provenance, Rank, SegmentId } from "@shadowrunr/contract";

export const RANK_ORDER: readonly Rank[] = ["thrall", "acolyte", "cultist", "warlock", "lich", "dread_council"];

/** The engine's plain rank names, for a package that gives a rank no title. */
export const RANK_NAME: Readonly<Record<Rank, string>> = {
  thrall: "Thrall",
  acolyte: "Acolyte",
  cultist: "Cultist",
  warlock: "Warlock",
  lich: "Lich",
  dread_council: "Dread Council",
};

/** A rank as this character says it: its `[ranks]` title, else the engine's name. */
export const rankLabel = (titles: Readonly<Partial<Record<Rank, string>>>, r: Rank): string => titles[r] ?? RANK_NAME[r];

export const SEGMENT_NAME: Readonly<Record<SegmentId, string>> = {
  "SEG-01": "Improv room",
  "SEG-02A": "Investigation",
  "SEG-02B-I": "Bounty hunt, part one",
  "SEG-02B-II": "Bounty hunt, part two",
};

export const PROVENANCE_NAME: Readonly<Record<Provenance, string>> = { measured: "Measured", assumption: "Assumption" };

/** `[avatar] face`: the stage's face renderers. */
export const FACE_NAME: Readonly<Record<string, string>> = { glow: "Glow rig", cut: "Cut-out portrait", depth: "Depth portrait" };
export const faceLabel = (f: string): string => FACE_NAME[f] ?? "Unknown renderer";

/** A segment state as words (`cold_open` -> "cold open"). */
export const stateLabel = (s: string): string => s.replace(/_/g, " ");

/** A tuning value in its key's unit: `_ms` and `_s` keys read as seconds, others as counts. */
export function tuningValue(key: string, v: number): string {
  if (key.endsWith("_ms")) return `${(v / 1000).toLocaleString("en-US", { maximumFractionDigits: 1 })} s`;
  if (key.endsWith("_s")) return `${v.toLocaleString("en-US")} s`;
  return v.toLocaleString("en-US");
}

/** A tuning row's label: its one-line doc without the trailing unit note (", ms." / ", s."). */
export const tuningLabel = (doc: string): string => doc.replace(/,\s*(ms|s)\.?\s*$/, "").replace(/\.\s*$/, "");
