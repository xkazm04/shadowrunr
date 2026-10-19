//! Simulation engine for Shadowrunr OS (WP4).
//!
//! - [`run_one`] / [`run_one_with`]: one simulated stream on a compressed clock, against a
//!   calibrated chat room, written to `<out>/<run_id>/` and optionally judged.
//! - [`run_tournament`]: a matrix of runs (`sim/matrices/*.toml`), resumable, with a
//!   concurrency limit and per-run instrument hashes. It pauses on quota instead of burning
//!   runs ([`pacing`]), and an engine failure is never a result ([`outcome`]).
//! - [`summarize`]: `results.json`, `heatmap.md` and `report.md` from a tournament directory.
//!
//! Method (from the owner's `/uat` Phase LC): a situation bank with required responses and
//! benign near-misses; two axes never averaged (rules at a 100% bar with
//! pass/fail/not_provoked/not_evaluable, quality as rates with n and bounds); an instrument
//! hash; a judge on a different tier that never sees the constitution; every quote verified
//! mechanically; the verdict assembled programmatically.

pub mod calibration;
pub mod casefile;
pub mod chat;
pub mod instrument;
pub mod judge;
pub mod ledger;
pub mod matrix;
pub mod outcome;
pub mod pacing;
pub mod population;
pub mod rng;
pub mod runner;
pub mod situations;
pub mod stats;
pub mod summary;
pub mod tournament;
pub mod transcript;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use shadowrunr_types::reconcile::StreamBrief;
use shadowrunr_types::{Command, Engine, JudgeVerdict, ModelTier, Rank, SimRunSpec};

pub use outcome::{classify, RunClass};
pub use pacing::{FleetQuota, PaceClock, QuotaBlock, QuotaGate};
pub use runner::{judge_existing, run_one_with, Mind, RunRecord, RunStatus, Stage};
pub use summary::{summarize, Results};
pub use tournament::{
    exit_code, plan, plan_matrix, run_tournament, PlanReason, RunAction, StopReason,
    TournamentOptions, TournamentPlan, TournamentSummary,
};

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
    /// The judge would run on the core thread's tier (or no valid judge tier exists).
    #[error("judge tier: {0}")]
    JudgeTier(String),
    /// Too many failed ticks: the run is void, not a verdict.
    #[error("run aborted: {0}")]
    Aborted(String),
}

/// Seams to sibling crates, injectable so the simulator's own logic is testable while
/// `shadowrunr_segments` is still being built.
#[derive(Debug, Clone, Copy)]
pub struct Hooks {
    pub rank: fn(Option<u8>, u32, u32) -> Rank,
    pub parse: fn(&str) -> Option<Command>,
}

impl Default for Hooks {
    fn default() -> Self {
        Self {
            rank: shadowrunr_segments::rank_for,
            parse: shadowrunr_segments::parse_command,
        }
    }
}

/// Where a run's brief from the Lord comes from (WP15). The default is no brief, which keeps
/// the stable prefix and the instrument hash exactly as they were before briefs existed.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum BriefSource {
    #[default]
    None,
    /// What the next live stream would receive: `<data>/next_plan.json` plus the active
    /// directives in `<data>/directives.json` ([`SimOptions::data_dir`]).
    Current,
    /// A `StreamBrief` JSON file.
    File(PathBuf),
    /// An already-resolved brief (callers resolve once, then run many specs with it).
    Inline(Box<StreamBrief>),
}

impl BriefSource {
    /// `"current"` or a path, as a matrix or the CLI spells it. Empty or `"none"` is no brief.
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "" | "none" => Self::None,
            "current" => Self::Current,
            p => Self::File(PathBuf::from(p)),
        }
    }

    /// The brief this source names, through the one builder's rules
    /// (`shadowrunr_brain::build_stream_brief` / `normalize_stream_brief`). `None` when the
    /// source is empty: an empty brief is no brief.
    pub fn resolve(&self, data_dir: &std::path::Path) -> Result<Option<StreamBrief>, SimError> {
        match self {
            Self::None => Ok(None),
            Self::Current => Ok(shadowrunr_brain::load_stream_brief(data_dir)?),
            Self::File(p) => {
                let b: StreamBrief = read_json(p)?;
                Ok(shadowrunr_brain::normalize_stream_brief(b))
            }
            Self::Inline(b) => Ok(shadowrunr_brain::normalize_stream_brief((**b).clone())),
        }
    }

    /// This source with the brief read now, so a tournament reads `<data>` once.
    pub fn resolved(&self, data_dir: &std::path::Path) -> Result<Self, SimError> {
        Ok(match self.resolve(data_dir)? {
            Some(b) => Self::Inline(Box::new(b)),
            None => Self::None,
        })
    }
}

