//! Loading simulated runs from disk (`runs/sim/<tournament_id>/<run_id>/`), transcript line
//! refs (`<run_id>#<line>`, 1-based JSONL line numbers) and a compact line renderer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use shadowrunr_sim::outcome::{classify, RunClass};
use shadowrunr_types::{JudgeVerdict, Op, SimRunSpec, Speaker, StreamEvent, TsMs};

use crate::types::Source;
use crate::ReconcileError;

/// One run as found on disk. Only `transcript.jsonl` is required.
#[derive(Debug, Clone)]
pub struct RunData {
    pub run_id: String,
    pub tournament_id: Option<String>,
    pub dir: PathBuf,
    pub spec: Option<SimRunSpec>,
    /// Indexed by line - 1. `None` for a blank or unparseable line, so line numbers always
    /// match the file.
    pub events: Vec<Option<StreamEvent>>,
    pub verdict: Option<JudgeVerdict>,
    /// Ledger rows, read leniently: the engine's and the sim's ledgers differ in a few
    /// fields, and only `role`, `duration_ms`, `first_text_ms`, `error_kind` and
    /// `fallback_reason` are used.
    pub ledger: Vec<Value>,
    pub instrument_hash: Option<String>,
}

impl RunData {
    /// The event on a 1-based line.
    pub fn line(&self, line: usize) -> Option<&StreamEvent> {
        line.checked_sub(1)
            .and_then(|i| self.events.get(i))
            .and_then(Option::as_ref)
    }

