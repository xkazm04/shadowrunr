// The evidence board's display names (appendix (screens and board)). The board never draws a
// role or relation id: what a card is and what a string says come from here.

import type { HexKind, HexRole, LinkRel } from "@shadowrunr/contract";

/** What a face-up card is, above its label. */
export const ROLE_LABEL: Record<HexRole, string> = {
  victim: "Victim",
  promoter: "Promoter",
  shell: "Shell company",
  processor: "Payment processor",
  regulator: "Regulator",
  claim: "Claim",
  wallet: "Wallet",
  exchange: "Exchange",
  person: "Person",
  company: "Company",
};

/** What a card without a role is, by its hex kind. */
export const KIND_NAME: Record<HexKind, string> = {
  entity: "Entity",
  wallet: "Wallet",
  domain: "Website",
  shell_company: "Shell company",
  claim: "Claim",
};

/** The glyph a card without a role wears, by its hex kind. */
export const KIND_ROLE: Record<HexKind, HexRole> = {
  entity: "company",
  wallet: "wallet",
  domain: "company",
  shell_company: "shell",
  claim: "claim",
};

/** What a string says, read from its `from` card to its `to` card. */
export const REL_LABEL: Record<LinkRel, string> = {
  paid: "paid",
  owns: "owns",
  controls: "controls",
  promoted: "promoted",
  processed: "processed for",
  regulates: "regulates",
  claims: "claims",
  supports: "supports",
  contradicts: "contradicts",
};

/** The board's regions when a case names no acts: its hexes grouped by kind, in this order. */
export const KIND_REGION: readonly (readonly [HexKind, string])[] = [
  ["claim", "The claims"],
  ["entity", "The players"],
  ["domain", "The websites"],
  ["shell_company", "The shells"],
  ["wallet", "The money"],
];

/** The region for case hexes no act names, and hexes the case does not have. */
export const LOOSE_REGION = "Loose ends";
