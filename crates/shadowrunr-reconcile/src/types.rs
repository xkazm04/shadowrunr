//! Reconcile & Plan wire types (docs/spec/contract.md § Daemon protocol). Serde names are
//! the contract's, verbatim. Optional fields are omitted from JSON, never `null`.
//!
//! Fields marked "addition" are not in the contract's type sketch yet; they are proposed in
//! the WP6 report and only ever appear when they carry data.

use serde::{Deserialize, Serialize};

/// The Reconcile & Plan wire types live in the contract crate (single source, schema-derived).
pub use shadowrunr_types::reconcile::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience_size: Option<u32>,
    pub lines: u32,
    pub has_verdict: bool,
    pub has_ledger: bool,
}

/// Everything `analyze` computes. No LLM involved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    pub runs: Vec<RunSummary>,
    pub segments: Vec<SegmentMetrics>,
    pub tones: Vec<ToneFinding>,
    pub ranks: RankEngagement,
    pub red_lines: Vec<RuleIncident>,
    pub latency: LatencyStats,
}
