//! Tournaments: run a matrix with a concurrency limit, resumably.
//!
//! Layout: `<out_root>/<matrix_id>-<base_hash[..10]>/<run_id>/`. The base hash covers
//! everything but the tone, so editing the rubric, the segment tables, the calibration or the
//! character package starts a new tournament directory instead of mixing instruments. Inside
//! a tournament, a run whose tone file changed since it ran is moved aside
//! (`<run_id>.stale-<hash8>`) and run again.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use shadowrunr_types::{Engine, SimRunSpec};

use crate::calibration;
use crate::instrument::Instrument;
use crate::matrix::Matrix;
use crate::runner::{instrument_for, judge_existing, run_one_with, RunRecord, RunStatus};
use crate::summary::{summarize, Results};
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
}

/// Matrix settings applied over the caller's options.
pub fn effective_options(matrix: &Matrix, base: &SimOptions) -> SimOptions {
    let mut o = base.clone();
    o.tick_s = matrix.tick_s;
    if !matrix.judge_tiers.is_empty() {
        o.judge_tiers = matrix.judge_tiers.clone();
    }
    if let Some(f) = matrix.featured_every_ticks {
        o.featured_every_ticks = f;
    }
    if let Some(m) = matrix.max_featured {
        o.max_featured = m;
    }
    o
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunAction {
    /// Already complete for this instrument.
    Skip,
    /// Simulated, but the judge was void: judge again without re-simulating.
    Rejudge,
    Run,
}

#[derive(Debug, Clone)]
pub struct PlannedRun {
    pub spec: SimRunSpec,
    pub run_dir: PathBuf,
    pub instrument_hash: String,
    pub action: RunAction,
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
        let (action, stale_hash) = match existing {
            Some(h) if h != hash => (RunAction::Run, Some(h)),
            Some(_) => {
                let record: Option<RunRecord> = read_json(&run_dir.join("run.json")).ok();
                let action = if opts.judge {
                    if run_dir.join("verdict.json").is_file() {
                        RunAction::Skip
                    } else if record
                        .as_ref()
                        .is_some_and(|r| r.status == RunStatus::JudgeVoid)
                        && run_dir.join("transcript.jsonl").is_file()
                    {
                        RunAction::Rejudge
                    } else {
                        RunAction::Run
                    }
                } else if record.is_some() {
                    RunAction::Skip
                } else {
                    RunAction::Run
                };
                (action, None)
            }
            None => (RunAction::Run, None),
        };
        runs.push(PlannedRun {
            spec,
            run_dir,
            instrument_hash: hash,
            action,
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

#[derive(Debug, Clone)]
pub struct TournamentSummary {
    pub tournament_id: String,
    pub dir: PathBuf,
    pub planned: usize,
    pub skipped: usize,
    pub executed: usize,
    pub failed: Vec<(String, String)>,
    pub results: Results,
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

/// Execute a plan with any runner, at most `concurrency` runs in flight. Runs execute on the
/// calling task (no spawn), so the brain need not be `Send + 'static`.
pub(crate) async fn execute_plan<R, Fut>(
    plan: &TournamentPlan,
    concurrency: usize,
    max_runs: Option<usize>,
    runner: R,
) -> (usize, Vec<(String, String)>)
where
    R: Fn(PlannedRun) -> Fut,
    Fut: Future<Output = Result<(), SimError>>,
{
    let todo: Vec<PlannedRun> = plan
        .runs
        .iter()
        .filter(|r| r.action != RunAction::Skip)
        .take(max_runs.unwrap_or(usize::MAX))
        .cloned()
        .collect();
    let executed = todo.len();
    let results: Vec<(String, Result<(), SimError>)> = stream::iter(todo)
        .map(|r| {
            let id = r.spec.run_id.clone();
            let fut = async {
                if r.action == RunAction::Run {
                    if let Some(old) = &r.stale_hash {
                        move_aside(&r.run_dir, &format!("stale-{}", &old[..old.len().min(8)]))?;
                    } else if r.run_dir.exists() {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or(0);
                        move_aside(&r.run_dir, &format!("attempt-{ts}"))?;
                    }
                }
                runner(r).await
            };
            async move { (id, fut.await) }
        })
        .buffer_unordered(concurrency.max(1))
        .collect()
        .await;
    let failed = results
        .into_iter()
        .filter_map(|(id, r)| r.err().map(|e| (id, e.to_string())))
        .collect();
    (executed, failed)
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
    let sim = effective_options(&matrix, &opts.sim);
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
    let dir = plan.dir.clone();
    let (executed, failed) = execute_plan(&plan, concurrency, opts.max_runs, |r| {
        let engine = engine.clone();
        let sim = sim.clone();
        let dir = dir.clone();
        async move {
            match r.action {
                RunAction::Rejudge => judge_existing(&r.run_dir, engine, &sim).await.map(|_| ()),
                _ => run_one_with(r.spec, dir, engine, &sim).await.map(|_| ()),
            }
        }
    })
    .await;
    let results = summarize(&plan.dir)?;
    Ok(TournamentSummary {
        tournament_id: plan.tournament_id.clone(),
        dir: plan.dir.clone(),
        planned: plan.runs.len(),
        skipped: plan.count(RunAction::Skip),
        executed,
        failed,
        results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::tests::{fake_run, repo, test_opts, FakeEngine};
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    async fn run_with_fakes(
        plan: &TournamentPlan,
        sim: &SimOptions,
        counter: &AtomicUsize,
    ) -> (usize, Vec<(String, String)>) {
        let dir = plan.dir.clone();
        execute_plan(plan, 2, None, |r| {
            let dir = dir.clone();
            let sim = sim.clone();
            counter.fetch_add(1, Ordering::SeqCst);
            async move {
                match r.action {
                    RunAction::Rejudge => {
                        judge_existing(&r.run_dir, Arc::new(FakeEngine::new("")), &sim)
                            .await
                            .map(|_| ())
                    }
                    _ => {
                        fake_run(&dir, &sim, r.spec, Arc::new(FakeEngine::new(""))).await;
                        Ok(())
                    }
                }
            }
        })
        .await
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
        let counter = AtomicUsize::new(0);
        let (executed, failed) = run_with_fakes(&p1, &sim, &counter).await;
        assert_eq!((executed, failed.len()), (4, 0));
        let verdict = p1.runs[0].run_dir.join("verdict.json");
        let mtime = std::fs::metadata(&verdict).unwrap().modified().unwrap();

        // Second pass: everything is complete, nothing runs.
        let p2 = plan(&matrix, &root, &sim).unwrap();
        assert_eq!(p2.count(RunAction::Skip), 4);
        let counter2 = AtomicUsize::new(0);
        let (executed, _) = run_with_fakes(&p2, &sim, &counter2).await;
        assert_eq!(executed, 0);
        assert_eq!(counter2.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::metadata(&verdict).unwrap().modified().unwrap(),
            mtime
        );

        // A partial run (no verdict) is re-run; a void judge is re-judged only.
        std::fs::remove_file(p1.runs[1].run_dir.join("verdict.json")).unwrap();
        std::fs::remove_file(p1.runs[2].run_dir.join("verdict.json")).unwrap();
        std::fs::remove_file(p1.runs[2].run_dir.join("run.json")).unwrap();
        let mut rec: RunRecord = read_json(&p1.runs[1].run_dir.join("run.json")).unwrap();
        rec.status = RunStatus::JudgeVoid;
        write_json(&p1.runs[1].run_dir.join("run.json"), &rec).unwrap();
        let p3 = plan(&matrix, &root, &sim).unwrap();
        assert_eq!(p3.runs[1].action, RunAction::Rejudge);
        assert_eq!(p3.runs[2].action, RunAction::Run);
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
        let res = summarize(&p1.dir).unwrap();
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
        let r1 = summarize(&p1.dir).unwrap();
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
        assert!(p2
            .runs
            .iter()
            .all(|r| r.action == RunAction::Run && r.stale_hash.is_some()));
        run_with_fakes(&p2, &sim, &AtomicUsize::new(0)).await;
        let r2 = summarize(&p2.dir).unwrap();
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
                max_runs: None,
            },
        )
        .await
        .unwrap();
        assert_eq!((s.planned, s.skipped, s.executed), (4, 4, 0));
        assert!(s.dir.join("tournament.json").is_file());
        assert!(s.dir.join("matrix.toml").is_file());
        assert_eq!(s.results.judged, 4);
    }
}
