//! Tournaments: run a matrix with a concurrency limit, resumably, paced on quota.
//!
//! Layout: `<out_root>/<matrix_id>-<base_hash[..10]>/<run_id>/`. The base hash covers
//! everything but the tone, so editing the rubric, the segment tables, the calibration or the
//! character package starts a new tournament directory instead of mixing instruments. Inside
//! a tournament, a run whose tone file changed since it ran is moved aside
//! (`<run_id>.stale-<hash8>`) and run again.
//!
//! WP13: an engine failure is not a result ([`crate::outcome`]). Resume re-runs
//! `engine_failed` runs (the old directory moved aside as `<run_id>.engine-failed`) and
//! re-judges `judge_pending` runs only; a content `judge_void` is kept. Before every run the
//! tournament asks the engine whether any account can serve ([`crate::pacing`]); when none
//! can, or a run comes back `engine_failed` / `judge_pending`, it stops admitting runs, lets
//! the in-flight ones finish, pauses until the earliest reset plus a margin, probes with the
//! cheapest role, and retries (at most [`MAX_RETRIES`] times per run).

use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use shadowrunr_types::{Engine, SimRunSpec};

use crate::calibration;
use crate::instrument::Instrument;
use crate::matrix::Matrix;
use crate::outcome::{self, RunClass};
use crate::pacing::{
    self, ClockHandle, GateHandle, PaceClock, QuotaBlock, QuotaGate, SystemPaceClock,
    DEFAULT_MAX_PAUSE_MS, MAX_RETRIES, RESUME_MARGIN_MS,
};
use crate::runner::{instrument_for, judge_existing, run_one_with};
use crate::summary::{summarize_full, Results};
use crate::{read_json, write_json, SimError, SimOptions};

#[derive(Debug, Clone, Default)]
pub struct TournamentOptions {
    /// Per-run options. Matrix settings (tick, judge tiers, featured cadence and cap)
    /// override these.
    pub sim: SimOptions,
    /// Overrides the matrix's concurrency.
    pub concurrency: Option<usize>,
    /// Stop after starting this many runs (for staged overnight batches).
    pub max_runs: Option<usize>,
    /// Asked before every run and every judge call. `None` (the scripted engine, or an engine
    /// that is not the fleet) never pauses on the pre-check; an `engine_failed` run still
    /// pauses on the default cooldown and a probe.
    pub quota: Option<GateHandle>,
    /// The clock pauses wait on (default: the system clock).
    pub clock: Option<ClockHandle>,
    /// Total pause cap in ms (default [`DEFAULT_MAX_PAUSE_MS`], 8 h). Past it, the
    /// tournament stops with its summary.
    pub max_pause_ms: Option<u64>,
}

/// Matrix settings applied over the caller's options.
pub fn effective_options(matrix: &Matrix, base: &SimOptions) -> SimOptions {
    let mut o = base.clone();
    o.tick_s = matrix.tick_s;
    if o.brief == crate::BriefSource::None {
        o.brief = matrix.brief_source();
    }
    if !matrix.judge_tiers.is_empty() {
        o.judge_tiers = matrix.judge_tiers.clone();
    }
    if let Some(f) = matrix.featured_every_ticks {
        o.featured_every_ticks = f;
    }
    if let Some(m) = matrix.max_featured {
        o.max_featured = m;
    }
    if matrix.rate_cap_exemptions {
        o.rate_cap_exemptions = true;
    }
    o
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunAction {
    /// Already complete for this instrument (judged, or void for a content reason).
    Skip,
    /// Simulated, but the judge call failed on the engine: judge again without re-simulating.
    Rejudge,
    Run,
}

/// Why the planner chose a run's action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanReason {
    /// Never ran.
    New,
    /// Judged (or simulated, with judging off).
    Complete,
    /// The judge replied without a usable verdict: void for a content reason, kept.
    VoidKept,
    /// Made by a different instrument (moved aside as `.stale-<hash8>`).
    Stale,
    /// Interrupted or failed attempt (moved aside as `.attempt-<ms>`).
    Partial,
    /// The engine cut the stream (moved aside as `.engine-failed`).
    EngineFailed,
    /// Sound transcript, judge failed on the engine: re-judge only.
    JudgePending,
}

#[derive(Debug, Clone)]
pub struct PlannedRun {
    pub spec: SimRunSpec,
    pub run_dir: PathBuf,
    pub instrument_hash: String,
    pub action: RunAction,
    pub reason: PlanReason,
    /// Hash of an existing run directory made by a different instrument (moved aside first).
    pub stale_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TournamentPlan {
    pub tournament_id: String,
    pub dir: PathBuf,
    pub base_hash: String,
    pub runs: Vec<PlannedRun>,
}

impl TournamentPlan {
    pub fn count(&self, a: RunAction) -> usize {
        self.runs.iter().filter(|r| r.action == a).count()
    }
    pub fn count_reason(&self, why: PlanReason) -> usize {
        self.runs.iter().filter(|r| r.reason == why).count()
    }
}

/// Action and reason for a run directory made by the current instrument.
fn decide(run_dir: &Path, judge: bool) -> (RunAction, PlanReason) {
    let class = outcome::classify(run_dir);
    let has_transcript = run_dir.join("transcript.jsonl").is_file();
    match class {
        RunClass::Judged => (RunAction::Skip, PlanReason::Complete),
        RunClass::EngineFailed => (RunAction::Run, PlanReason::EngineFailed),
        _ if !judge => match class {
            RunClass::Simulated | RunClass::JudgeVoid | RunClass::JudgePending => {
                (RunAction::Skip, PlanReason::Complete)
            }
            _ => (RunAction::Run, PlanReason::Partial),
        },
        RunClass::JudgePending if has_transcript => (RunAction::Rejudge, PlanReason::JudgePending),
        RunClass::JudgeVoid => (RunAction::Skip, PlanReason::VoidKept),
        _ => (RunAction::Run, PlanReason::Partial),
    }
}

/// Decide what to do with every run of the matrix. Reads only; writes nothing.
pub fn plan(
    matrix: &Matrix,
    out_root: &Path,
    opts: &SimOptions,
) -> Result<TournamentPlan, SimError> {
    let (_, _, cal_text) = calibration::load(&opts.sim_root)?;
    let first_tone = &matrix.axes.tones[0];
    let base = instrument_for(opts, &matrix.character, first_tone, &cal_text)?.base_hash;
    let tournament_id = format!("{}-{}", matrix.id, &base[..10]);
    let dir = out_root.join(&tournament_id);
    let mut runs = Vec::new();
    for spec in matrix.specs() {
        let hash =
            instrument_for(opts, &spec.character, &spec.tone_variant, &cal_text)?.instrument_hash;
        let run_dir = dir.join(&spec.run_id);
        let existing: Option<String> = read_json::<Instrument>(&run_dir.join("instrument.json"))
            .ok()
            .map(|i| i.instrument_hash);
        let (action, reason, stale_hash) = match existing {
            Some(h) if h != hash => (RunAction::Run, PlanReason::Stale, Some(h)),
            Some(_) => {
                let (a, r) = decide(&run_dir, opts.judge);
                (a, r, None)
            }
            None if run_dir.exists() => (RunAction::Run, PlanReason::Partial, None),
            None => (RunAction::Run, PlanReason::New, None),
        };
        runs.push(PlannedRun {
            spec,
            run_dir,
            instrument_hash: hash,
            action,
            reason,
            stale_hash,
        });
    }
    Ok(TournamentPlan {
        tournament_id,
        dir,
        base_hash: base,
        runs,
    })
}

/// Load a matrix and plan it with the matrix's settings over `opts.sim`. Reads only: this is
/// `shadowrunr sim tournament --plan-only`.
pub fn plan_matrix(
    matrix_path: &Path,
    out_dir: &Path,
    opts: &TournamentOptions,
) -> Result<TournamentPlan, SimError> {
    let matrix = Matrix::load(matrix_path)?;
    let sim = effective_options(&matrix, &opts.sim);
    plan(&matrix, out_dir, &sim)
}

/// Why a tournament stopped before working through its plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The next quota pause would exceed the total pause cap.
    PauseCap,
}

