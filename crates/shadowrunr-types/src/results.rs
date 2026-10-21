//! `tournament.results`: a tournament's `results.json` as `shadowrunr-sim` writes it
//! (`shadowrunr sim summarize`). Cells are keyed by instrument hash, so results measured with
//! different instruments are never merged. The aggregation itself (tallies, worst-first
//! ordering, the markdown reports) stays in `shadowrunr-sim::summary`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::reconcile::{Bound, DirectiveOutcome};

/// The axes of one heatmap cell.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct CellKey {
    pub instrument_hash: String,
    pub tone: String,
    pub core_tier: String,
    pub segment: String,
    pub audience_size: u32,
}

/// One hard rule's checks within a cell (100% bar).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuleTally {
    pub pass: u32,
    pub fail: u32,
    pub not_provoked: u32,
    pub not_evaluable: u32,
    /// Evaluated runs: pass + fail. not_provoked and not_evaluable are never passes.
    pub n: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass_rate: Option<f64>,
    /// 95% bound on the FAIL rate (3/n when no run failed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_bound: Option<Bound>,
    /// `<run_id>#<line>` refs of the failing evidence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fail_refs: Vec<String>,
}

/// One quality dimension within a cell: a rate with its n.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QualityTally {
    #[serde(rename = "true")]
    pub true_count: u32,
    pub n: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound: Option<Bound>,
    /// Facts whose quote or line did not verify (excluded from n).
    pub not_evaluable: u32,
}

/// `first_text_ms` percentiles.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Latency {
    pub n: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p50_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p95_ms: Option<u64>,
}

/// Engine usage summed over ledger rows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UsageTotals {
    pub calls: u64,
    pub errors: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cost_usd: f64,
}

/// One heatmap cell: every run that shares a [`CellKey`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CellResult {
    pub cell: CellKey,
    pub runs: u32,
    pub judged: u32,
    /// Simulated but the judge was void (failed call or unparseable reply).
    pub void: u32,
    /// Failed before a verdict (`error.json`).
    pub failed: u32,
    pub judge_tiers: Vec<String>,
    pub rules: BTreeMap<String, RuleTally>,
    pub quality: BTreeMap<String, QualityTally>,
    /// Core-thread `first_text_ms` from the ledger.
    pub core_latency: Latency,
    pub usage: UsageTotals,
    pub usage_by_role: BTreeMap<String, UsageTotals>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_say_gap_s: Option<u32>,
    /// Judged runs holding at least one hard-rule fail, counted per run (a run with three
    /// failing checks is one run). Omitted in a results.json written before tier decisions:
    /// absent means unknown, never zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_with_fail: Option<u32>,
    /// Judged runs with no hard-rule fail: the denominator of cost per usable stream.
    /// Omitted in a results.json written before tier decisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usable_runs: Option<u32>,
}

/// The cells one tier decision chooses among: every axis of a [`CellKey`] but the core tier.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct DecisionGroup {
    pub instrument_hash: String,
    pub tone: String,
    pub segment: String,
    pub audience_size: u32,
}

/// Why a cell cannot win.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IneligibleReason {
    /// At least one judged run holds a hard-rule fail.
    HardFail,
    /// Too few of its hard checks were evaluated, or nothing was judged: not a pass.
    Unmeasured,
}

/// A cell the gates kept out of the ranking, with the figures that decided it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IneligibleCell {
    pub core_tier: String,
    pub reason: IneligibleReason,
    pub judged: u32,
    /// Judged runs with any hard-rule fail.
    pub runs_with_fail: u32,
    /// Evaluated hard checks (pass + fail) over pass + fail + not_evaluable. Omitted when
    /// the cell has no such checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluated_share: Option<f64>,
    /// The hard rules that failed (`hard_fail` only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failing_rules: Vec<String>,
}

/// Why a cell is reported beside the ranking instead of in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BesideReason {
    /// Judged by a judge tier other than the group's reference, or by more than one.
    JudgeMismatch,
}

/// A cell measured by a different instrument slice: reported with its own n, never ranked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BesideCell {
    pub core_tier: String,
    pub reason: BesideReason,
    pub judge_tiers: Vec<String>,
    pub judged: u32,
}

/// An eligible cell, in rank order (best quality first).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RankedCell {
    pub core_tier: String,
    pub judged: u32,
    pub usable_runs: u32,
    pub mean_quality: f64,
    /// Standard error of the per-run quality share. Omitted below two runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_se: Option<f64>,
    /// Cell cost / usable runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_per_usable_stream: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_p50_ms: Option<u64>,
}

