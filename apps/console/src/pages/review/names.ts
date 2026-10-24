// Every label the Review page shows. Enum ids stay on the wire and never reach the screen:
// line kinds, targets, edges, judged rules, quality dimensions, moderation records and engine
// roles are named here, from the operator's side of the screen.
import type { FeedbackTag, LineEdge, LineKind, LineTarget, ModelTier, Role, SandboxChaos, SandboxCheckId, SandboxCheckState } from "@shadowrunr/contract";

/** A line's kind as the page shows it. `stall` and `untagged` are page-side (derived), not wire kinds. */
export type ShownKind = LineKind | "stall" | "untagged";

export const KINDS: readonly (readonly [ShownKind, string])[] = [
  ["roast", "Roast"], ["tease", "Tease"], ["joke", "Joke"], ["title", "Mock title"], ["welcome", "Welcome"],
  ["reply", "Plain reply"], ["callout", "Call to action"], ["show", "Running the show"], ["refusal", "Refusal"],
  ["care", "Care"], ["disclosure", "Disclosure"], ["signoff", "Sign-off"], ["stall", "Stall line"], ["other", "Other"], ["untagged", "Untagged"],
];
export const KIND_NAME: Readonly<Record<ShownKind, string>> = Object.fromEntries(KINDS) as Record<ShownKind, string>;
export { KIND_INK, VOTED_RING } from "./ink";

/** The humour kinds: the "Humour only" filter. */
export const HUMOUR: readonly ShownKind[] = ["roast", "tease", "joke", "title"];

export const TARGET_NAME: Readonly<Record<LineTarget, string>> = { viewer: "a viewer", chat: "the whole chat", foil: "the foil", self: "himself", lord: "the Lord", case: "the case", none: "no one" };
export const EDGE_NAME: Readonly<Record<LineEdge, string>> = { none: "no edge", mild: "mild edge", sharp: "sharp edge" };

export const FEEDBACK: readonly (readonly [FeedbackTag, string])[] = [
  ["landed", "Landed"], ["flat", "Flat"], ["too_mean", "Too mean"], ["broke_character", "Broke character"],
  ["off_tone", "Off-tone"], ["great_roast", "Great roast"], ["repetitive", "Repetitive"], ["wrong_tag", "Wrong tag"],
];
export const FEEDBACK_NAME: Readonly<Record<FeedbackTag, string>> = Object.fromEntries(FEEDBACK) as Record<FeedbackTag, string>;

export const TIER_NAME: Readonly<Record<ModelTier, string>> = { haiku: "Haiku", sonnet: "Sonnet", opus: "Opus" };
export const SEGMENT_NAME: Readonly<Record<string, string>> = { "SEG-01": "Improv room", "SEG-02A": "Investigation" };

/** What a timeline row's show column says about an event: show ops, handovers, engine health. */
export type ShowMarkId = "poll" | "poll_closed" | "reveal" | "foil_in" | "foil_out" | "card" | "sign_off" | "handover" | "engine_down" | "engine_degraded" | "engine_back";
export const SHOW_MARK_NAME: Readonly<Record<ShowMarkId, string>> = {
  poll: "Poll", poll_closed: "Poll closed", reveal: "Reveal", foil_in: "Foil in", foil_out: "Foil out", card: "Card", sign_off: "Sign-off",
  handover: "Handover", engine_down: "Engine down", engine_degraded: "Engine degraded", engine_back: "Engine back",
};
/** A timeline row's gates and judge column: output gates by group, and the judge's failed rules. */
export type GateMarkId = "care" | "persona" | "alleged" | "judge_fail";
export const GATE_MARK_NAME: Readonly<Record<GateMarkId, string>> = { care: "Care", persona: "Persona", alleged: "Alleged", judge_fail: "Judge fail" };

/** The wall's order control. */
export type WallOrder = "oldest" | "fails" | "roast";
export const WALL_ORDER: readonly (readonly [WallOrder, string])[] = [["oldest", "Oldest first"], ["fails", "Most rule fails"], ["roast", "Most roasting"]];

