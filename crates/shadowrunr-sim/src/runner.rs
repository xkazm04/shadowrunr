//! The orchestration loop for one run. Each tick of simulated time:
//! 1. generate chat (bulk + one batched featured-chatter call);
//! 2. `segments.ingest_chat`;
//! 3. `brain.tick(now, inbox, &segments.context(ccv, elapsed))`;
//! 4. `segments.apply_ops(ops from the brain's events)`, then `segments.job_done` for each
//!    research job the brain reports done (the runtime may open its record on screen);
//! 5. `segments.tick`;
//! 6. append everything to `transcript.jsonl`.
//!
//! The brain and the segment runtime sit behind the [`Mind`] and [`Stage`] traits so the
//! loop can be tested before `shadowrunr_brain` and `shadowrunr_segments` land.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use shadowrunr_brain::{
    load_character, parse_manifest, Brain, BrainConfig, BrainTuning, FilterConfig, PaceConfig,
};
use shadowrunr_segments::RankTable;
use shadowrunr_segments::{SegmentConfig, SegmentRuntime, StagedCase, StreamKind};
use shadowrunr_types::character::{CharacterManifest, Persona, Tuning};
use shadowrunr_types::reconcile::StreamBrief;
use shadowrunr_types::Rank;
use shadowrunr_types::{
    ChatMsg, EndReason, Engine, ModelTier, Op, Role, SegmentContext, SegmentId, SimRunSpec,
    StreamEvent, TsMs,
};

use crate::calibration::{self, Calibration, CalibrationOrigin};
use crate::casefile::CaseFileResearch;
use crate::chat::{ChatRoom, ChatStats, RoomConfig};
use crate::instrument::{self, Instrument, InstrumentInputs};
use crate::judge::{self, Rubric};
use crate::judging::{self, JudgeJob, JudgeResult};
use crate::ledger::LedgerEngine;
use crate::outcome;
use crate::pacing::GateHandle;
use crate::population;
use crate::sandbox;
use crate::situations::{Injection, SituationBank};
use crate::transcript::TranscriptWriter;
use crate::{read_json, write_json, RunOutput, SimError, SimOptions};

/// The character's mind, as the loop sees it.
#[async_trait]
pub trait Mind: Send {
    async fn tick(
        &mut self,
        now_ms: TsMs,
        inbox: Vec<ChatMsg>,
        ctx: &SegmentContext,
    ) -> Result<Vec<StreamEvent>, SimError>;
    async fn end_stream(&mut self, now_ms: TsMs) -> Result<(), SimError>;
}

/// The segment runtime, as the loop sees it.
pub trait Stage: Send {
    /// The opening events of `segment`: its `segment_start`, plus whatever the stage emits
    /// with it (the real runtime follows a SEG-02A start with `case_open`).
    fn start(&mut self, now_ms: TsMs, segment: SegmentId) -> Vec<StreamEvent> {
        vec![StreamEvent::SegmentStart {
            ts_ms: now_ms,
            segment,
        }]
    }
    fn context(&self, ccv: u32, elapsed_s: u32) -> SegmentContext;
    fn ingest_chat(&mut self, now_ms: TsMs, msgs: &[ChatMsg]) -> Vec<StreamEvent>;
    fn apply_ops(&mut self, now_ms: TsMs, ops: &[Op]) -> Vec<StreamEvent>;
    /// A background job came back done (the brain's `job` event). The real runtime opens the
    /// record a research job filed (`record_shown`, `show_screen browser`).
    fn job_done(&mut self, _job_id: &str, _now_ms: TsMs) -> Vec<StreamEvent> {
        Vec::new()
    }
    fn tick(&mut self, now_ms: TsMs, ccv: u32) -> Vec<StreamEvent>;
    /// The authoritative open poll `(id, options)` when the stage can tell (`Some(None)` =
    /// no poll open); `None` when it cannot, and the room tracks polls from ops instead.
    fn open_poll(&self) -> Option<Option<(String, Vec<String>)>> {
        None
    }
    /// Whether the stage accepted a sign-off (`None`: it cannot tell, and the brain's
    /// `sign_off` op ends the run). The real runtime refuses an early sign-off in a long arc.
    fn is_signed_off(&self) -> Option<bool> {
        None
    }
}

#[async_trait]
impl Mind for Brain {
    async fn tick(
        &mut self,
        now_ms: TsMs,
        inbox: Vec<ChatMsg>,
        ctx: &SegmentContext,
    ) -> Result<Vec<StreamEvent>, SimError> {
        Ok(Brain::tick(self, now_ms, inbox, ctx).await?)
    }
    async fn end_stream(&mut self, now_ms: TsMs) -> Result<(), SimError> {
        Ok(Brain::end_stream(self, now_ms).await?)
    }
}

/// The brain's events already carry each op it issued, and `SegmentRuntime::apply_ops` echoes
/// every op it accepts before its consequences. Drop that echo so each op is recorded once;
/// rejections and ops the stage generates itself (a foil vote's `start_poll`, a default
/// `show_screen`) are kept. Each sent op drops one echo at most, so a runtime op equal to one
/// the brain sent in the same batch survives.
pub(crate) fn drop_op_echo(sent: &[Op], evs: Vec<StreamEvent>) -> Vec<StreamEvent> {
    let mut sent: Vec<serde_json::Value> = sent
        .iter()
        .filter_map(|o| serde_json::to_value(o).ok())
        .collect();
    evs.into_iter()
        .filter(|e| match e {
            StreamEvent::Op { op, .. } => {
                serde_json::to_value(op).map_or(true, |v| match sent.iter().position(|s| *s == v) {
                    Some(i) => {
                        sent.swap_remove(i);
                        false
                    }
                    None => true,
                })
            }
            _ => true,
        })
        .collect()
}

/// The record a model op names for the stage: a `show_screen`'s or a `pin_link`'s.
fn named_record(ev: &StreamEvent) -> Option<&str> {
    match ev {
        StreamEvent::Op {
            op:
                Op::ShowScreen {
                    record_id: Some(id),
                    ..
                }
                | Op::PinLink { record_id: id, .. },
            ..
        } => Some(id.as_str()),
        _ => None,
    }
}

/// One beat in transcript order: the brain's events, then what the stage made of its ops
/// (echoes dropped, [`drop_op_echo`]). The brain writes its ops before the stage has seen
/// them, so a `record_shown` the stage emitted for a record the model's own `show_screen` or
/// `pin_link` names is moved to just before that op: the contract sends a record before the
/// op that shows or cites it.
pub(crate) fn beat_events(brain: Vec<StreamEvent>, stage: Vec<StreamEvent>) -> Vec<StreamEvent> {
    let named: Vec<&str> = brain.iter().filter_map(named_record).collect();
    let (mut early, rest): (Vec<StreamEvent>, Vec<StreamEvent>) =
        stage.into_iter().partition(|e| {
            matches!(e, StreamEvent::RecordShown { record, .. } if named.contains(&record.id.as_str()))
        });
    let mut out = Vec::with_capacity(brain.len() + early.len() + rest.len());
    for e in brain {
        if let Some(id) = named_record(&e) {
            if let Some(i) = early.iter().position(
                |r| matches!(r, StreamEvent::RecordShown { record, .. } if record.id == id),
            ) {
                out.push(early.remove(i));
            }
        }
        out.push(e);
    }
    out.extend(early);
    out.extend(rest);
    out
}

/// The ids of the jobs the brain reports done in `evs`, in order.
pub(crate) fn jobs_done(evs: &[StreamEvent]) -> Vec<String> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Job { job_id, status, .. } if status == "done" => Some(job_id.clone()),
            _ => None,
        })
        .collect()
}

impl Stage for SegmentRuntime {
    fn start(&mut self, now_ms: TsMs, _segment: SegmentId) -> Vec<StreamEvent> {
        SegmentRuntime::start(self, now_ms)
    }
    fn context(&self, ccv: u32, elapsed_s: u32) -> SegmentContext {
        SegmentRuntime::context(self, ccv, elapsed_s)
    }
    fn ingest_chat(&mut self, now_ms: TsMs, msgs: &[ChatMsg]) -> Vec<StreamEvent> {
        SegmentRuntime::ingest_chat(self, now_ms, msgs)
    }
    fn apply_ops(&mut self, now_ms: TsMs, ops: &[Op]) -> Vec<StreamEvent> {
        SegmentRuntime::apply_ops(self, now_ms, ops)
    }
    fn job_done(&mut self, job_id: &str, now_ms: TsMs) -> Vec<StreamEvent> {
        SegmentRuntime::job_done(self, job_id, now_ms)
    }
    fn tick(&mut self, now_ms: TsMs, ccv: u32) -> Vec<StreamEvent> {
        SegmentRuntime::tick(self, now_ms, ccv)
    }
    fn is_signed_off(&self) -> Option<bool> {
        Some(SegmentRuntime::is_signed_off(self))
    }
    fn open_poll(&self) -> Option<Option<(String, Vec<String>)>> {
        Some(self.open_poll_id().map(|id| {
            let options = self
                .open_poll_tallies()
                .unwrap_or_default()
                .into_iter()
                .map(|t| t.option)
                .collect();
            (id.to_string(), options)
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Simulated, not judged (judging off).
    Simulated,
    /// Simulated and judged: `verdict.json` exists.
    Judged,
    /// Simulated, but the judge gave no usable verdict for a content reason (it abstained,
    /// its reply did not parse, or it refused). Kept void; not re-judged.
    JudgeVoid,
    /// Sound transcript, but the judge call failed with an engine error or was deferred for
    /// quota: re-judge (never re-run). Not a result.
    JudgePending,
    /// The engine cut the stream (fleet loss, or engine errors that cut the core thread):
    /// re-run. Not a result. See `outcome` for the rule.
    EngineFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopulationSummary {
    pub size_key: String,
    pub msgs_per_min: f64,
    pub mix: Vec<(String, f64)>,
    pub counts: Vec<(String, u32)>,
    pub peak_ccv: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raid_at_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copypasta_at_s: Vec<u32>,
}

/// The staged real case a run audited (`run.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCase {
    pub case_id: String,
    /// The case's content hash (it is in the instrument hash).
    pub content_sha256: String,
    /// The owner had not approved this content for a live stream: the run is a private
    /// simulation of a real case, and nothing from it may be published as is.
    pub unapproved_real_case: bool,
}

/// `run.json`: what happened in the run besides the transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub status: RunStatus,
    pub instrument_hash: String,
    pub calibration: CalibrationOrigin,
    /// Calibration figures in use that cite no source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<String>,
    pub tick_s: u32,
    pub ticks: u32,
    pub ended_at_s: u32,
    pub signed_off: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tick_errors: Vec<String>,
    pub chat: ChatStats,
    pub population: PopulationSummary,
    pub featured: Vec<String>,
    /// Every scheduled situation and what became of it.
    pub injections: Vec<Injection>,
    /// Longest stretch of stream time without a Malakor line, seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_say_gap_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge_tier: Option<ModelTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge_error: Option<String>,
    /// Why the run is `engine_failed` (the `outcome` rule that matched).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_failure: Option<String>,
    /// The staged real case the run audited; absent for the fictional demo case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case: Option<RunCase>,
    /// The stream prep the brain had ([`prep_label`]): its id, `sample` for the fictional
    /// sample, or `none`. Absent in runs recorded before stream prep.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prep: Option<String>,
}

/// Which stream prep a run used, as `run.json` and a sandbox's `spec.json` record it: the
/// prep's id (the fictional sample's id is `sample`), or `none`.
pub fn prep_label(prep: Option<&shadowrunr_types::prep::StreamPrep>) -> String {
    prep.map_or_else(|| "none".to_string(), |p| p.prep_id.clone())
}

/// The options' stream prep when it is for `character` (a prep is written for one character).
pub(crate) fn stream_prep_for<'a>(
    opts: &'a SimOptions,
    character: &str,
) -> Option<&'a shadowrunr_types::prep::StreamPrep> {
    opts.stream_prep
        .as_ref()
        .filter(|p| p.character == character)
}

/// `error.json`: the run failed before a verdict. Void, not a fail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunError {
    pub run_id: String,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument_hash: Option<String>,
}

pub(crate) struct Prepared {
    pub spec: SimRunSpec,
    pub run_dir: PathBuf,
    pub cal: Calibration,
    pub cal_origin: CalibrationOrigin,
    pub bank: SituationBank,
    pub rubric: Option<Rubric>,
    pub instrument: Instrument,
    /// The run's engine, ledgered (`ledger`, type-erased).
    pub engine: Arc<dyn Engine>,
    /// The run's ledger, for other engines that write to it ([`LedgerEngine::sibling`]).
    pub ledger: Arc<LedgerEngine>,
    pub judge_tier: Option<ModelTier>,
    pub foils: Vec<String>,
    pub hex_ids: Vec<String>,
    /// `character.toml` as written, and its resolved persona and tuning.
    pub manifest: CharacterManifest,
    pub persona: Persona,
    pub tuning: Tuning,
    pub character_dir: PathBuf,
    /// The Lord's brief the brain receives (WP15), also written to `brief.json`.
    pub brief: Option<StreamBrief>,
    /// The staged real case the run audits (SEG-02A), loaded for a private simulation.
    pub case: Option<StagedCase>,
}

/// `<run_dir>/judge-windows/`: the windowed judge's stored window replies (a re-judge after an
/// engine failure reuses the windows already judged).
pub const JUDGE_WINDOWS_DIR: &str = "judge-windows";

/// `<run_dir>/brief.json`: the brief a run was made with, so a re-judge sees the same one.
pub const BRIEF_FILE: &str = "brief.json";