    /// Numbered events (1-based).
    pub fn numbered(&self) -> impl Iterator<Item = (usize, &StreamEvent)> {
        self.events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|e| (i + 1, e)))
    }

    pub fn tone_variant(&self) -> Option<String> {
        if let Some(spec) = &self.spec {
            return Some(spec.tone_variant.clone());
        }
        self.numbered().find_map(|(_, e)| match e {
            StreamEvent::StreamStart { tone_variant, .. } => Some(tone_variant.clone()),
            _ => None,
        })
    }

    pub fn audience_size(&self) -> Option<u32> {
        self.numbered()
            .find_map(|(_, e)| match e {
                StreamEvent::StreamStart { audience_size, .. } => Some(*audience_size),
                _ => None,
            })
            .or_else(|| self.spec.as_ref().map(|s| s.audience_size))
    }

    pub fn character(&self) -> Option<String> {
        self.spec.as_ref().map(|s| s.character.clone()).or_else(|| {
            self.numbered().find_map(|(_, e)| match e {
                StreamEvent::StreamStart { character, .. } => Some(character.clone()),
                _ => None,
            })
        })
    }

    pub fn line_ref(&self, line: usize) -> String {
        format!("{}#{}", self.run_id, line)
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn io_err(path: &Path, e: std::io::Error) -> ReconcileError {
    ReconcileError::Io(format!("{}: {e}", path.display()))
}

/// Reject ids that could escape the runs root.
pub fn check_id(kind: &str, id: &str) -> Result<(), ReconcileError> {
    let ok = !id.is_empty()
        && id != "."
        && id != ".."
        && !id.contains(['/', '\\', ':'])
        && !id.contains("..");
    if ok {
        Ok(())
    } else {
        Err(ReconcileError::InvalidInput(format!(
            "invalid {kind}: {id:?}"
        )))
    }
}

/// Load one run directory (must contain `transcript.jsonl`).
pub fn load_run(dir: &Path, tournament_id: Option<String>) -> Result<RunData, ReconcileError> {
    let tpath = dir.join("transcript.jsonl");
    let text = std::fs::read_to_string(&tpath).map_err(|e| io_err(&tpath, e))?;
    let events = text
        .lines()
        .map(|l| {
            let l = l.trim();
            if l.is_empty() {
                None
            } else {
                serde_json::from_str::<StreamEvent>(l).ok()
            }
        })
        .collect();
    let run_id = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ledger = std::fs::read_to_string(dir.join("ledger.jsonl"))
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
                .filter(Value::is_object)
                .collect()
        })
        .unwrap_or_default();
    let instrument_hash = read_json::<Value>(&dir.join("instrument.json")).and_then(|v| {
        v.get("instrument_hash")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    Ok(RunData {
        run_id,
        tournament_id,
        dir: dir.to_path_buf(),
        spec: read_json(&dir.join("spec.json")),
        events,
        verdict: read_json(&dir.join("verdict.json")),
        ledger,
        instrument_hash,
    })
}

/// What a tournament directory held that Reconcile did not load (WP13). Only results are
/// analysed; the engine's failures and the moved-aside history are counted here instead.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Excluded {
    /// The engine cut the stream: not a performance of the character (a resume re-runs it).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub engine_failed: u32,
    /// Sound transcript, but the judge call failed on the engine (a resume re-judges it).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub judge_pending: u32,
    /// Moved-aside copies (`<run_id>.stale-*`, `.attempt-*`, `.engine-failed*`): history.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub copies: u32,
    /// Neither `run.json` nor `error.json` yet: still running, or interrupted.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub incomplete: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Excluded {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// `Not loaded (not results): 3 engine_failed, 2 moved-aside copies.`, or `None`.
    pub fn describe(&self) -> Option<String> {
        let parts: Vec<String> = [
            (self.engine_failed, "engine_failed"),
            (self.judge_pending, "judge_pending"),
            (self.copies, "moved-aside copies"),
            (self.incomplete, "incomplete"),
        ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();
        (!parts.is_empty()).then(|| format!("Not loaded (not results): {}.", parts.join(", ")))
    }
}

/// Suffixes a tournament gives the run directories it moves aside.
const COPY_MARKERS: [&str; 3] = [".stale-", ".attempt-", ".engine-failed"];

/// Whether a run directory is a moved-aside copy: its name is not the run id in its
/// `spec.json`, or (without a readable spec) it carries a copy suffix.
fn is_copy(dir: &Path) -> bool {
    let name = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match read_json::<SimRunSpec>(&dir.join("spec.json")) {
        Some(spec) => spec.run_id != name,
        None => COPY_MARKERS.iter().any(|m| name.contains(m)),
    }
}

/// A tournament's run directories that are results, plus what was left out.
fn run_dirs(dir: &Path) -> Result<(Vec<PathBuf>, Excluded), ReconcileError> {
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io_err(dir, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("transcript.jsonl").is_file())
        .collect();
    all.sort();
    let mut excluded = Excluded::default();
    let mut out = Vec::with_capacity(all.len());
    for d in all {
        if is_copy(&d) {
            excluded.copies += 1;
            continue;
        }
        match classify(&d) {
            c if c.is_result() => out.push(d),
            RunClass::EngineFailed => excluded.engine_failed += 1,
            RunClass::JudgePending => excluded.judge_pending += 1,
            _ => excluded.incomplete += 1,
        }
    }
    Ok((out, excluded))
}

/// Load every run a [`Source`] names. `runs_root` is `runs/sim`.
pub fn load_source(runs_root: &Path, source: &Source) -> Result<Vec<RunData>, ReconcileError> {
    load_source_full(runs_root, source).map(|(runs, _)| runs)
}

/// [`load_source`], plus what a tournament source left out. A tournament loads only its
/// results (`shadowrunr_sim::outcome::classify(dir).is_result()`) from live run directories;
/// runs named explicitly (`Source::Runs`) load as asked.
pub fn load_source_full(
    runs_root: &Path,
    source: &Source,
) -> Result<(Vec<RunData>, Excluded), ReconcileError> {
    let mut excluded = Excluded::default();
    let runs = match source {
        Source::Tournament { tournament_id } => {
            check_id("tournament_id", tournament_id)?;
            let dir = runs_root.join(tournament_id);
            if !dir.is_dir() {
                return Err(ReconcileError::NotFound(format!(
                    "tournament {tournament_id}"
                )));
            }
            let (dirs, ex) = run_dirs(&dir)?;
            excluded = ex;
            dirs.iter()
                .map(|d| load_run(d, Some(tournament_id.clone())))
                .collect::<Result<Vec<_>, _>>()?
        }
        Source::Runs { run_ids } => {
            let mut out = Vec::new();
            for id in run_ids {
                check_id("run_id", id)?;
                out.push(find_run(runs_root, id)?);
            }
            out
        }
    };
    if runs.is_empty() {
        return Err(ReconcileError::NotFound(match excluded.describe() {
            Some(d) => format!("no results in source. {d}"),
            None => "no runs in source".into(),
        }));
    }
    let mut seen = std::collections::HashSet::new();
    for r in &runs {
        if !seen.insert(r.run_id.clone()) {
            return Err(ReconcileError::InvalidInput(format!(
                "run {} appears twice",
                r.run_id
            )));
        }
    }
    Ok((runs, excluded))
}

/// Find `<runs_root>/<tournament>/<run_id>/` (or `<runs_root>/<run_id>/`).
fn find_run(runs_root: &Path, run_id: &str) -> Result<RunData, ReconcileError> {
    let direct = runs_root.join(run_id);
    let mut hits: Vec<(PathBuf, Option<String>)> = Vec::new();
    if direct.join("transcript.jsonl").is_file() {
        hits.push((direct, None));
    }
    if let Ok(rd) = std::fs::read_dir(runs_root) {
        let mut tdirs: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        tdirs.sort();
        for t in tdirs {
            let cand = t.join(run_id);
            if cand.join("transcript.jsonl").is_file() {
                let tid = t.file_name().map(|s| s.to_string_lossy().into_owned());
                hits.push((cand, tid));
            }
        }
    }
    match hits.len() {
        0 => Err(ReconcileError::NotFound(format!("run {run_id}"))),
        1 => {
            let (dir, tid) = hits.remove(0);
            load_run(&dir, tid)
        }
        _ => Err(ReconcileError::InvalidInput(format!(
            "run id {run_id} is ambiguous: found in {} places",
            hits.len()
        ))),
    }
}

/// Parse `<run_id>#<line>` (an `L` before the number is tolerated).
pub fn parse_ref(s: &str) -> Option<(&str, usize)> {
    let (run, line) = s.trim().rsplit_once('#')?;
    let line = line.trim();
    let line = line.strip_prefix(['L', 'l']).unwrap_or(line);
    let n: usize = line.parse().ok()?;
    let run = run.trim();
    (!run.is_empty() && n >= 1).then_some((run, n))
}

/// Resolves refs against the loaded runs.
pub struct RefIndex<'a> {
    runs: HashMap<&'a str, &'a RunData>,
}