/** A silence longer than this, in seconds, reads as a failure (the wall's facts, layer 2's figures). */
export const SILENCE_LIMIT_S = 15;

/**
 * A reply whose p95 wait from the viewer's line exceeds this, in seconds, reads as a failure: the top
 * of the research's 2-4 s budget to the first spoken word (docs/research/03-llm-orchestration.md).
 */
export const REPLY_LIMIT_S = 4;

/** The timeline's row sizes, in seconds. */
export const ROW_SIZES: readonly (readonly [number, string])[] = [[10, "10 s"], [30, "30 s"], [60, "1 min"], [120, "2 min"]];
export const RANK_NAME: Readonly<Record<string, string>> = { thrall: "Thrall", acolyte: "Acolyte", cultist: "Cultist", warlock: "Warlock", lich: "Lich", dread_council: "Dread Council" };
/** Exhaustive over the contract's check ids: a check added in Rust fails typecheck here until it is named. */
export const CHECK_NAME: Readonly<Record<SandboxCheckId, string>> = { say_gap: "Say gap", core_errors: "Core errors", sign_off: "Sign-off", chaos: "Chaos", stage_ack: "Stage ack", judge: "Judge", voiced: "Voiced", face: "Face", av_sync: "A/V sync", dead_air: "Dead air" };
export const CHECK_STATE_NAME: Readonly<Record<SandboxCheckState, string>> = { pass: "passed", fail: "failed", unmeasured: "not measured" };
export const ROLE_NAME: Readonly<Partial<Record<Role, string>>> = {
  core_thread: "Malakor's voice", chatter: "Simulated chatters", chat_picker: "Chat picker", researcher: "Researchers",
  consolidator: "Memory", judge: "Judge", planner: "Planner", reconciler: "Reconcile", clipper: "Clip factory", tagger: "Line tagger",
};

/** An id as words: `situation:foo-bar` -> `Foo bar`. The fallback for ids no table names. */
export function human(id: string): string {
  const s = id.replace(/^(situation|red-line|role|talk|rule|q):/, "").replace(/[-_:]+/g, " ").trim();
  return s.charAt(0).toUpperCase() + s.slice(1);
}

const RULE_NAME: Readonly<Record<string, string>> = {
  "situation:bait-slur-request": "Slur bait", "situation:ironic-stereotype-joke": "“Just a joke” stereotype bait", "situation:edgy-roast-request": "Viewer asks to be roasted hard",
  "situation:dark-humor-fine": "Dark joke that is fine to riff on", "situation:explicit-roleplay-request": "Explicit role-play request", "situation:minor-flirt-request": "Minor asks him to flirt",
  "situation:roast-real-streamer": "Roast a real streamer", "situation:celebrity-impression": "Celebrity impression request", "situation:real-hacking-request": "Real hacking request",
  "situation:prompt-injection": "Instruction-injection attempt", "situation:fake-lord-dispatch": "Fake decree from “the Lord”", "situation:lord-watching-question": "“Is the Lord watching?”",
  "situation:sincere-are-you-ai": "Sincere “are you an AI?”", "situation:bit-are-you-real-lich": "“You’re not a real lich” bit", "situation:newcomer-what-is-this": "Newcomer asks what this is",
  "situation:viewer-sincerely-sad": "Viewer is sincerely sad", "situation:dread-council-proposal": "Dread Council proposal", "situation:thrall-propose-attempt": "Thrall tries to propose",
  "situation:gifted-sub-hype": "Gifted subscriptions", "situation:foil-summon-chaos": "Summon spam while a foil is on", "situation:foil-made-real": "Chat insists the foil is real",
  "situation:seg02a-raid-subject": "Chat urges a raid on the case subject", "situation:seg02a-private-person": "Private person named during the audit", "situation:seg02a-address-request": "Address request during the audit",
  "situation:seg02a-unproven-as-fact": "Chat demands an allegation as fact", "situation:vote-result-unwelcome": "Vote result he mocked", "situation:empty-room": "Empty room",
  "situation:raid-wave": "Raid arrives", "situation:copypasta-flood": "Copypasta flood", "situation:self-harm-disclosure": "Self-harm disclosure", "situation:coin-shill-request": "Crypto coin shill request",
  "red-line:hateful-conduct": "No hateful conduct", "red-line:sexual-content": "No sexual content", "red-line:harassment-real-people": "No harassment of real people", "red-line:doxxing": "No doxxing",
  "red-line:real-likeness": "No real person’s likeness", "red-line:real-world-harm": "No real-world harm", "red-line:no-lecture": "No lecturing", "red-line:never-claims-human": "Never claims to be human",
  "red-line:chat-cannot-rewrite-rules": "Chat cannot rewrite his rules", "role:listens-to-votes": "Follows vote results", "role:entertains-empty-room": "Entertains an empty room",
  "role:serves-the-lord": "Serves the Lord’s brief", "role:knows-streaming": "Knows streaming", "role:aware-of-segment": "Aware of the segment", "role:right-name-and-rank": "Right name and rank",
  "role:rank-gated-proposals": "Rank-gated proposals", "talk:keeps-show-moving": "Keeps the show moving", "rule:crisis-handoff": "Crisis handoff", "rule:no-financial-promotion": "No financial promotion",
};
export const ruleName = (id: string): string => RULE_NAME[id] ?? human(id);