/// What settled the winner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecidedBy {
    /// One cell passed the gates.
    OnlyEligible,
    /// The best quality, clear of every other eligible cell's noise.
    Quality,
    /// Several cells within the quality noise of the best: the cheapest per usable stream.
    CostPerUsableStream,
}

/// Which core tier goes live for one group: hard rules gate, coverage gates, quality ranks,
/// cost decides within the noise. Cells judged by a judge other than the reference are
/// reported beside, never ranked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TierDecision {
    pub group: DecisionGroup,
    /// The judge tier that judged the most cells (ties go to the higher tier).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_judge: Option<String>,
    pub ranked: Vec<RankedCell>,
    pub ineligible: Vec<IneligibleCell>,
    pub beside: Vec<BesideCell>,
    /// Core tiers within the quality noise of the best, cheapest first.
    pub tie_set: Vec<String>,
    /// None: nothing goes live here; `ineligible` lists what to fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<String>,
    /// The winner has fewer judged runs than the thin-n bar.
    pub provisional: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<DecidedBy>,
}

/// Where the audience calibration a run used came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CalibrationOrigin {
    /// The researched file (`sim/calibration.json`).
    File { path: String },
    /// The compiled-in default: every figure is an assumption.
    DefaultAssumption,
}

/// `tournament.results`: the whole `results.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TournamentResults {
    pub tournament_id: String,
    pub instrument_hashes: Vec<String>,
    pub calibration: Vec<CalibrationOrigin>,
    /// Figures without a source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<String>,
    pub runs: u32,
    pub judged: u32,
    pub void: u32,
    pub failed: u32,
    /// Runs the engine cut (WP13): not results, so left out of every count and cell; a
    /// resume re-runs them. Omitted when 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub engine_failed: u32,
    /// Runs whose judge call failed on the engine (WP13): not results, so left out of every
    /// count and cell; a resume re-judges them. Omitted when 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub judge_pending: u32,
    pub cells: Vec<CellResult>,
    /// tier -> role -> first_text_ms latency, from every ledger row.
    pub latency_by_tier: BTreeMap<String, BTreeMap<String, Latency>>,
    pub usage_total: UsageTotals,
    pub usage_by_role: BTreeMap<String, UsageTotals>,
    /// Addition (WP15): per-directive outcomes over every judged result run, for runs made
    /// with a brief. Omitted when no verdict carried a directive check.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directives: Vec<DirectiveOutcome>,
    /// Addition: the tier decision per group (instrument, tone, segment, audience size).
    /// Omitted in a results.json written before it existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tier_decisions: Vec<TierDecision>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A results.json written before tier decisions (no `tier_decisions`, no per-cell
    /// `runs_with_fail` / `usable_runs`) still reads, with the new fields unknown.
    #[test]
    fn a_results_json_from_before_tier_decisions_still_reads() {
        let old = r#"{"tournament_id":"t","instrument_hashes":["h"],
            "calibration":[{"kind":"default_assumption"}],
            "runs":1,"judged":1,"void":0,"failed":0,
            "cells":[{"cell":{"instrument_hash":"h","tone":"t","core_tier":"haiku",
              "segment":"SEG-01","audience_size":3},
              "runs":1,"judged":1,"void":0,"failed":0,"judge_tiers":["sonnet"],
              "rules":{},"quality":{},"core_latency":{"n":0},
              "usage":{"calls":0,"errors":0,"input_tokens":0,"output_tokens":0,
                "cache_read_tokens":0,"cache_creation_tokens":0,"cost_usd":0.0},
              "usage_by_role":{}}],
            "latency_by_tier":{},
            "usage_total":{"calls":0,"errors":0,"input_tokens":0,"output_tokens":0,
              "cache_read_tokens":0,"cache_creation_tokens":0,"cost_usd":0.0},
            "usage_by_role":{}}"#;
        let r: TournamentResults = serde_json::from_str(old).unwrap();
        assert!(r.tier_decisions.is_empty());
        assert_eq!(
            (r.cells[0].runs_with_fail, r.cells[0].usable_runs),
            (None, None)
        );
        let back = serde_json::to_string(&r).unwrap();
        assert!(!back.contains("tier_decisions") && !back.contains("runs_with_fail"));
    }
}
