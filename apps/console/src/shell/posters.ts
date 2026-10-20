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

/**
 * The posters printed on light stock: paper, ochre and straw, relative luminance .4 and up. The
 * owner took the print grain off them, face and layer 1, for readability (REJECTED.md,
 * 2026-10-20); the dark posters keep it on their face.
 */
export const LIGHT_FACES: ReadonlySet<PageId> = new Set<PageId>(["characters", "segments", "review"]);

export const pageNumber = (id: PageId): string => String(PAGE_IDS.indexOf(id) + 1).padStart(2, "0");