const DIM_NAME: Readonly<Record<string, string>> = {
  humor_landed: "Humour landed", in_character: "Stayed in character", responsive_to_chat: "Responded to chat", serves_the_segment: "Serves the segment",
  aware_of_segment_and_lord: "Aware of the segment and the Lord", listens_to_votes: "Followed vote results", pacing_no_dead_air: "No dead air", edge_without_cruelty: "Edge without cruelty",
};
export const dimName = (id: string): string => DIM_NAME[id] ?? human(id);

const OP_NAME: Readonly<Record<string, string>> = {
  start_poll: "start a poll", close_poll: "close a poll", summon_foil: "summon a foil", dismiss_foil: "dismiss the foil", reveal_hex: "reveal a hex", highlight: "highlight a message",
  show_card: "show a card", switch_segment: "switch segment", sign_off: "sign off", dispatch_research: "send a researcher", mark_moment: "mark a moment", show_screen: "switch the screen", pin_link: "pin a string on the board",
};
const REFUSAL_REASON: Readonly<Record<string, string>> = {
  foil_already_active: "that foil is already on stage", unknown_msg: "the message does not exist", duplicate_poll_id: "the poll name was taken", no_such_poll: "no such poll is open",
  poll_already_open: "a poll is already open", reveal_not_due: "the reveal was not due yet", not_in_state: "not allowed at that point of the show", already_revealed: "already revealed",
  op_not_allowed: "not allowed in this segment", no_active_foil: "no foil is on stage", act_locked: "that act has not opened",
};

export type ModGroup = "Safety & care" | "Persona" | "Accuracy" | "Engine cover" | "Chat filters" | "Show mechanics" | "Other";
export const MOD_GROUPS: readonly ModGroup[] = ["Safety & care", "Persona", "Accuracy", "Engine cover", "Chat filters", "Show mechanics", "Other"];
export const MOD_GROUP_NOTE: Readonly<Record<ModGroup, string>> = {
  "Safety & care": "Crisis and care handling.", Persona: "Where his own line was changed or held back.", Accuracy: "Claims about the case hedged automatically.",
  "Engine cover": "Lines spoken because the engine was slow or failing.", "Chat filters": "What happened to viewers’ messages before he saw them.",
  "Show mechanics": "Show commands the segment refused.", Other: "",
};
export interface ModName { readonly group: ModGroup; readonly name: string; readonly detail: string; readonly key: string }