impl<'a> RefIndex<'a> {
    pub fn new(runs: &'a [RunData]) -> Self {
        Self {
            runs: runs.iter().map(|r| (r.run_id.as_str(), r)).collect(),
        }
    }

    /// `Ok(canonical ref)` when the ref names an existing event line of a run in scope.
    pub fn resolve(&self, r: &str) -> Result<String, String> {
        let Some((run, line)) = parse_ref(r) else {
            return Err("malformed ref (want <run_id>#<line>)".into());
        };
        let Some(data) = self.runs.get(run) else {
            return Err(format!("run {run} is not in this report's source"));
        };
        if data.line(line).is_none() {
            return Err(format!(
                "run {run} has no event on line {line} ({} lines)",
                data.events.len()
            ));
        }
        Ok(data.line_ref(line))
    }
}

pub fn ts(ev: &StreamEvent) -> TsMs {
    match ev {
        StreamEvent::StreamStart { ts_ms, .. }
        | StreamEvent::SegmentStart { ts_ms, .. }
        | StreamEvent::SegmentEnd { ts_ms, .. }
        | StreamEvent::CaseOpen { ts_ms, .. }
        | StreamEvent::Chat { ts_ms, .. }
        | StreamEvent::Picked { ts_ms, .. }
        | StreamEvent::Say { ts_ms, .. }
        | StreamEvent::Op { ts_ms, .. }
        | StreamEvent::Vote { ts_ms, .. }
        | StreamEvent::Moderation { ts_ms, .. }
        | StreamEvent::Handover { ts_ms, .. }
        | StreamEvent::Job { ts_ms, .. }
        | StreamEvent::Health { ts_ms, .. }
        | StreamEvent::StreamEnd { ts_ms, .. } => *ts_ms,
    }
}

