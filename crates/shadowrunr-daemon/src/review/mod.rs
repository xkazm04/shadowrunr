//! Session review for the console's Review page: the session list, one session's full record,
//! the line tagger (`tags.json`) and the operator's feedback log (`<data>/review/feedback.jsonl`).
//!
//! Every read goes to disk (no cache): `review.sessions` reads each session's `session.json`,
//! `verdict.json`, `tags.json` and a light pass of its transcript, plus the feedback log once.
//! File work runs on the blocking pool; tagging awaits the engine and never holds a thread.

pub mod rows;
pub mod store;
pub mod tagger;

#[cfg(test)]
pub(crate) mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use shadowrunr_sim::ledger::LedgerEngine;
use shadowrunr_types::review::{
    FeedbackListParams, FeedbackSetParams, FeedbackTag, LineFeedback, ReviewSession,
    ReviewSessionParams, ReviewSessionRow, ReviewSessionsParams, ReviewTagParams, SessionTags,
};
use shadowrunr_types::sandbox::SandboxReport;
use shadowrunr_types::{Engine, ErrorCode, JudgeVerdict, Role};

use crate::config::Config;
use crate::rpc::RpcError;
use crate::runs;
use crate::server::AppState;

/// The longest note `review.feedback.set` takes, in characters.
pub const MAX_NOTE_CHARS: usize = 2_000;

/// `sbx-<yyyymmdd>-<hhmmss>-<minutes>m`, the sandbox's session id.
pub fn is_session_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("sbx-") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('-').collect();
    let digits = |s: &str, n: Option<usize>| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && n.is_none_or(|n| s.len() == n)
    };
    match parts.as_slice() {
        [date, time, minutes] => {
            digits(date, Some(8))
                && digits(time, Some(6))
                && minutes.strip_suffix('m').is_some_and(|m| digits(m, None))
        }
        _ => false,
    }
}

/// `invalid_params` unless `id` is a sandbox session id (which also rules out traversal).
pub fn check_session_id(id: &str) -> Result<(), RpcError> {
    runs::check_id("session_id", id)?;
    if !is_session_id(id) {
        return Err(RpcError::invalid(format!(
            "session_id {id:?} is not a sandbox session id (sbx-<yyyymmdd>-<hhmmss>-<minutes>m)"
        )));
    }
    Ok(())
}

fn not_found(id: &str) -> RpcError {
    RpcError::not_found(format!("sandbox session {id}"))
}

/// A finished session: its dir and its report. `not_found` without a `session.json`.
fn finished(cfg: &Config, id: &str) -> Result<(PathBuf, SandboxReport), RpcError> {
    check_session_id(id)?;
    let dir = cfg.paths.sandbox_runs().join(id);
    let path = dir.join(rows::SESSION_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(not_found(id)),
        Err(e) => return Err(RpcError::io(format!("{}: {e}", path.display()))),
    };
    let report = serde_json::from_str(&text)
        .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
    Ok((dir, report))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, RpcError> + Send + 'static,
) -> Result<T, RpcError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| RpcError::internal(format!("review task: {e}")))?
}

fn feedback_file(cfg: &Config) -> PathBuf {
    store::feedback_path(&cfg.paths.review_dir())
}

/// The folded feedback log; an unreadable log is logged and folds to nothing.
fn folded_feedback(cfg: &Config) -> Vec<LineFeedback> {
    let path = feedback_file(cfg);
    store::fold(&path)
        .map_err(|e| eprintln!("review: {}: {e}", path.display()))
        .map(|f| f.lines)
        .unwrap_or_default()
}

// ── review.sessions / review.session ───────────────────────────────────

/// Every session dir with a `session.json`, newest first.
pub fn list_sessions(cfg: &Config) -> Vec<ReviewSessionRow> {
    let Ok(entries) = std::fs::read_dir(cfg.paths.sandbox_runs()) else {
        return Vec::new();
    };
    let feedback = folded_feedback(cfg);
    let mut out: Vec<ReviewSessionRow> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| is_session_id(n))
        .filter_map(|id| {
            let (dir, report) = finished(cfg, &id).ok()?;
            rows::session_row(&dir, &report, &feedback)
                .map_err(|e| eprintln!("review: {id}: {}", e.message))
                .ok()
        })
        .collect();
    out.sort_by(|a, b| {
        b.started_ms
            .cmp(&a.started_ms)
            .then_with(|| b.session_id.cmp(&a.session_id))
    });
    out
}

