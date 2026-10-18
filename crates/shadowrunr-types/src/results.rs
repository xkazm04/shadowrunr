//! `tournament.results`: a tournament's `results.json` as `shadowrunr-sim` writes it
//! (`shadowrunr sim summarize`). Cells are keyed by instrument hash, so results measured with
//! different instruments are never merged. The aggregation itself (tallies, worst-first
//! ordering, the markdown reports) stays in `shadowrunr-sim::summary`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::reconcile::Bound;

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
    pub cells: Vec<CellResult>,
    /// tier -> role -> first_text_ms latency, from every ledger row.
    pub latency_by_tier: BTreeMap<String, BTreeMap<String, Latency>>,
    pub usage_total: UsageTotals,
    pub usage_by_role: BTreeMap<String, UsageTotals>,
}
