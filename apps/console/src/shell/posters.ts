import { PAGE_IDS, type PageId } from "../router";
import type { PosterCopy } from "./types";

/** The season wall's copy, from the winner's PAGES (colours live in @shadowrunr/tokens). */
export const POSTERS: Readonly<Record<PageId, PosterCopy>> = {
  onair: { name: "On Air", short: "Air", tag: "The Broadcast", lines: ["On", "Air"] },
  characters: { name: "Characters", short: "Cast", tag: "The Cast", lines: ["The", "Cast"] },
  brain: { name: "Brain", short: "Brain", tag: "The Brain", lines: ["The", "Brain"] },
  segments: { name: "Segments & ranks", short: "Ranks", tag: "The Programme", lines: ["The", "Pro-", "gramme"] },
  simulator: { name: "Simulator", short: "Trials", tag: "The Trials", lines: ["The", "Trials"] },
  review: { name: "Session review", short: "Tape", tag: "The Tape", lines: ["The", "Tape"] },
  reconcile: { name: "Reconcile & plan", short: "Plan", tag: "The Morning After", lines: ["Morning", "After"] },
};

export const pageNumber = (id: PageId): string => String(PAGE_IDS.indexOf(id) + 1).padStart(2, "0");