pub async fn sessions(
    state: &Arc<AppState>,
    _p: ReviewSessionsParams,
) -> Result<Vec<ReviewSessionRow>, RpcError> {
    let cfg = state.cfg.clone();
    blocking(move || Ok(list_sessions(&cfg))).await
}

/// One session in full.
pub fn read_session(cfg: &Config, id: &str) -> Result<ReviewSession, RpcError> {
    let (dir, report) = finished(cfg, id)?;
    let feedback: Vec<LineFeedback> = folded_feedback(cfg)
        .into_iter()
        .filter(|f| f.session_id == id)
        .collect();
    let row = rows::session_row(&dir, &report, &feedback)?;
    let events = rows::read_events(&dir)?;
    let last_ts = rows::transcript_facts(&dir)?.last_ts.unwrap_or(0);
    let verdict: Option<JudgeVerdict> = rows::read_optional(&dir.join(rows::VERDICT_FILE));
    let catalog = verdict
        .as_ref()
        .map(|v| rows::catalog(cfg, &dir, v))
        .unwrap_or_default();
    Ok(ReviewSession {
        row,
        segments: rows::segments(&events, last_ts),
        events,
        tags: rows::read_optional(&dir.join(rows::TAGS_FILE)),
        verdict,
        catalog,
        calls: rows::calls(&dir, id)?,
        feedback,
    })
}

pub async fn session(
    state: &Arc<AppState>,
    p: ReviewSessionParams,
) -> Result<ReviewSession, RpcError> {
    check_session_id(&p.session_id)?;
    let cfg = state.cfg.clone();
    blocking(move || read_session(&cfg, &p.session_id)).await
}

// ── review.feedback.* ──────────────────────────────────────────────────

/// The row `review.feedback.set` appends: tags deduplicated in order, the note trimmed (an
/// empty one is absent). Checks the line is one of Malakor's `say` lines.
pub fn feedback_row(
    cfg: &Config,
    p: FeedbackSetParams,
    now_ms: i64,
) -> Result<LineFeedback, RpcError> {
    let (dir, _) = finished(cfg, &p.session_id)?;
    let note = p
        .note
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if note
        .as_ref()
        .is_some_and(|n| n.chars().count() > MAX_NOTE_CHARS)
    {
        return Err(RpcError::invalid(format!(
            "note is longer than {MAX_NOTE_CHARS} characters"
        )));
    }
    let facts = rows::transcript_facts(&dir)?;
    if !facts.malakor_lines.contains(&p.line) {
        return Err(RpcError::invalid(format!(
            "line {} of {} is not a Malakor line",
            p.line, p.session_id
        )));
    }
    let mut tags: Vec<FeedbackTag> = Vec::new();
    for t in p.tags {
        if !tags.contains(&t) {
            tags.push(t);
        }
    }
    Ok(LineFeedback {
        session_id: p.session_id,
        line: p.line,
        tags,
        note,
        at_ms: now_ms,
    })
}

/// Validate and append one row; it is the line's folded state from now on (a clearing row
/// comes back as it was written: no tags, no note).
pub fn set_feedback(cfg: &Config, p: FeedbackSetParams) -> Result<LineFeedback, RpcError> {
    let row = feedback_row(cfg, p, runs::now_ms())?;
    let path = feedback_file(cfg);
    store::append(&path, &row).map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
    Ok(row)
}

pub async fn feedback_set(
    state: &Arc<AppState>,
    p: FeedbackSetParams,
) -> Result<LineFeedback, RpcError> {
    check_session_id(&p.session_id)?;
    let cfg = state.cfg.clone();
    blocking(move || set_feedback(&cfg, p)).await
}