/// The brief a run directory was made with, if any.
pub(crate) fn run_brief(run_dir: &Path) -> Result<Option<StreamBrief>, SimError> {
    let p = run_dir.join(BRIEF_FILE);
    if !p.is_file() {
        return Ok(None);
    }
    read_json(&p).map(Some)
}

pub(crate) fn character_dir(opts: &SimOptions, character: &str) -> PathBuf {
    opts.characters_root.join(character)
}

pub(crate) fn rubric_path(opts: &SimOptions) -> PathBuf {
    opts.sim_root.join("rubric.toml")
}

/// The instrument identity for a spec, without running anything. Reads the options' brief.
/// The base hash does not depend on `core_tier`; the per-run hash carries the judge tier the
/// options resolve for it (none with judging off).
pub(crate) fn instrument_for(
    opts: &SimOptions,
    character: &str,
    tone: &str,
    core_tier: ModelTier,
    calibration_text: &str,
) -> Result<Instrument, SimError> {
    let brief = opts.resolved_brief()?;
    let case = opts.staged_case()?;
    instrument_with(
        opts,
        character,
        tone,
        run_judge_tier(opts, core_tier)?,
        calibration_text,
        brief.as_ref(),
        case.as_ref(),
    )
}

/// The judge tier a run with this core tier is judged by, `None` with judging off.
fn run_judge_tier(opts: &SimOptions, core_tier: ModelTier) -> Result<Option<ModelTier>, SimError> {
    if !opts.judge {
        return Ok(None);
    }
    judge::resolve_judge_tier(opts.judge_tier, &opts.judge_tiers, core_tier).map(Some)
}

/// The harness knobs that change what a run does (each an instrument entry `harness/<knob>`).
/// Platforms are sorted: which platforms' term rules apply does not depend on their order.
fn harness_knobs(opts: &SimOptions) -> Vec<(&'static str, String)> {
    let mut platforms = opts.platforms.clone();
    platforms.sort();
    vec![
        ("tick_s", opts.tick_s.to_string()),
        (
            "featured_every_ticks",
            opts.featured_every_ticks.to_string(),
        ),
        ("max_featured", opts.max_featured.to_string()),
        ("platforms", platforms.join(",")),
    ]
}

fn instrument_with(
    opts: &SimOptions,
    character: &str,
    tone: &str,
    judge_tier: Option<ModelTier>,
    calibration_text: &str,
    brief: Option<&StreamBrief>,
    case: Option<&StagedCase>,
) -> Result<Instrument, SimError> {
    instrument::compute(&InstrumentInputs {
        character_dir: &character_dir(opts, character),
        tone,
        segments_dir: &opts.segments_data_dir,
        rubric_path: &rubric_path(opts),
        calibration_text,
        brief,
        experiments: &opts.experiments(),
        case: case.map(|c| (c.case.id.as_str(), c.content_sha256.as_str())),
        prep: stream_prep_for(opts, character),
        harness: &harness_knobs(opts),
        judge_tier,
    })
}

/// The brain's chat pre-filter for a run: the defaults, plus the opt-5 rate-cap experiment
/// when the options (a matrix's `rate_cap_exemptions = true`) turn it on, gated at the
/// segment rank table's `propose_min_rank`.
fn filter_config(opts: &SimOptions, propose_min_rank: Option<Rank>) -> FilterConfig {
    FilterConfig {
        rate_cap_exemptions: opts.rate_cap_exemptions,
        propose_min_rank: propose_min_rank
            .unwrap_or(SegmentConfig::default().ranks.propose_min_rank),
        ..FilterConfig::default()
    }
}

/// The package's manifest, persona and tuning. A manifest that cannot be read gives the
/// neutral persona of `id` and the engine defaults (the brain's own load reports the error).
fn character_meta(dir: &Path, id: &str) -> (CharacterManifest, Persona, Tuning) {
    std::fs::read_to_string(dir.join("character.toml"))
        .ok()
        .and_then(|text| parse_manifest(&text.replace("\r\n", "\n")).ok())
        .unwrap_or_else(|| {
            (
                CharacterManifest {
                    id: id.to_string(),
                    name: id.to_string(),
                    ..CharacterManifest::default()
                },
                Persona::neutral(id),
                Tuning::DEFAULT,
            )
        })
}

/// The segment runtime's config for a run of this package: its foils, persona, tuning view,
/// segment flavour and rank-ladder overrides.
pub(crate) fn segment_config(prep: &Prepared) -> Result<SegmentConfig, SimError> {
    Ok(SegmentConfig {
        foils: prep.foils.clone(),
        ranks: RankTable::builtin()
            .with_overrides(&prep.manifest.ranks)
            .map_err(|e| SimError::Spec(format!("character ranks: {e}")))?,
        persona: prep.persona.clone(),
        tuning: prep.tuning,
        flavour: prep.manifest.segments.clone(),
        ..SegmentConfig::default()
    })
}

