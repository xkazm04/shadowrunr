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

/// Items that made it into the synthesis prompt, and items cut to fit its budget.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counted {
    pub included: u32,
    pub omitted: u32,
}

/// What the synthesis prompt carried (WP12), stored with the report as `synthesis_input`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SynthesisInput {
    /// The budget the prompt was sized against, in estimated tokens.
    pub budget_tokens: u32,
    /// The prompt's estimated size (characters / 3.5), system prefix and schema included.
    pub estimated_tokens: u32,
    /// Runs with at least one excerpt line.
    pub runs: Counted,
    pub excerpt_lines: u32,
    /// Transcript lines the judge cited for rule failures.
    pub failure_refs: Counted,
    /// Turns the judge marked as failed quality facts.
    pub quality_refs: Counted,
    /// Incidents listed individually (all are counted by rule).
    pub incidents: Counted,
    pub tone_cells: Counted,
    /// Rule-failure rows (per segment and per tone cell) listed in the top-N tables.
    pub rule_failures: Counted,
    /// What the prompt told the Reconciler was omitted; empty when nothing was cut.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<String>,
}