/// The serde wire name of a unit enum value (`SEG-01`, `cultist`, `output`...).
pub fn wire<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

/// One-line human/LLM rendering of an event.
pub fn body(ev: &StreamEvent) -> String {
    match ev {
        StreamEvent::StreamStart {
            tone_variant,
            audience_size,
            core_tier,
            ..
        } => format!(
            "stream_start tone={tone_variant} audience={audience_size} core={}",
            wire(core_tier)
        ),
        StreamEvent::SegmentStart { segment, .. } => format!("segment_start {}", wire(segment)),
        StreamEvent::SegmentEnd { segment, ses, .. } => match ses {
            Some(s) => format!("segment_end {} ses={s:.2}", wire(segment)),
            None => format!("segment_end {}", wire(segment)),
        },
        StreamEvent::CaseOpen { case, .. } => format!(
            "case_open {} \"{}\" hexes={} fictional={}",
            case.id,
            case.title,
            case.hexes.len(),
            case.fictional
        ),
        StreamEvent::Chat { msg, .. } => format!(
            "chat [{}] {} ({}): {}",
            msg.id,
            msg.viewer.name,
            wire(&msg.viewer.rank),
            clip(&msg.text, 300)
        ),
        StreamEvent::Picked {
            msg_ids, reason, ..
        } => format!("picked {} ({})", msg_ids.join(","), clip(reason, 120)),
        StreamEvent::Say {
            speaker,
            foil,
            text,
            replying_to,
            ..
        } => {
            let who = match speaker {
                Speaker::Malakor => "MALAKOR".to_string(),
                Speaker::Foil => format!("FOIL {}", foil.as_deref().unwrap_or("?")),
            };
            let to = if replying_to.is_empty() {
                String::new()
            } else {
                format!(" (to {})", replying_to.join(","))
            };
            format!("{who}{to}: {}", clip(text, 400))
        }
        StreamEvent::Op { op, .. } => match op {
            Op::StartPoll {
                question, options, ..
            } => format!("op start_poll {:?} {:?}", clip(question, 120), options),
            Op::ShowCard { title, .. } => format!("op show_card {:?}", clip(title, 120)),
            other => format!(
                "op {}",
                clip(&serde_json::to_string(other).unwrap_or_default(), 160)
            ),
        },
        StreamEvent::Vote {
            poll_id,
            tallies,
            closed,
            ..
        } => {
            let t: Vec<String> = tallies
                .iter()
                .map(|t| format!("{}={}v/{:.1}w", t.option, t.voters, t.weight))
                .collect();
            format!(
                "vote {poll_id} {}{}",
                t.join(" "),
                if *closed { " (closed)" } else { "" }
            )
        }
        StreamEvent::Moderation {
            stage,
            rule,
            msg_id,
            action,
            ..
        } => format!(
            "moderation {} rule={rule} action={action}{}",
            wire(stage),
            msg_id
                .as_ref()
                .map(|m| format!(" msg={m}"))
                .unwrap_or_default()
        ),
        StreamEvent::Handover {
            from, to, reason, ..
        } => format!(
            "handover {} -> {} ({})",
            wire(from),
            wire(to),
            clip(reason, 120)
        ),
        StreamEvent::Job {
            job_id,
            status,
            summary,
            ..
        } => format!(
            "job {job_id} {status}{}",
            summary
                .as_ref()
                .map(|s| format!(": {}", clip(s, 160)))
                .unwrap_or_default()
        ),
        StreamEvent::Health { state, detail, .. } => format!(
            "health {}{}",
            wire(state),
            detail
                .as_ref()
                .map(|d| format!(": {}", clip(d, 160)))
                .unwrap_or_default()
        ),
        StreamEvent::StreamEnd { reason, .. } => format!("stream_end {}", wire(reason)),
    }
}

/// `<run_id>#<line> [mm:ss] <body>`
pub fn render_line(run: &RunData, line: usize, ev: &StreamEvent) -> String {
    let s = ts(ev).max(0) / 1000;
    format!(
        "{} [{:02}:{:02}] {}",
        run.line_ref(line),
        s / 60,
        s % 60,
        body(ev)
    )
}
