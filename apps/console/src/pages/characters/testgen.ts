// Test-only data (imported by *.test.tsx only): the real Malakor package read from
// characters/malakor as the daemon would serve it, plus generated rosters, rule sets and
// tournament results at the 10x shape. Nothing here reaches the production bundle.
import type { CharacterDetail, CharacterSummary, RuleState, SegmentId, Situation, TournamentResults } from "@shadowrunr/contract";
import bible from "../../../../../characters/malakor/bible.md?raw";
import constitution from "../../../../../characters/malakor/constitution.md?raw";
import coinProphet from "../../../../../characters/malakor/foils/the-coin-prophet.md?raw";
import disruptor from "../../../../../characters/malakor/foils/the-disruptor.md?raw";
import hasBeenHero from "../../../../../characters/malakor/foils/the-has-been-hero.md?raw";
import inspector from "../../../../../characters/malakor/foils/the-inspector.md?raw";
import lifeCoach from "../../../../../characters/malakor/foils/the-life-coach.md?raw";
import crisisHandoff from "../../../../../characters/malakor/rules/crisis-handoff.md?raw";
import noFinancial from "../../../../../characters/malakor/rules/no-financial-promotion.md?raw";
import pkg from "../../../../../characters/malakor/situations.json";
import deadpan from "../../../../../characters/malakor/tones/deadpan-bureaucrat.md?raw";
import gleeful from "../../../../../characters/malakor/tones/gleeful-chaos.md?raw";
import theatrical from "../../../../../characters/malakor/tones/theatrical.md?raw";
import roastmaster from "../../../../../characters/malakor/tones/unhinged-roastmaster.md?raw";
import weary from "../../../../../characters/malakor/tones/weary-middle-manager.md?raw";

const SEGMENTS: readonly SegmentId[] = ["SEG-01", "SEG-02A", "SEG-02B-I", "SEG-02B-II"];
const isSegment = (s: string): s is SegmentId => (SEGMENTS as readonly string[]).includes(s);

export const FOIL_IDS = ["the-disruptor", "the-life-coach", "the-has-been-hero", "the-inspector", "the-coin-prophet"] as const;
export const TONE_IDS = ["deadpan-bureaucrat", "gleeful-chaos", "theatrical", "unhinged-roastmaster", "weary-middle-manager"] as const;

const SITUATIONS: Situation[] = pkg.situations.map((s) => ({
  id: s.id,
  archetype: s.archetype,
  behaviour: s.behaviour,
  required: s.required,
  forbidden: s.forbidden,
  invariants: s.invariants,
  near_miss: s.near_miss,
  ...("segments" in s ? { segments: s.segments.filter(isSegment) } : {}),
}));

export function malakorSummary(over: Partial<CharacterSummary> = {}): CharacterSummary {
  return {
    id: "malakor",
    name: "Arch-Lich Malakor",
    default_tone: "theatrical",
    active_tone: "theatrical",
    foils: [...FOIL_IDS],
    tones: [...TONE_IDS],
    rules_enabled: 2,
    rules_total: 2,
    ...over,
  };
}

export function malakorDetail(): CharacterDetail {
  return {
    id: "malakor",
    name: "Arch-Lich Malakor",
    default_tone: "theatrical",
    active_tone: "theatrical",
    constitution,
    bible,
    files: [{ path: "character.toml", bytes: 900 }],
    foils: [
      { id: "the-coin-prophet", declared: true, text: coinProphet },
      { id: "the-disruptor", declared: true, text: disruptor },
      { id: "the-has-been-hero", declared: true, text: hasBeenHero },
      { id: "the-inspector", declared: true, text: inspector },
      { id: "the-life-coach", declared: true, text: lifeCoach },
    ],
    rank_titles: { thrall: "Thrall", acolyte: "Acolyte", cultist: "Cultist", warlock: "Warlock", lich: "Lesser Lich", dread_council: "Dread Council" },
    rules: [
      { id: "crisis-handoff", title: "Crisis handoff", default: true, enabled: true, text: crisisHandoff },
      { id: "no-financial-promotion", title: "No financial promotion", default: true, enabled: true, text: noFinancial },
    ],
    situations: SITUATIONS,
    tones: [
      { id: "deadpan-bureaucrat", text: deadpan },
      { id: "gleeful-chaos", text: gleeful },
      { id: "theatrical", text: theatrical },
      { id: "unhinged-roastmaster", text: roastmaster },
      { id: "weary-middle-manager", text: weary },
    ],
  };
}

export const malakorRules = (enabled: Readonly<Record<string, boolean>> = {}): RuleState[] => [
  // A rail: the daemon ignores any override for it and refuses character.rules.set.
  { id: "crisis-handoff", title: "Crisis handoff", default: true, enabled: true, locked: true },
  { id: "no-financial-promotion", title: "No financial promotion", default: true, enabled: enabled["no-financial-promotion"] ?? true },
];

/** Malakor first, then `n - 1` generated characters (the 10x roster). */
export function genRoster(n: number): CharacterSummary[] {
  const out = [malakorSummary()];
  for (let i = 1; i < n; i++) {
    out.push({
      id: `char-${String(i).padStart(4, "0")}`,
      name: `Warden Number ${i}`,
      default_tone: TONE_IDS[i % TONE_IDS.length] ?? "theatrical",
      active_tone: TONE_IDS[i % TONE_IDS.length] ?? "theatrical",
      foils: FOIL_IDS.slice(0, 1 + (i % 5)),
      tones: [...TONE_IDS],
      rules_enabled: i % 3,
      rules_total: 2,
    });
  }
  return out;
}

/** results.json with `cells` cells, each judging every situation (2,000 failing refs at 10x). */
export function genResults(cells: number, failsPerCell = 1): TournamentResults {
  const usage = { calls: 0, errors: 0, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_creation_tokens: 0, cost_usd: 0 };
  return {
    tournament_id: "t-scale",
    instrument_hashes: ["h"],
    calibration: [{ kind: "default_assumption" }],
    runs: cells,
    judged: cells,
    void: 0,
    failed: 0,
    latency_by_tier: {},
    usage_total: usage,
    usage_by_role: {},
    cells: Array.from({ length: cells }, (_, c) => ({
      runs: 1,
      judged: 1,
      void: 0,
      failed: 0,
      judge_tiers: ["opus"],
      core_latency: { n: 0 },
      usage,
      usage_by_role: {},
      cell: { tone: "theatrical", core_tier: "haiku", segment: "SEG-01", audience_size: 30, instrument_hash: "h" },
      rules: Object.fromEntries(
        SITUATIONS.map((s, k) => [
          `situation:${s.id}`,
          {
            pass: 3,
            fail: k === 0 ? failsPerCell : 0,
            not_provoked: 1,
            not_evaluable: 0,
            n: 3 + (k === 0 ? failsPerCell : 0),
            fail_refs: k === 0 ? Array.from({ length: failsPerCell }, (_, f) => `run-${c}#${10 + f}`) : [],
          },
        ]),
      ),
      quality: {},
    })),
  };
}