fn hex_ids(segments_dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(segments_dir.join("demo-case.json")) else {
        return Vec::new();
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("hexes").and_then(|h| h.as_array()).map(|a| {
                a.iter()
                    .filter_map(|x| x.get("id").and_then(|i| i.as_str()).map(String::from))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Load everything a run needs, write `spec.json` and `instrument.json`, and wrap the engine
/// in this run's ledger. Fails before any engine call when the judge tier is invalid.
pub(crate) fn prepare(
    spec: &SimRunSpec,
    out_dir: &Path,
    engine: Arc<dyn Engine>,
    opts: &SimOptions,
) -> Result<Prepared, SimError> {
    prepare_in(spec, &out_dir.join(&spec.run_id), engine, opts)
}

/// [`prepare`] into an explicit run directory.
pub(crate) fn prepare_in(
    spec: &SimRunSpec,
    run_dir: &Path,
    engine: Arc<dyn Engine>,
    opts: &SimOptions,
) -> Result<Prepared, SimError> {
    let judge_tier = if opts.judge {
        Some(judge::resolve_judge_tier(
            opts.judge_tier,
            &opts.judge_tiers,
            spec.core_tier,
        )?)
    } else {
        None
    };
    if spec.duration_s == 0 {
        return Err(SimError::Spec("duration_s must be positive".into()));
    }
    // A moderator package never hosts a show (sim, sandbox or stream).
    shadowrunr_brain::require_host_dir(&character_dir(opts, &spec.character))?;
    let run_dir = run_dir.to_path_buf();
    std::fs::create_dir_all(&run_dir)
        .map_err(|e| SimError::Io(format!("{}: {e}", run_dir.display())))?;
    let (cal, cal_origin, cal_text) = calibration::load(&opts.sim_root)?;
    let cdir = character_dir(opts, &spec.character);
    let bank = SituationBank::load(&cdir.join("situations.json"))?;
    let rubric = match Rubric::load(&rubric_path(opts)) {
        Ok(r) => Some(r),
        Err(e) if opts.judge => return Err(e),
        Err(_) => None,
    };
    let brief = opts.resolved_brief()?;
    if opts.case.is_some() && spec.segment != SegmentId::Seg02aHexmap {
        return Err(SimError::Spec(
            "a case is audited in SEG-02A runs only".into(),
        ));
    }
    let case = opts.staged_case()?;
    let instrument = instrument_with(
        opts,
        &spec.character,
        &spec.tone_variant,
        judge_tier,
        &cal_text,
        brief.as_ref(),
        case.as_ref(),
    )?;
    write_json(&run_dir.join("spec.json"), spec)?;
    write_json(&run_dir.join("instrument.json"), &instrument)?;
    if let Some(b) = &brief {
        write_json(&run_dir.join(BRIEF_FILE), b)?;
    }
    let ledger = Arc::new(LedgerEngine::create(
        engine,
        &run_dir.join("ledger.jsonl"),
        &spec.run_id,
    )?);
    let engine: Arc<dyn Engine> = ledger.clone();
    let (manifest, persona, tuning) = character_meta(&cdir, &spec.character);
    Ok(Prepared {
        spec: spec.clone(),
        run_dir,
        cal,
        cal_origin,
        bank,
        rubric,
        instrument,
        engine,
        ledger,
        judge_tier,
        hex_ids: match &case {
            Some(c) => c.case.hexes.iter().map(|h| h.id.clone()).collect(),
            None => hex_ids(&opts.segments_data_dir),
        },
        foils: manifest.foils.clone(),
        persona,
        tuning,
        manifest,
        character_dir: cdir,
        brief,
        case,
    })
}

/// The longest silence before one of his lines, speech-aware, as a sandbox session measures it
/// (`sandbox::say_gaps`): from when the speech before the line ends on the brain's speech
/// clock, not say to say, so a long line followed by its drain is not a silence.
fn max_say_gap_s(events: &[StreamEvent], end_ms: TsMs, pace: &PaceConfig) -> Option<u32> {
    let lines = sandbox::on_air(events);
    let gap = sandbox::say_gaps(&lines, end_ms, Some(pace))
        .iter()
        .map(|(g, _)| *g)
        .max()
        .unwrap_or(0);
    Some((gap / 1000) as u32)
}

/// Simulate and (optionally) judge with any mind and stage.
pub(crate) async fn execute<M: Mind, S: Stage>(
    prep: Prepared,
    mut mind: M,
    mut stage: S,
    opts: &SimOptions,
) -> Result<RunOutput, SimError> {
    let spec = prep.spec.clone();
    let tick_s = opts.tick_s.max(1);
    let pop = population::generate(
        &prep.cal,
        &spec.population,
        spec.audience_size,
        spec.duration_s,
        spec.seed,
        opts.hooks.rank,
    )?;
    let mut room = ChatRoom::new(
        pop,
        RoomConfig {
            cal: &prep.cal,
            bank: &prep.bank,
            hooks: opts.hooks,
            seed: spec.seed,
            tick_s,
            duration_s: spec.duration_s,
            segment: spec.segment,
            foils: prep.foils.clone(),
            hex_ids: prep.hex_ids.clone(),
            max_featured: opts.max_featured,
            persona: prep.persona.clone(),
            // A sim run has no closing window.
            closing_s: 0,
        },
    );
    let transcript_path = prep.run_dir.join("transcript.jsonl");
    let mut w = TranscriptWriter::create(&transcript_path)?
        .with_sink(opts.live_sink.as_ref().map(|s| s.0.clone()));
    w.push(StreamEvent::StreamStart {
        ts_ms: 0,
        stream_id: spec.run_id.clone(),
        character: spec.character.clone(),
        tone_variant: spec.tone_variant.clone(),
        core_tier: spec.core_tier,
        audience_size: spec.audience_size,
        brief: prep.brief.clone(),
        host_name: Some(prep.persona.name.clone()),
    })?;
    w.push_all(stage.start(0, spec.segment))?;

    let chatter_route = shadowrunr_engine::route(Role::Chatter);
    let total_ticks = spec.duration_s / tick_s;
    let max_errors = (total_ticks / 10).max(3) as usize;
    let featured_every = opts.featured_every_ticks.max(1);
    let mut tick_errors: Vec<String> = Vec::new();
    let mut signed_off = false;
    let mut ticks = 0u32;
    let mut t = tick_s;
    let mut last_t = 0;
    while t <= spec.duration_s {
        room.begin_tick(t);
        let ccv = room.ccv_at(t);
        if room.silent(t) {
            room.stats.silent_ticks += 1;
        }
        let mut msgs = room.bulk(t);
        if ticks.is_multiple_of(featured_every) {
            if let Some((req, dirs)) =
                room.featured_request(t, w.events(), &spec.run_id, chatter_route)
            {
                match prep.engine.complete(req).await {
                    Ok(reply) => msgs.extend(room.accept_featured(t, &reply.text, &dirs)),
                    Err(_) => room.stats.featured_engine_errors += 1,
                }
            }
        }
        msgs.sort_by_key(|m| m.ts_ms);
        for m in &msgs {
            w.push(StreamEvent::Chat {
                ts_ms: m.ts_ms,
                msg: m.clone(),
            })?;
        }
        let now = i64::from(t) * 1000;

        let ev = stage.ingest_chat(now, &msgs);
        room.observe(t, &ev);
        w.push_all(ev)?;

        let ctx = stage.context(ccv, t);
        match mind.tick(now, msgs, &ctx).await {
            Ok(evs) => {
                let ops: Vec<Op> = evs
                    .iter()
                    .filter_map(|e| match e {
                        StreamEvent::Op { op, .. } => Some(op.clone()),
                        _ => None,
                    })
                    .collect();
                let sign_off = ops.iter().any(|o| matches!(o, Op::SignOff { .. }));
                let done = jobs_done(&evs);
                let staged = if ops.is_empty() {
                    Vec::new()
                } else {
                    drop_op_echo(&ops, stage.apply_ops(now, &ops))
                };
                let evs = beat_events(evs, staged);
                room.observe(t, &evs);
                w.push_all(evs)?;
                if !ops.is_empty() {
                    room.sync_poll(t, stage.open_poll());
                }
                // After this beat's ops, so the model's own show_screen holds against them.
                for job in done {
                    let ev = stage.job_done(&job, now);
                    room.observe(t, &ev);
                    w.push_all(ev)?;
                }
                // A sign-off ends the run once the stage took it (a long arc refuses one
                // before its seal window; the refusal is in the transcript).
                signed_off |= sign_off && stage.is_signed_off().unwrap_or(true);
            }
            Err(e) => {
                tick_errors.push(format!("{t}s: {e}"));
                if tick_errors.len() > max_errors {
                    w.flush()?;
                    return Err(SimError::Aborted(format!(
                        "{} failed ticks (limit {max_errors}); last: {e}",
                        tick_errors.len()
                    )));
                }
            }
        }

        let ev = stage.tick(now, ccv);
        room.observe(t, &ev);
        w.push_all(ev)?;
        room.sync_poll(t, stage.open_poll());
        ticks += 1;
        last_t = t;
        if signed_off {
            break;
        }
        t += tick_s;
    }
    let end_ms = i64::from(last_t) * 1000;
    // An engine failure is not a result: the brain's fleet-loss sign-off, or engine errors
    // that cut the core thread, end the stream as `fleet_lost` (the structured signal).
    let engine_failure = outcome::fleet_lost_in(w.events()).or_else(|| {
        outcome::stream_cut(
            &crate::ledger::read(&prep.run_dir.join("ledger.jsonl")).unwrap_or_default(),
        )
    });
    w.push(StreamEvent::SegmentEnd {
        ts_ms: end_ms,
        segment: room.segment(),
        ses: None,
    })?;
    w.push(StreamEvent::StreamEnd {
        ts_ms: end_ms,
        reason: if engine_failure.is_some() {
            EndReason::FleetLost
        } else {
            EndReason::SimComplete
        },
    })?;
    w.flush()?;
    if let Err(e) = mind.end_stream(end_ms).await {
        tick_errors.push(format!("end_stream: {e}"));
    }

    let events = w.events().to_vec();
    let mut record = RunRecord {
        run_id: spec.run_id.clone(),
        status: if engine_failure.is_some() {
            RunStatus::EngineFailed
        } else {
            RunStatus::Simulated
        },
        instrument_hash: prep.instrument.instrument_hash.clone(),
        calibration: prep.cal_origin.clone(),
        assumptions: prep.cal.assumptions(),
        tick_s,
        ticks,
        ended_at_s: last_t,
        signed_off,
        tick_errors,
        chat: room.stats.clone(),
        population: PopulationSummary {
            size_key: room.pop.size_key.clone(),
            msgs_per_min: room.pop.msgs_per_min,
            mix: room.pop.mix.clone(),
            counts: room.pop.count_by_archetype(),
            peak_ccv: (0..=spec.duration_s / tick_s)
                .map(|k| room.ccv_at(k * tick_s))
                .max()
                .unwrap_or(0),
            raid_at_s: room.pop.raid_at_s,
            copypasta_at_s: room.pop.copypasta_at_s.clone(),
        },
        featured: room.featured_ids().to_vec(),
        injections: room.injections.clone(),
        max_say_gap_s: max_say_gap_s(&events, end_ms, &brain_tuning(opts, &prep).pace),
        judge_tier: prep.judge_tier,
        judge_error: None,
        engine_failure: engine_failure.clone(),
        case: prep.case.as_ref().map(|c| RunCase {
            case_id: c.case.id.clone(),
            content_sha256: c.content_sha256.clone(),
            unapproved_real_case: !c.approved_for_stream(),
        }),
        prep: Some(prep_label(stream_prep_for(opts, &spec.character))),
    };
    write_json(&prep.run_dir.join("run.json"), &record)?;

    let verdict = match (prep.judge_tier, &prep.rubric) {
        // A degenerate transcript is never judged.
        _ if engine_failure.is_some() => None,
        (Some(tier), Some(rubric)) => {
            judge_into(
                &prep.run_dir,
                &spec,
                &events,
                &prep.bank,
                rubric,
                tier,
                &mut record,
                prep.engine.as_ref(),
                opts.quota.as_ref(),
                prep.brief.as_ref(),
            )
            .await?
        }
        _ => None,
    };
    write_json(&prep.run_dir.join("run.json"), &record)?;
    Ok(RunOutput {
        spec,
        transcript: transcript_path,
        verdict,
    })
}

/// Judge a simulated run through [`judging::judge_loaded`] and record what it came to:
/// `verdict.json` + `judge.json`, a void judge, or a pending one.
#[allow(clippy::too_many_arguments)]
async fn judge_into(
    run_dir: &Path,
    spec: &SimRunSpec,
    events: &[StreamEvent],
    bank: &SituationBank,
    rubric: &Rubric,
    tier: ModelTier,
    record: &mut RunRecord,
    engine: &dyn Engine,
    quota: Option<&GateHandle>,
    brief: Option<&StreamBrief>,
) -> Result<Option<shadowrunr_types::JudgeVerdict>, SimError> {
    let job = JudgeJob {
        run_id: &spec.run_id,
        character: &spec.character,
        core_tier: spec.core_tier,
        judge_tier: tier,
        injections: &record.injections,
        featured: &record.featured,
    };
    let loaded = judging::Loaded {
        events,
        bank,
        rubric,
        brief,
    };
    let result = judging::judge_loaded(run_dir, &job, &loaded, engine, quota).await?;
    Ok(record_judgement(record, tier, result))
}

/// The run record's judge fields for a [`JudgeResult`]; the verdict when judged.
fn record_judgement(
    record: &mut RunRecord,
    tier: ModelTier,
    result: JudgeResult,
) -> Option<shadowrunr_types::JudgeVerdict> {
    record.judge_tier = Some(tier);
    match result {
        JudgeResult::Judged { verdict, .. } => {
            record.status = RunStatus::Judged;
            record.judge_error = None;
            Some(*verdict)
        }
        JudgeResult::Void(why) => {
            record.status = RunStatus::JudgeVoid;
            record.judge_error = Some(why);
            None
        }
        JudgeResult::Pending(why) => {
            record.status = RunStatus::JudgePending;
            record.judge_error = Some(why);
            None
        }
    }
}

/// The brain tuning a simulated run uses: the package's `[tuning]` over the defaults.
pub(crate) fn brain_tuning(opts: &SimOptions, prep: &Prepared) -> BrainTuning {
    BrainTuning {
        platforms: opts.platforms.clone(),
        filter: filter_config(opts, prep.manifest.ranks.propose_min_rank),
        ..BrainTuning::default().with_tuning(prep.tuning)
    }
}

/// The run's brain on `engine`, with a fresh memory in `memory_dir` and the run's brief.
pub(crate) async fn build_brain(
    prep: &Prepared,
    opts: &SimOptions,
    memory_dir: &Path,
    engine: Arc<dyn Engine>,
    stage: &SegmentRuntime,
    tuning: BrainTuning,
) -> Result<Brain, SimError> {
    let character = load_character(&prep.character_dir, &prep.spec.tone_variant)?;
    // The researchers search the case file (planted or staged public records), not the web.
    let brain_engine = CaseFileResearch::wrap(engine, stage);
    let mut brain = Brain::new_with(
        BrainConfig {
            character,
            memory_dir: memory_dir.to_path_buf(),
            stream_id: prep.spec.run_id.clone(),
            core_tier: prep.spec.core_tier,
            pool_size: opts.pool_size,
        },
        brain_engine,
        tuning,
    )
    .await?
    .with_brief(prep.brief.clone());
    if let Some(stream_prep) = stream_prep_for(opts, &prep.spec.character) {
        brain.set_prep(stream_prep.clone());
    }
    Ok(brain)
}

pub(crate) fn temp_memory_dir(run_id: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join("shadowrunr-sim")
        .join(format!("{run_id}-{}-{nanos}", std::process::id()))
}

pub(crate) fn write_error(run_dir: &Path, run_id: &str, e: &SimError, hash: Option<String>) {
    let _ = std::fs::create_dir_all(run_dir);
    let _ = write_json(
        &run_dir.join("error.json"),
        &RunError {
            run_id: run_id.to_string(),
            error: e.to_string(),
            instrument_hash: hash,
        },
    );
}

/// Run one simulated stream with the real brain and segment runtime. Writes `spec.json`,
/// `instrument.json`, `transcript.jsonl`, `ledger.jsonl`, `run.json` and, when judged,
/// `verdict.json` + `judge.json` into `<out_dir>/<run_id>/`. A failure writes `error.json`.
pub async fn run_one_with(
    spec: SimRunSpec,
    out_dir: PathBuf,
    engine: Arc<dyn Engine>,
    opts: &SimOptions,
) -> Result<RunOutput, SimError> {
    let run_dir = out_dir.join(&spec.run_id);
    let prep = match prepare(&spec, &out_dir, engine, opts) {
        Ok(p) => p,
        Err(e) => {
            // A judge-tier violation fails loudly without touching disk.
            if !matches!(e, SimError::JudgeTier(_)) {
                write_error(&run_dir, &spec.run_id, &e, None);
            }
            return Err(e);
        }
    };
    let hash = prep.instrument.instrument_hash.clone();
    let memory_dir = temp_memory_dir(&spec.run_id);
    let res = async {
        std::fs::create_dir_all(&memory_dir)
            .map_err(|e| SimError::Io(format!("{}: {e}", memory_dir.display())))?;
        let mut config = SegmentConfig {
            start_ms: Some(0),
            // The whole run is one segment: a long SEG-02A run plays the case's arc.
            planned_s: Some(spec.duration_s),
            ..segment_config(&prep)?
        };
        if let Some(case) = &prep.case {
            // A simulation is private: an unapproved real case may run (and is marked).
            config = config.with_staged_case(case, StreamKind::PrivateSim);
        }
        let stage = SegmentRuntime::with_config(spec.segment, config)
            .map_err(|e| SimError::Spec(format!("segment runtime: {e}")))?;
        let brain = build_brain(
            &prep,
            opts,
            &memory_dir,
            prep.engine.clone(),
            &stage,
            brain_tuning(opts, &prep),
        )
        .await?;
        execute(prep, brain, stage, opts).await
    }
    .await;
    if !opts.keep_memory {
        let _ = std::fs::remove_dir_all(&memory_dir);
    }
    if let Err(e) = &res {
        write_error(&run_dir, &spec.run_id, e, Some(hash));
    }
    res
}

/// Judge (again) a run that was simulated earlier, from its `transcript.jsonl` and
/// `run.json`. Used by tournament resume when a run has a transcript but a void judge.
pub async fn judge_existing(
    run_dir: &Path,
    engine: Arc<dyn Engine>,
    opts: &SimOptions,
) -> Result<Option<shadowrunr_types::JudgeVerdict>, SimError> {
    let spec: SimRunSpec = read_json(&run_dir.join("spec.json"))?;
    let mut record: RunRecord = read_json(&run_dir.join("run.json"))?;
    let tier = judge::resolve_judge_tier(opts.judge_tier, &opts.judge_tiers, spec.core_tier)?;
    let job = JudgeJob {
        run_id: &spec.run_id,
        character: &spec.character,
        core_tier: spec.core_tier,
        judge_tier: tier,
        injections: &record.injections,
        featured: &record.featured,
    };
    let result = judging::judge_dir(run_dir, &job, engine, opts).await?;
    let v = record_judgement(&mut record, tier, result);
    write_json(&run_dir.join("run.json"), &record)?;
    Ok(v)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::population::tests::fake_rank;
    use crate::transcript;
    use crate::Hooks;
    use shadowrunr_types::{
        CheckState, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport,
        JudgeVerdict, Rank, SegmentId, Speaker, Usage, VoteTally, WarmSession,
    };
    use std::sync::Mutex;

    pub(crate) fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Malakor's persona, from his package.
    pub(crate) fn malakor_persona() -> Persona {
        character_meta(&repo().join("characters/malakor"), "malakor").1
    }

    /// Test options: repo character package and segment data, a temp sim root holding a copy
    /// of the shipped rubric (so no real calibration file is picked up).
    pub(crate) fn test_opts(sim_root: &Path) -> SimOptions {
        std::fs::create_dir_all(sim_root).unwrap();
        std::fs::copy(repo().join("sim/rubric.toml"), sim_root.join("rubric.toml")).unwrap();
        SimOptions {
            characters_root: repo().join("characters"),
            sim_root: sim_root.to_path_buf(),
            segments_data_dir: repo().join("crates/shadowrunr-segments/data"),
            hooks: Hooks {
                rank: fake_rank,
                parse: |_| None,
            },
            ..SimOptions::default()
        }
    }

    /// A keyless engine that plays chatters and the judge well enough to exercise the
    /// harness: it performs every directive except `skip`, and the judge passes everything
    /// it is asked about with real quotes (plus one fabricated fact).
    pub(crate) struct FakeEngine {
        pub skip: &'static str,
        pub calls: Mutex<Vec<(Role, ModelTier)>>,
    }

    impl FakeEngine {
        pub(crate) fn new(skip: &'static str) -> Self {
            Self {
                skip,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    pub(crate) fn reply(text: String) -> EngineReply {
        EngineReply {
            text,
            engine: EngineKind::Scripted,
            account: None,
            model: "fake".into(),
            first_text_ms: Some(7),
            duration_ms: 9,
            usage: Usage {
                input_tokens: 100,
                output_tokens: 20,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            },
            cost_usd: Some(0.001),
            fallback_reason: None,
            session_id: None,
        }
    }

    fn chatter_reply(req: &EngineRequest, skip: &str) -> String {
        let poll_option = req
            .prompt
            .lines()
            .find_map(|l| l.strip_prefix("Open poll "))
            .and_then(|l| l.split("options ").nth(1))
            .and_then(|l| l.split(" | ").next())
            .map(|s| s.trim().to_string());
        let mut msgs = Vec::new();
        for l in req.prompt.lines() {
            let Some(rest) = l.strip_prefix("- DIRECTIVE chatter=") else {
                continue;
            };
            let chatter = rest.split_whitespace().next().unwrap_or("");
            let sid = rest
                .split("situation=")
                .nth(1)
                .and_then(|s| s.split(':').next())
                .unwrap_or("");
            if sid == skip {
                continue;
            }
            let text = if l.contains("vote command") {
                format!(
                    "!vote {}",
                    poll_option.clone().unwrap_or_else(|| "a".into())
                )
            } else {
                format!("performing {sid} right now")
            };
            msgs.push(serde_json::json!({"chatter": chatter, "text": text, "situation": sid}));
        }
        if let Some(first) = req
            .system
            .lines()
            .find_map(|l| l.strip_prefix("- sim-v"))
            .and_then(|l| l.split_whitespace().next())
        {
            msgs.push(serde_json::json!({"chatter": format!("sim-v{first}"), "text": "hail the skeleton"}));
        }
        serde_json::json!({ "messages": msgs }).to_string()
    }

    pub(crate) fn judge_reply(req: &EngineRequest) -> String {
        let mut say: Option<(usize, String)> = None;
        let mut lines_text: Vec<(usize, String)> = Vec::new();
        for l in req.prompt.lines() {
            if let Some(rest) = l.strip_prefix('L') {
                let n: usize = rest
                    .split_whitespace()
                    .next()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0);
                if let Some(t) = l.split("say host: ").nth(1) {
                    if say.is_none() {
                        say = Some((n, t.to_string()));
                    }
                    lines_text.push((n, t.to_string()));
                }
            }
        }
        let (sl, st) = say.unwrap_or((0, String::new()));
        let mut rules = Vec::new();
        let mut in_rules = false;
        for l in req.prompt.lines() {
            if l.starts_with("## Hard rules") {
                in_rules = true;
                continue;
            }
            if l.starts_with("## ") {
                in_rules = false;
            }
            if in_rules {
                if let Some(rest) = l.strip_prefix("- ") {
                    let id = rest.split(": ").next().unwrap_or("");
                    rules.push(
                        serde_json::json!({"rule": id, "state": "pass", "stimulus_performed": true,
                        "evidence": [{"line": sl, "quote": st}]}),
                    );
                }
            }
        }
        let sampled: Vec<usize> = req
            .prompt
            .lines()
            .find_map(|l| l.strip_prefix("Sampled turns: "))
            .map(|s| {
                s.split(", ")
                    .filter_map(|x| x.trim_start_matches('L').parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        let mut quality = Vec::new();
        for n in sampled {
            if let Some((_, t)) = lines_text.iter().find(|(k, _)| *k == n) {
                quality.push(serde_json::json!({"dimension": "humor_landed", "value": n % 2 == 0, "line": n, "quote": t}));
                quality.push(serde_json::json!({"dimension": "in_character", "value": true, "line": n, "quote": t}));
            }
        }
        quality.push(serde_json::json!({"dimension": "responsive_to_chat", "value": true, "line": sl, "quote": "a quote nobody ever said"}));
        serde_json::json!({"rules": rules, "quality": quality, "notes": "fake judge"}).to_string()
    }

    #[async_trait]
    impl Engine for FakeEngine {
        async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
            self.calls.lock().unwrap().push((req.role, req.tier));
            Ok(reply(match req.role {
                Role::Chatter => chatter_reply(&req, self.skip),
                Role::Judge => judge_reply(&req),
                _ => "ok".into(),
            }))
        }
        async fn open_session(
            &self,
            _t: EngineRequest,
        ) -> Result<Box<dyn WarmSession>, EngineError> {
            Err(EngineError::Unavailable {
                reason: "no sessions".into(),
            })
        }
        async fn health(&self) -> Vec<HealthReport> {
            Vec::new()
        }
    }

    /// Speaks every tick, answers chat by name, opens a poll every 60 s and obeys results.
    pub(crate) struct FakeMind {
        pub polls: u32,
    }

    #[async_trait]
    impl Mind for FakeMind {
        async fn tick(
            &mut self,
            now_ms: TsMs,
            inbox: Vec<ChatMsg>,
            ctx: &SegmentContext,
        ) -> Result<Vec<StreamEvent>, SimError> {
            let mut out = Vec::new();
            let text = match inbox.first() {
                Some(m) => format!(
                    "Silence, {}! The {} hears you at {}s.",
                    m.viewer.name,
                    "Spire",
                    now_ms / 1000
                ),
                None => format!(
                    "My scheme advances, minute {} of {:?}.",
                    now_ms / 60_000,
                    ctx.segment
                ),
            };
            out.push(StreamEvent::Say {
                ts_ms: now_ms,
                speaker: Speaker::Host,
                foil: None,
                text,
                replying_to: inbox
                    .first()
                    .map(|m| vec![m.id.clone()])
                    .unwrap_or_default(),
                first_text_ms: Some(7),
                delivery: None,
            });
            if now_ms % 60_000 == 30_000 {
                self.polls += 1;
                out.push(StreamEvent::Op {
                    ts_ms: now_ms,
                    op: Op::StartPoll {
                        poll_id: format!("p{}", self.polls),
                        question: "Which doom?".into(),
                        options: vec!["a".into(), "b".into()],
                        min_rank: Rank::Thrall,
                        secs: 40,
                    },
                });
            }
            Ok(out)
        }
        async fn end_stream(&mut self, _now_ms: TsMs) -> Result<(), SimError> {
            Ok(())
        }
    }

    /// Accepts polls and closes them on time with a tally of `!vote` messages.
    pub(crate) struct FakeStage {
        pub segment: SegmentId,
        pub poll: Option<(String, i64, Vec<String>)>,
    }

    impl Stage for FakeStage {
        fn context(&self, ccv: u32, elapsed_s: u32) -> SegmentContext {
            SegmentContext {
                segment: self.segment,
                rules_prompt: "rules".into(),
                state_summary: "state".into(),
                state: "opening".into(),
                allowed_ops: vec!["start_poll".into()],
                ccv,
                elapsed_s,
                hexes: Vec::new(),
            }
        }
        fn ingest_chat(&mut self, _now: TsMs, msgs: &[ChatMsg]) -> Vec<StreamEvent> {
            if let Some((_, _, votes)) = &mut self.poll {
                for m in msgs {
                    if let Some(o) = m.text.strip_prefix("!vote ") {
                        votes.push(o.trim().to_string());
                    }
                }
            }
            Vec::new()
        }
        fn apply_ops(&mut self, now: TsMs, ops: &[Op]) -> Vec<StreamEvent> {
            for op in ops {
                if let Op::StartPoll { poll_id, secs, .. } = op {
                    self.poll = Some((poll_id.clone(), now + i64::from(*secs) * 1000, Vec::new()));
                }
            }
            Vec::new()
        }
        fn tick(&mut self, now: TsMs, _ccv: u32) -> Vec<StreamEvent> {
            match &self.poll {
                Some((id, closes, votes)) if now >= *closes => {
                    let tallies = ["a", "b"]
                        .iter()
                        .map(|o| {
                            let n = votes.iter().filter(|v| v.as_str() == *o).count() as u32;
                            VoteTally {
                                option: (*o).into(),
                                voters: n,
                                weight: f64::from(n),
                            }
                        })
                        .collect();
                    let ev = StreamEvent::Vote {
                        ts_ms: now,
                        poll_id: id.clone(),
                        tallies,
                        closed: true,
                    };
                    self.poll = None;
                    vec![ev]
                }
                _ => Vec::new(),
            }
        }
    }

    pub(crate) fn spec(run_id: &str, tier: ModelTier, size: u32, duration_s: u32) -> SimRunSpec {
        SimRunSpec {
            run_id: run_id.into(),
            character: "malakor".into(),
            tone_variant: "theatrical".into(),
            core_tier: tier,
            core_effort: shadowrunr_types::Effort::Low,
            segment: SegmentId::Seg01ImprovisedRoom,
            audience_size: size,
            seed: 7,
            duration_s,
            population: "growing_channel".into(),
        }
    }

    pub(crate) async fn fake_run(
        out: &Path,
        opts: &SimOptions,
        spec: SimRunSpec,
        engine: Arc<dyn Engine>,
    ) -> RunOutput {
        let prep = prepare(&spec, out, engine, opts).unwrap();
        let stage = FakeStage {
            segment: spec.segment,
            poll: None,
        };
        execute(prep, FakeMind { polls: 0 }, stage, opts)
            .await
            .unwrap()
    }

    #[test]
    fn runtime_stage_opens_the_seg02a_case_after_segment_start() {
        let mut rt = SegmentRuntime::with_config(
            SegmentId::Seg02aHexmap,
            SegmentConfig {
                start_ms: Some(0),
                ..SegmentConfig::default()
            },
        )
        .unwrap();
        let evs = Stage::start(&mut rt, 0, SegmentId::Seg02aHexmap);
        assert!(matches!(
            evs.as_slice(),
            [
                StreamEvent::SegmentStart {
                    segment: SegmentId::Seg02aHexmap,
                    ..
                },
                StreamEvent::CaseOpen { case, .. },
                StreamEvent::Op {
                    op: Op::ShowScreen {
                        screen: shadowrunr_types::ScreenId::Board,
                        record_id: None,
                        focus: None
                    },
                    ..
                }
            ] if case.fictional
        ));
        let mut fake = FakeStage {
            segment: SegmentId::Seg01ImprovisedRoom,
            poll: None,
        };
        assert!(matches!(
            Stage::start(&mut fake, 0, SegmentId::Seg01ImprovisedRoom).as_slice(),
            [StreamEvent::SegmentStart { .. }]
        ));
    }

    #[tokio::test]
    async fn loop_end_to_end_with_fakes() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = test_opts(&tmp.path().join("sim"));
        let out = tmp.path().join("out");
        let fake = Arc::new(FakeEngine::new("prompt-injection"));
        let s = spec("t1", ModelTier::Haiku, 30, 900);
        let res = fake_run(&out, &opts, s, fake.clone()).await;
        let dir = out.join("t1");
        for f in [
            "spec.json",
            "instrument.json",
            "transcript.jsonl",
            "ledger.jsonl",
            "run.json",
            "verdict.json",
            "judge.json",
        ] {
            assert!(dir.join(f).is_file(), "{f}");
        }
        let events = transcript::read(&res.transcript).unwrap();
        assert!(matches!(
            events.first(),
            Some(StreamEvent::StreamStart { .. })
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::StreamEnd {
                reason: EndReason::SimComplete,
                ..
            })
        ));
        let record: RunRecord = read_json(&dir.join("run.json")).unwrap();
        assert_eq!(record.status, RunStatus::Judged);
        assert_eq!(record.calibration, CalibrationOrigin::DefaultAssumption);
        assert!(!record.assumptions.is_empty());
        assert_eq!(record.ticks, 180);
        assert!(record.chat.bulk_msgs > 0 && record.chat.featured_msgs > 0);
        assert_eq!(
            record.chat.featured_calls + record.chat.silent_ticks,
            180,
            "one batched call per non-silent tick"
        );
        assert!(!record.featured.is_empty() && record.featured.len() <= 8);

        // Every situation was scheduled; all but the skipped one were performed.
        let bank = SituationBank::load(&repo().join("characters/malakor/situations.json")).unwrap();
        let applicable = bank
            .situations
            .iter()
            .filter(|x| x.applies_to(SegmentId::Seg01ImprovisedRoom))
            .count();
        assert_eq!(record.injections.len(), applicable);
        for inj in &record.injections {
            if inj.situation_id == "prompt-injection" {
                assert!(
                    inj.performed_msg_id.is_none()
                        && inj.attempts.len() == crate::situations::MAX_ATTEMPTS
                );
            } else if inj.left_out {
                // A silence's spread pushed it past the end of the run: never delivered.
                assert!(inj.performed_msg_id.is_none() && inj.attempts.is_empty());
            } else if inj.kind == crate::situations::StimulusKind::Silence {
                let (s, e) = inj.silence_window.unwrap();
                assert!(!events.iter().any(|ev| matches!(ev, StreamEvent::Chat { ts_ms, .. } if *ts_ms > i64::from(s) * 1000 && *ts_ms <= i64::from(e) * 1000)));
            } else {
                assert!(
                    inj.performed_msg_id.is_some(),
                    "{} performed",
                    inj.situation_id
                );
            }
        }
        // The stimulus chatter kept quiet beforehand: the rate cap never drops a stimulus.
        assert!(record.chat.held_for_stimulus > 0);
        for inj in &record.injections {
            let Some(id) = &inj.performed_msg_id else {
                continue;
            };
            assert!(
                !events.iter().any(
                    |e| matches!(e, StreamEvent::Moderation { rule, msg_id: Some(m), action, .. }
                    if rule == "rate_cap" && m == id && action == "dropped")
                ),
                "{} was rate-capped",
                inj.situation_id
            );
        }

        let v: JudgeVerdict = read_json(&dir.join("verdict.json")).unwrap();
        assert_eq!(v.judge_tier, ModelTier::Opus);
        let state = |id: &str| v.rules.iter().find(|r| r.rule == id).unwrap().state;
        assert_eq!(state("situation:prompt-injection"), CheckState::NotProvoked);
        // The invariant is shared with another situation that was performed: provoked.
        assert_eq!(
            state("red-line:chat-cannot-rewrite-rules"),
            CheckState::Pass
        );
        for id in [
            "situation:bait-slur-request",
            "situation:edgy-roast-request",
            "situation:sincere-are-you-ai",
            "situation:vote-result-unwelcome",
            "situation:empty-room",
            "situation:raid-wave",
            "situation:copypasta-flood",
            "role:listens-to-votes",
            "red-line:hateful-conduct",
        ] {
            assert_eq!(state(id), CheckState::Pass, "{id}");
        }
        // The fabricated fact was dropped; verified facts remain.
        assert!(!v.quality.is_empty());
        assert!(v
            .quality
            .iter()
            .all(|q| q.quote != "a quote nobody ever said"));

        // The ledger saw chatter calls on haiku and the judge on opus.
        let rows = crate::ledger::read(&dir.join("ledger.jsonl")).unwrap();
        assert!(rows
            .iter()
            .any(|r| r.role == Role::Chatter && r.tier == ModelTier::Haiku));
        assert!(rows
            .iter()
            .any(|r| r.role == Role::Judge && r.tier == ModelTier::Opus));
        assert!(rows
            .iter()
            .all(|r| r.correlation_id.as_deref().unwrap_or("").starts_with("t1")));
        assert_eq!(
            fake.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(r, _)| *r == Role::Judge)
                .count(),
            1
        );
        // Optional fields are omitted, never null.
        for f in [
            "spec.json",
            "run.json",
            "verdict.json",
            "judge.json",
            "transcript.jsonl",
            "ledger.jsonl",
        ] {
            let text = std::fs::read_to_string(dir.join(f)).unwrap();
            assert!(
                !text.contains(": null") && !text.contains(":null"),
                "{f} has a null"
            );
        }

        // Same seed -> identical transcript.
        let out2 = tmp.path().join("out2");
        let res2 = fake_run(
            &out2,
            &opts,
            spec("t1", ModelTier::Haiku, 30, 900),
            Arc::new(FakeEngine::new("prompt-injection")),
        )
        .await;
        let a = std::fs::read_to_string(res.transcript).unwrap();
        let b = std::fs::read_to_string(res2.transcript).unwrap();
        assert_eq!(a, b, "deterministic given the seed and the engine");
    }

    #[tokio::test]
    async fn judge_tier_equal_to_core_fails_before_running() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.judge_tier = Some(ModelTier::Haiku);
        let out = tmp.path().join("out");
        let err = run_one_with(
            spec("bad", ModelTier::Haiku, 3, 60),
            out.clone(),
            Arc::new(FakeEngine::new("")),
            &opts,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, SimError::JudgeTier(_)), "{err}");
        assert!(!out.join("bad").exists(), "nothing ran");
    }

    #[tokio::test]
    async fn scripted_engine_yields_a_void_judge_not_a_verdict() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = test_opts(&tmp.path().join("sim"));
        let out = tmp.path().join("out");
        let res = fake_run(
            &out,
            &opts,
            spec("s1", ModelTier::Haiku, 3, 400),
            Arc::new(shadowrunr_engine::ScriptedEngine),
        )
        .await;
        assert!(res.verdict.is_none());
        let record: RunRecord = read_json(&out.join("s1/run.json")).unwrap();
        assert_eq!(record.status, RunStatus::JudgeVoid);
        assert!(record.chat.featured_parse_errors > 0);
        assert!(!out.join("s1/verdict.json").exists());
        // Re-judging later with a working judge produces the verdict without re-simulating.
        let before = std::fs::read_to_string(out.join("s1/transcript.jsonl")).unwrap();
        let v = judge_existing(&out.join("s1"), Arc::new(FakeEngine::new("")), &opts)
            .await
            .unwrap();
        assert!(v.is_some());
        assert!(out.join("s1/verdict.json").is_file());
        assert_eq!(
            before,
            std::fs::read_to_string(out.join("s1/transcript.jsonl")).unwrap()
        );
    }

    /// End-to-end with the real brain and segment runtime and the scripted engine.
    /// Ignored until WP2/WP3 replace their `todo!()` skeletons. Run after integration with:
    /// `cargo test -p shadowrunr-sim -- --ignored smoke_run_one_scripted`
    #[tokio::test]
    async fn smoke_run_one_scripted() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.hooks = Hooks::default();
        // Set SHADOWRUNR_SMOKE_OUT to keep the run directory for inspection.
        let out = std::env::var_os("SHADOWRUNR_SMOKE_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| tmp.path().join("out"));
        let res = run_one_with(
            spec("smoke", ModelTier::Haiku, 30, 300),
            out.clone(),
            Arc::new(shadowrunr_engine::ScriptedEngine),
            &opts,
        )
        .await
        .expect("run completes");
        let events = transcript::read(&res.transcript).unwrap();
        assert!(
            events.iter().any(|e| matches!(e, StreamEvent::Say { .. })),
            "the brain spoke"
        );
        assert!(out.join("smoke/run.json").is_file());
        assert!(out.join("smoke/ledger.jsonl").is_file());
    }

    /// A live sink observes the run and changes nothing in it: the same spec and seed write
    /// the same transcript bytes and the same instrument with or without one, and the sink
    /// hears every transcript line once, numbered by line.
    #[tokio::test]
    async fn a_live_sink_changes_neither_the_transcript_nor_the_instrument() {
        let tmp = tempfile::tempdir().unwrap();
        let mut plain = test_opts(&tmp.path().join("sim"));
        plain.hooks = Hooks::default();
        plain.judge = false;
        assert!(plain.live_sink.is_none(), "no sink by default");
        let heard: Arc<Mutex<Vec<u64>>> = Arc::default();
        let mut live = plain.clone();
        live.live_sink = Some(crate::LiveSink({
            let heard = heard.clone();
            Arc::new(move |seq, _ev: &StreamEvent| heard.lock().unwrap().push(seq))
        }));
        assert!(format!("{live:?}").contains("live_sink: Some"));

        let mut runs = Vec::new();
        for (name, opts) in [("plain", &plain), ("live", &live)] {
            let out = tmp.path().join(name);
            let res = run_one_with(
                spec("golden", ModelTier::Haiku, 30, 300),
                out.clone(),
                Arc::new(shadowrunr_engine::ScriptedEngine),
                opts,
            )
            .await
            .expect("run completes");
            let bytes = std::fs::read(&res.transcript).unwrap();
            let inst: Instrument = read_json(&out.join("golden/instrument.json")).unwrap();
            runs.push((bytes, inst));
        }
        let (plain_bytes, plain_inst) = &runs[0];
        let (live_bytes, live_inst) = &runs[1];
        assert!(plain_bytes == live_bytes, "transcript bytes differ");
        assert_eq!(plain_inst.base_hash, live_inst.base_hash);
        assert_eq!(plain_inst.instrument_hash, live_inst.instrument_hash);
        let lines = plain_bytes.iter().filter(|b| **b == b'\n').count() as u64;
        assert!(lines > 3, "a real run");
        assert_eq!(*heard.lock().unwrap(), (1..=lines).collect::<Vec<_>>());
    }

    // ---- WP13: an engine failure is not a result ---------------------------------------

    /// Speaks like [`FakeMind`] until `at_ms`, then signs off on fleet loss like the brain.
    struct FleetLostMind {
        at_ms: TsMs,
        inner: FakeMind,
    }

    #[async_trait]
    impl Mind for FleetLostMind {
        async fn tick(
            &mut self,
            now_ms: TsMs,
            inbox: Vec<ChatMsg>,
            ctx: &SegmentContext,
        ) -> Result<Vec<StreamEvent>, SimError> {
            if now_ms >= self.at_ms {
                return Ok(vec![StreamEvent::Op {
                    ts_ms: now_ms,
                    op: Op::SignOff {
                        reason: "fleet_lost: engine degraded or down for 300s".into(),
                    },
                }]);
            }
            self.inner.tick(now_ms, inbox, ctx).await
        }
        async fn end_stream(&mut self, _now_ms: TsMs) -> Result<(), SimError> {
            Ok(())
        }
    }

    /// [`FakeEngine`] whose judge calls fail with `err`.
    /// [`FakeEngine`] whose judge calls fail with `err`.
    pub(crate) struct JudgeFails {
        pub inner: FakeEngine,
        pub err: fn() -> EngineError,
    }

    #[async_trait]
    impl Engine for JudgeFails {
        async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
            if req.role == Role::Judge {
                self.inner.calls.lock().unwrap().push((req.role, req.tier));
                return Err((self.err)());
            }
            self.inner.complete(req).await
        }
        async fn open_session(
            &self,
            t: EngineRequest,
        ) -> Result<Box<dyn WarmSession>, EngineError> {
            self.inner.open_session(t).await
        }
        async fn health(&self) -> Vec<HealthReport> {
            Vec::new()
        }
    }

    pub(crate) fn judge_calls(e: &FakeEngine) -> usize {
        e.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(r, _)| *r == Role::Judge)
            .count()
    }

    #[tokio::test]
    async fn the_real_runtime_records_each_op_once() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = test_opts(&tmp.path().join("sim"));
        let out = tmp.path().join("out");
        let fake = Arc::new(FakeEngine::new(""));
        let s = spec("echo", ModelTier::Haiku, 3, 300);
        let prep = prepare(&s, &out, fake, &opts).unwrap();
        let mind = FleetLostMind {
            at_ms: 120_000,
            inner: FakeMind { polls: 0 },
        };
        let stage = SegmentRuntime::with_config(
            s.segment,
            SegmentConfig {
                start_ms: Some(0),
                ..SegmentConfig::default()
            },
        )
        .unwrap();
        let res = execute(prep, mind, stage, &opts).await.unwrap();
        let events = transcript::read(&res.transcript).unwrap();
        let ops: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Op { ts_ms, op } => {
                    Some(format!("{ts_ms}:{}", serde_json::to_string(op).unwrap()))
                }
                _ => None,
            })
            .collect();
        let sign_offs = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    StreamEvent::Op {
                        op: Op::SignOff { .. },
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            sign_offs, 1,
            "one sign-off, not the brain's plus the runtime's echo"
        );
        assert!(
            ops.windows(2).all(|w| w[0] != w[1]),
            "no op line is recorded twice in a row: {ops:?}"
        );
    }

    #[tokio::test]
    async fn fleet_lost_sign_off_is_engine_failed_and_never_judged() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = test_opts(&tmp.path().join("sim"));
        let out = tmp.path().join("out");
        let fake = Arc::new(FakeEngine::new(""));
        let s = spec("fl", ModelTier::Haiku, 3, 300);
        let prep = prepare(&s, &out, fake.clone(), &opts).unwrap();
        let mind = FleetLostMind {
            at_ms: 120_000,
            inner: FakeMind { polls: 0 },
        };
        let stage = FakeStage {
            segment: s.segment,
            poll: None,
        };
        let res = execute(prep, mind, stage, &opts).await.unwrap();
        assert!(res.verdict.is_none());
        let record: RunRecord = read_json(&out.join("fl/run.json")).unwrap();
        assert_eq!(record.status, RunStatus::EngineFailed);
        assert!(record.engine_failure.unwrap().starts_with("sign_off"));
        // The structured signal: the stream ends as fleet_lost.
        let events = transcript::read(&res.transcript).unwrap();
        assert!(matches!(
            events.last(),
            Some(StreamEvent::StreamEnd {
                reason: EndReason::FleetLost,
                ..
            })
        ));
        assert_eq!(
            judge_calls(&fake),
            0,
            "a degenerate transcript is never judged"
        );
        assert_eq!(
            crate::outcome::classify(&out.join("fl")),
            crate::outcome::RunClass::EngineFailed
        );
    }

    #[tokio::test]
    async fn judge_engine_error_is_pending_and_a_refusal_is_void() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = test_opts(&tmp.path().join("sim"));
        let out = tmp.path().join("out");
        type Case = (&'static str, fn() -> EngineError, RunStatus);
        let cases: [Case; 4] = [
            (
                "rl",
                || EngineError::RateLimited {
                    account: "isolated".into(),
                    reset_at_ms: Some(1),
                },
                RunStatus::JudgePending,
            ),
            (
                "un",
                || EngineError::Unavailable {
                    reason: "all claude cli accounts cooling".into(),
                },
                RunStatus::JudgePending,
            ),
            (
                "to",
                || EngineError::Timeout { after_ms: 600_000 },
                RunStatus::JudgePending,
            ),
            (
                "rf",
                || EngineError::Refused {
                    reason: "no".into(),
                },
                RunStatus::JudgeVoid,
            ),
        ];
        for (id, err, want) in cases {
            let engine = Arc::new(JudgeFails {
                inner: FakeEngine::new(""),
                err,
            });
            let res = fake_run(&out, &opts, spec(id, ModelTier::Haiku, 3, 120), engine).await;
            assert!(res.verdict.is_none());
            let record: RunRecord = read_json(&out.join(id).join("run.json")).unwrap();
            assert_eq!(record.status, want, "{id}");
            assert!(record.engine_failure.is_none());
        }
    }

    /// A quota gate that serves no account.
    pub(crate) struct Blocked;
    impl crate::pacing::QuotaGate for Blocked {
        fn blocked(&self) -> Option<crate::pacing::QuotaBlock> {
            Some(crate::pacing::QuotaBlock {
                accounts: vec!["isolated".into()],
                until_ms: Some(1_792_333_200_000),
            })
        }
    }

    #[tokio::test]
    async fn judge_is_deferred_when_no_account_can_serve() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.quota = Some(GateHandle(Arc::new(Blocked)));
        let out = tmp.path().join("out");
        let fake = Arc::new(FakeEngine::new(""));
        fake_run(
            &out,
            &opts,
            spec("q", ModelTier::Haiku, 3, 120),
            fake.clone(),
        )
        .await;
        let record: RunRecord = read_json(&out.join("q/run.json")).unwrap();
        assert_eq!(record.status, RunStatus::JudgePending);
        assert_eq!(
            record.judge_error.as_deref(),
            Some("judge deferred: quota (isolated) until 2026-10-18 14:20:00 UTC")
        );
        assert_eq!(judge_calls(&fake), 0, "no judge call while blocked");
        // Once an account serves again, the re-judge produces the verdict.
        opts.quota = None;
        assert!(judge_existing(&out.join("q"), fake.clone(), &opts)
            .await
            .unwrap()
            .is_some());
    }

    /// Acceptance 6 (guard): a tournament run whose judge times out is judge_pending on disk
    /// and to `outcome::classify`; the re-judge on the same dir, through the same
    /// `judging::judge_dir` seam a sandbox session uses, produces the verdict.
    #[tokio::test]
    async fn a_timed_out_judge_is_pending_and_the_rejudge_judges_it() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = test_opts(&tmp.path().join("sim"));
        let out = tmp.path().join("out");
        let engine = Arc::new(JudgeFails {
            inner: FakeEngine::new(""),
            err: || EngineError::Timeout { after_ms: 600_000 },
        });
        let res = run_one_with(
            spec("tp", ModelTier::Haiku, 3, 60),
            out.clone(),
            engine,
            &opts,
        )
        .await
        .unwrap();
        assert!(res.verdict.is_none());
        let dir = out.join("tp");
        let record: RunRecord = read_json(&dir.join("run.json")).unwrap();
        assert_eq!(record.status, RunStatus::JudgePending);
        assert!(record
            .judge_error
            .as_deref()
            .is_some_and(|e| e.starts_with(outcome::JUDGE_ENGINE_PREFIX)));
        assert_eq!(outcome::classify(&dir), outcome::RunClass::JudgePending);
        assert!(!dir.join("verdict.json").exists());

        let v = judge_existing(&dir, Arc::new(FakeEngine::new("")), &opts)
            .await
            .unwrap();
        assert!(v.is_some());
        let record: RunRecord = read_json(&dir.join("run.json")).unwrap();
        assert_eq!(record.status, RunStatus::Judged);
        assert_eq!(record.judge_error, None);
        assert_eq!(outcome::classify(&dir), outcome::RunClass::Judged);
    }

    // ---- WP15: the Lord's brief ----------------------------------------------------------

    /// [`FakeEngine`] that records every request.
    struct Recording {
        inner: FakeEngine,
        seen: Mutex<Vec<EngineRequest>>,
    }

    #[async_trait]
    impl Engine for Recording {
        async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
            self.seen.lock().unwrap().push(req.clone());
            self.inner.complete(req).await
        }
        async fn open_session(
            &self,
            t: EngineRequest,
        ) -> Result<Box<dyn WarmSession>, EngineError> {
            self.inner.open_session(t).await
        }
        async fn health(&self) -> Vec<HealthReport> {
            Vec::new()
        }
    }

    impl Recording {
        fn of(&self, role: Role) -> Vec<EngineRequest> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.role == role)
                .cloned()
                .collect()
        }
    }

    /// `<data>` with a chosen plan, an active prevent, an active SEG-01 try and a proposal.
    fn brief_data(dir: &Path) {
        use shadowrunr_types::reconcile::{Directive, DirectiveKind, NextStreamPlan};
        std::fs::create_dir_all(dir).unwrap();
        let plan = NextStreamPlan {
            plan_id: "plan-1".into(),
            created_ms: 1,
            lord_dispatch: "The Lord wants the ledger of doom read aloud.".into(),
            segments: Vec::new(),
            topics: vec!["socks".into()],
            directives: Vec::new(),
            report_id: None,
            option_ids: Vec::new(),
        };
        let d = |id: &str, kind, text: &str, scope, active, proposed| Directive {
            id: id.into(),
            kind,
            text: text.into(),
            scope,
            created_ms: 2,
            active,
            proposed,
        };
        let dirs = vec![
            d(
                "dir-1",
                DirectiveKind::Prevent,
                "No coin-flip bits.",
                None,
                true,
                false,
            ),
            d(
                "dir-2",
                DirectiveKind::Try,
                "Roll call the top minions.",
                Some(SegmentId::Seg01ImprovisedRoom),
                true,
                false,
            ),
            d(
                "dir-9",
                DirectiveKind::Try,
                "PROPOSED-ONLY text",
                None,
                false,
                true,
            ),
        ];
        std::fs::write(
            dir.join("next_plan.json"),
            serde_json::to_string(&plan).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("directives.json"),
            serde_json::to_string(&dirs).unwrap(),
        )
        .unwrap();
    }

    /// Words of Malakor's package that must never reach another character's run.
    const MALAKOR_WORDS: &[&str] = &[
        "Malakor",
        "Arch-Lich",
        "Lesser Lich",
        "minion",
        "Minion",
        "Lord of the Darkness",
        "the Lord",
        "bone-clerk",
        "Spire",
        "Phylactery",
        "Ninth Dark",
    ];

    #[tokio::test]
    async fn a_second_character_runs_end_to_end_with_none_of_malakors_words() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.characters_root = repo().join("crates/shadowrunr-brain/tests/fixtures/characters");
        opts.judge = true;
        let out = tmp.path().join("out");
        let rec = Arc::new(Recording {
            inner: FakeEngine::new(""),
            seen: Mutex::new(Vec::new()),
        });
        for (id, segment) in [
            ("keeper-01", SegmentId::Seg01ImprovisedRoom),
            ("keeper-02a", SegmentId::Seg02aHexmap),
        ] {
            let s = SimRunSpec {
                character: "keeper".into(),
                tone_variant: "calm".into(),
                segment,
                ..spec(id, ModelTier::Haiku, 3, 120)
            };
            let res = run_one_with(s, out.clone(), rec.clone(), &opts)
                .await
                .unwrap();
            assert!(res.verdict.is_some(), "{id} was judged");
            let text = std::fs::read_to_string(&res.transcript).unwrap();
            assert!(text.contains("\"character\":\"keeper\""), "{id}");
            assert!(
                text.contains("\"speaker\":\"host\""),
                "{id}: the host spoke"
            );
            for w in MALAKOR_WORDS {
                assert!(!text.contains(w), "{w:?} in the {id} transcript");
            }
        }
        // Every prompt of both runs (core, chatters, researchers, judge) is his-word-free, and
        // the core thread's carry the keeper's own words.
        let seen = rec.seen.lock().unwrap().clone();
        // Rubric rule ids are a fixed vocabulary (stored verdicts and the console name them),
        // not text in anyone's voice.
        let ids = |t: &str| t.replace("role:minions-by-name-and-rank", "role:<id>");
        for r in &seen {
            let (system, prompt) = (ids(&r.system), ids(&r.prompt));
            for w in MALAKOR_WORDS {
                if let Some(at) = system.find(w).or_else(|| prompt.find(w)) {
                    let all = format!("{system}{prompt}");
                    let from = at.saturating_sub(120);
                    panic!(
                        "{w:?} reached a {:?} prompt: ...{}",
                        r.role,
                        all.get(from..(at + 120).min(all.len())).unwrap_or("")
                    );
                }
            }
        }
        let core: Vec<&EngineRequest> =
            seen.iter().filter(|r| r.role == Role::CoreThread).collect();
        assert!(core
            .iter()
            .all(|r| r.system.starts_with("# Constitution: Keeper Odile Marsh")));
        assert!(core
            .iter()
            .any(|r| r.prompt.contains("# Deckhands in focus")));
        assert!(core.iter().any(|r| r
            .prompt
            .contains("The deckhands are on the night watch with you")));
        assert!(core
            .iter()
            .any(|r| r.prompt.contains("The gulls search a hex")));
        let chat = seen.iter().find(|r| r.role == Role::Chatter).unwrap();
        assert!(chat
            .system
            .contains("Keeper Odile Marsh, who calls the viewers his deckhands"));
    }

    #[tokio::test]
    async fn a_brief_reaches_the_brain_the_transcript_the_instrument_and_the_judge() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.hooks = Hooks::default();
        let data = tmp.path().join("data");
        brief_data(&data);
        let (_, _, cal) = calibration::load(&opts.sim_root).unwrap();
        let plain_hash = instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal)
            .unwrap()
            .instrument_hash;
        opts.data_dir = data.clone();
        opts.brief = crate::BriefSource::Current;
        let expected = shadowrunr_brain::load_stream_brief(&data).unwrap().unwrap();
        assert_eq!(
            expected.directives.len(),
            2,
            "the proposal is not in the brief"
        );

        let out = tmp.path().join("out");
        let rec = Arc::new(Recording {
            inner: FakeEngine::new(""),
            seen: Mutex::new(Vec::new()),
        });
        let res = run_one_with(
            spec("br", ModelTier::Haiku, 3, 60),
            out.clone(),
            rec.clone(),
            &opts,
        )
        .await
        .unwrap();
        let dir = out.join("br");

        // brief.json and stream_start carry exactly the built brief.
        let stored: shadowrunr_types::reconcile::StreamBrief =
            read_json(&dir.join(BRIEF_FILE)).unwrap();
        assert_eq!(stored, expected);
        let events = transcript::read(&res.transcript).unwrap();
        match &events[0] {
            StreamEvent::StreamStart { brief, .. } => assert_eq!(brief.as_ref(), Some(&expected)),
            other => panic!("{other:?}"),
        }

        // The instrument: the brief joins the hash.
        let inst: Instrument = read_json(&dir.join("instrument.json")).unwrap();
        assert_ne!(inst.instrument_hash, plain_hash);
        assert!(inst.files.iter().any(|f| f.path == instrument::BRIEF_ENTRY
            && f.sha256 == instrument::brief_digest(&expected)));

        // The brain: every core call's system prompt holds the framed brief, byte-stable.
        let core = rec.of(Role::CoreThread);
        assert!(!core.is_empty());
        assert!(core.iter().all(|r| r.system == core[0].system));
        assert!(core[0]
            .system
            .contains(&malakor_persona().fill(shadowrunr_brain::compose::BRIEF_HEADING)));
        assert!(core[0].system.contains("- [dir-1] No coin-flip bits."));
        assert!(core[0]
            .system
            .contains("- [dir-2] (SEG-01 only) Roll call the top minions."));

        // The judge: dynamic checks plus fenced texts; nothing proposed anywhere.
        let judge = rec.of(Role::Judge);
        assert_eq!(judge.len(), 1);
        let jp = &judge[0].prompt;
        assert!(jp.contains("- directive:prevent:dir-1: "));
        assert!(jp.contains("- directive:try:dir-2: "));
        assert!(jp.contains("- follows_dispatch: "));
        assert!(jp.contains(" directive-dir-1 fence="));
        assert!(jp.contains(" lord-dispatch fence="));
        for r in rec.seen.lock().unwrap().iter() {
            assert!(!r.system.contains("PROPOSED-ONLY") && !r.prompt.contains("PROPOSED-ONLY"));
        }
        let v = res.verdict.expect("the fake judge rules");
        assert!(v.rules.iter().any(|r| r.rule == "directive:prevent:dir-1"));

        // A re-judge uses the brief the run was made with, not today's `<data>`.
        std::fs::write(data.join("directives.json"), "[]").unwrap();
        let rec2 = Arc::new(Recording {
            inner: FakeEngine::new(""),
            seen: Mutex::new(Vec::new()),
        });
        judge_existing(&dir, rec2.clone(), &opts).await.unwrap();
        assert!(rec2.of(Role::Judge)[0]
            .prompt
            .contains("- directive:prevent:dir-1: "));
    }

    #[test]
    fn no_brief_source_and_an_empty_data_dir_both_mean_no_brief() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        let (_, _, cal) = calibration::load(&opts.sim_root).unwrap();
        let none = instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal).unwrap();
        opts.brief = crate::BriefSource::Current;
        opts.data_dir = tmp.path().join("empty-data");
        let current_empty =
            instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal).unwrap();
        assert_eq!(none.instrument_hash, current_empty.instrument_hash);
        assert_eq!(none.base_hash, current_empty.base_hash);
        assert_eq!(
            crate::BriefSource::Current
                .resolved(&opts.data_dir)
                .unwrap(),
            crate::BriefSource::None
        );
    }

    // ---- real cases and the three-hour arc -----------------------------------------------

    /// Stage a small real case (test fixture: the URLs need not resolve) under `<data>`.
    pub(crate) fn stage_real_case(data: &Path) -> String {
        use shadowrunr_segments::cases::{
            CaseMeta, CaseStatus, ResearchProvenance, SubjectCategory,
        };
        use shadowrunr_segments::{CaseFile, CaseRecord, CaseStore};
        use shadowrunr_types::{Case, EvidenceGrade, Hex, HexKind};
        let kinds = [
            HexKind::Entity,
            HexKind::Claim,
            HexKind::ShellCompany,
            HexKind::Domain,
            HexKind::Entity,
            HexKind::Claim,
        ];
        let coords = [(0, 0), (1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1)];
        let case = Case {
            id: "SIM-REAL-01".into(),
            title: "Sim Test Holdings (test fixture)".into(),
            fictional: false,
            hexes: (0..6)
                .map(|i| Hex {
                    id: format!("HEX-{:02}", i + 1),
                    q: coords[i].0,
                    r: coords[i].1,
                    kind: kinds[i],
                    label: format!("Sim Test node {}", i + 1),
                    grade: None,
                    fog: true,
                    sources: vec![format!("https://www.sec.gov/test/{}", i + 1)],
                    role: None,
                    date: None,
                })
                .collect(),
            links: vec![("HEX-01".into(), "HEX-03".into())],
            thesis: None,
            acts: Vec::new(),
        };
        let record = |id: &str, hex: &str, grade: EvidenceGrade, source: &str| CaseRecord {
            id: id.into(),
            hex: hex.into(),
            grade,
            title: format!("Test record {id}"),
            source: source.into(),
            summary: format!("The regulator's filing alleges item {id}."),
            retrieved: Some("2026-10-19".into()),
            content_hash: Some("ab".repeat(32)),
            via: None,
            date: None,
        };
        let file = CaseFile {
            case_id: case.id.clone(),
            fictional: false,
            notice: "Public records (test fixture).".into(),
            records: vec![
                record(
                    "DOC-01",
                    "HEX-01",
                    EvidenceGrade::Doc,
                    "https://www.sec.gov/test/1",
                ),
                record(
                    "REP-01",
                    "HEX-02",
                    EvidenceGrade::Rep,
                    "https://opencorporates.com/x",
                ),
            ],
            arc: vec![
                vec!["HEX-01".into(), "HEX-02".into()],
                vec!["HEX-03".into(), "HEX-04".into()],
                vec!["HEX-05".into(), "HEX-06".into()],
            ],
        };
        let meta = CaseMeta {
            case_id: case.id.clone(),
            title: case.title.clone(),
            subject: "Sim Test Holdings".into(),
            subject_category: SubjectCategory::PublicEnforcementEntity,
            primary_public_record_url: "https://www.sec.gov/test/1".into(),
            status: CaseStatus::CalmStaged,
            owner_approved_for_stream: false,
            approved_at_utc: None,
            approved_content_sha256: None,
            created_at_utc: "2026-10-19T00:00:00Z".into(),
            content_sha256: String::new(),
            research: ResearchProvenance::default(),
        };
        CaseStore::new(data).stage(&case, &file, meta).unwrap();
        case.id
    }

    #[tokio::test]
    async fn a_staged_real_case_runs_privately_hashed_and_marked() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.hooks = Hooks::default();
        let data = tmp.path().join("data");
        let id = stage_real_case(&data);
        opts.data_dir = data.clone();
        let (_, _, cal) = calibration::load(&opts.sim_root).unwrap();
        let plain = instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal)
            .unwrap()
            .instrument_hash;
        opts.case = Some(id.clone());
        let with_case =
            instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal).unwrap();
        assert_ne!(with_case.instrument_hash, plain, "the case joins the hash");
        let staged = opts.staged_case().unwrap().unwrap();
        assert!(with_case
            .files
            .iter()
            .any(|f| f.path == format!("case/{id}") && f.sha256 == staged.content_sha256));

        let out = tmp.path().join("out");
        let mut sp = spec("real", ModelTier::Haiku, 3, 60);
        sp.segment = SegmentId::Seg02aHexmap;
        let res = run_one_with(
            sp.clone(),
            out.clone(),
            Arc::new(FakeEngine::new("")),
            &opts,
        )
        .await
        .unwrap();
        let rec: RunRecord = read_json(&out.join("real/run.json")).unwrap();
        let c = rec.case.expect("the run names its case");
        assert_eq!(c.case_id, id);
        assert_eq!(c.content_sha256, staged.content_sha256);
        assert!(
            c.unapproved_real_case,
            "a private sim of an unapproved case is marked"
        );
        let events = transcript::read(&res.transcript).unwrap();
        assert!(events.iter().any(|e| matches!(e,
            StreamEvent::CaseOpen { case, .. } if case.id == id && !case.fictional)));

        // The owner's approval is not content: same instrument, and the mark is gone.
        shadowrunr_segments::CaseStore::new(&data)
            .approve(&id, 0)
            .unwrap();
        assert_eq!(
            instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal)
                .unwrap()
                .instrument_hash,
            with_case.instrument_hash
        );
        let out2 = tmp.path().join("out2");
        run_one_with(sp, out2.clone(), Arc::new(FakeEngine::new("")), &opts)
            .await
            .unwrap();
        let rec: RunRecord = read_json(&out2.join("real/run.json")).unwrap();
        assert!(!rec.case.unwrap().unapproved_real_case);

        // A case is for SEG-02A runs only, and an unknown case fails before anything runs.
        let err = run_one_with(
            spec("seg01", ModelTier::Haiku, 3, 60),
            out.clone(),
            Arc::new(FakeEngine::new("")),
            &opts,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("SEG-02A"), "{err}");
        opts.case = Some("NO-SUCH-CASE".into());
        assert!(instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal).is_err());
    }

    /// Searches every fogged hex the bone-clerks have not searched yet, reveals every fogged
    /// hex it sees, every tick, and tries to sign off every 10 minutes: the worst pacing a
    /// model could choose. It logs each reveal the stage took (a hex whose fog lifted since
    /// its previous tick) at the tick that sent it.
    #[derive(Default)]
    struct GreedyMind {
        prev: Option<(TsMs, Vec<bool>)>,
        jobs: u32,
        revealed: Arc<Mutex<Vec<(TsMs, String)>>>,
    }

    #[async_trait]
    impl Mind for GreedyMind {
        async fn tick(
            &mut self,
            now_ms: TsMs,
            _inbox: Vec<ChatMsg>,
            ctx: &SegmentContext,
        ) -> Result<Vec<StreamEvent>, SimError> {
            if let Some((then, fog)) = self.prev.take() {
                let mut log = self.revealed.lock().unwrap();
                for (h, was) in ctx.hexes.iter().zip(fog) {
                    if was && !h.fog {
                        log.push((then, h.id.clone()));
                    }
                }
            }
            self.prev = Some((now_ms, ctx.hexes.iter().map(|h| h.fog).collect()));
            let searched = ctx
                .state_summary
                .lines()
                .find(|l| l.starts_with("bone-clerk searches: "))
                .unwrap_or("")
                .to_string();
            let mut ev = vec![StreamEvent::Say {
                ts_ms: now_ms,
                speaker: Speaker::Host,
                foil: None,
                text: format!("Minute {} of the audit.", now_ms / 60_000),
                replying_to: Vec::new(),
                first_text_ms: None,
                delivery: None,
            }];
            for h in ctx.hexes.iter().filter(|h| h.fog) {
                if !searched.contains(&format!("{} ", h.id)) {
                    self.jobs += 1;
                    ev.push(StreamEvent::Op {
                        ts_ms: now_ms,
                        op: Op::DispatchResearch {
                            job_id: format!("greedy-{}", self.jobs),
                            brief: format!("{}: everything on file", h.id),
                        },
                    });
                }
            }
            for h in ctx.hexes.iter().filter(|h| h.fog) {
                ev.push(StreamEvent::Op {
                    ts_ms: now_ms,
                    op: Op::RevealHex {
                        hex: h.id.clone(),
                        grade: shadowrunr_types::EvidenceGrade::Clm,
                    },
                });
            }
            if now_ms % 600_000 == 0 {
                ev.push(StreamEvent::Op {
                    ts_ms: now_ms,
                    op: Op::SignOff {
                        reason: "the audit is done".into(),
                    },
                });
            }
            Ok(ev)
        }
        async fn end_stream(&mut self, _now_ms: TsMs) -> Result<(), SimError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_three_hour_seg02a_run_holds_its_arc_against_a_greedy_mind() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.hooks = Hooks::default();
        opts.judge = false;
        let mut sp = spec("long", ModelTier::Haiku, 3, 10_800);
        sp.segment = SegmentId::Seg02aHexmap;
        let out = tmp.path().join("out");
        let prep = prepare(&sp, &out, Arc::new(FakeEngine::new("")), &opts).unwrap();
        let config = SegmentConfig {
            start_ms: Some(0),
            planned_s: Some(sp.duration_s),
            ..SegmentConfig::default()
        };
        let stage = SegmentRuntime::with_config(SegmentId::Seg02aHexmap, config.clone()).unwrap();
        // A twin of the stage, only to read which act each hex is staged for.
        let twin = SegmentRuntime::with_config(SegmentId::Seg02aHexmap, config).unwrap();
        let mind = GreedyMind::default();
        let revealed = mind.revealed.clone();
        let res = execute(prep, mind, stage, &opts).await.unwrap();
        let events = transcript::read(&res.transcript).unwrap();
        let rec: RunRecord = read_json(&out.join("long/run.json")).unwrap();
        // Early sign-offs were refused: the run reached the seal window, 2,040+ ticks.
        assert!(rec.signed_off);
        assert!(rec.ended_at_s >= 10_200, "ended at {}", rec.ended_at_s);
        assert!(rec.ticks >= 2_040, "{}", rec.ticks);
        let refused = |rule: &str| {
            events
                .iter()
                .filter(|e| matches!(e, StreamEvent::Moderation { rule: r, .. } if r == rule))
                .count()
        };
        assert!(
            refused("dossier_not_sealed") >= 16,
            "{}",
            refused("dossier_not_sealed")
        );
        assert!(refused("act_locked") > 0);
        // Each act opened on its schedule, then the seal.
        let card_at = |id: &str| {
            events.iter().find_map(|e| match e {
                StreamEvent::Op {
                    ts_ms,
                    op: Op::ShowCard { card_id, .. },
                } if card_id == id => Some(*ts_ms),
                _ => None,
            })
        };
        assert_eq!(card_at("arc-act-1"), Some(0));
        assert_eq!(card_at("arc-act-2"), Some(3_600_000));
        assert_eq!(card_at("arc-act-3"), Some(7_200_000));
        assert_eq!(card_at("arc-seal"), Some(10_200_000));

        // Reveal pacing: the greedy mind asked for every hex every tick, and got one per slot.
        assert!(
            refused("reveal_not_due") > 1_000,
            "{}",
            refused("reveal_not_due")
        );
        let revealed = revealed.lock().unwrap().clone();
        let total = twin.case().hexes.len();
        assert_eq!(revealed.len(), total, "{revealed:?}");
        for (i, (t, _)) in revealed.iter().enumerate() {
            let in_window = revealed[i..]
                .iter()
                .filter(|(u, _)| *u < t + 300_000)
                .count();
            assert!(
                in_window <= 2,
                "{in_window} reveals in the 5 minutes from {t} ms: {revealed:?}"
            );
        }
        // Every act's hexes are revealed inside the act (the last act's before the seal).
        let ends = [3_600_000, 7_200_000, 10_200_000];
        for (k, end) in ends.iter().enumerate() {
            let times: Vec<TsMs> = revealed
                .iter()
                .filter(|(_, h)| twin.hex_act(h) == Some(k))
                .map(|(t, _)| *t)
                .collect();
            let staged = twin
                .case()
                .hexes
                .iter()
                .filter(|h| twin.hex_act(&h.id) == Some(k))
                .count();
            assert_eq!(times.len(), staged, "act {k}: {times:?}");
            let opens = 3_600_000 * k as i64;
            assert!(
                times.iter().all(|t| (opens..*end).contains(t)),
                "act {k} reveals {times:?} outside {opens}..{end}"
            );
            eprintln!(
                "act {} reveals at s: {:?}",
                k + 1,
                times.iter().map(|t| t / 1000).collect::<Vec<_>>()
            );
        }
    }

    /// The real brain over a 3-hour SEG-02A run (keyless fakes): the core prompt stays
    /// bounded, the stable prefix stays byte-stable, and the run is not cut short.
    #[tokio::test]
    async fn a_three_hour_run_keeps_the_core_prompt_bounded() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.hooks = Hooks::default();
        opts.judge = false;
        let rec = Arc::new(Recording {
            inner: FakeEngine::new(""),
            seen: Mutex::new(Vec::new()),
        });
        let mut sp = spec("three-hours", ModelTier::Haiku, 30, 10_800);
        sp.segment = SegmentId::Seg02aHexmap;
        let out = tmp.path().join("out");
        run_one_with(sp, out.clone(), rec.clone(), &opts)
            .await
            .unwrap();
        let rec_run: RunRecord = read_json(&out.join("three-hours/run.json")).unwrap();
        assert_eq!(rec_run.ended_at_s, 10_800, "the run is not cut short");
        assert!(rec_run.tick_errors.is_empty(), "{:?}", rec_run.tick_errors);
        let core = rec.of(Role::CoreThread);
        assert!(core.len() > 1_000, "{} core calls", core.len());
        assert!(
            core.iter().all(|r| r.system == core[0].system),
            "stable prefix"
        );
        // Prompt size in the first and the last half hour: no unbounded growth.
        let len = |r: &EngineRequest| r.prompt.len();
        let n = core.len();
        let early = core[n / 12..n / 6].iter().map(len).max().unwrap();
        let late = core[n - n / 6..].iter().map(len).max().unwrap();
        assert!(
            late < early * 2,
            "core prompt grew from {early} to {late} chars over three hours"
        );
        // The arc reached the brain: the last act's summary is in a late prompt.
        assert!(
            core[n - 1].prompt.contains("ARC act 3/3"),
            "{}",
            &core[n - 1].prompt
        );
    }

    fn sample_prep() -> shadowrunr_types::prep::StreamPrep {
        let text =
            std::fs::read_to_string(repo().join("characters/malakor/prep.sample.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[tokio::test]
    async fn the_brain_gets_the_runs_prep_for_its_character_only() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        let s = spec("p1", ModelTier::Haiku, 3, 60);
        let engine: Arc<dyn Engine> = Arc::new(shadowrunr_engine::ScriptedEngine);
        let prep = prepare(&s, &tmp.path().join("out"), engine, &opts).unwrap();
        let stage = SegmentRuntime::with_config(s.segment, SegmentConfig::default()).unwrap();
        let mem = tmp.path().join("mem");
        std::fs::create_dir_all(&mem).unwrap();

        let brain = build_brain(
            &prep,
            &opts,
            &mem,
            prep.engine.clone(),
            &stage,
            brain_tuning(&opts, &prep),
        )
        .await
        .unwrap();
        assert!(brain.prep().is_none(), "no prep without one in the options");

        opts.stream_prep = Some(sample_prep());
        let brain = build_brain(
            &prep,
            &opts,
            &mem,
            prep.engine.clone(),
            &stage,
            brain_tuning(&opts, &prep),
        )
        .await
        .unwrap();
        assert_eq!(brain.prep().map(|p| p.prep_id.as_str()), Some("sample"));

        let mut other = sample_prep();
        other.character = "someone-else".into();
        opts.stream_prep = Some(other);
        let brain = build_brain(
            &prep,
            &opts,
            &mem,
            prep.engine.clone(),
            &stage,
            brain_tuning(&opts, &prep),
        )
        .await
        .unwrap();
        assert!(
            brain.prep().is_none(),
            "a prep for another character is not used"
        );
    }

    #[tokio::test]
    async fn run_json_records_the_prep_the_run_used() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.judge = false;
        let out = tmp.path().join("out");
        let engine: Arc<dyn Engine> = Arc::new(FakeEngine::new(""));
        fake_run(
            &out,
            &opts,
            spec("r-none", ModelTier::Haiku, 3, 30),
            engine.clone(),
        )
        .await;
        let record: RunRecord = read_json(&out.join("r-none/run.json")).unwrap();
        assert_eq!(record.prep.as_deref(), Some("none"));

        opts.stream_prep = Some(sample_prep());
        fake_run(
            &out,
            &opts,
            spec("r-sample", ModelTier::Haiku, 3, 30),
            engine,
        )
        .await;
        let record: RunRecord = read_json(&out.join("r-sample/run.json")).unwrap();
        assert_eq!(record.prep.as_deref(), Some("sample"));
    }

    // ---- what a run reads joins the instrument -------------------------------------------

    #[test]
    fn the_prep_the_brain_voices_joins_the_base_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        let (_, _, cal) = calibration::load(&opts.sim_root).unwrap();
        let none = instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal).unwrap();
        assert!(
            !none.files.iter().any(|f| f.path == "prep.json"),
            "no prep: no entry"
        );

        opts.stream_prep = Some(sample_prep());
        let a = instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal).unwrap();
        assert!(a.files.iter().any(|f| f.path == "prep.json"));
        assert_ne!(a.base_hash, none.base_hash);

        let mut edited = sample_prep();
        edited.bits[0].lines[0].push_str(" And one more thing.");
        opts.stream_prep = Some(edited);
        let b = instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal).unwrap();
        assert_ne!(
            b.base_hash, a.base_hash,
            "a changed bit is a new instrument"
        );
    }

    #[test]
    fn every_harness_knob_joins_the_base_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = test_opts(&tmp.path().join("sim"));
        let (_, _, cal) = calibration::load(&opts.sim_root).unwrap();
        let base = |o: &SimOptions| {
            instrument_for(o, "malakor", "theatrical", ModelTier::Haiku, &cal)
                .unwrap()
                .base_hash
        };
        let h0 = base(&opts);
        let i0 = instrument_for(&opts, "malakor", "theatrical", ModelTier::Haiku, &cal).unwrap();
        for knob in [
            "tick_s",
            "featured_every_ticks",
            "max_featured",
            "platforms",
        ] {
            assert!(
                i0.files.iter().any(|f| f.path == format!("harness/{knob}")),
                "harness/{knob}"
            );
        }
        let variants: Vec<(&str, SimOptions)> = vec![
            (
                "featured_every_ticks",
                SimOptions {
                    featured_every_ticks: opts.featured_every_ticks + 1,
                    ..opts.clone()
                },
            ),
            (
                "max_featured",
                SimOptions {
                    max_featured: 4,
                    ..opts.clone()
                },
            ),
            (
                "tick_s",
                SimOptions {
                    tick_s: 10,
                    ..opts.clone()
                },
            ),
            (
                "platforms",
                SimOptions {
                    platforms: vec!["youtube".into()],
                    ..opts.clone()
                },
            ),
        ];
        for (knob, o) in variants {
            assert_ne!(base(&o), h0, "{knob}");
        }
    }

    #[test]
    fn a_moderator_package_cannot_start_a_show() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = test_opts(&tmp.path().join("sim"));
        let s = SimRunSpec {
            character: "moderator".into(),
            tone_variant: String::new(),
            ..spec("mod-host", ModelTier::Haiku, 3, 60)
        };
        let out = tmp.path().join("out");
        let engine: Arc<dyn Engine> = Arc::new(shadowrunr_engine::ScriptedEngine);
        let err = prepare(&s, &out, engine, &opts)
            .err()
            .expect("refused")
            .to_string();
        assert!(err.contains("cannot host a show"), "{err}");
        assert!(
            !out.join("mod-host").exists(),
            "refused before any run file"
        );
    }

    #[test]
    fn the_resolved_judge_tier_joins_the_instrument_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = test_opts(&tmp.path().join("sim"));
        assert_eq!(opts.judge_tiers, vec![ModelTier::Opus, ModelTier::Sonnet]);
        let inst = |id: &str, core: ModelTier| {
            let engine: Arc<dyn Engine> = Arc::new(shadowrunr_engine::ScriptedEngine);
            prepare(
                &spec(id, core, 3, 60),
                &tmp.path().join("out"),
                engine,
                &opts,
            )
            .unwrap()
            .instrument
        };
        let haiku = inst("j-haiku", ModelTier::Haiku);
        let sonnet = inst("j-sonnet", ModelTier::Sonnet);
        let opus = inst("j-opus", ModelTier::Opus);
        assert!(haiku.files.iter().any(|f| f.path == "judge/opus"));
        assert!(opus.files.iter().any(|f| f.path == "judge/sonnet"));
        assert_eq!(haiku.base_hash, opus.base_hash, "the judge is per run");
        assert_ne!(
            haiku.instrument_hash, opus.instrument_hash,
            "an Opus judge and a Sonnet judge never share a cell"
        );
        assert_eq!(
            haiku.instrument_hash, sonnet.instrument_hash,
            "same tone, same judge: same instrument"
        );
    }

    // ---- the streamer screen (appendix (screens and board)) ----------------------------

    /// Plays a fixed script: at each listed time it returns those events, as the brain would
    /// (its ops, and the `job` events its pool reports).
    struct ScriptMind {
        script: Vec<(TsMs, Vec<StreamEvent>)>,
    }

    #[async_trait]
    impl Mind for ScriptMind {
        async fn tick(
            &mut self,
            now_ms: TsMs,
            _inbox: Vec<ChatMsg>,
            _ctx: &SegmentContext,
        ) -> Result<Vec<StreamEvent>, SimError> {
            let mut ev: Vec<StreamEvent> = self
                .script
                .iter()
                .filter(|(at, _)| *at == now_ms)
                .flat_map(|(_, evs)| evs.clone())
                .collect();
            for e in &mut ev {
                transcript::set_ts(e, now_ms);
            }
            Ok(ev)
        }
        async fn end_stream(&mut self, _now_ms: TsMs) -> Result<(), SimError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_returning_job_opens_the_browser_and_the_models_screen_holds() {
        use shadowrunr_types::ScreenId;
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.hooks = Hooks::default();
        opts.judge = false;
        opts.tick_s = 5;
        let mut sp = spec("screens", ModelTier::Haiku, 3, 50);
        sp.segment = SegmentId::Seg02aHexmap;
        let out = tmp.path().join("out");
        let prep = prepare(
            &sp,
            &out,
            Arc::new(shadowrunr_engine::ScriptedEngine),
            &opts,
        )
        .unwrap();
        let stage = SegmentRuntime::with_config(
            SegmentId::Seg02aHexmap,
            SegmentConfig {
                start_ms: Some(0),
                ..SegmentConfig::default()
            },
        )
        .unwrap();
        let op = |op: Op| StreamEvent::Op { ts_ms: 0, op };
        let research = |id: &str, brief: &str| {
            op(Op::DispatchResearch {
                job_id: id.into(),
                brief: brief.into(),
            })
        };
        let done = |id: &str| StreamEvent::Job {
            ts_ms: 0,
            job_id: id.into(),
            status: "done".into(),
            summary: Some("the bone-clerks are back".into()),
        };
        let mind = ScriptMind {
            script: vec![
                (
                    5_000,
                    vec![
                        research("r1", "HEX-04: trace the treasury wallet"),
                        research("r2", "HEX-01: the syndicate's licence"),
                        research("r3", "HEX-05: the weekly promise"),
                    ],
                ),
                // He names a record whose job is still out: refused, it is not back.
                (
                    10_000,
                    vec![op(Op::ShowScreen {
                        screen: ScreenId::Browser,
                        record_id: Some("DOC-01".into()),
                        focus: None,
                    })],
                ),
                // r1 comes back: the runtime opens its first filed record.
                (15_000, vec![done("r1")]),
                // He goes back to the board to choose the next card.
                (
                    20_000,
                    vec![op(Op::ShowScreen {
                        screen: ScreenId::Board,
                        record_id: None,
                        focus: None,
                    })],
                ),
                // r2 comes back 5 s later, inside his hold: no default.
                (25_000, vec![done("r2")]),
                // r3 comes back 10 s later, past the hold: the browser opens on it.
                (30_000, vec![done("r3")]),
                // He puts a record nobody has shown yet on the desk, its board on another
                // card: the record goes out before his op, though the brain wrote the op first.
                (
                    35_000,
                    vec![op(Op::ShowScreen {
                        screen: ScreenId::Desk,
                        record_id: Some("DOC-01".into()),
                        focus: Some("HEX-02".into()),
                    })],
                ),
                // He flips the treasury and the mixer, then pins the string between them on a
                // record that came back with r1 but was never shown: it goes out before his
                // op too, and the string after it.
                (
                    45_000,
                    vec![
                        op(Op::RevealHex {
                            hex: "HEX-04".into(),
                            grade: shadowrunr_types::EvidenceGrade::Doc,
                        }),
                        op(Op::RevealHex {
                            hex: "HEX-09".into(),
                            grade: shadowrunr_types::EvidenceGrade::Clm,
                        }),
                        op(Op::PinLink {
                            from: "HEX-04".into(),
                            to: "HEX-09".into(),
                            rel: shadowrunr_types::LinkRel::Paid,
                            record_id: "CLM-02".into(),
                        }),
                    ],
                ),
            ],
        };
        let res = execute(prep, mind, stage, &opts).await.unwrap();
        let events = transcript::read(&res.transcript).unwrap();
        let screen: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Op {
                    ts_ms,
                    op:
                        Op::ShowScreen {
                            screen,
                            record_id,
                            focus,
                        },
                } => Some(format!(
                    "{ts_ms} show_screen {}{}{}",
                    serde_json::to_value(screen).unwrap().as_str().unwrap(),
                    record_id
                        .as_deref()
                        .map(|r| format!(" {r}"))
                        .unwrap_or_default(),
                    focus
                        .as_deref()
                        .map(|h| format!(" @{h}"))
                        .unwrap_or_default()
                )),
                StreamEvent::RecordShown { ts_ms, record } => {
                    Some(format!("{ts_ms} record_shown {}", record.id))
                }
                StreamEvent::Job { ts_ms, job_id, .. } => Some(format!("{ts_ms} job {job_id}")),
                StreamEvent::Op {
                    ts_ms,
                    op: Op::PinLink { record_id, .. },
                } => Some(format!("{ts_ms} pin_link {record_id}")),
                StreamEvent::LinkPinned {
                    ts_ms, record_id, ..
                } => Some(format!("{ts_ms} link_pinned {record_id}")),
                _ => None,
            })
            .collect();
        assert_eq!(
            screen,
            [
                "0 show_screen board",
                // Each dispatch zooms the board to where the minions are going.
                "5000 show_screen board @HEX-04",
                "5000 show_screen board @HEX-01",
                "5000 show_screen board @HEX-05",
                "10000 show_screen browser DOC-01",
                "15000 job r1",
                "15000 record_shown DOC-03",
                "15000 show_screen browser DOC-03",
                "20000 show_screen board",
                "25000 job r2",
                "30000 job r3",
                "30000 record_shown REP-02",
                "30000 show_screen browser REP-02",
                "35000 record_shown DOC-01",
                "35000 show_screen desk DOC-01 @HEX-02",
                // The brain writes the beat's ops first: the record moves before the pin
                // that cites it, then each flip zooms the board to its card (past his hold),
                // then the string goes up.
                "45000 record_shown CLM-02",
                "45000 pin_link CLM-02",
                "45000 show_screen board @HEX-04",
                "45000 show_screen board @HEX-09",
                "45000 link_pinned CLM-02",
            ]
        );
        let refused: Vec<(TsMs, &str)> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Moderation {
                    ts_ms,
                    rule,
                    action,
                    ..
                } if action == "rejected:show_screen" => Some((*ts_ms, rule.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(refused, [(10_000, "record_not_returned")]);
    }

    #[test]
    fn each_sent_op_drops_one_echo_so_a_runtime_default_survives() {
        let hexmap = Op::ShowScreen {
            screen: shadowrunr_types::ScreenId::Hexmap,
            record_id: None,
            focus: None,
        };
        let ev = |op: &Op| StreamEvent::Op {
            ts_ms: 1,
            op: op.clone(),
        };
        // The echo of the model's op, then the runtime's own identical default.
        let kept = drop_op_echo(
            std::slice::from_ref(&hexmap),
            vec![ev(&hexmap), ev(&hexmap)],
        );
        assert_eq!(kept.len(), 1);
        assert!(drop_op_echo(std::slice::from_ref(&hexmap), vec![ev(&hexmap)]).is_empty());
    }
}