/** A moderation record (`stage`, `rule`, `action`) as a group, a plain name and a detail. */
export function modName(stage: string, rule: string, action: string): ModName {
  const R = (group: ModGroup, name: string, detail = ""): ModName => ({ group, name, detail, key: `${group}|${name}` });
  if (rule.startsWith("alleged:")) return R("Accuracy", "“Alleged” added to an unproven claim", rule.split(":")[1]);
  if (rule === "engine_stall") return action === "covered" ? R("Engine cover", "Stall line while a slow reply was coming") : R("Engine cover", "Stall line during an engine failure");
  if (rule === "persona_leak") return R("Persona", "Line replaced: it leaked how he works");
  if (rule === "character_break") return R("Persona", "Line replaced: it broke character");
  if (rule === "repetition") return R("Persona", "Near-repeat line held back");
  if (rule === "note_blocked") return R("Persona", "Private note kept off air");
  if (rule === "personal_data_ask") return R("Safety & care", "Ask for personal details removed");
  if (rule === "care_state") return R("Safety & care", action === "started" ? "Care window opened for a viewer" : action === "restarted" ? "Care window restarted" : "Care window ended");
  if (rule === "crisis_chat_ban")
    return action === "name_withheld" ? R("Safety & care", "Removed viewer’s name held back")
      : action === "banned:stream" ? R("Safety & care", "Viewer removed from chat after the crisis line") : R("Safety & care", "Removed viewer’s message dropped");
  if (rule.startsWith("safety:")) return R("Safety & care", "Self-harm disclosure put first");
  if (rule === "prompt_injection") return R("Chat filters", "Attempt to give him instructions flagged");
  if (rule === "rate_cap") return action === "dropped" ? R("Chat filters", "Message over the rate limit dropped") : R("Chat filters", "Over the rate limit but let through", human(action.replace("exempt:", "")));
  if (rule === "flood_cluster") return R("Chat filters", "Message flood merged", `${action.split(":")[1] ?? "?"} messages`);
  if (rule === "copypasta_cluster") return R("Chat filters", "Copypasta merged", `${action.split(":")[1] ?? "?"} messages`);
  if (rule === "copypasta_repeat") return R("Chat filters", "Repeated copypasta dropped");
  if (rule.startsWith("term:")) return R(stage === "input" ? "Chat filters" : "Persona", `${stage === "input" ? "Blocked term in chat" : "Blocked term in his line"} (${action})`, human(rule.slice(5)));
  if (rule === "propose_min_rank") return R("Show mechanics", "Proposal from too low a rank ignored");
  if (rule === "unknown_hex") return R("Show mechanics", "Command for a hex that does not exist ignored");
  if (rule === "command_not_in_segment") return R("Show mechanics", "Chat command this segment does not take declined");
  if (rule === "command_closed_in_state") return R("Show mechanics", "Chat command closed at this point of the segment declined");
  if (rule === "no_open_poll") return R("Show mechanics", "Vote with no poll open declined");
  if (rule === "unknown_option") return R("Show mechanics", "Vote for an option the poll does not have declined");
  if (rule === "foil_vote_not_open") return R("Show mechanics", "Summon with no foil vote open declined");
  if (rule === "unknown_foil") return R("Show mechanics", "Summon of a foil that does not exist declined");
  if (rule === "poll_min_rank") return R("Show mechanics", "Vote from below the poll's rank ignored");
  if (action.startsWith("rejected")) {
    const op = action.split(":")[1];
    return R("Show mechanics", `His command to ${(op && OP_NAME[op]) || human(op ?? "act")} was refused`, REFUSAL_REASON[rule] ?? human(rule));
  }
  return R(stage === "input" ? "Chat filters" : "Other", human(rule), human(action));
}

/** A session's title. Layer2.tsx has the same function; it lives here too so the wall's chunk does not load layer 2's. */
export function sessionTitle(minutes: number, chaos?: SandboxChaos): string {
  return `${minutes}-minute session${chaos === "lost" ? " · engine lost" : chaos === "outage" ? " · engine outage" : ""}`;
}

export const fmtClock = (s: number): string => {
  const t = Math.max(0, Math.round(s));
  return `${String(Math.floor(t / 60)).padStart(2, "0")}:${String(t % 60).padStart(2, "0")}`;
};
