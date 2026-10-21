// Every label the Tone Lab shows. Situation tags, line statuses and feedback marks stay on the
// wire and never reach the screen: they are named here.
import type { FeedbackTag, LineSituation, LineStatus } from "@shadowrunr/contract";

export const SITUATION_ORDER: readonly LineSituation[] = [
  "roast_request",
  "bait",
  "newcomer",
  "vote_loss",
  "gift",
  "ai_question",
  "quiet",
  "research_wait",
  "proposal",
  "patron",
  "flood",
  "general",
];

export const SITUATION_NAME: Readonly<Record<LineSituation, string>> = {
  roast_request: "Asked to be roasted",
  bait: "Baited",
  newcomer: "A newcomer speaks",
  vote_loss: "A vote was lost",
  gift: "A gift or a sub",
  ai_question: "Asked if he is an AI",
  quiet: "Chat is quiet",
  research_wait: "Waiting on research",
  proposal: "A proposal",
  patron: "His patron comes up",
  flood: "Chat floods",
  general: "Ordinary chat",
};

export const STATUS_ORDER: readonly LineStatus[] = ["seed", "approved", "retired"];

export const STATUS_NAME: Readonly<Record<LineStatus, string>> = {
  seed: "Seed",
  approved: "Approved",
  retired: "Retired",
};

/** What each status means to the brain. */
export const STATUS_HINT: Readonly<Record<LineStatus, string>> = {
  seed: "drafted for your review; shown to the model until retired",
  approved: "your pick; shown to the model",
  retired: "never shown",
};

export const FEEDBACK_NAME: Readonly<Record<FeedbackTag, string>> = {
  landed: "Landed",
  flat: "Flat",
  too_mean: "Too mean",
  broke_character: "Broke character",
  off_tone: "Off tone",
  great_roast: "Great roast",
  repetitive: "Repetitive",
  wrong_tag: "Wrong tag",
};

/** A Review line's place, as words. */
export const reviewRef = (session: string, line: number): string => `${session.replace(/^sbx-/, "session ")}, line ${line}`;