/// Knobs for one run. Paths are relative to the working directory (the repo root) unless
/// absolute.
#[derive(Debug, Clone)]
pub struct SimOptions {
    /// Judge the run after simulating it.
    pub judge: bool,
    /// Explicit judge tier; must differ from the run's core tier (error otherwise).
    pub judge_tier: Option<ModelTier>,
    /// Without an explicit tier, the first of these that differs from the core tier judges.
    pub judge_tiers: Vec<ModelTier>,
    /// Simulated seconds per tick.
    pub tick_s: u32,
    /// `characters/` (holds `<id>/character.toml` ...).
    pub characters_root: PathBuf,
    /// `sim/` (calibration.json, rubric.toml, matrices/).
    pub sim_root: PathBuf,
    /// Segment tables, hashed into the instrument (`crates/shadowrunr-segments/data`).
    pub segments_data_dir: PathBuf,
    /// Max featured (LLM-played) chatters per run.
    pub max_featured: usize,
    /// Featured-chatter call cadence in ticks (1 = one batched call every tick).
    pub featured_every_ticks: u32,
    /// Brain worker-pool size.
    pub pool_size: usize,
    /// Keep the throwaway memory dir after the run (debugging only).
    pub keep_memory: bool,
    /// Platforms whose term rules the simulated brain applies (empty = strictest union).
    pub platforms: Vec<String>,
    pub hooks: Hooks,
    /// Asked before every judge call: when no account can serve, the judge is deferred and
    /// the run is `judge_pending` instead of burning the call (WP13).
    pub quota: Option<pacing::GateHandle>,
    /// The Lord's brief the run's brain receives (WP15). Its canonical digest joins the
    /// instrument hash only when a brief is present.
    pub brief: BriefSource,
    /// `<data>`, read by [`BriefSource::Current`].
    pub data_dir: PathBuf,
}

impl SimOptions {
    /// The run's brief, read from its source.
    pub fn resolved_brief(&self) -> Result<Option<StreamBrief>, SimError> {
        self.brief.resolve(&self.data_dir)
    }
}

impl Default for SimOptions {
    fn default() -> Self {
        Self {
            judge: true,
            judge_tier: None,
            judge_tiers: vec![ModelTier::Opus, ModelTier::Sonnet],
            tick_s: 5,
            characters_root: PathBuf::from("characters"),
            sim_root: PathBuf::from("sim"),
            segments_data_dir: PathBuf::from("crates/shadowrunr-segments/data"),
            max_featured: 8,
            featured_every_ticks: 1,
            pool_size: 2,
            keep_memory: false,
            platforms: vec!["twitch".into()],
            hooks: Hooks::default(),
            quota: None,
            brief: BriefSource::None,
            data_dir: PathBuf::from("data"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunOutput {
    pub spec: SimRunSpec,
    /// `<out>/<run_id>/transcript.jsonl`
    pub transcript: PathBuf,
    /// `None` when judging was off or the judge's reply was void (see `run.json`).
    pub verdict: Option<JudgeVerdict>,
}

/// Run one simulated stream and (optionally) judge it, with default options. `out_dir` is
/// the tournament directory; the run lands in `<out_dir>/<run_id>/`.
pub async fn run_one(
    spec: SimRunSpec,
    out_dir: PathBuf,
    engine: Arc<dyn Engine>,
    judge: bool,
) -> Result<RunOutput, SimError> {
    let opts = SimOptions {
        judge,
        ..SimOptions::default()
    };
    run_one_with(spec, out_dir, engine, &opts).await
}

/// The serde wire name of a unit enum value (`haiku`, `SEG-01`, `cultist`...).
pub fn wire<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

pub(crate) fn write_json<T: Serialize>(path: &std::path::Path, v: &T) -> Result<(), SimError> {
    let text = serde_json::to_string_pretty(v).map_err(|e| SimError::Io(e.to_string()))?;
    std::fs::write(path, text + "\n").map_err(|e| SimError::Io(format!("{}: {e}", path.display())))
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(
    path: &std::path::Path,
) -> Result<T, SimError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| SimError::Io(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| SimError::Io(format!("{}: {e}", path.display())))
}
