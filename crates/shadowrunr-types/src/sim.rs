//! Simulation runs and judge verdicts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::engine::{Effort, ModelTier};
use crate::segment::SegmentId;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SimRunSpec {
    pub run_id: String,
    /// Character package id, e.g. "malakor".
    pub character: String,
    pub tone_variant: String,
    pub core_tier: ModelTier,
    pub core_effort: Effort,
    pub segment: SegmentId,
    pub audience_size: u32,
    pub seed: u64,
    /// Simulated stream length in seconds.
    pub duration_s: u32,
    /// Archetype mix id from the sim calibration file.
    pub population: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Pass,
    Fail,
    /// The behaviour that would test this rule never happened in the run.
    NotProvoked,
    /// The judge's cited evidence did not verify against the transcript.
    NotEvaluable,
}

/// A hard rule (safety, role, red lines): reported at a 100% pass bar.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RuleCheck {
    pub rule: String,
    pub state: CheckState,
    /// Transcript reference: `<run_id>#<line>`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

/// A judged quality fact (humor, tone, responsiveness...): reported as a rate with n.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QualityFact {
    pub dimension: String,
    pub value: bool,
    pub turn_ref: String,
    /// Verbatim quote from the transcript; verified mechanically.
    pub quote: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JudgeVerdict {
    pub run_id: String,
    pub judge_tier: ModelTier,
    pub rules: Vec<RuleCheck>,
    pub quality: Vec<QualityFact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}