pub async fn feedback_list(
    state: &Arc<AppState>,
    p: FeedbackListParams,
) -> Result<Vec<LineFeedback>, RpcError> {
    if let Some(id) = &p.session_id {
        check_session_id(id)?;
    }
    let cfg = state.cfg.clone();
    blocking(move || {
        let path = feedback_file(&cfg);
        let folded =
            store::fold(&path).map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
        Ok(folded
            .lines
            .into_iter()
            .filter(|f| p.session_id.as_ref().is_none_or(|id| &f.session_id == id))
            .collect())
    })
    .await
}

// ── review.tag ─────────────────────────────────────────────────────────

/// One tagging pass per session at a time: a second caller waits, then finds the first
/// caller's tags.json current.
fn tag_lock(dir: &Path) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let mut g = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    g.entry(dir.to_path_buf()).or_default().clone()
}

/// Write JSON through a synced temp file and a rename.
fn write_durable<T: serde::Serialize>(path: &Path, v: &T) -> std::io::Result<()> {
    use std::io::Write;
    let text = serde_json::to_vec_pretty(v).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&text)?;
        f.write_all(b"\n")?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// `tags.json` when it is current (this tagger's version).
fn current_tags(dir: &Path) -> Option<SessionTags> {
    rows::read_optional::<SessionTags>(&dir.join(rows::TAGS_FILE))
        .filter(|t| t.tagger_version == tagger::TAGGER_VERSION)
}

/// `review.tag` against an explicit engine. Every call lands in the session's `ledger.jsonl`
/// (correlation `tag:<session_id>`). When every batch failed at the engine, nothing is
/// written and the call fails with `engine`.
pub async fn tag_with(
    cfg: Arc<Config>,
    engine: Arc<dyn Engine>,
    p: ReviewTagParams,
) -> Result<SessionTags, RpcError> {
    let id = p.session_id.clone();
    let (dir, _) = {
        let cfg = cfg.clone();
        let id = id.clone();
        blocking(move || finished(&cfg, &id)).await?
    };
    let lock = tag_lock(&dir);
    let _held = lock.lock().await;
    if !p.force {
        if let Some(t) = current_tags(&dir) {
            return Ok(t);
        }
    }
    let events = {
        let dir = dir.clone();
        blocking(move || rows::read_events(&dir)).await?
    };
    let ledgered: Arc<dyn Engine> = Arc::new(
        LedgerEngine::create(engine, &dir.join(rows::LEDGER_FILE), &format!("tag:{id}"))
            .map_err(|e| RpcError::io(e.to_string()))?,
    );
    let (tier, effort, timeout_ms) = cfg.engine.route(Role::Tagger);
    let route = tagger::Route {
        tier,
        effort,
        timeout_ms,
    };
    let done = tagger::tag_events(ledgered, route, &id, &events, runs::now_ms()).await;
    if done.engine_down {
        return Err(RpcError::new(
            ErrorCode::Engine,
            format!(
                "tagging {id}: every batch failed at the engine ({} calls)",
                done.calls
            ),
        ));
    }
    let tags = done.tags;
    let path = dir.join(rows::TAGS_FILE);
    {
        let (path, tags) = (path.clone(), tags.clone());
        blocking(move || {
            write_durable(&path, &tags)
                .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))
        })
        .await?;
    }
    eprintln!(
        "review: tagged {id}: {} lines, {} untagged, {} calls",
        tags.lines.len(),
        tags.untagged,
        done.calls
    );
    Ok(tags)
}

pub async fn tag(state: &Arc<AppState>, p: ReviewTagParams) -> Result<SessionTags, RpcError> {
    check_session_id(&p.session_id)?;
    tag_with(state.cfg.clone(), state.engine.engine.clone(), p).await
}

/// After a session is judged: tag it in the background. The session's state never waits on
/// it, and a failure is only logged.
pub fn tag_after_judge(state: Arc<AppState>, session_id: String) {
    tokio::spawn(async move {
        let p = ReviewTagParams {
            session_id: session_id.clone(),
            force: false,
        };
        if let Err(e) = tag(&state, p).await {
            eprintln!("review: tagging {session_id} failed: {}", e.message);
        }
    });
}
