// `start_sim`'s spec builder: flat optional args filled from the same defaults the CLI's
// `shadowrunr sim run` uses (crates/shadowrunr-daemon/src/config.rs SimDefaults::default and
// cli.rs sim_run), so Athena never has to guess the shape of a SimRunSpec.
import type { Effort, EngineStatus, ModelTier, SegmentId, SimRunSpec } from "@shadowrunr/contract";

/** `[sim]` defaults (SimDefaults::default): the CLI falls back to these, and so does start_sim. */
export const SIM_DEFAULTS = {
  character: "malakor",
  tier: "sonnet" as ModelTier,
  effort: "low" as Effort,
  segment: "SEG-01" as SegmentId,
  audience: 30,
  seed: 1,
  duration_s: 600,
  population: "growing_channel",
  /** Audience size -> population id (from sim/calibration.json). */
  populations: { 3: "small_new_channel", 30: "growing_channel", 300: "raid_moment" } as Readonly<Record<number, string>>,
} as const;

/** The segments a sim may run: the descoped SEG-02B-I and -II are refused by the daemon. */
export const SIM_SEGMENTS: readonly SegmentId[] = ["SEG-01", "SEG-02A"];
export const SIM_TIERS: readonly ModelTier[] = ["haiku", "sonnet", "opus"];
/** The largest calibrated audience (3, 30 and 300 are calibrated). */
export const MAX_AUDIENCE = 300;

/** SimDefaults::population_for: an exact entry, else the entry nearest on a log scale. */
export function populationFor(audience: number): string {
  const exact = SIM_DEFAULTS.populations[audience];
  if (exact !== undefined) return exact;
  const target = Math.log(Math.max(1, audience));
  let best: [number, string] | undefined;
  for (const [k, v] of Object.entries(SIM_DEFAULTS.populations)) {
    const d = Math.abs(Math.log(Math.max(1, Number(k))) - target);
    if (best === undefined || d < best[0]) best = [d, v];
  }
  return best?.[1] ?? SIM_DEFAULTS.population;
}

/** shadowrunr_sim::matrix::run_id: `<tone>.<tier>.<segment>.a<size>.s<seed>`, tone made path-safe. */
export function simRunId(tone: string, tier: ModelTier, segment: SegmentId, audience: number, seed: number): string {
  return `${tone.replace(/[^A-Za-z0-9_-]/g, "_")}.${tier}.${segment}.a${audience}.s${seed}`;
}

/** The core thread's route from engine.status, else the CLI's [sim] tier and effort. */
export function coreRoute(status: EngineStatus): { tier: ModelTier; effort: Effort } {
  const r = status.routes.find((x) => x.role === "core_thread");
  return r === undefined ? { tier: SIM_DEFAULTS.tier, effort: SIM_DEFAULTS.effort } : { tier: r.tier, effort: r.effort };
}

export interface SimArgs {
  readonly character: string;
  readonly tone: string;
  readonly tier: ModelTier;
  readonly effort: Effort;
  readonly segment: SegmentId;
  readonly audience: number;
  readonly seed: number;
}

export function buildSpec(a: SimArgs): SimRunSpec {
  return {
    run_id: simRunId(a.tone, a.tier, a.segment, a.audience, a.seed),
    character: a.character,
    tone_variant: a.tone,
    core_tier: a.tier,
    core_effort: a.effort,
    segment: a.segment,
    audience_size: a.audience,
    seed: a.seed,
    duration_s: SIM_DEFAULTS.duration_s,
    population: populationFor(a.audience),
  };
}