/// Exit code of `shadowrunr sim tournament`: 0 when every planned run is judged (or void
/// for a content reason), 4 when it stopped at the quota pause cap, 5 when it finished with
/// `engine_failed` or `judge_pending` runs left.
pub fn exit_code(stop: Option<StopReason>, engine_failed: usize, judge_pending: usize) -> u8 {
    match stop {
        Some(StopReason::PauseCap) => 4,
        None if engine_failed + judge_pending > 0 => 5,
        None => 0,
    }
}

#[derive(Debug, Clone)]
pub struct TournamentSummary {
    pub tournament_id: String,
    pub dir: PathBuf,
    pub planned: usize,
    pub skipped: usize,
    /// Run and judge attempts started (in-session retries included).
    pub executed: usize,
    pub failed: Vec<(String, String)>,
    /// Runs left `engine_failed` (re-run on the next resume). Not results.
    pub engine_failed: usize,
    /// Runs left `judge_pending` (re-judged on the next resume). Not results.
    pub judge_pending: usize,
    /// Total time spent paused on quota.
    pub paused_ms: u64,
    pub pauses: u32,
    /// The `paused: quota ...` lines, in order.
    pub pause_log: Vec<String>,
    pub stop: Option<StopReason>,
    pub results: Results,
}

impl TournamentSummary {
    pub fn exit_code(&self) -> u8 {
        exit_code(self.stop, self.engine_failed, self.judge_pending)
    }
}

#[derive(Serialize, Deserialize)]
struct TournamentMeta {
    tournament_id: String,
    matrix_id: String,
    base_hash: String,
    character: String,
    runs: usize,
}

/// Move a run directory aside so a fresh attempt starts clean (history is kept).
fn move_aside(run_dir: &Path, tag: &str) -> Result<(), SimError> {
    if !run_dir.exists() {
        return Ok(());
    }
    let name = run_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut target = run_dir.with_file_name(format!("{name}.{tag}"));
    let mut k = 1;
    while target.exists() {
        target = run_dir.with_file_name(format!("{name}.{tag}-{k}"));
        k += 1;
    }
    std::fs::rename(run_dir, &target)
        .map_err(|e| SimError::Io(format!("{}: {e}", run_dir.display())))
}

/// Clear the way for a fresh run attempt.
fn prepare_dir(r: &PlannedRun) -> Result<(), SimError> {
    if r.action != RunAction::Run {
        return Ok(());
    }
    if let Some(old) = &r.stale_hash {
        return move_aside(&r.run_dir, &format!("stale-{}", &old[..old.len().min(8)]));
    }
    if r.reason == PlanReason::EngineFailed {
        return move_aside(&r.run_dir, "engine-failed");
    }
    if r.run_dir.exists() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        move_aside(&r.run_dir, &format!("attempt-{ts}"))?;
    }
    Ok(())
}

/// What one attempt came to, as far as pacing cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunOutcome {
    /// Judged, content-void, or simulated: done.
    Done,
    JudgePending,
    EngineFailed,
}

/// Map a finished attempt onto an outcome by classifying what it left on disk.
pub(crate) fn outcome_of(res: Result<(), SimError>, dir: &Path) -> Result<RunOutcome, SimError> {
    match (res, outcome::classify(dir)) {
        (_, RunClass::EngineFailed) => Ok(RunOutcome::EngineFailed),
        (Err(SimError::Engine(e)), _) if outcome::is_engine_error(&e) => {
            Ok(RunOutcome::EngineFailed)
        }
        (Err(e), _) => Err(e),
        (Ok(()), RunClass::JudgePending) => Ok(RunOutcome::JudgePending),
        (Ok(()), _) => Ok(RunOutcome::Done),
    }
}

/// How the executor paces itself.
pub(crate) struct Pacing {
    pub gate: Option<Arc<dyn QuotaGate>>,
    pub clock: Arc<dyn PaceClock>,
    /// Probed with the cheapest role after a pause; `None` = assume it serves.
    pub probe: Option<Arc<dyn Engine>>,
    pub max_pause_ms: u64,
    pub max_retries: u32,
}

