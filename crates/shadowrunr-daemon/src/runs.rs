//! The on-disk index of simulation output: `<runs>/sim/<tournament_id>/<run_id>/`.
//!
//! Only a run's live directory counts (its name equals `spec.run_id`); directories moved aside
//! by a tournament (`<run_id>.stale-<hash>`, `<run_id>.attempt-<ts>`) are history.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use shadowrunr_sim::instrument::Instrument;
use shadowrunr_sim::{classify, RunClass};
pub use shadowrunr_types::rpc::{RunRow, TournamentRow};
use shadowrunr_types::{CheckState, JudgeVerdict, SimRunSpec, StreamEvent};

use crate::rpc::RpcError;

/// `[A-Za-z0-9._-]+`, not `.` or `..`: safe as one path component on every OS.
pub fn check_id(kind: &str, id: &str) -> Result<(), RpcError> {
    let ok = !id.is_empty()
        && id.len() <= 200
        && id != "."
        && id != ".."
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if ok {
        Ok(())
    } else {
        Err(RpcError::invalid(format!(
            "{kind} {id:?} must be [A-Za-z0-9._-]"
        )))
    }
}

#[derive(Debug, Clone)]
pub struct RunEntry {
    pub row: RunRow,
    pub dir: PathBuf,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub(crate) fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

pub(crate) fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A verdict's run health: the share of rule checks that held (pass / (pass + fail); absent
/// when no rule was provoked) and the failed `red-line:*` checks.
pub fn verdict_health(v: &JudgeVerdict) -> (Option<f64>, u32) {
    let pass = v
        .rules
        .iter()
        .filter(|r| r.state == CheckState::Pass)
        .count();
    let fail = v
        .rules
        .iter()
        .filter(|r| r.state == CheckState::Fail)
        .count();
    let red = v
        .rules
        .iter()
        .filter(|r| r.state == CheckState::Fail && r.rule.starts_with("red-line"))
        .count();
    let n = pass + fail;
    let health = (n > 0).then(|| pass as f64 / n as f64);
    (health, u32::try_from(red).unwrap_or(u32::MAX))
}

pub(crate) fn load_entry(dir: &Path, tournament_id: &str) -> Option<RunEntry> {
    let spec: SimRunSpec = read_json(&dir.join("spec.json"))?;
    if name_of(dir) != spec.run_id {
        return None;
    }
    Some(RunEntry {
        row: RunRow {
            run_id: spec.run_id.clone(),
            tournament_id: tournament_id.to_string(),
            has_verdict: dir.join("verdict.json").is_file(),
            health: None,
            red_line_fails: 0,
            spec,
        },
        dir: dir.to_path_buf(),
    })
}

impl RunEntry {
    /// Fills `health` and `red_line_fails` from `verdict.json`. Only rows that go on the wire
    /// pay for it (`runs.list`, `runs.page`), not the tournament counts. An unreadable verdict
    /// still counts as present (`has_verdict`), just without health. Only a judged run has
    /// health: `verdict.json` is what makes `classify` say `judged`, so an `engine_failed` or
    /// `judge_pending` run never carries a health figure.
    pub(crate) fn with_health(mut self) -> Self {
        if self.row.has_verdict {
            if let Some(v) = read_json::<JudgeVerdict>(&self.dir.join("verdict.json")) {
                (self.row.health, self.row.red_line_fails) = verdict_health(&v);
            }
        }
        self
    }
}

pub(crate) fn tournament_runs(sim_root: &Path, tournament_id: &str) -> Vec<RunEntry> {
    subdirs(&sim_root.join(tournament_id))
        .iter()
        .filter_map(|d| load_entry(d, tournament_id))
        .collect()
}

/// `runs.list`.
pub fn list_runs(sim_root: &Path, tournament_id: Option<&str>) -> Result<Vec<RunEntry>, RpcError> {
    match tournament_id {
        Some(t) => {
            check_id("tournament_id", t)?;
            if !sim_root.join(t).is_dir() {
                return Err(RpcError::not_found(format!("tournament {t}")));
            }
            Ok(tournament_runs(sim_root, t)
                .into_iter()
                .map(RunEntry::with_health)
                .collect())
        }
        None => Ok(subdirs(sim_root)
            .iter()
            .flat_map(|t| tournament_runs(sim_root, &name_of(t)))
            .map(RunEntry::with_health)
            .collect()),
    }
}

/// Find a run by `run_id`, `<tournament_id>/<run_id>`, or `run_id` plus an explicit
/// tournament. A bare id found in several tournaments is ambiguous.
pub fn find_run(
    sim_root: &Path,
    run_ref: &str,
    tournament_id: Option<&str>,
) -> Result<RunEntry, RpcError> {
    let (tid, rid) = match run_ref.split_once('/') {
        Some((t, r)) => (Some(t), r),
        None => (tournament_id, run_ref),
    };
    check_id("run_id", rid)?;
    if let Some(t) = tid {
        check_id("tournament_id", t)?;
        return load_entry(&sim_root.join(t).join(rid), t)
            .ok_or_else(|| RpcError::not_found(format!("run {t}/{rid}")));
    }
    let mut hits: Vec<RunEntry> = subdirs(sim_root)
        .iter()
        .filter_map(|t| load_entry(&t.join(rid), &name_of(t)))
        .collect();
    match hits.len() {
        0 => Err(RpcError::not_found(format!("run {rid}"))),
        1 => Ok(hits.remove(0)),
        _ => Err(RpcError::invalid(format!(
            "run id {rid} is in {} tournaments ({}); use <tournament_id>/<run_id>",
            hits.len(),
            hits.iter()
                .map(|h| h.row.tournament_id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

#[derive(serde::Deserialize)]
struct TournamentMeta {
    base_hash: String,
    #[serde(default)]
    runs: Option<usize>,
}

/// Whether a run finished, by the WP13 rule (`shadowrunr_sim::outcome::classify`): judged, or
/// simulated with judging off. A void judge is not complete, and neither is a run the engine
/// decided: `judge_pending` (a resume re-judges it) or `engine_failed` (a resume re-runs it),
/// whatever status its `run.json` carries.
fn finished(dir: &Path) -> bool {
    matches!(classify(dir), RunClass::Judged | RunClass::Simulated)
}

#[derive(Default)]
struct RunsFold {
    completed: usize,
    judged: usize,
    held_sum: f64,
    held_n: usize,
    red_line_fails: u32,
}

/// One verdict parse per run. `classify` says `judged` exactly when `verdict.json` parses, so a
/// parsed verdict is a finished, judged run; any other run falls to [`finished`].
fn fold_runs(runs: &[RunEntry]) -> RunsFold {
    let mut f = RunsFold::default();
    for r in runs {
        match read_json::<JudgeVerdict>(&r.dir.join("verdict.json")) {
            Some(v) => {
                let (health, red) = verdict_health(&v);
                f.completed += 1;
                f.judged += 1;
                f.red_line_fails = f.red_line_fails.saturating_add(red);
                if let Some(h) = health {
                    f.held_sum += h;
                    f.held_n += 1;
                }
            }
            None if finished(&r.dir) => f.completed += 1,
            None => {}
        }
    }
    f
}

/// One `tournament.list` row for `<sim_root>/<tournament_id>/`, or `None` when the directory
/// is neither a tournament nor holds runs. The instrument hash is the base hash from
/// `tournament.json`; a directory without one (the ad-hoc directory) reports the runs' common
/// base hash, or `mixed`. `completed` counts runs that finished (see [`finished`]); `judged`,
/// `held` and `red_line_fails` fold the judged runs' verdicts (see [`fold_runs`]).
pub(crate) fn tournament_row(sim_root: &Path, tid: &str) -> Option<TournamentRow> {
    let t = sim_root.join(tid);
    let runs = tournament_runs(sim_root, tid);
    let meta: Option<TournamentMeta> = read_json(&t.join("tournament.json"));
    if meta.is_none() && runs.is_empty() {
        return None;
    }
    let instrument_hash = match &meta {
        Some(m) => m.base_hash.clone(),
        None => {
            let mut hashes: Vec<String> = runs
                .iter()
                .filter_map(|r| {
                    read_json::<Instrument>(&r.dir.join("instrument.json")).map(|i| i.base_hash)
                })
                .collect();
            hashes.sort();
            hashes.dedup();
            match hashes.len() {
                1 => hashes.remove(0),
                0 => "unknown".into(),
                _ => "mixed".into(),
            }
        }
    };
    let fold = fold_runs(&runs);
    Some(TournamentRow {
        completed: fold.completed,
        judged: fold.judged,
        held: (fold.held_n > 0).then(|| fold.held_sum / fold.held_n as f64),
        red_line_fails: fold.red_line_fails,
        runs: meta.and_then(|m| m.runs).unwrap_or(runs.len()),
        instrument_hash,
        tournament_id: tid.to_string(),
    })
}

/// `tournament.list`.
pub fn list_tournaments(sim_root: &Path) -> Vec<TournamentRow> {
    subdirs(sim_root)
        .iter()
        .filter_map(|t| tournament_row(sim_root, &name_of(t)))
        .collect()
}

/// Read a transcript as raw lines, each verified to parse as a `StreamEvent`.
pub fn read_transcript_lines(path: &Path) -> Result<Vec<(String, StreamEvent)>, RpcError> {
    let f = std::fs::File::open(path)
        .map_err(|e| RpcError::not_found(format!("{}: {e}", path.display())))?;
    let mut out = Vec::new();
    for (i, line) in BufReader::new(f).lines().enumerate() {
        let line = line.map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
        if line.trim().is_empty() {
            continue;
        }
        let ev: StreamEvent = serde_json::from_str(&line)
            .map_err(|e| RpcError::io(format!("{}:{}: {e}", path.display(), i + 1)))?;
        out.push((line, ev));
    }
    Ok(out)
}

/// Move a run directory aside (`<name>.<tag>`) so a fresh run starts clean.
pub fn move_aside(run_dir: &Path, tag: &str) -> std::io::Result<Option<PathBuf>> {
    if !run_dir.exists() {
        return Ok(None);
    }
    let name = name_of(run_dir);
    let mut target = run_dir.with_file_name(format!("{name}.{tag}"));
    let mut k = 1;
    while target.exists() {
        target = run_dir.with_file_name(format!("{name}.{tag}-{k}"));
        k += 1;
    }
    std::fs::rename(run_dir, &target)?;
    Ok(Some(target))
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::rpc::{RunsPageParams, TournamentsPageParams};

    #[test]
    fn verdict_health_counts_held_checks_and_red_line_fails() {
        let v: JudgeVerdict = serde_json::from_value(serde_json::json!({
            "run_id": "r", "judge_tier": "opus", "quality": [],
            "rules": [
                { "rule": "situation:a", "state": "pass" },
                { "rule": "red-line:doxxing", "state": "fail" },
                { "rule": "role:b", "state": "fail" },
                { "rule": "red-line:x", "state": "pass" },
                { "rule": "talk:c", "state": "not_provoked" },
                { "rule": "rule:d", "state": "not_evaluable" }
            ]
        }))
        .unwrap();
        assert_eq!(verdict_health(&v), (Some(0.5), 1));
        let none: JudgeVerdict = serde_json::from_value(serde_json::json!({
            "run_id": "r", "judge_tier": "opus", "quality": [],
            "rules": [{ "rule": "talk:c", "state": "not_provoked" }]
        }))
        .unwrap();
        assert_eq!(verdict_health(&none), (None, 0));
    }

    /// A run directory as the sim writes it. `status` is `run.json`'s status (`None`: no
    /// `run.json`); `transcript` is the JSONL body.
    pub(crate) fn write_run(
        dir: &Path,
        run_id: &str,
        status: Option<&str>,
        judge_error: Option<&str>,
        verdict: bool,
        transcript: &str,
    ) {
        std::fs::create_dir_all(dir).unwrap();
        let spec = serde_json::json!({
            "run_id": run_id, "character": "malakor", "tone_variant": "theatrical",
            "core_tier": "haiku", "core_effort": "low", "segment": "SEG-01",
            "audience_size": 3, "seed": 1, "duration_s": 60, "population": "fixture"
        });
        std::fs::write(dir.join("spec.json"), spec.to_string()).unwrap();
        std::fs::write(dir.join("transcript.jsonl"), transcript).unwrap();
        if let Some(status) = status {
            let mut rec = serde_json::json!({
                "run_id": run_id, "status": status, "instrument_hash": "h",
                "calibration": { "kind": "default_assumption" },
                "tick_s": 5, "ticks": 12, "ended_at_s": 60, "signed_off": false,
                "chat": { "bulk_msgs": 0, "featured_msgs": 0, "vote_msgs": 0,
                          "command_msgs": 0, "featured_calls": 0,
                          "featured_engine_errors": 0, "featured_parse_errors": 0,
                          "silent_ticks": 0 },
                "population": { "size_key": "3", "msgs_per_min": 1.0, "mix": [],
                                "counts": [], "peak_ccv": 3 },
                "featured": [], "injections": []
            });
            if let Some(e) = judge_error {
                rec["judge_error"] = e.into();
            }
            std::fs::write(dir.join("run.json"), rec.to_string()).unwrap();
        }
        if verdict {
            let v = serde_json::json!({
                "run_id": run_id, "judge_tier": "opus", "quality": [],
                "rules": [{ "rule": "situation:a", "state": "pass" }]
            });
            std::fs::write(dir.join("verdict.json"), v.to_string()).unwrap();
        }
    }

    pub(crate) const CLEAN: &str = concat!(
        r#"{"type":"stream_start","ts_ms":0,"stream_id":"s","character":"malakor","tone_variant":"theatrical","core_tier":"haiku","audience_size":3}"#,
        "\n",
        r#"{"type":"stream_end","ts_ms":60000,"reason":"sim_complete"}"#,
        "\n"
    );
    pub(crate) const FLEET_LOST: &str = concat!(
        r#"{"type":"stream_start","ts_ms":0,"stream_id":"s","character":"malakor","tone_variant":"theatrical","core_tier":"haiku","audience_size":3}"#,
        "\n",
        r#"{"type":"op","ts_ms":5000,"op":{"op":"sign_off","reason":"fleet_lost: engine degraded or down for 300s"}}"#,
        "\n",
        r#"{"type":"stream_end","ts_ms":5000,"reason":"sim_complete"}"#,
        "\n"
    );

    #[test]
    fn engine_failed_and_judge_pending_runs_are_not_completed() {
        let tmp = tempfile::tempdir().unwrap();
        let sim = tmp.path();
        let t = sim.join("t1");
        std::fs::create_dir_all(&t).unwrap();
        std::fs::write(t.join("tournament.json"), r#"{"base_hash":"b","runs":8}"#).unwrap();
        let cooling = "judge engine call failed: engine unavailable: all accounts cooling";
        write_run(
            &t.join("a-judged"),
            "a-judged",
            Some("judged"),
            None,
            true,
            CLEAN,
        );
        write_run(
            &t.join("b-sim"),
            "b-sim",
            Some("simulated"),
            None,
            false,
            CLEAN,
        );
        write_run(
            &t.join("c-void"),
            "c-void",
            Some("judge_void"),
            Some("judge reply is not the rubric JSON"),
            false,
            CLEAN,
        );
        // Pre-WP13: a judge_void whose judge call failed on the engine is judge_pending.
        write_run(
            &t.join("d-pend"),
            "d-pend",
            Some("judge_void"),
            Some(cooling),
            false,
            CLEAN,
        );
        write_run(
            &t.join("e-pend"),
            "e-pend",
            Some("judge_pending"),
            None,
            false,
            CLEAN,
        );
        write_run(
            &t.join("f-ef"),
            "f-ef",
            Some("engine_failed"),
            None,
            false,
            CLEAN,
        );
        // Pre-WP13: status `simulated`, but the transcript holds the fleet-loss sign-off.
        write_run(
            &t.join("g-legacy"),
            "g-legacy",
            Some("simulated"),
            None,
            false,
            FLEET_LOST,
        );
        write_run(&t.join("h-running"), "h-running", None, None, false, CLEAN);
        // A moved-aside copy is history: never a row, never completed.
        write_run(
            &t.join("a-judged.engine-failed"),
            "a-judged",
            Some("judged"),
            None,
            true,
            CLEAN,
        );

        let row = tournament_row(sim, "t1").unwrap();
        assert_eq!((row.runs, row.completed), (8, 2), "{row:?}");
        let page = crate::paging::tournaments_page(
            sim,
            TournamentsPageParams {
                cursor: None,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(page.items[0].completed, 2);

        let rows = crate::paging::runs_page(
            sim,
            RunsPageParams {
                tournament_id: Some("t1".into()),
                filter: None,
                cursor: None,
                limit: 50,
            },
        )
        .unwrap()
        .items;
        assert_eq!(rows.len(), 8, "the copy is not a row");
        for r in &rows {
            let judged = r.run_id == "a-judged";
            assert_eq!(r.has_verdict, judged, "{}", r.run_id);
            assert_eq!(r.health.is_some(), judged, "{}", r.run_id);
        }
    }

    /// A verdict of `pass` held and `fail` failed checks, `red` of the fails on red lines.
    fn write_verdict(dir: &Path, run_id: &str, pass: usize, fail: usize, red: usize) {
        let mut rules = Vec::new();
        for i in 0..pass {
            rules.push(serde_json::json!({ "rule": format!("situation:p{i}"), "state": "pass" }));
        }
        for i in 0..fail {
            let rule = if i < red {
                format!("red-line:r{i}")
            } else {
                format!("role:f{i}")
            };
            rules.push(serde_json::json!({ "rule": rule, "state": "fail" }));
        }
        rules.push(serde_json::json!({ "rule": "talk:q", "state": "not_provoked" }));
        let v = serde_json::json!({
            "run_id": run_id, "judge_tier": "opus", "quality": [], "rules": rules
        });
        std::fs::write(dir.join("verdict.json"), v.to_string()).unwrap();
    }

    #[test]
    fn tournament_row_holds_the_mean_of_judged_runs_and_sums_their_red_line_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let sim = tmp.path();
        let t = sim.join("ab");
        std::fs::create_dir_all(&t).unwrap();
        std::fs::write(t.join("tournament.json"), r#"{"base_hash":"b","runs":3}"#).unwrap();
        write_run(&t.join("a"), "a", Some("judged"), None, false, CLEAN);
        write_verdict(&t.join("a"), "a", 1, 0, 0);
        write_run(&t.join("b"), "b", Some("judged"), None, false, CLEAN);
        write_verdict(&t.join("b"), "b", 50, 50, 2);
        write_run(&t.join("c"), "c", Some("simulated"), None, false, CLEAN);
        let row = serde_json::to_value(tournament_row(sim, "ab").unwrap()).unwrap();
        // The mean of the runs' shares (1 and 0.5), not the pooled 51 / 101.
        assert_eq!(row["judged"], serde_json::json!(2), "{row}");
        assert_eq!(row["held"], serde_json::json!(0.75), "{row}");
        assert_eq!(row["red_line_fails"], serde_json::json!(2), "{row}");
        assert_eq!(row["completed"], serde_json::json!(3), "{row}");
        let page = crate::paging::tournaments_page(
            sim,
            TournamentsPageParams {
                cursor: None,
                limit: 10,
            },
        )
        .unwrap();
        let first = serde_json::to_value(&page.items[0]).unwrap();
        assert_eq!(first["held"], serde_json::json!(0.75), "{first}");
    }

    #[test]
    fn a_judged_run_with_no_evaluated_check_is_counted_but_not_held() {
        let tmp = tempfile::tempdir().unwrap();
        let sim = tmp.path();
        let t = sim.join("mixed");
        write_run(&t.join("a"), "a", Some("judged"), None, false, CLEAN);
        write_verdict(&t.join("a"), "a", 3, 1, 0);
        write_run(&t.join("z"), "z", Some("judged"), None, false, CLEAN);
        write_verdict(&t.join("z"), "z", 0, 0, 0);
        let row = serde_json::to_value(tournament_row(sim, "mixed").unwrap()).unwrap();
        assert_eq!(row["judged"], serde_json::json!(2), "{row}");
        assert_eq!(row["held"], serde_json::json!(0.75), "{row}");
        assert!(row.get("red_line_fails").is_none(), "{row}");

        let t = sim.join("empty");
        write_run(&t.join("z"), "z", Some("judged"), None, false, CLEAN);
        write_verdict(&t.join("z"), "z", 0, 0, 0);
        write_run(&t.join("y"), "y", Some("judged"), None, false, CLEAN);
        write_verdict(&t.join("y"), "y", 0, 0, 0);
        let row = serde_json::to_value(tournament_row(sim, "empty").unwrap()).unwrap();
        assert_eq!(row["judged"], serde_json::json!(2), "{row}");
        assert!(row.get("held").is_none(), "{row}");
    }

    #[test]
    fn reconcile_dry_run_counts_without_an_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = crate::config::Config::from_toml("", tmp.path()).unwrap();
        let t = cfg.paths.sim_runs().join("t2");
        write_run(&t.join("a"), "a", Some("judged"), None, true, CLEAN);
        write_run(&t.join("b"), "b", Some("simulated"), None, false, CLEAN);
        write_run(
            &t.join("c"),
            "c",
            Some("simulated"),
            None,
            false,
            FLEET_LOST,
        );
        write_run(&t.join("a.stale-1"), "a", Some("judged"), None, true, CLEAN);
        let r = crate::cli::reconcile_dry_run_report(&cfg, "t2").unwrap();
        assert_eq!((r.loaded, r.judged), (2, 1));
        assert_eq!((r.excluded.engine_failed, r.excluded.copies), (1, 1));
        assert!(!cfg.paths.data.exists(), "a dry run writes nothing");
        assert!(crate::cli::reconcile_dry_run_report(&cfg, "../x").is_err());
    }

    #[test]
    fn ids_are_single_safe_components() {
        assert!(check_id("run_id", "theatrical.haiku.SEG-01.a30.s1").is_ok());
        for bad in ["", ".", "..", "a/b", "a\\b", "c:x", "a b"] {
            assert!(check_id("run_id", bad).is_err(), "{bad}");
        }
    }
}
