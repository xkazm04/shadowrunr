// `app_map{topic?}`: the console in answers that each fit Athena's result cut (every one at or
// under MAP_MAX chars, tested), written from the pages' own route readers and the guides
// (docs/guide/characters.md, cli.md, docs/spec/contract.md). With no topic it is the index: one
// line per page and the topics; a topic is the glossary, the recipes, or one page's route grammar.
import { PKG_TABS } from "../../pages/characters/routes";

/** The largest app_map answer, as JSON text: under the 1,500-char result budget with room. */
export const MAP_MAX = 1400;

interface PageEntry {
  readonly purpose: string;
  readonly example: string;
  readonly grammar: string;
  readonly notes?: string;
}

const PAGES: Readonly<Record<string, PageEntry>> = {
  wall: { purpose: "The season wall: one poster per page.", example: "#/wall", grammar: "#/wall (no sub-pages)" },
  onair: {
    purpose: "The broadcast: stream screen, rundown of runs, the Lord's dispatch, clips.",
    example: "#/onair/rundown",
    grammar: "#/onair[/rundown[/<tournament>/<run>] | /dispatch[/<report>] | /clips[/<clip>]]",
  },
  characters: {
    purpose: "The cast: each package, its documents, tones, foils, rules, voice, the Tone Lab.",
    example: "#/characters/malakor/package/bible",
    grammar: `#/characters/all | #/characters/<id>/package/<tab>, tab one of ${PKG_TABS.map((t) => t.id).join(", ")}; then situations/<situation> or tone-lab/<session>/<line>`,
    notes: "Documents: read_character_doc pages one; edit_character_doc changes a long one by find and replace.",
  },
  brain: {
    purpose: "The engine: calls and cost per role, run transcripts line by line, pre-flight.",
    example: "#/brain/transcripts/theatrical.haiku.SEG-01.a30.s1/12",
    grammar: "#/brain/engine | #/brain/transcripts[/<run>[/<line>]] (run may be <tournament>/<run_id>, one segment) | #/brain/preflight",
  },
  segments: {
    purpose: "The programme: segment tables and states, chat commands, the rank ladder.",
    example: "#/segments/tables/SEG-01",
    grammar: "#/segments/tables[/<SEG>[/<state>]] | #/segments/commands | #/segments/ranks[/<report>]",
  },
  simulator: {
    purpose: "The trials: tournaments of sim runs, runs, judge verdicts, the cell heatmap.",
    example: "#/simulator/verdicts/theatrical.haiku.SEG-01.a30.s1",
    grammar: "#/simulator/tournaments[/<id>] | #/simulator/runs | #/simulator/verdicts[/<run>] | #/simulator/heatmap[/<cell key>]",
  },
  review: {
    purpose: "The tape: finished sandbox sessions, every line typed, judged and marked.",
    example: "#/review/<session>/timeline/42",
    grammar: "#/review/<session>[/timeline[/<line>] | /types | /player/<kind> | /judge | /moderation | /calls] | #/review/<A>/compare/<B>",
  },
  reconcile: {
    purpose: "The morning after: a tournament's reconcile report, plan options, directives, the Lord's chat.",
    example: "#/reconcile/report",
    grammar: "#/reconcile/report[/held|/failed] | #/reconcile/plan | #/reconcile/directives | #/reconcile/chat",
  },
};

const GLOSSARY = {
  Malakor: "The flagship character (id malakor): a skull-faced sorcerer-villain who streams his plans for world domination, calls viewers minions, serves the Lord of the Darkness.",
  package: "characters/<id>/: character.toml, constitution.md, bible.md, tones/, foils/, rules/, stalls.toml, situations.json, directions.md, lines/.",
  "constitution vs bible": "constitution.md is identity, awareness and red lines; the brain never edits it, so an edit is always an operator-approved card. bible.md is lore, wants, the arc, running jokes.",
  tones: "tones/<variant>.md; default_tone names one, the operator's active tone overrides it (set_tone).",
  foils: "SEG-01 guest foils (foils/<slug>.md): parody archetypes, never real people.",
  segments: "SEG-01 Improv room, SEG-02A Investigation (SEG-02B-I and -II are descoped).",
  "sandbox session": "A wall-clock rehearsal of 5, 10 or 30 min, judged; read it in Review.",
  "sim run": "One simulated text stream, run id <tone>.<tier>.<segment>.a<audience>.s<seed>.",
  tournament: "A matrix of sim runs summarised per cell (tone x tier x segment x audience).",
  judge: "A model on another tier that checks each rule, citing <run>#<line>.",
  "Tone Lab": "Where the owner directs a tone: line bank, variants, From Review inbox, directions.md.",
  line: "A 1-based transcript line, the same number as the judge's refs.",
} as const;

const RECIPES = {
  recipes: [
    "Orient: where_am_i -> app_map -> go_to{view}.",
    "Walk a session: list_sessions -> get_session{session} -> open_moment{session, line} -> leave_feedback{session_id, line, tags, note}.",
    "Shape a character: get_character{id} -> read_character_doc{id, doc: bible} (page on with offset: next_offset) -> edit_character_doc{id, doc, base_digest, find, replace} -> validate_character{id}.",
    "Add a foil: set_character_doc{id, doc: foils/<slug>, text, base_digest: ''}.",
    "Analyze sims: list_tournaments -> get_tournament{tournament} -> list_runs{tournament} -> get_verdict{run} -> get_transcript{run, from}.",
    "Start a sim: start_sim{} (every arg optional) -> list_runs -> get_verdict{run}.",
    "Rehearse: start_sandbox{minutes: 5} -> query{method: sandbox.status} -> list_sessions.",
  ],
  notes: "Reads move the console to what they fetched unless show:false. Ids are always arguments. Writes reach the operator as approval cards. Live sims, clips, publishing, the moderator and Twitch writes are out of reach.",
} as const;

export const MAP_TOPICS: readonly string[] = ["glossary", "recipes", ...Object.keys(PAGES)];

/** The index: what the console is, one line per page, and the topics to ask for. */
function index(): Record<string, unknown> {
  return {
    app: "Shadowrunr OS console: the operator's desk for an AI character that streams. Routes are hashes #/<page>/<segment>/...; go_to takes one.",
    pages: Object.fromEntries(Object.entries(PAGES).map(([id, p]) => [id, `${p.purpose} e.g. ${p.example}`])),
    topics: MAP_TOPICS,
    next: "app_map{topic} for a page's route grammar, the glossary or the recipes.",
  };
}

/** One topic, or the index when `topic` is absent; undefined for an unknown topic. */
export function mapAnswer(topic: string | undefined): Record<string, unknown> | undefined {
  if (topic === undefined) return index();
  if (topic === "glossary") return { topic, glossary: GLOSSARY };
  if (topic === "recipes") return { topic, ...RECIPES };
  const p = PAGES[topic];
  return p === undefined ? undefined : { topic, page: topic, ...p };
}
