//! The orchestration loop for one run. Each tick of simulated time:
//! 1. generate chat (bulk + one batched featured-chatter call);
//! 2. `segments.ingest_chat`;
//! 3. `brain.tick(now, inbox, &segments.context(ccv, elapsed))`;
//! 4. `segments.apply_ops(ops from the brain's events)`;
//! 5. `segments.tick`;
//! 6. append everything to `transcript.jsonl`.
//!
//! The brain and the segment runtime sit behind the [`Mind`] and [`Stage`] traits so the
//! loop can be tested before `shadowrunr_brain` and `shadowrunr_segments` land.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use shadowrunr_brain::{load_character, Brain, BrainConfig};
use shadowrunr_segments::{SegmentConfig, SegmentRuntime};
use shadowrunr_types::{
    ChatMsg, EndReason, Engine, ModelTier, Op, Role, SegmentContext, SimRunSpec, Speaker,
    StreamEvent, TsMs,
};

use crate::calibration::{self, Calibration, CalibrationOrigin};
use crate::chat::{ChatRoom, ChatStats, RoomConfig};
use crate::instrument::{self, Instrument, InstrumentInputs};
use crate::judge::{self, JudgeInputs, JudgeOutcome, Rubric};
use crate::ledger::LedgerEngine;
use crate::population;
use crate::situations::{Injection, SituationBank};
use crate::transcript::{self, TranscriptWriter};
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
    fn context(&self, ccv: u32, elapsed_s: u32) -> SegmentContext;
    fn ingest_chat(&mut self, now_ms: TsMs, msgs: &[ChatMsg]) -> Vec<StreamEvent>;
    fn apply_ops(&mut self, now_ms: TsMs, ops: &[Op]) -> Vec<StreamEvent>;
    fn tick(&mut self, now_ms: TsMs, ccv: u32) -> Vec<StreamEvent>;
    /// The authoritative open poll `(id, options)` when the stage can tell (`Some(None)` =
    /// no poll open); `None` when it cannot, and the room tracks polls from ops instead.
    fn open_poll(&self) -> Option<Option<(String, Vec<String>)>> {
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

impl Stage for SegmentRuntime {
    fn context(&self, ccv: u32, elapsed_s: u32) -> SegmentContext {
        SegmentRuntime::context(self, ccv, elapsed_s)
    }
    fn ingest_chat(&mut self, now_ms: TsMs, msgs: &[ChatMsg]) -> Vec<StreamEvent> {
        SegmentRuntime::ingest_chat(self, now_ms, msgs)
    }
    fn apply_ops(&mut self, now_ms: TsMs, ops: &[Op]) -> Vec<StreamEvent> {
        SegmentRuntime::apply_ops(self, now_ms, ops)
    }
    fn tick(&mut self, now_ms: TsMs, ccv: u32) -> Vec<StreamEvent> {
        SegmentRuntime::tick(self, now_ms, ccv)
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
    /// Simulated, but the judge call failed or its reply did not parse: void, re-judge later.
    JudgeVoid,
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
    pub engine: Arc<dyn Engine>,
    pub judge_tier: Option<ModelTier>,
    pub foils: Vec<String>,
    pub hex_ids: Vec<String>,
    pub streamer: String,
    pub character_dir: PathBuf,
}

pub(crate) fn character_dir(opts: &SimOptions, character: &str) -> PathBuf {
    opts.characters_root.join(character)
}

pub(crate) fn rubric_path(opts: &SimOptions) -> PathBuf {
    opts.sim_root.join("rubric.toml")
}

/// The instrument identity for a spec, without running anything.
pub(crate) fn instrument_for(
    opts: &SimOptions,
    character: &str,
    tone: &str,
    calibration_text: &str,
) -> Result<Instrument, SimError> {
    instrument::compute(&InstrumentInputs {
        character_dir: &character_dir(opts, character),
        tone,
        segments_dir: &opts.segments_data_dir,
        rubric_path: &rubric_path(opts),
        calibration_text,
    })
}

fn character_meta(dir: &Path) -> (String, Vec<String>) {
    let text = std::fs::read_to_string(dir.join("character.toml")).unwrap_or_default();
    let v: toml::Value = toml::from_str(&text).unwrap_or(toml::Value::Table(Default::default()));
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .and_then(|n| n.split_whitespace().last())
        .unwrap_or("Malakor")
        .to_string();
    let foils = v
        .get("foils")
        .and_then(|f| f.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    (name, foils)
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
    let run_dir = out_dir.join(&spec.run_id);
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
    let instrument = instrument_for(opts, &spec.character, &spec.tone_variant, &cal_text)?;
    write_json(&run_dir.join("spec.json"), spec)?;
    write_json(&run_dir.join("instrument.json"), &instrument)?;
    let engine: Arc<dyn Engine> = Arc::new(LedgerEngine::create(
        engine,
        &run_dir.join("ledger.jsonl"),
        &spec.run_id,
    )?);
    let (streamer, foils) = character_meta(&cdir);
    Ok(Prepared {
        spec: spec.clone(),
        run_dir,
        cal,
        cal_origin,
        bank,
        rubric,
        instrument,
        engine,
        judge_tier,
        foils,
        hex_ids: hex_ids(&opts.segments_data_dir),
        streamer,
        character_dir: cdir,
    })
}

fn max_say_gap_s(events: &[StreamEvent], end_ms: TsMs) -> Option<u32> {
    let says: Vec<TsMs> = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                StreamEvent::Say {
                    speaker: Speaker::Malakor,
                    ..
                }
            )
        })
        .map(transcript::ts)
        .collect();
    if says.is_empty() {
        return Some((end_ms / 1000) as u32);
    }
    let mut prev = 0;
    let mut gap = 0;
    for t in says.iter().copied().chain(std::iter::once(end_ms)) {
        gap = gap.max(t - prev);
        prev = t;
    }
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
            streamer_name: prep.streamer.clone(),
        },
    );
    let transcript_path = prep.run_dir.join("transcript.jsonl");
    let mut w = TranscriptWriter::create(&transcript_path)?;
    w.push(StreamEvent::StreamStart {
        ts_ms: 0,
        stream_id: spec.run_id.clone(),
        character: spec.character.clone(),
        tone_variant: spec.tone_variant.clone(),
        core_tier: spec.core_tier,
        audience_size: spec.audience_size,
    })?;
    w.push(StreamEvent::SegmentStart {
        ts_ms: 0,
        segment: spec.segment,
    })?;

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
                signed_off |= ops.iter().any(|o| matches!(o, Op::SignOff { .. }));
                room.observe(t, &evs);
                w.push_all(evs)?;
                if !ops.is_empty() {
                    let ev = stage.apply_ops(now, &ops);
                    room.observe(t, &ev);
                    w.push_all(ev)?;
                    room.sync_poll(t, stage.open_poll());
                }
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
    w.push(StreamEvent::SegmentEnd {
        ts_ms: end_ms,
        segment: room.segment(),
        ses: None,
    })?;
    w.push(StreamEvent::StreamEnd {
        ts_ms: end_ms,
        reason: EndReason::SimComplete,
    })?;
    w.flush()?;
    if let Err(e) = mind.end_stream(end_ms).await {
        tick_errors.push(format!("end_stream: {e}"));
    }

    let events = w.events().to_vec();
    let mut record = RunRecord {
        run_id: spec.run_id.clone(),
        status: RunStatus::Simulated,
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
        max_say_gap_s: max_say_gap_s(&events, end_ms),
        judge_tier: prep.judge_tier,
        judge_error: None,
    };
    write_json(&prep.run_dir.join("run.json"), &record)?;

    let verdict = match (prep.judge_tier, &prep.rubric) {
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

/// Judge a simulated run; writes `verdict.json` + `judge.json`, or records a void judge.
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
) -> Result<Option<shadowrunr_types::JudgeVerdict>, SimError> {
    let featured: HashSet<String> = record.featured.iter().cloned().collect();
    let inp = JudgeInputs {
        run_id: &spec.run_id,
        events,
        bank,
        injections: &record.injections,
        featured_viewers: &featured,
        rubric,
        judge_tier: tier,
        core_tier: spec.core_tier,
    };
    record.judge_tier = Some(tier);
    match judge::judge_run(&inp, engine).await {
        Ok(JudgeOutcome::Verdict(b)) => {
            let (verdict, audit) = *b;
            write_json(&run_dir.join("judge.json"), &audit)?;
            write_json(&run_dir.join("verdict.json"), &verdict)?;
            record.status = RunStatus::Judged;
            record.judge_error = None;
            Ok(Some(verdict))
        }
        Ok(JudgeOutcome::Void(audit, why)) => {
            write_json(&run_dir.join("judge.json"), &audit)?;
            record.status = RunStatus::JudgeVoid;
            record.judge_error = Some(why);
            Ok(None)
        }
        Err(SimError::Engine(e)) => {
            record.status = RunStatus::JudgeVoid;
            record.judge_error = Some(format!("judge engine call failed: {e}"));
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

fn temp_memory_dir(run_id: &str) -> PathBuf {
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
        let character = load_character(&prep.character_dir, &spec.tone_variant)?;
        let brain = Brain::new(
            BrainConfig {
                character,
                memory_dir: memory_dir.clone(),
                stream_id: spec.run_id.clone(),
                core_tier: spec.core_tier,
                pool_size: opts.pool_size,
            },
            prep.engine.clone(),
        )
        .await?;
        let stage = SegmentRuntime::with_config(
            spec.segment,
            SegmentConfig {
                foils: prep.foils.clone(),
                start_ms: Some(0),
                ..SegmentConfig::default()
            },
        )
        .map_err(|e| SimError::Spec(format!("segment runtime: {e}")))?;
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
    let events = transcript::read(&run_dir.join("transcript.jsonl"))?;
    let bank = SituationBank::load(&character_dir(opts, &spec.character).join("situations.json"))?;
    let rubric = Rubric::load(&rubric_path(opts))?;
    let engine: Arc<dyn Engine> = Arc::new(LedgerEngine::create(
        engine,
        &run_dir.join("ledger.jsonl"),
        &spec.run_id,
    )?);
    let v = judge_into(
        run_dir,
        &spec,
        &events,
        &bank,
        &rubric,
        tier,
        &mut record,
        engine.as_ref(),
    )
    .await?;
    write_json(&run_dir.join("run.json"), &record)?;
    Ok(v)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::population::tests::fake_rank;
    use crate::Hooks;
    use shadowrunr_types::{
        CheckState, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport,
        JudgeVerdict, Rank, SegmentId, Usage, VoteTally, WarmSession,
    };
    use std::sync::Mutex;

    pub(crate) fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

    fn reply(text: String) -> EngineReply {
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

    fn judge_reply(req: &EngineRequest) -> String {
        let mut say: Option<(usize, String)> = None;
        let mut lines_text: Vec<(usize, String)> = Vec::new();
        for l in req.prompt.lines() {
            if let Some(rest) = l.strip_prefix('L') {
                let n: usize = rest
                    .split_whitespace()
                    .next()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0);
                if let Some(t) = l.split("say malakor: ").nth(1) {
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
                speaker: Speaker::Malakor,
                foil: None,
                text,
                replying_to: inbox
                    .first()
                    .map(|m| vec![m.id.clone()])
                    .unwrap_or_default(),
                first_text_ms: Some(7),
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
                allowed_ops: vec!["start_poll".into()],
                ccv,
                elapsed_s,
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
        assert_eq!(record.injections.len(), bank.situations.len());
        for inj in &record.injections {
            if inj.situation_id == "prompt-injection" {
                assert!(
                    inj.performed_msg_id.is_none()
                        && inj.attempts.len() == crate::situations::MAX_ATTEMPTS
                );
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

        let v: JudgeVerdict = read_json(&dir.join("verdict.json")).unwrap();
        assert_eq!(v.judge_tier, ModelTier::Sonnet);
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

        // The ledger saw chatter calls on haiku and the judge on sonnet.
        let rows = crate::ledger::read(&dir.join("ledger.jsonl")).unwrap();
        assert!(rows
            .iter()
            .any(|r| r.role == Role::Chatter && r.tier == ModelTier::Haiku));
        assert!(rows
            .iter()
            .any(|r| r.role == Role::Judge && r.tier == ModelTier::Sonnet));
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
}
