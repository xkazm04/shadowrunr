//! The on-disk index of simulation output: `<runs>/sim/<tournament_id>/<run_id>/`.
//!
//! Only a run's live directory counts (its name equals `spec.run_id`); directories moved aside
//! by a tournament (`<run_id>.stale-<hash>`, `<run_id>.attempt-<ts>`) are history.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use shadowrunr_sim::instrument::Instrument;
use shadowrunr_sim::{RunRecord, RunStatus};
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
    /// still counts as present (`has_verdict`), just without health.
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

fn finished(dir: &Path) -> bool {
    match read_json::<RunRecord>(&dir.join("run.json")) {
        Some(r) => r.status != RunStatus::JudgeVoid,
        None => false,
    }
}

/// One `tournament.list` row for `<sim_root>/<tournament_id>/`, or `None` when the directory
/// is neither a tournament nor holds runs. The instrument hash is the base hash from
/// `tournament.json`; a directory without one (the ad-hoc directory) reports the runs' common
/// base hash, or `mixed`. `completed` counts runs that finished (simulated, or judged when
/// judging was on); a void judge is not complete.
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
    Some(TournamentRow {
        completed: runs.iter().filter(|r| finished(&r.dir)).count(),
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

    #[test]
    fn ids_are_single_safe_components() {
        assert!(check_id("run_id", "theatrical.haiku.SEG-01.a30.s1").is_ok());
        for bad in ["", ".", "..", "a/b", "a\\b", "c:x", "a b"] {
            assert!(check_id("run_id", bad).is_err(), "{bad}");
        }
    }
}
