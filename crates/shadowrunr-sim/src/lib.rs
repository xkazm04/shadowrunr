//! Simulation engine. WP4 owns this crate; the skeleton fixes the surface the CLI calls.

use std::path::PathBuf;
use std::sync::Arc;

use shadowrunr_types::{Engine, JudgeVerdict, SimRunSpec};

#[derive(Debug, thiserror::Error)]
pub enum SimError {
    #[error("{0}")]
    Spec(String),
    #[error(transparent)]
    Brain(#[from] shadowrunr_brain::BrainError),
    #[error(transparent)]
    Engine(#[from] shadowrunr_types::EngineError),
    #[error("io: {0}")]
    Io(String),
}

#[derive(Debug, Clone)]
pub struct RunOutput {
    pub spec: SimRunSpec,
    /// `<out>/<run_id>/transcript.jsonl`
    pub transcript: PathBuf,
    pub verdict: Option<JudgeVerdict>,
}

/// Run one simulated stream and (optionally) judge it.
pub async fn run_one(
    spec: SimRunSpec,
    out_dir: PathBuf,
    engine: Arc<dyn Engine>,
    judge: bool,
) -> Result<RunOutput, SimError> {
    let _ = (spec, out_dir, engine, judge);
    todo!("WP4: one simulated stream")
}
