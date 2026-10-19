//! The clip factory behind `clips.*` (RPC) and `shadowrunr clips` (CLI): resolving runs,
//! the gate's policy, and the error mapping. The factory itself is `shadowrunr-clips`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use shadowrunr_brain::moderation::{Policy, PolicySource};
use shadowrunr_clips::{ClipError, RunInput, ScanOptions};
use shadowrunr_sim::{classify, RunClass};
use shadowrunr_types::clips::{ClipCandidate, ClipScanReport, ClipScanSource, ClipsScanParams};
use shadowrunr_types::{Engine, ErrorCode};

use crate::config::Config;
use crate::rpc::RpcError;
use crate::runs;

impl From<ClipError> for RpcError {
    fn from(e: ClipError) -> Self {
        let code = match &e {
            ClipError::NotFound(_) => ErrorCode::NotFound,
            ClipError::Invalid(_) => ErrorCode::InvalidParams,
            ClipError::Conflict(_) => ErrorCode::Conflict,
            ClipError::Io(_) => ErrorCode::Io,
        };
        RpcError::new(code, e.to_string())
    }
}

/// The term rules the gate checks with: `<policy>/platform-rules.json` (with the operator's
/// installed lists beside it, as the brain loads them), or the built-in rules when absent.
pub fn policy(cfg: &Config) -> Result<Arc<Policy>, RpcError> {
    let file = cfg.paths.policy.join("platform-rules.json");
    let src = if file.is_file() {
        PolicySource::File(file)
    } else {
        PolicySource::Builtin
    };
    Policy::load(&src, &cfg.paths.characters, &[])
        .map(Arc::new)
        .map_err(|e| RpcError::io(format!("policy: {e}")))
}

/// The transcript as candidates record it: relative to the runs root, `/`-separated.
fn label(cfg: &Config, dir: &Path) -> String {
    let t = dir.join("transcript.jsonl");
    t.strip_prefix(&cfg.paths.runs)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| "transcript.jsonl".into())
}

fn input(cfg: &Config, tournament_id: Option<String>, run_id: String, dir: PathBuf) -> RunInput {
    RunInput {
        transcript_label: label(cfg, &dir),
        tournament_id,
        run_id,
        dir,
    }
}

/// The runs a scan reads. A tournament contributes every run whose transcript the engine did
/// not cut (`engine_failed` runs are left out; moved-aside copies are never runs).
pub fn inputs(cfg: &Config, source: &ClipScanSource) -> Result<Vec<RunInput>, RpcError> {
    let sim_root = cfg.paths.sim_runs();
    match source {
        ClipScanSource::Run {
            run_id,
            tournament_id,
        } => {
            let e = runs::find_run(&sim_root, run_id, tournament_id.as_deref())?;
            if classify(&e.dir) == RunClass::EngineFailed {
                return Err(RpcError::invalid(format!(
                    "run {} is engine_failed: the engine cut its stream",
                    e.row.run_id
                )));
            }
            Ok(vec![input(
                cfg,
                Some(e.row.tournament_id.clone()),
                e.row.run_id.clone(),
                e.dir,
            )])
        }
        ClipScanSource::Tournament { tournament_id } => {
            let rows = runs::list_runs(&sim_root, Some(tournament_id))?;
            Ok(rows
                .into_iter()
                .filter(|e| classify(&e.dir) != RunClass::EngineFailed)
                .map(|e| {
                    input(
                        cfg,
                        Some(e.row.tournament_id.clone()),
                        e.row.run_id.clone(),
                        e.dir,
                    )
                })
                .collect())
        }
    }
}

/// A run named by its directory (`shadowrunr clips scan --run <dir>`): any directory holding
/// a `transcript.jsonl`, read-only. A directory under `<runs>/sim/<tournament>/` keeps its
/// tournament id.
pub fn input_from_dir(cfg: &Config, dir: &Path) -> Result<RunInput, RpcError> {
    if !dir.join("transcript.jsonl").is_file() {
        return Err(RpcError::not_found(format!(
            "{}: no transcript.jsonl",
            dir.display()
        )));
    }
    let run_id = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| RpcError::invalid(format!("{}: not a run directory", dir.display())))?;
    runs::check_id("run_id", &run_id)?;
    let tournament_id = dir
        .parent()
        .filter(|p| p.parent() == Some(cfg.paths.sim_runs().as_path()))
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned());
    Ok(input(cfg, tournament_id, run_id, dir.to_path_buf()))
}

pub fn options(cfg: &Config, hook: bool) -> Result<ScanOptions, RpcError> {
    Ok(ScanOptions {
        hook,
        policy: policy(cfg)?,
        care_window_ms: shadowrunr_brain::care::DEFAULT_CARE_WINDOW_S as i64 * 1000,
    })
}

/// `clips.scan`.
pub async fn scan(
    cfg: &Config,
    engine: Arc<dyn Engine>,
    p: &ClipsScanParams,
) -> Result<ClipScanReport, RpcError> {
    let runs = inputs(cfg, &p.source)?;
    let hook = p.hook.unwrap_or(true);
    let opts = options(cfg, hook)?;
    Ok(shadowrunr_clips::scan(&cfg.clip_store(), &runs, Some(engine.as_ref()), &opts).await?)
}

/// `clips.render`: records the stage when the tools exist; blocking, minutes for a video.
pub fn render(cfg: &Config, id: &str) -> Result<ClipCandidate, RpcError> {
    let tools = cfg.render_tools();
    Ok(shadowrunr_clips::scan::rerender(
        &cfg.clip_store(),
        id,
        Some(&tools),
    )?)
}