impl Pacing {
    fn blocked(&self) -> Option<QuotaBlock> {
        self.gate.as_ref().and_then(|g| g.blocked())
    }
    fn cooldown_ms(&self) -> u64 {
        self.gate
            .as_ref()
            .map(|g| g.default_cooldown_ms())
            .unwrap_or(pacing::DEFAULT_COOLDOWN_MS)
    }
    async fn probe_ok(&self) -> bool {
        match &self.probe {
            Some(e) => pacing::probe(e.as_ref()).await,
            None => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// The pre-run check found no account able to serve.
    Blocked,
    /// A run came back engine_failed or judge_pending.
    RunFailed,
}

#[derive(Debug, Default)]
pub(crate) struct ExecReport {
    pub executed: usize,
    pub failed: Vec<(String, String)>,
    pub paused_ms: u64,
    pub pauses: u32,
    pub pause_log: Vec<String>,
    pub stop: Option<StopReason>,
}

impl ExecReport {
    fn log(&mut self, line: String) {
        eprintln!("{line}");
        self.pause_log.push(line);
    }

    /// Pause until some account can serve. Returns false when the pause cap stops the
    /// tournament. Called only with nothing in flight.
    async fn pause(&mut self, p: &Pacing, trigger: Trigger, left: usize) -> bool {
        // After an engine failure a "clear" gate is verified with a probe before trusting it.
        let mut need_probe = trigger == Trigger::RunFailed;
        loop {
            let now = p.clock.now_ms();
            let block = match p.blocked() {
                Some(b) => Some(b),
                None if !need_probe => return true,
                None => {
                    if p.probe_ok().await && p.blocked().is_none() {
                        return true;
                    }
                    // The failed probe may have taught the engine a reset time.
                    p.blocked()
                }
            };
            let (labels, until) = match block {
                Some(QuotaBlock {
                    accounts,
                    until_ms: Some(u),
                }) => (
                    accounts,
                    u.max(now) + RESUME_MARGIN_MS + pacing::jitter_ms(self.pauses, u),
                ),
                // No reset time known: the fleet's default cooldown, then a probe.
                Some(QuotaBlock { accounts, .. }) => (accounts, now + p.cooldown_ms() as i64),
                None => (Vec::new(), now + p.cooldown_ms() as i64),
            };
            let wait = (until - now).max(0) as u64;
            let labels = if labels.is_empty() {
                "unknown".to_string()
            } else {
                labels.join(", ")
            };
            if self.paused_ms + wait > p.max_pause_ms {
                self.log(format!(
                    "stopped: quota ({labels}) pause cap {}s reached ({}s paused, next wait {}s) - {left} runs left",
                    p.max_pause_ms / 1000,
                    self.paused_ms / 1000,
                    wait / 1000
                ));
                self.stop = Some(StopReason::PauseCap);
                return false;
            }
            self.log(format!(
                "paused: quota ({labels}) until {} - {left} runs left",
                pacing::utc(until)
            ));
            self.pauses += 1;
            p.clock.sleep_until_ms(until).await;
            self.paused_ms += wait;
            need_probe = true;
        }
    }
}

/// Execute a plan with any runner, at most `concurrency` runs in flight, paced on quota.
/// Runs execute on the calling task (no spawn), so the brain need not be `Send + 'static`.
/// Pauses happen only when nothing is in flight.
pub(crate) async fn execute_plan<R, Fut>(
    plan: &TournamentPlan,
    concurrency: usize,
    max_runs: Option<usize>,
    pacing: &Pacing,
    runner: R,
) -> ExecReport
where
    R: Fn(PlannedRun) -> Fut,
    Fut: Future<Output = Result<RunOutcome, SimError>>,
{
    let mut queue: VecDeque<(PlannedRun, u32)> = plan
        .runs
        .iter()
        .filter(|r| r.action != RunAction::Skip)
        .take(max_runs.unwrap_or(usize::MAX))
        .cloned()
        .map(|r| (r, 0))
        .collect();
    let launch = |r: PlannedRun, attempt: u32| {
        let runner = &runner;
        async move {
            let res = match prepare_dir(&r) {
                Ok(()) => runner(r.clone()).await,
                Err(e) => Err(e),
            };
            (r, attempt, res)
        }
    };
    let mut rep = ExecReport::default();
    let mut in_flight = FuturesUnordered::new();
    let mut halt: Option<Trigger> = None;
    let concurrency = concurrency.max(1);
    loop {
        // Admit runs while nothing asks us to stop.
        while halt.is_none() && in_flight.len() < concurrency {
            let Some((r, attempt)) = queue.pop_front() else {
                break;
            };
            if pacing.blocked().is_some() {
                queue.push_front((r, attempt));
                halt = Some(Trigger::Blocked);
                break;
            }
            rep.executed += 1;
            in_flight.push(launch(r, attempt));
        }
        if in_flight.is_empty() {
            if queue.is_empty() {
                break;
            }
            if let Some(trigger) = halt.take() {
                if !rep.pause(pacing, trigger, queue.len()).await {
                    break;
                }
            }
            continue;
        }
        let Some((mut r, attempt, res)) = in_flight.next().await else {
            continue;
        };
        match res {
            Ok(RunOutcome::Done) => {}
            Ok(RunOutcome::EngineFailed) => {
                halt = Some(Trigger::RunFailed);
                if attempt < pacing.max_retries {
                    r.action = RunAction::Run;
                    r.reason = PlanReason::EngineFailed;
                    r.stale_hash = None;
                    queue.push_front((r, attempt + 1));
                }
            }
            Ok(RunOutcome::JudgePending) => {
                halt = Some(Trigger::RunFailed);
                if attempt < pacing.max_retries {
                    r.action = RunAction::Rejudge;
                    r.reason = PlanReason::JudgePending;
                    queue.push_front((r, attempt + 1));
                }
            }
            Err(e) => rep.failed.push((r.spec.run_id.clone(), e.to_string())),
        }
    }
    rep
}

/// Run (or resume) the tournament a matrix defines, then summarise it. `out_dir` is the root
/// (e.g. `runs/sim`); the tournament lands in `<out_dir>/<matrix_id>-<base_hash>/`.
pub async fn run_tournament(
    matrix_path: &Path,
    out_dir: &Path,
    engine: Arc<dyn Engine>,
    opts: &TournamentOptions,
) -> Result<TournamentSummary, SimError> {
    let matrix = Matrix::load(matrix_path)?;
    let mut sim = effective_options(&matrix, &opts.sim);
    if opts.quota.is_some() {
        sim.quota = opts.quota.clone();
    }
    let plan = plan(&matrix, out_dir, &sim)?;
    std::fs::create_dir_all(&plan.dir)
        .map_err(|e| SimError::Io(format!("{}: {e}", plan.dir.display())))?;
    write_json(
        &plan.dir.join("tournament.json"),
        &TournamentMeta {
            tournament_id: plan.tournament_id.clone(),
            matrix_id: matrix.id.clone(),
            base_hash: plan.base_hash.clone(),
            character: matrix.character.clone(),
            runs: plan.runs.len(),
        },
    )?;
    let _ = std::fs::copy(matrix_path, plan.dir.join("matrix.toml"));
    let concurrency = opts.concurrency.or(matrix.concurrency).unwrap_or(2);
    let pacing = Pacing {
        gate: sim.quota.as_ref().map(|g| g.0.clone()),
        clock: opts
            .clock
            .as_ref()
            .map(|c| c.0.clone())
            .unwrap_or_else(|| Arc::new(SystemPaceClock)),
        probe: Some(engine.clone()),
        max_pause_ms: opts.max_pause_ms.unwrap_or(DEFAULT_MAX_PAUSE_MS),
        max_retries: MAX_RETRIES,
    };
    let dir = plan.dir.clone();
    let rep = execute_plan(&plan, concurrency, opts.max_runs, &pacing, |r| {
        let engine = engine.clone();
        let sim = sim.clone();
        let dir = dir.clone();
        async move {
            let res = match r.action {
                RunAction::Rejudge => judge_existing(&r.run_dir, engine, &sim).await.map(|_| ()),
                _ => run_one_with(r.spec, dir, engine, &sim).await.map(|_| ()),
            };
            outcome_of(res, &r.run_dir)
        }
    })
    .await;
    let (results, _) = summarize_full(&plan.dir)?;
    // What is left, from the directory itself (also covers runs a cap or max_runs left).
    let after = self::plan(&matrix, out_dir, &sim)?;
    Ok(TournamentSummary {
        tournament_id: plan.tournament_id.clone(),
        dir: plan.dir.clone(),
        planned: plan.runs.len(),
        skipped: plan.count(RunAction::Skip),
        executed: rep.executed,
        failed: rep.failed,
        engine_failed: after.count_reason(PlanReason::EngineFailed),
        judge_pending: after.count_reason(PlanReason::JudgePending),
        paused_ms: rep.paused_ms,
        pauses: rep.pauses,
        pause_log: rep.pause_log,
        stop: rep.stop,
        results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pacing::tests::ManualClock;
    use crate::runner::tests::{fake_run, repo, spec, test_opts, FakeEngine};
    use crate::runner::{RunRecord, RunStatus};
    use async_trait::async_trait;
    use shadowrunr_types::{
        EngineError, EngineReply, EngineRequest, HealthReport, ModelTier, Role, WarmSession,
    };
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    const MATRIX: &str = r#"
id = "unit"
character = "malakor"
duration_s = 200
tick_s = 5
judge_tiers = ["sonnet", "opus"]
population = "small_new_channel"
[axes]
tones = ["theatrical"]
core_tiers = ["haiku", "sonnet"]
segments = ["SEG-01"]
audience_sizes = [3]
seeds = [1, 2]
"#;

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            if p.is_dir() {
                copy_dir(&p, &to.join(e.file_name()));
            } else {
                std::fs::copy(&p, to.join(e.file_name())).unwrap();
            }
        }
    }

    /// Options over a private copy of the character package, so tests can edit it.
    fn private_opts(tmp: &Path) -> SimOptions {
        let mut o = test_opts(&tmp.join("sim"));
        copy_dir(
            &repo().join("characters/malakor"),
            &tmp.join("characters/malakor"),
        );
        o.characters_root = tmp.join("characters");
        o
    }

    fn no_pacing() -> Pacing {
        Pacing {
            gate: None,
            clock: Arc::new(SystemPaceClock),
            probe: None,
            max_pause_ms: DEFAULT_MAX_PAUSE_MS,
            max_retries: MAX_RETRIES,
        }
    }

    async fn run_with_fakes(
        plan: &TournamentPlan,
        sim: &SimOptions,
        counter: &AtomicUsize,
    ) -> ExecReport {
        let dir = plan.dir.clone();
        execute_plan(plan, 2, None, &no_pacing(), |r| {
            let dir = dir.clone();
            let sim = sim.clone();
            counter.fetch_add(1, Ordering::SeqCst);
            async move {
                let res = match r.action {
                    RunAction::Rejudge => {
                        judge_existing(&r.run_dir, Arc::new(FakeEngine::new("")), &sim)
                            .await
                            .map(|_| ())
                    }
                    _ => {
                        fake_run(&dir, &sim, r.spec, Arc::new(FakeEngine::new(""))).await;
                        Ok(())
                    }
                };
                outcome_of(res, &r.run_dir)
            }
        })
        .await
    }

    fn set_record(dir: &Path, f: impl FnOnce(&mut RunRecord)) {
        let mut rec: RunRecord = read_json(&dir.join("run.json")).unwrap();
        f(&mut rec);
        write_json(&dir.join("run.json"), &rec).unwrap();
    }

    fn append(path: &Path, lines: &[String]) {
        let mut text = std::fs::read_to_string(path).unwrap_or_default();
        for l in lines {
            text.push_str(l);
            text.push('\n');
        }
        std::fs::write(path, text).unwrap();
    }

    #[tokio::test]
    async fn resume_skips_completed_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let base = private_opts(tmp.path());
        let matrix = Matrix::parse(MATRIX).unwrap();
        let sim = effective_options(&matrix, &base);
        let root = tmp.path().join("runs");

        let p1 = plan(&matrix, &root, &sim).unwrap();
        assert_eq!(p1.runs.len(), 4);
        assert_eq!(p1.count(RunAction::Run), 4);
        assert_eq!(p1.count_reason(PlanReason::New), 4);
        let counter = AtomicUsize::new(0);
        let rep = run_with_fakes(&p1, &sim, &counter).await;
        assert_eq!((rep.executed, rep.failed.len()), (4, 0));
        let verdict = p1.runs[0].run_dir.join("verdict.json");
        let mtime = std::fs::metadata(&verdict).unwrap().modified().unwrap();

        // Second pass: everything is complete, nothing runs.
        let p2 = plan(&matrix, &root, &sim).unwrap();
        assert_eq!(p2.count(RunAction::Skip), 4);
        let counter2 = AtomicUsize::new(0);
        let rep = run_with_fakes(&p2, &sim, &counter2).await;
        assert_eq!(rep.executed, 0);
        assert_eq!(counter2.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::metadata(&verdict).unwrap().modified().unwrap(),
            mtime
        );

        // A partial run (no verdict) is re-run; a judge that failed on the engine is
        // re-judged only.
        std::fs::remove_file(p1.runs[1].run_dir.join("verdict.json")).unwrap();
        std::fs::remove_file(p1.runs[2].run_dir.join("verdict.json")).unwrap();
        std::fs::remove_file(p1.runs[2].run_dir.join("run.json")).unwrap();
        set_record(&p1.runs[1].run_dir, |r| {
            r.status = RunStatus::JudgeVoid;
            r.judge_error = Some("judge engine call failed: timed out after 600000 ms".into());
        });
        let p3 = plan(&matrix, &root, &sim).unwrap();
        assert_eq!(p3.runs[1].action, RunAction::Rejudge);
        assert_eq!(p3.runs[2].action, RunAction::Run);
        assert_eq!(p3.runs[2].reason, PlanReason::Partial);
        assert_eq!(p3.count(RunAction::Skip), 2);
        let counter3 = AtomicUsize::new(0);
        run_with_fakes(&p3, &sim, &counter3).await;
        assert_eq!(counter3.load(Ordering::SeqCst), 2);
        assert_eq!(
            plan(&matrix, &root, &sim).unwrap().count(RunAction::Skip),
            4
        );
        // The interrupted attempt was moved aside, not mixed in.
        assert!(std::fs::read_dir(&p1.dir).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".attempt-")));
        let res = crate::summary::summarize(&p1.dir).unwrap();
        assert_eq!(res.runs, 4);
    }

    #[tokio::test]
    async fn instrument_change_gives_separate_results() {
        let tmp = tempfile::tempdir().unwrap();
        let base = private_opts(tmp.path());
        let matrix = Matrix::parse(MATRIX).unwrap();
        let sim = effective_options(&matrix, &base);
        let root = tmp.path().join("runs");
        let p1 = plan(&matrix, &root, &sim).unwrap();
        run_with_fakes(&p1, &sim, &AtomicUsize::new(0)).await;
        let r1 = crate::summary::summarize(&p1.dir).unwrap();
        assert_eq!(r1.instrument_hashes.len(), 1);

        // Tone edit: same tournament (base hash unchanged), the stale runs are redone and
        // the old ones moved aside; results hold only the new hash.
        std::fs::write(
            tmp.path().join("characters/malakor/tones/theatrical.md"),
            "a new tone\n",
        )
        .unwrap();
        let p2 = plan(&matrix, &root, &sim).unwrap();
        assert_eq!(p2.tournament_id, p1.tournament_id);
        assert!(p2.runs.iter().all(|r| r.action == RunAction::Run
            && r.reason == PlanReason::Stale
            && r.stale_hash.is_some()));
        run_with_fakes(&p2, &sim, &AtomicUsize::new(0)).await;
        let r2 = crate::summary::summarize(&p2.dir).unwrap();
        assert_eq!(r2.instrument_hashes.len(), 1);
        assert_ne!(r2.instrument_hashes, r1.instrument_hashes);
        assert_eq!(r2.runs, 4, "stale copies are not counted");
        assert!(std::fs::read_dir(&p2.dir).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".stale-")));

        // Rubric edit: a different base hash, so a different tournament directory.
        let rubric = sim.sim_root.join("rubric.toml");
        let text = std::fs::read_to_string(&rubric).unwrap();
        std::fs::write(
            &rubric,
            text.replace("sample_turns = 12", "sample_turns = 10"),
        )
        .unwrap();
        let p3 = plan(&matrix, &root, &sim).unwrap();
        assert_ne!(p3.tournament_id, p1.tournament_id);
        assert_eq!(p3.count(RunAction::Run), 4);
    }

    #[tokio::test]
    async fn run_tournament_with_everything_complete_only_summarises() {
        let tmp = tempfile::tempdir().unwrap();
        let base = private_opts(tmp.path());
        let mpath = tmp.path().join("m.toml");
        std::fs::write(&mpath, MATRIX).unwrap();
        let matrix = Matrix::parse(MATRIX).unwrap();
        let sim = effective_options(&matrix, &base);
        let root = tmp.path().join("runs");
        let p = plan(&matrix, &root, &sim).unwrap();
        run_with_fakes(&p, &sim, &AtomicUsize::new(0)).await;
        // The real runner would need the brain; with everything complete it is never called.
        let s = run_tournament(
            &mpath,
            &root,
            Arc::new(FakeEngine::new("")),
            &TournamentOptions {
                sim: base,
                concurrency: Some(3),
                ..TournamentOptions::default()
            },
        )
        .await
        .unwrap();
        assert_eq!((s.planned, s.skipped, s.executed), (4, 4, 0));
        assert_eq!((s.engine_failed, s.judge_pending, s.pauses), (0, 0, 0));
        assert_eq!(s.exit_code(), 0);
        assert!(s.dir.join("tournament.json").is_file());
        assert!(s.dir.join("matrix.toml").is_file());
        assert_eq!(s.results.judged, 4);
    }

    // ---- WP13: reclassification on resume -------------------------------------------

    const MATRIX6: &str = r#"
