// `app_map`: the console in one answer, written from the pages' own route readers and the
// guides (docs/guide/characters.md, cli.md, docs/spec/contract.md). Kept under about 6 KB:
// Athena cuts tool descriptions to 240 chars, so this result is where the map lives.
import { PKG_TABS } from "../../pages/characters/routes";

export const APP_MAP = {
  app: "Shadowrunr OS console: the operator's desk for an AI character that streams. Routes are hashes, #/<page>/<segment>/...; go_to takes one.",
  pages: [
    { page: "wall", purpose: "The season wall: one poster per page.", grammar: "#/wall", example: "#/wall" },
    {
      page: "onair",
      purpose: "The broadcast: the stream screen, the rundown of runs, the Lord's dispatch, clips.",
      grammar: "#/onair[/rundown[/<tournament>/<run>] | /dispatch[/<report>] | /clips[/<clip>]]",
      example: "#/onair/rundown",
    },
    {
      page: "characters",
      purpose: "The cast: each character package, its documents, tones, foils, rules, voice and the Tone Lab.",
      grammar: `#/characters/all | #/characters/<id>/package/<tab> with tab one of ${PKG_TABS.map((t) => t.id).join(", ")}; situations/<situation>; tone-lab/<session>/<line>`,
      example: "#/characters/malakor/package/bible",
    },
    {
      page: "brain",
      purpose: "The engine: calls and cost per role, run transcripts line by line, pre-flight checks.",
      grammar: "#/brain/engine | #/brain/transcripts[/<run>[/<line>]] (run may be <tournament>/<run_id>, one segment) | #/brain/preflight",
      example: "#/brain/transcripts/theatrical.haiku.SEG-01.a30.s1/12",
    },
    {
      page: "segments",
      purpose: "The programme: segment tables and states, chat commands, the rank ladder and weights.",
      grammar: "#/segments/tables[/<SEG>[/<state>]] | #/segments/commands | #/segments/ranks[/<report>]",
      example: "#/segments/tables/SEG-01",
    },
    {
      page: "simulator",
      purpose: "The trials: tournaments of simulated runs, runs, judge verdicts, the cell heatmap.",
      grammar: "#/simulator/tournaments[/<id>] | #/simulator/runs | #/simulator/verdicts[/<run>] | #/simulator/heatmap[/<cell key>]",
      example: "#/simulator/verdicts/theatrical.haiku.SEG-01.a30.s1",
    },
    {
      page: "review",
      purpose: "The tape: finished sandbox sessions, every line typed, judged and marked by the operator.",
      grammar: "#/review/<session>[/timeline[/<line>] | /types | /player/<kind> | /judge | /moderation | /calls] | #/review/<A>/compare/<B>",
      example: "#/review/<session>/timeline/42",
    },
    {
      page: "reconcile",
      purpose: "The morning after: a reconcile report on a tournament, plan options, directives, the Lord's chat.",
      grammar: "#/reconcile/report[/held|/failed] | #/reconcile/plan | #/reconcile/directives | #/reconcile/chat",
      example: "#/reconcile/report",
    },
  ],
  glossary: {
    Malakor: "The flagship character (id malakor): a theatrical skull-faced sorcerer-villain who streams his plans for world domination, calls viewers minions and serves the Lord of the Darkness.",
    "character package": "characters/<id>/: character.toml, constitution.md, bible.md, tones/, foils/, rules/, stalls.toml, situations.json, directions.md, lines/.",
    "constitution vs bible": "constitution.md is identity, awareness and red lines; the brain never edits it, and an edit here is always an operator-approved card. bible.md is optional lore, wants, the long arc and running jokes.",
    tones: "Tone variants (tones/<variant>.md); default_tone names one, the operator's active tone overrides it (set_tone).",
    foils: "SEG-01 guest foils (foils/<slug>.md): original parody archetypes, never real people; listed in character.toml to be summonable.",
    segments: "Table-driven show segments: SEG-01 Improv room, SEG-02A Investigation (SEG-02B-I and -II are descoped).",
    "sandbox session": "A wall-clock rehearsal of 5, 10 or 30 minutes against the simulated room, ending in a gated report; read it in Review.",
    "sim run": "One simulated stream with a run id <tone>.<tier>.<segment>.a<audience>.s<seed>; text only.",
    tournament: "A matrix of sim runs under one instrument hash, summarised per cell (tone x tier x segment x audience).",
    judge: "A model on a different tier from the core that checks each rule (pass, fail, not provoked) and quality facts, citing <run>#<line>.",
    "Tone Lab": "The package tab where the owner directs a tone: the line bank (lines/<tone>.toml), variants, the From Review inbox and directions.md.",
    line: "A 1-based transcript line, the same number as the /events seq and the judge's refs.",
  },
  how_to: [
    "Orient: where_am_i -> app_map -> go_to{view}.",
    "Walk a session: list_sessions -> get_session{session} -> open_moment{session, line} -> leave_feedback{session_id, line, tags}.",
    "Shape Malakor: get_character{id} -> read_character_doc{id, doc} -> set_character_doc{id, doc, text, base_digest: the digest read} -> validate_character{id}.",
    "Analyze sims: list_tournaments -> get_tournament{tournament} -> list_runs{tournament} -> get_verdict{run} -> get_transcript{run, from}.",
    "Rehearse: start_sandbox{minutes: 5} -> query{method: sandbox.status} -> list_sessions once it is done.",
  ],
  notes: "Reads move the console to what they fetched unless show:false. Ids are always arguments; where_am_i lists the ids in view. Writes reach the operator as approval cards. Live sims, clips, publishing, the moderator and Twitch writes are out of reach.",
} as const;
