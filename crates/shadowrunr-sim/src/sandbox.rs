//! Sandbox sessions: the simulated room and the real brain on the wall clock. Stream time
//! is wall time since the session started, so a slow engine call shows up as a silence
//! (`say-gap-ceiling`). The daemon hosts a session (`sandbox.start`), hands every event to
//! its live broadcast through the [`EventSink`], and completes the report's `stage_ack` and
//! `judge` checks; this module fills in `say_gap`, `core_errors`, `sign_off` and `chaos`.

use std::path::PathBuf;
use std::sync::Arc;

use shadowrunr_types::sandbox::{SandboxReport, SandboxStartParams};
use shadowrunr_types::{Engine, ModelTier, StreamEvent};
use tokio::sync::watch;

use crate::{SimError, SimOptions};

/// Audience size of a sandbox session's simulated room.
pub const SANDBOX_AUDIENCE: u32 = 30;

/// One session to run.
#[derive(Debug, Clone)]
pub struct SandboxRun {
    pub session_id: String,
    pub params: SandboxStartParams,
    /// `runs/sandbox/<session_id>`: transcript.jsonl, ledger.jsonl, spec.json, session.json.
    pub dir: PathBuf,
    pub core_tier: ModelTier,
    /// Tone when `params.tone` is absent (the character's active tone).
    pub default_tone: String,
}

/// Called once per transcript event, in order, as it happens; `seq` is the event's 1-based
/// line in transcript.jsonl (the `/events` envelope seq).
pub type EventSink = Arc<dyn Fn(u64, &StreamEvent) + Send + Sync>;

/// Live progress, published on every beat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SandboxProgress {
    pub elapsed_s: u32,
    pub planned_s: u32,
    pub events: u64,
    pub max_say_gap_s: f64,
    pub core_errors: u32,
    /// The closing window has started (planned end or a stop request).
    pub closing: bool,
}

/// Run one session on the wall clock until its planned end or until `stop` turns true, then
/// write `session.json` and return its report. `stage_ack` and `judge` are left
/// `unmeasured` for the caller to complete.
pub async fn run_sandbox(
    run: SandboxRun,
    engine: Arc<dyn Engine>,
    opts: &SimOptions,
    sink: EventSink,
    stop: watch::Receiver<bool>,
    progress: watch::Sender<SandboxProgress>,
) -> Result<SandboxReport, SimError> {
    let _ = (run, engine, opts, sink, stop, progress);
    Err(SimError::Spec("sandbox sessions are not built yet".into()))
}