id = "evidence"
character = "malakor"
duration_s = 200
tick_s = 5
judge_tiers = ["sonnet", "opus"]
population = "small_new_channel"
[axes]
tones = ["theatrical"]
core_tiers = ["haiku", "sonnet"]
segments = ["SEG-01"]
audience_sizes = [3]
seeds = [1, 2, 3]
"#;

    const FLEET_LOST: &str = r#"{"type":"op","ts_ms":565000,"op":{"op":"sign_off","reason":"fleet_lost: engine degraded or down for 300s"}}"#;
    const COOLING: &str =
        "judge engine call failed: engine unavailable: all claude cli accounts cooling";

    fn core_error_row(kind: &str) -> String {
        format!(
            r#"{{"ts_unix_ms":1792331625964,"role":"core_thread","tier":"haiku","effort":"low","duration_ms":0,"usage":{{"input_tokens":0,"output_tokens":0,"cache_read_tokens":0,"cache_creation_tokens":0}},"correlation_id":"x","error_kind":"{kind}","session":false}}"#
        )
    }

    /// Six finished runs rewritten into the shapes of `runs/sim/night-0dd890a1ad/`
    /// (fictional fixtures): judged, fleet-lost void, ledger-cut void, timed-out judge,
    /// cooling judge, content void. Returns the plan before reclassification.
    async fn evidence_fixture(tmp: &Path) -> (Matrix, SimOptions, PathBuf, TournamentPlan) {
        let base = private_opts(tmp);
        let matrix = Matrix::parse(MATRIX6).unwrap();
        let sim = effective_options(&matrix, &base);
        let root = tmp.join("runs");
        let p = plan(&matrix, &root, &sim).unwrap();
        run_with_fakes(&p, &sim, &AtomicUsize::new(0)).await;
        let d: Vec<PathBuf> = p.runs.iter().map(|r| r.run_dir.clone()).collect();
        for dir in &d[1..] {
            std::fs::remove_file(dir.join("verdict.json")).unwrap();
        }
        // 1: the brain signed off on fleet loss (89 of these in the evidence).
        append(&d[1].join("transcript.jsonl"), &[FLEET_LOST.to_string()]);
        set_record(&d[1], |r| {
            r.status = RunStatus::JudgeVoid;
            r.judge_error = Some(COOLING.into());
        });
        // 2: clean transcript, but the core thread's last calls failed on the engine.
        append(
            &d[2].join("ledger.jsonl"),
            &(0..12)
                .map(|_| core_error_row("unavailable"))
                .collect::<Vec<_>>(),
        );
        set_record(&d[2], |r| {
            r.status = RunStatus::JudgeVoid;
            r.judge_error = Some(COOLING.into());
        });
        // 3: clean stream, the judge timed out.
        set_record(&d[3], |r| {
            r.status = RunStatus::JudgeVoid;
            r.judge_error = Some("judge engine call failed: timed out after 600000 ms".into());
        });
        // 4: clean stream, the judge found every account cooling.
        set_record(&d[4], |r| {
            r.status = RunStatus::JudgeVoid;
            r.judge_error = Some(COOLING.into());
        });
        // 5: the judge replied with something that is not the rubric JSON.
        set_record(&d[5], |r| {
            r.status = RunStatus::JudgeVoid;
            r.judge_error = Some("judge reply is not the rubric JSON".into());
        });
        (matrix, sim, root, p)
    }

    #[tokio::test]
    async fn old_judge_void_runs_are_reclassified_on_resume() {
        let tmp = tempfile::tempdir().unwrap();
        let (matrix, sim, root, p) = evidence_fixture(tmp.path()).await;
        let d: Vec<PathBuf> = p.runs.iter().map(|r| r.run_dir.clone()).collect();
        let want = [
            (RunClass::Judged, RunAction::Skip, PlanReason::Complete),
            (
                RunClass::EngineFailed,
                RunAction::Run,
                PlanReason::EngineFailed,
            ),
            (
                RunClass::EngineFailed,
                RunAction::Run,
                PlanReason::EngineFailed,
            ),
            (
                RunClass::JudgePending,
                RunAction::Rejudge,
                PlanReason::JudgePending,
            ),
            (
                RunClass::JudgePending,
                RunAction::Rejudge,
                PlanReason::JudgePending,
            ),
            (RunClass::JudgeVoid, RunAction::Skip, PlanReason::VoidKept),
        ];
        let p2 = plan(&matrix, &root, &sim).unwrap();
        for (i, (class, action, reason)) in want.iter().enumerate() {
            assert_eq!(outcome::classify(&d[i]), *class, "run {i}");
            assert_eq!(
                (p2.runs[i].action, p2.runs[i].reason),
                (*action, *reason),
                "run {i}"
            );
        }

        // Results exclude engine_failed and judge_pending; void counts content only.
        let (res, ex) = summarize_full(&p.dir).unwrap();
        assert_eq!(
            ex,
            crate::summary::Excluded {
                engine_failed: 2,
                judge_pending: 2
            }
        );
        assert_eq!((res.runs, res.judged, res.void, res.failed), (2, 1, 1, 0));
        let cell_runs: u32 = res.cells.iter().map(|c| c.runs).sum();
        assert_eq!(cell_runs, 2);
        assert!(std::fs::read_to_string(p.dir.join("report.md"))
            .unwrap()
            .contains("2 engine_failed"));

        // Resume: engine_failed runs are re-run (old dir moved aside as .engine-failed),
        // judge_pending runs are re-judged without re-simulating.
        let t3 = std::fs::read_to_string(d[3].join("transcript.jsonl")).unwrap();
        let counter = AtomicUsize::new(0);
        let rep = run_with_fakes(&p2, &sim, &counter).await;
        assert_eq!(counter.load(Ordering::SeqCst), 4);
        assert!(rep.failed.is_empty());
        assert_eq!(
            t3,
            std::fs::read_to_string(d[3].join("transcript.jsonl")).unwrap()
        );
        for i in [1, 2] {
            let mut aside = d[i].clone().into_os_string();
            aside.push(".engine-failed");
            assert!(PathBuf::from(aside).is_dir(), "run {i} moved aside");
        }
        let p3 = plan(&matrix, &root, &sim).unwrap();
        assert_eq!(p3.count(RunAction::Skip), 6);
        assert_eq!(p3.count_reason(PlanReason::VoidKept), 1);
        let (res, ex) = summarize_full(&p.dir).unwrap();
        assert_eq!(ex, crate::summary::Excluded::default());
        assert_eq!((res.runs, res.judged, res.void), (6, 5, 1));
    }

    // ---- WP13: quota pacing ------------------------------------------------------------

    const T0: i64 = 1_792_330_000_000;

    /// A fleet stand-in: one account cooling until `until` (reporting the reset only when
    /// `known`), read on the shared manual clock.
    struct FakeGate {
        clock: Arc<ManualClock>,
        until: Mutex<Option<i64>>,
        known: bool,
        cooldown_ms: u64,
    }

    impl FakeGate {
        fn new(clock: &Arc<ManualClock>, known: bool) -> Arc<Self> {
            Arc::new(Self {
                clock: clock.clone(),
                until: Mutex::new(None),
                known,
                cooldown_ms: 900_000,
            })
        }
        fn cool_until(&self, t: i64) {
            *self.until.lock().unwrap() = Some(t);
        }
    }

    impl QuotaGate for FakeGate {
        fn blocked(&self) -> Option<QuotaBlock> {
            let until = (*self.until.lock().unwrap())?;
            (until > self.clock.now_ms()).then(|| QuotaBlock {
                accounts: vec!["isolated".into()],
                until_ms: self.known.then_some(until),
            })
        }
        fn default_cooldown_ms(&self) -> u64 {
            self.cooldown_ms
        }
    }

    /// Answers probes from a script (then from the gate); records every request's role.
    struct ProbeEngine {
        gate: Option<Arc<FakeGate>>,
        script: Mutex<VecDeque<bool>>,
        roles: Mutex<Vec<Role>>,
    }

    impl ProbeEngine {
        fn new(gate: Option<Arc<FakeGate>>, script: &[bool]) -> Arc<Self> {
            Arc::new(Self {
                gate,
                script: Mutex::new(script.iter().copied().collect()),
                roles: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl Engine for ProbeEngine {
        async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
            self.roles.lock().unwrap().push(req.role);
            let scripted = self.script.lock().unwrap().pop_front();
            let ok = scripted
                .unwrap_or_else(|| self.gate.as_ref().is_none_or(|g| g.blocked().is_none()));
            if ok {
                shadowrunr_engine::ScriptedEngine.complete(req).await
            } else {
                Err(EngineError::RateLimited {
                    account: "isolated".into(),
                    reset_at_ms: None,
                })
            }
        }
        async fn open_session(
            &self,
            _t: EngineRequest,
        ) -> Result<Box<dyn WarmSession>, EngineError> {
            Err(EngineError::Unavailable {
                reason: "probe only".into(),
            })
        }
        async fn health(&self) -> Vec<HealthReport> {
            Vec::new()
        }
    }

    fn synthetic_plan(tmp: &Path, n: usize) -> TournamentPlan {
        let dir = tmp.join("t");
        TournamentPlan {
            tournament_id: "t".into(),
            dir: dir.clone(),
            base_hash: "h".into(),
            runs: (0..n)
                .map(|i| {
                    let s = spec(&format!("r{i}"), ModelTier::Haiku, 3, 60);
                    PlannedRun {
                        run_dir: dir.join(&s.run_id),
                        spec: s,
                        instrument_hash: "h".into(),
                        action: RunAction::Run,
                        reason: PlanReason::New,
                        stale_hash: None,
                    }
                })
                .collect(),
        }
    }

    /// Per run: (start ms, outcome) of every attempt.
    type Attempts = Arc<Mutex<HashMap<String, Vec<(i64, RunOutcome)>>>>;

    /// A runner that fails (engine_failed) while the gate is cooling, and makes the quota
    /// run out at the start of attempt number `limit_at` (0-based, across all runs).
    fn quota_runner(
        clock: Arc<ManualClock>,
        gate: Arc<FakeGate>,
        attempts: Attempts,
        limit_at: Option<(usize, i64)>,
    ) -> impl Fn(PlannedRun) -> std::pin::Pin<Box<dyn Future<Output = Result<RunOutcome, SimError>>>>
    {
        let started = Arc::new(AtomicUsize::new(0));
        move |r: PlannedRun| {
            let (clock, gate, attempts, started) = (
                clock.clone(),
                gate.clone(),
                attempts.clone(),
                started.clone(),
            );
            let fut: std::pin::Pin<Box<dyn Future<Output = Result<RunOutcome, SimError>>>> =
                Box::pin(async move {
                    clock.in_flight.fetch_add(1, Ordering::SeqCst);
                    let n = started.fetch_add(1, Ordering::SeqCst);
                    let now = clock.now_ms();
                    if let Some((k, reset_in)) = limit_at {
                        if n == k {
                            gate.cool_until(now + reset_in);
                        }
                    }
                    tokio::task::yield_now().await;
                    let out = if gate.blocked().is_some() {
                        RunOutcome::EngineFailed
                    } else {
                        RunOutcome::Done
                    };
                    attempts
                        .lock()
                        .unwrap()
                        .entry(r.spec.run_id.clone())
                        .or_default()
                        .push((now, out));
                    clock.in_flight.fetch_sub(1, Ordering::SeqCst);
                    Ok(out)
                });
            fut
        }
    }

    fn pacing_with(
        clock: &Arc<ManualClock>,
        gate: Option<Arc<FakeGate>>,
        probe: Arc<ProbeEngine>,
        max_pause_ms: u64,
    ) -> Pacing {
        Pacing {
            gate: gate.map(|g| g as Arc<dyn QuotaGate>),
            clock: clock.clone(),
            probe: Some(probe),
            max_pause_ms,
            max_retries: MAX_RETRIES,
        }
    }

    #[tokio::test]
    async fn rate_limit_with_reset_20s_ahead_pauses_then_completes_every_run() {
        let tmp = tempfile::tempdir().unwrap();
        let clock = ManualClock::new(T0);
        let gate = FakeGate::new(&clock, true);
        let probe = ProbeEngine::new(Some(gate.clone()), &[]);
        let attempts: Attempts = Arc::default();
        let plan = synthetic_plan(tmp.path(), 6);
        // The limit hits as the 4th attempt starts; it resets 20 s later.
        let runner = quota_runner(
            clock.clone(),
            gate.clone(),
            attempts.clone(),
            Some((3, 20_000)),
        );
        let pacing = pacing_with(
            &clock,
            Some(gate.clone()),
            probe.clone(),
            DEFAULT_MAX_PAUSE_MS,
        );
        let rep = execute_plan(&plan, 2, None, &pacing, runner).await;

        let attempts = attempts.lock().unwrap();
        assert_eq!(attempts.len(), 6);
        for (id, a) in attempts.iter() {
            assert_eq!(a.last().unwrap().1, RunOutcome::Done, "{id} completed");
        }
        assert!(rep.failed.is_empty() && rep.stop.is_none());
        assert_eq!(rep.pauses, 1);
        // Until the reset, plus 60 s, plus jitter under 30 s.
        let sleeps = clock.sleeps.lock().unwrap().clone();
        assert_eq!(sleeps.len(), 1);
        let (from, until) = sleeps[0];
        assert!(
            (from + 80_000..from + 110_000).contains(&until),
            "{from} {until}"
        );
        assert_eq!(rep.paused_ms, (until - from) as u64);
        let line = &rep.pause_log[0];
        assert!(
            line.starts_with("paused: quota (isolated) until 2026-10-")
                && line.contains(" UTC - ")
                && line.ends_with(" runs left"),
            "{line}"
        );
        // One probe with the cheapest role, never a run.
        assert_eq!(*probe.roles.lock().unwrap(), vec![Role::ChatPicker]);
        // No attempt started while cooling.
        let all: Vec<(i64, RunOutcome)> = attempts.values().flatten().copied().collect();
        assert!(all.iter().all(|(t, _)| *t <= from || *t >= until));
        assert!(all.iter().any(|(_, o)| *o == RunOutcome::EngineFailed));
        assert_eq!(clock.slept_with_runs.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn no_run_is_written_while_paused() {
        let tmp = tempfile::tempdir().unwrap();
        let clock = ManualClock::new(T0);
        let gate = FakeGate::new(&clock, true);
        gate.cool_until(T0 + 20_000);
        let probe = ProbeEngine::new(Some(gate.clone()), &[]);
        let attempts: Attempts = Arc::default();
        let plan = synthetic_plan(tmp.path(), 5);
        let runner = quota_runner(clock.clone(), gate.clone(), attempts.clone(), None);
        let pacing = pacing_with(&clock, Some(gate.clone()), probe, DEFAULT_MAX_PAUSE_MS);
        let rep = execute_plan(&plan, 3, None, &pacing, runner).await;

        // The pre-run check found the account cooling: nothing started until the reset plus
        // the margin, nothing failed, nothing was written before it.
        let sleeps = clock.sleeps.lock().unwrap().clone();
        assert_eq!(sleeps, vec![(T0, sleeps[0].1)]);
        let resume = sleeps[0].1;
        assert!(resume >= T0 + 80_000);
        let attempts = attempts.lock().unwrap();
        assert_eq!(attempts.len(), 5);
        assert!(attempts
            .values()
            .flatten()
            .all(|(t, o)| *t >= resume && *o == RunOutcome::Done));
        assert_eq!(rep.executed, 5);
        assert_eq!(clock.slept_with_runs.load(Ordering::SeqCst), 0);
        assert!(!plan.dir.exists(), "the runner never touched disk");
    }

    #[tokio::test]
    async fn unknown_reset_waits_the_cooldown_then_probes() {
        // The gate knows the account is cooling but not until when.
        let tmp = tempfile::tempdir().unwrap();
        let clock = ManualClock::new(T0);
        let gate = FakeGate::new(&clock, false);
        gate.cool_until(T0 + 600_000);
        let probe = ProbeEngine::new(Some(gate.clone()), &[]);
        let attempts: Attempts = Arc::default();
        let plan = synthetic_plan(tmp.path(), 2);
        let runner = quota_runner(clock.clone(), gate.clone(), attempts.clone(), None);
        let pacing = pacing_with(
            &clock,
            Some(gate.clone()),
            probe.clone(),
            DEFAULT_MAX_PAUSE_MS,
        );
        let rep = execute_plan(&plan, 2, None, &pacing, runner).await;
        assert_eq!(
            *clock.sleeps.lock().unwrap(),
            vec![(T0, T0 + 900_000)],
            "the fleet's default cooldown"
        );
        assert_eq!(*probe.roles.lock().unwrap(), vec![Role::ChatPicker]);
        assert!(rep.pause_log[0].starts_with("paused: quota (isolated) until "));
        assert_eq!(attempts.lock().unwrap().len(), 2);

        // No gate at all (an engine that is not the fleet): an engine_failed run is checked
        // with an immediate probe; it fails, so the default cooldown, then a probe that passes.
        let clock = ManualClock::new(T0);
        let probe = ProbeEngine::new(None, &[false, true]);
        let pacing = pacing_with(&clock, None, probe.clone(), DEFAULT_MAX_PAUSE_MS);
        let calls = Arc::new(AtomicUsize::new(0));
        let c2 = calls.clone();
        let rep = execute_plan(
            &synthetic_plan(tmp.path(), 1),
            1,
            None,
            &pacing,
            move |_r| {
                let n = c2.fetch_add(1, Ordering::SeqCst);
                async move {
                    Ok(if n == 0 {
                        RunOutcome::EngineFailed
                    } else {
                        RunOutcome::Done
                    })
                }
            },
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 2, "retried once");
        assert_eq!(
            *clock.sleeps.lock().unwrap(),
            vec![(T0, T0 + pacing::DEFAULT_COOLDOWN_MS as i64)]
        );
        assert_eq!(probe.roles.lock().unwrap().len(), 2);
        assert!(rep.pause_log[0].starts_with("paused: quota (unknown) until "));
    }

    #[tokio::test]
    async fn pause_cap_stops_with_exit_code_4() {
        let tmp = tempfile::tempdir().unwrap();
        let clock = ManualClock::new(T0);
        let gate = FakeGate::new(&clock, true);
        gate.cool_until(T0 + 10 * 3600 * 1000);
        let probe = ProbeEngine::new(Some(gate.clone()), &[]);
        let attempts: Attempts = Arc::default();
        let runner = quota_runner(clock.clone(), gate.clone(), attempts.clone(), None);
        let pacing = pacing_with(&clock, Some(gate), probe, DEFAULT_MAX_PAUSE_MS);
        let rep = execute_plan(&synthetic_plan(tmp.path(), 3), 2, None, &pacing, runner).await;
        assert_eq!(rep.stop, Some(StopReason::PauseCap));
        assert_eq!((rep.executed, rep.pauses, rep.paused_ms), (0, 0, 0));
        assert!(clock.sleeps.lock().unwrap().is_empty());
        assert!(rep.pause_log[0].starts_with("stopped: quota (isolated) pause cap 28800s"));
        assert!(attempts.lock().unwrap().is_empty());
        assert_eq!(exit_code(rep.stop, 0, 0), 4);
        assert_eq!(exit_code(rep.stop, 3, 0), 4, "the cap wins");
        assert_eq!(exit_code(None, 1, 0), 5);
        assert_eq!(exit_code(None, 0, 2), 5);
        assert_eq!(exit_code(None, 0, 0), 0);
    }

    #[tokio::test]
    async fn engine_failed_is_retried_twice_then_left_for_resume() {
        let tmp = tempfile::tempdir().unwrap();
        let clock = ManualClock::new(T0);
        let probe = ProbeEngine::new(None, &[]);
        let pacing = pacing_with(&clock, None, probe.clone(), DEFAULT_MAX_PAUSE_MS);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let c2 = calls.clone();
        let rep = execute_plan(&synthetic_plan(tmp.path(), 2), 1, None, &pacing, move |r| {
            c2.lock()
                .unwrap()
                .push((r.spec.run_id.clone(), r.action, r.reason));
            let fail = r.spec.run_id == "r0";
            async move {
                Ok(if fail {
                    RunOutcome::EngineFailed
                } else {
                    RunOutcome::JudgePending
                })
            }
        })
        .await;
        let calls = calls.lock().unwrap();
        let r0: Vec<_> = calls.iter().filter(|c| c.0 == "r0").collect();
        let r1: Vec<_> = calls.iter().filter(|c| c.0 == "r1").collect();
        assert_eq!(r0.len(), 3, "first attempt plus two retries");
        assert!(r0[1..]
            .iter()
            .all(|c| c.1 == RunAction::Run && c.2 == PlanReason::EngineFailed));
        assert_eq!(r1.len(), 3);
        assert!(r1[1..]
            .iter()
            .all(|c| c.1 == RunAction::Rejudge && c.2 == PlanReason::JudgePending));
        // The engine kept answering the probe, so no pause was needed.
        assert_eq!((rep.pauses, rep.stop), (0, None));
        assert!(probe.roles.lock().unwrap().len() >= 4);
    }
}
