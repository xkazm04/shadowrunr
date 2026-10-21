//! `sandbox.start` / `sandbox.stop` / `sandbox.status`: the daemon hosts a wall-clock
//! session ([`shadowrunr_sim::sandbox::run_sandbox`]), broadcasts its events on `/events`,
//! records stage acks and judges the session when it ends.
//!
//! A session registers a [`LiveRun`] under its session id, so `/events` serves it as
//! `{"subscribe": "live"}` or `{"subscribe": "<session_id>"}`. The session's event sink pushes
//! each event into that run as it happens (no transcript tailing). When the run returns, the
//! daemon completes the report's `stage_ack` check from the [`AckBook`] (after up to
//! [`ACK_GRACE`] for the stage's final ack), runs the judge unless `judge` is false while the
//! voice plays out, fills the watched checks it can from `voice.jsonl`
//! ([`crate::record::watch`]), and rewrites `session.json`. A session that fails leaves
//! `error.json` instead.
//!
//! One session at a time, and never beside a live `sim.start` run (`conflict`).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures::future::LocalBoxFuture;
use serde::Serialize;
use shadowrunr_sim::sandbox::{judge_session, run_sandbox, EventSink, SandboxProgress, SandboxRun};
use shadowrunr_sim::{SimError, SimOptions};
use shadowrunr_types::sandbox::{
    SandboxChaos, SandboxCheck, SandboxCheckState, SandboxIdParams, SandboxJudge, SandboxReport,
    SandboxStartParams, SandboxStarted, SandboxState, SandboxStatus, SandboxStatusParams,
    SANDBOX_MINUTES,
};
use shadowrunr_types::{Engine, ErrorCode, StreamEvent};
use tokio::sync::watch;

use crate::config::{Config, FaceVariant};
use crate::events::{AckBook, AckRecord};
use crate::live::{self, Frame, LiveRun};
use crate::rpc::RpcError;
use crate::server::AppState;
use crate::{catalog, runs};

/// The stage's dev server (`pnpm -C apps/stage dev`). `localhost`, not `127.0.0.1`: the
/// daemon only accepts browser origins on `http://localhost:*`.
pub const STAGE_BASE_URL: &str = "http://localhost:5178/";

/// The port the stage connects to when its URL names no `ws`.
const STAGE_DEFAULT_DAEMON_PORT: u16 = 7717;

/// How long a finished session waits for a subscriber's final ack.
pub const ACK_GRACE: Duration = Duration::from_secs(6);

const ACK_POLL: Duration = Duration::from_millis(100);

/// The stage URL that follows the live session in broadcast mode, pointed at `daemon_port`,
/// drawing the face with `avatar` (`[stage] avatar`).
pub fn stage_url(daemon_port: u16, avatar: FaceVariant) -> String {
    let mut url = format!("{STAGE_BASE_URL}?run=live&broadcast=1");
    if daemon_port != STAGE_DEFAULT_DAEMON_PORT {
        url.push_str(&format!("&ws=ws://127.0.0.1:{daemon_port}/events"));
    }
    url.push_str(avatar.query());
    url
}

/// `sbx-<yyyymmdd-hhmmss>-<minutes>m`, in UTC.
pub fn session_id(now_ms: i64, minutes: u32) -> String {
    let secs = now_ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "sbx-{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{minutes}m",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (H. Hinnant's algorithm).
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

// ── hosting seams ───────────────────────────────────────────────────────

/// Runs one session: [`run_sandbox`], or a fake in tests.
pub type RunFn = Arc<
    dyn Fn(
            SandboxRun,
            Arc<dyn Engine>,
            SimOptions,
            EventSink,
            watch::Receiver<bool>,
            watch::Sender<SandboxProgress>,
        ) -> LocalBoxFuture<'static, Result<SandboxReport, SimError>>
        + Send
        + Sync,
>;

/// Judges a finished session dir: [`judge_session`], or a fake in tests.
pub type JudgeFn = Arc<
    dyn Fn(
            PathBuf,
            Arc<dyn Engine>,
            SimOptions,
        ) -> LocalBoxFuture<'static, Result<SandboxJudge, SimError>>
        + Send
        + Sync,
>;

/// How a session is hosted. [`Hosting::default`] is the real driver and judge; tests shorten
/// the session (`planned_s`) or replace either.
#[derive(Clone)]
pub struct Hosting {
    pub planned_s: Option<u32>,
    pub ack_grace: Duration,
    pub run: RunFn,
    pub judge: JudgeFn,
}

impl Default for Hosting {
    fn default() -> Self {
        Self {
            planned_s: None,
            ack_grace: ACK_GRACE,
            run: Arc::new(|run, engine, opts, sink, stop, progress| {
                Box::pin(async move { run_sandbox(run, engine, &opts, sink, stop, progress).await })
            }),
            judge: Arc::new(|dir, engine, opts| {
                Box::pin(async move { judge_session(&dir, engine, &opts).await })
            }),
        }
    }
}

// ── the registry ────────────────────────────────────────────────────────

struct Phase {
    state: SandboxState,
    report: Option<SandboxReport>,
}

/// One hosted session.
pub struct Session {
    pub id: String,
    pub dir: PathBuf,
    planned_s: u32,
    stop: watch::Sender<bool>,
    progress: watch::Receiver<SandboxProgress>,
    phase: Mutex<Phase>,
}

impl Session {
    fn phase(&self) -> MutexGuard<'_, Phase> {
        self.phase.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn set_state(&self, state: SandboxState) {
        self.phase().state = state;
    }

    fn end(&self, state: SandboxState, report: Option<SandboxReport>) {
        let mut p = self.phase();
        p.state = state;
        p.report = report;
    }

    fn ended(&self) -> bool {
        is_ended(self.phase().state)
    }

    fn status(&self, acks: &AckBook) -> SandboxStatus {
        let progress = self.progress.borrow().clone();
        let phase = self.phase();
        let state = match phase.state {
            SandboxState::Running if progress.closing => SandboxState::Closing,
            s => s,
        };
        let mut st = SandboxStatus {
            session_id: self.id.clone(),
            state,
            elapsed_s: progress.elapsed_s,
            planned_s: if progress.planned_s > 0 {
                progress.planned_s
            } else {
                self.planned_s
            },
            events: progress.events,
            max_say_gap_s: progress.max_say_gap_s,
            core_errors: progress.core_errors,
            stage_ack_seq: acks.max_seq(&self.id),
            report: None,
        };
        if let Some(r) = &phase.report {
            // Once ended, `session.json` is the report: the recorder's watched checks are
            // merged into the file after the session ends.
            let on_disk = is_ended(phase.state)
                .then(|| read_report(&self.dir))
                .flatten()
                .filter(|d| d.session_id == r.session_id);
            fold_report(&mut st, on_disk.as_ref().unwrap_or(r));
        }
        st
    }
}

/// `<dir>/session.json`, when it reads.
fn read_report(dir: &Path) -> Option<SandboxReport> {
    std::fs::read_to_string(dir.join("session.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

fn is_ended(s: SandboxState) -> bool {
    matches!(
        s,
        SandboxState::Done | SandboxState::Failed | SandboxState::Stopped
    )
}

/// A finished report's figures over the live ones.
fn fold_report(st: &mut SandboxStatus, r: &SandboxReport) {
    st.elapsed_s = u32::try_from((r.ended_ms - r.started_ms).max(0) / 1000).unwrap_or(u32::MAX);
    st.max_say_gap_s = r.max_say_gap_s;
    st.core_errors = r.core_errors;
    st.report = Some(r.clone());
}

#[derive(Default)]
struct RegistryInner {
    sessions: HashMap<String, Arc<Session>>,
    latest: Option<String>,
}

/// Sessions hosted since the daemon started.
#[derive(Default)]
pub struct SandboxRegistry {
    inner: Mutex<RegistryInner>,
}

impl SandboxRegistry {
    fn lock(&self) -> MutexGuard<'_, RegistryInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The session running, closing or judging now.
    pub fn running(&self) -> Option<String> {
        let g = self.lock();
        let id = g.latest.as_ref()?;
        g.sessions
            .get(id)
            .filter(|s| !s.ended())
            .map(|s| s.id.clone())
    }

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        self.lock().sessions.get(id).cloned()
    }

    pub fn latest(&self) -> Option<Arc<Session>> {
        let g = self.lock();
        g.latest.as_ref().and_then(|id| g.sessions.get(id).cloned())
    }
}

// ── the RPCs ────────────────────────────────────────────────────────────

fn validate(p: &SandboxStartParams) -> Result<(), RpcError> {
    if !SANDBOX_MINUTES.contains(&p.minutes) {
        return Err(RpcError::invalid(format!(
            "minutes must be 5, 10 or 30, got {}",
            p.minutes
        )));
    }
    if p.chaos == Some(SandboxChaos::Lost) && p.minutes < 10 {
        return Err(RpcError::invalid(
            "chaos \"lost\" needs a 10 or 30 minute session",
        ));
    }
    if let Some(t) = &p.tone {
        runs::check_id("tone", t)?;
    }
    Ok(())
}

/// The character's active tone, and a given tone checked against its tones.
fn tone_for(cfg: &Config, given: Option<&str>) -> Result<String, RpcError> {
    let summary = catalog::character_list(cfg)
        .into_iter()
        .find(|c| c.id == cfg.sim.character);
    match (given, summary) {
        (Some(t), Some(c)) if !c.tones.iter().any(|x| x == t) => Err(RpcError::invalid(format!(
            "character {} has no tone {t:?}",
            c.id
        ))),
        (Some(t), _) => Ok(t.to_string()),
        (None, Some(c)) => Ok(c.active_tone),
        (None, None) => Ok(cfg.sim.tone.clone()),
    }
}

fn conflict(m: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::Conflict, m)
}

pub async fn start(
    state: Arc<AppState>,
    p: SandboxStartParams,
) -> Result<SandboxStarted, RpcError> {
    start_with(state, p, Hosting::default()).await
}

/// `sandbox.start` with an explicit [`Hosting`].
pub async fn start_with(
    state: Arc<AppState>,
    p: SandboxStartParams,
    hosting: Hosting,
) -> Result<SandboxStarted, RpcError> {
    validate(&p)?;
    let default_tone = tone_for(&state.cfg, p.tone.as_deref())?;
    live::refuse_if_red(&state).await?;

    let session_id = session_id(runs::now_ms(), p.minutes);
    let dir = state.cfg.paths.sandbox_runs().join(&session_id);
    let planned_s = hosting.planned_s.unwrap_or(p.minutes * 60);
    let (stop_tx, stop_rx) = watch::channel(false);
    let (progress_tx, progress_rx) = watch::channel(SandboxProgress {
        planned_s,
        ..SandboxProgress::default()
    });
    let session = Arc::new(Session {
        id: session_id.clone(),
        dir: dir.clone(),
        planned_s,
        stop: stop_tx,
        progress: progress_rx,
        phase: Mutex::new(Phase {
            state: SandboxState::Running,
            report: None,
        }),
    });

    // Claim the slot: the checks and the registrations happen under the registry lock.
    let live_run = {
        let mut reg = state.sandbox.lock();
        if let Some(running) = reg
            .latest
            .as_ref()
            .and_then(|id| reg.sessions.get(id))
            .filter(|s| !s.ended())
        {
            return Err(conflict(format!(
                "sandbox session {} is running",
                running.id
            )));
        }
        if state.live.any_in_flight() {
            return Err(conflict("a live sim run is in flight"));
        }
        let live_run = state.live.register(&session_id, usize::MAX)?;
        reg.sessions.insert(session_id.clone(), session.clone());
        reg.latest = Some(session_id.clone());
        live_run
    };
    let made = runs::move_aside(&dir, &format!("attempt-{}", runs::now_ms()))
        .and_then(|_| std::fs::create_dir_all(&dir));
    if let Err(e) = made {
        let e = RpcError::io(format!("{}: {e}", dir.display()));
        state.live.remove(&session_id);
        live_run.finish(Some(e.clone()));
        session.end(SandboxState::Failed, None);
        return Err(e);
    }

    // The session's `voice.jsonl`: each event's arrival dates the stream clock until the
    // report gives it exactly.
    let voice_log = state.voice.logs().open(&session_id, &dir);
    let sink: EventSink = {
        let live_run = live_run.clone();
        let voice_log = voice_log.clone();
        Arc::new(move |seq: u64, ev: &StreamEvent| {
            voice_log.observe(shadowrunr_sim::transcript::ts(ev));
            let Ok(text) = serde_json::to_string(ev) else {
                return;
            };
            let pushed = live_run.push(Frame::from(text));
            if pushed != seq {
                eprintln!(
                    "sandbox {}: event seq {seq} broadcast as {pushed}",
                    live_run.run_id
                );
            }
        })
    };
    let run = SandboxRun {
        session_id: session_id.clone(),
        params: p.clone(),
        dir: dir.clone(),
        core_tier: state.cfg.sim.tier,
        default_tone,
        planned_s: hosting.planned_s,
    };
    let mut opts = state.cfg.sim_options(p.judge);
    opts.data_dir = state.cfg.paths.data.clone();
    // The newest approved prep, else the fictional sample (the session is a simulated room);
    // spec.json records which.
    opts.stream_prep = crate::prep::for_sim(&state.cfg, &state.cfg.sim.character);
    let engine = state.engine.engine.clone();

    let handle = tokio::runtime::Handle::current();
    let task = {
        let (state, session, live_run) = (state.clone(), session.clone(), live_run.clone());
        tokio::task::spawn_blocking(move || {
            handle.block_on(host(
                state,
                session,
                live_run,
                Hosted {
                    run,
                    engine,
                    opts,
                    sink,
                    stop: stop_rx,
                    progress: progress_tx,
                    hosting,
                },
            ))
        })
    };
    {
        let (state, session) = (state.clone(), session.clone());
        tokio::spawn(async move {
            if let Err(e) = task.await {
                let msg = format!("sandbox task: {e}");
                eprintln!("sandbox {}: {msg}", session.id);
                write_error(&session.dir, &msg);
                state.live.remove(&session.id);
                live_run.finish(Some(RpcError::internal(msg)));
                session.end(SandboxState::Failed, None);
            }
        });
    }
    eprintln!("sandbox.start {session_id} -> {}", dir.display());
    Ok(SandboxStarted {
        session_id,
        dir: dir.display().to_string(),
        stage_url: stage_url(state.cfg.daemon.port, state.cfg.stage.avatar),
    })
}

/// What [`host`] runs.
struct Hosted {
    run: SandboxRun,
    engine: Arc<dyn Engine>,
    opts: SimOptions,
    sink: EventSink,
    stop: watch::Receiver<bool>,
    progress: watch::Sender<SandboxProgress>,
    hosting: Hosting,
}

/// One session from its run to its final state.
async fn host(state: Arc<AppState>, session: Arc<Session>, live_run: Arc<LiveRun>, h: Hosted) {
    let dir = h.run.dir.clone();
    let judge = h.run.params.judge;
    let result = (h.hosting.run)(
        h.run,
        h.engine.clone(),
        h.opts.clone(),
        h.sink,
        h.stop,
        h.progress,
    )
    .await;
    let stopped = *session.stop.borrow();
    let mut report = match result {
        Ok(r) => {
            if let Some(log) = state.voice.logs().get(&session.id) {
                log.started(r.started_ms);
            }
            r
        }
        Err(e) => {
            let msg = e.to_string();
            eprintln!("sandbox {}: {msg}", session.id);
            write_error(&dir, &msg);
            state.live.remove(&session.id);
            live_run.finish(Some(
                RpcError::new(ErrorCode::Engine, msg)
                    .with_detail(serde_json::json!({ "reason": "sandbox_failed" })),
            ));
            session.end(SandboxState::Failed, None);
            return;
        }
    };

    let final_seq = live_run.len();
    wait_for_ack(&state.acks, &live_run, final_seq, h.hosting.ack_grace).await;
    let (ack_state, ack_detail) = stage_ack_check(&state.acks.for_run(&session.id), final_seq);
    set_check(&mut report, "stage_ack", ack_state, ack_detail);
    state.live.remove(&session.id);
    live_run.finish(None);

    if judge {
        session.set_state(SandboxState::Judging);
    }
    // The voice runs seconds behind the events: its last lines play out (logged in
    // voice.jsonl) while the judge reads the transcript.
    let judging = async {
        if judge {
            Some((h.hosting.judge)(dir.clone(), h.engine, h.opts).await)
        } else {
            None
        }
    };
    let (judged, _) = tokio::join!(judging, crate::record::wait_for_voice(&dir));
    if let Some(judged) = judged {
        match judged {
            Ok(j) => {
                let st = if j.rule_fails == 0 {
                    SandboxCheckState::Pass
                } else {
                    SandboxCheckState::Fail
                };
                set_check(
                    &mut report,
                    "judge",
                    st,
                    format!("{} rule fails ({})", j.rule_fails, j.verdict_path),
                );
                report.judge = Some(j);
            }
            Err(e) => set_check(&mut report, "judge", SandboxCheckState::Fail, e.to_string()),
        }
    } else {
        set_check(
            &mut report,
            "judge",
            SandboxCheckState::Unmeasured,
            "skipped".into(),
        );
    }
    // The watched checks from voice.jsonl; the recording is not in the run dir yet, so
    // `stream sandbox --record` measures it once it is.
    if let Some(log) = state.voice.logs().get(&session.id) {
        log.sync();
    }
    let watched = crate::record::watch::measure(&dir, &report, &state.cfg.clips.ffmpeg);
    crate::record::watch::merge(&mut report, watched);

    if let Err(e) = write_durable(&dir.join("session.json"), &report) {
        let msg = format!("write session.json: {e}");
        eprintln!("sandbox {}: {msg}", session.id);
        write_error(&dir, &msg);
        session.end(SandboxState::Failed, Some(report));
        return;
    }
    let end = if stopped {
        SandboxState::Stopped
    } else {
        SandboxState::Done
    };
    session.end(end, Some(report));
    // A judged session's lines are tagged in the background, after the slot is free; a
    // tagger failure is only logged.
    if judge {
        crate::review::tag_after_judge(state, session.id.clone());
    }
}

/// Wait, up to `grace`, for a subscriber to ack `final_seq` with no gaps. Nothing to wait for
/// when nobody acked and nobody is attached.
async fn wait_for_ack(acks: &AckBook, live_run: &LiveRun, final_seq: u64, grace: Duration) {
    let deadline = tokio::time::Instant::now() + grace;
    loop {
        let recs = acks.for_run(&live_run.run_id);
        if recs.is_empty() && live_run.subscribers() == 0 {
            return;
        }
        if stage_ack_check(&recs, final_seq).0 == SandboxCheckState::Pass
            || tokio::time::Instant::now() >= deadline
        {
            return;
        }
        tokio::time::sleep(ACK_POLL).await;
    }
}

/// `stage_ack`: pass when some subscriber acked the final seq with no gaps; fail when acks
/// fall short or show gaps; unmeasured when nobody acked.
pub fn stage_ack_check(recs: &[AckRecord], final_seq: u64) -> (SandboxCheckState, String) {
    if recs.is_empty() {
        return (
            SandboxCheckState::Unmeasured,
            "no stage subscriber acked".into(),
        );
    }
    if let Some(r) = recs.iter().find(|r| r.seq >= final_seq && r.gaps == 0) {
        return (
            SandboxCheckState::Pass,
            format!("acked seq {}/{final_seq}, 0 gaps", r.seq),
        );
    }
    let best = recs
        .iter()
        .max_by_key(|r| (r.seq, std::cmp::Reverse(r.gaps)))
        .copied()
        .unwrap_or(recs[0]);
    (
        SandboxCheckState::Fail,
        format!(
            "best ack seq {}/{final_seq}, {} gaps, {} subscribers",
            best.seq,
            best.gaps,
            recs.len()
        ),
    )
}

/// Replace check `id` (or append it).
fn set_check(report: &mut SandboxReport, id: &str, state: SandboxCheckState, detail: String) {
    let c = SandboxCheck {
        id: id.to_string(),
        state,
        detail,
    };
    match report.checks.iter_mut().find(|c| c.id == id) {
        Some(slot) => *slot = c,
        None => report.checks.push(c),
    }
}

/// Write JSON through a synced temp file and a rename, so a crash leaves the old file or the
/// new one, never half of either.
pub(crate) fn write_durable<T: Serialize>(path: &Path, v: &T) -> std::io::Result<()> {
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

/// `error.json`: why a session failed (best effort).
fn write_error(dir: &Path, msg: &str) {
    let body = serde_json::json!({ "error": msg, "ts_ms": runs::now_ms() });
    if let Err(e) = write_durable(&dir.join("error.json"), &body) {
        eprintln!("{}: error.json: {e}", dir.display());
    }
}

fn not_found(id: &str) -> RpcError {
    RpcError::not_found(format!("sandbox session {id}"))
}

/// A session hosted before this daemon started: `session.json` is `done`, its absence
/// `failed`. `None` when there is no such directory.
fn status_from_disk(cfg: &Config, id: &str) -> Option<SandboxStatus> {
    let dir = cfg.paths.sandbox_runs().join(id);
    if !dir.is_dir() {
        return None;
    }
    let report = read_report(&dir);
    let events = std::fs::read_to_string(dir.join("transcript.jsonl"))
        .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count() as u64)
        .unwrap_or(0);
    let mut st = SandboxStatus {
        session_id: id.to_string(),
        state: if report.is_some() {
            SandboxState::Done
        } else {
            SandboxState::Failed
        },
        elapsed_s: 0,
        planned_s: report.as_ref().map_or(0, |r| r.spec.minutes * 60),
        events,
        max_say_gap_s: 0.0,
        core_errors: 0,
        stage_ack_seq: None,
        report: None,
    };
    if let Some(r) = &report {
        fold_report(&mut st, r);
    }
    Some(st)
}

/// The newest session directory on disk (session ids sort by start time).
fn latest_on_disk(cfg: &Config) -> Option<String> {
    std::fs::read_dir(cfg.paths.sandbox_runs())
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| n.starts_with("sbx-") && !n.contains('.'))
        .max()
}

pub async fn stop(state: &Arc<AppState>, p: SandboxIdParams) -> Result<SandboxStatus, RpcError> {
    runs::check_id("session_id", &p.session_id)?;
    if let Some(s) = state.sandbox.get(&p.session_id) {
        if !s.ended() {
            s.stop.send_replace(true);
        }
        return Ok(s.status(&state.acks));
    }
    status_from_disk(&state.cfg, &p.session_id).ok_or_else(|| not_found(&p.session_id))
}

pub async fn status(
    state: &Arc<AppState>,
    p: SandboxStatusParams,
) -> Result<SandboxStatus, RpcError> {
    let id = match p.session_id {
        Some(id) => {
            runs::check_id("session_id", &id)?;
            id
        }
        None => match state.sandbox.latest() {
            Some(s) => return Ok(s.status(&state.acks)),
            None => latest_on_disk(&state.cfg)
                .ok_or_else(|| RpcError::not_found("no sandbox session yet"))?,
        },
    };
    if let Some(s) = state.sandbox.get(&id) {
        return Ok(s.status(&state.acks));
    }
    status_from_disk(&state.cfg, &id).ok_or_else(|| not_found(&id))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::e2e::{repo, test_config};
    use crate::engine::EngineHandle;
    use shadowrunr_types::sandbox::{StageAck, SANDBOX_CHECKS};

    fn rec(seq: u64, gaps: u32) -> AckRecord {
        AckRecord {
            seq,
            gaps,
            max_stalled_ms: 0,
            last_ms: 0,
        }
    }

    #[test]
    fn session_ids_carry_the_utc_start_and_length() {
        // 2026-10-20T12:34:56Z
        assert_eq!(session_id(1_792_499_696_000, 5), "sbx-20261020-123456-5m");
        assert_eq!(session_id(0, 30), "sbx-19700101-000000-30m");
        assert_eq!(session_id(951_782_400_000, 10), "sbx-20000229-000000-10m");
    }

    #[test]
    fn the_stage_url_names_the_daemon_only_off_the_default_port() {
        assert_eq!(
            stage_url(7717, FaceVariant::Glow),
            "http://localhost:5178/?run=live&broadcast=1"
        );
        assert_eq!(
            stage_url(9000, FaceVariant::Glow),
            "http://localhost:5178/?run=live&broadcast=1&ws=ws://127.0.0.1:9000/events"
        );
        assert_eq!(
            stage_url(7717, FaceVariant::Cut),
            "http://localhost:5178/?run=live&broadcast=1&avatar=cut"
        );
        assert_eq!(
            stage_url(9000, FaceVariant::Depth),
            "http://localhost:5178/?run=live&broadcast=1&ws=ws://127.0.0.1:9000/events&avatar=depth"
        );
        let url = stage_url(7717, FaceVariant::Glow);
        let origin = url.trim_end_matches(|c| c != '/').trim_end_matches('/');
        assert!(crate::server::origin_allowed(origin, &[]), "{origin}");
    }

    #[test]
    fn stage_ack_passes_fails_or_is_unmeasured() {
        assert_eq!(stage_ack_check(&[], 40).0, SandboxCheckState::Unmeasured);
        let (st, detail) = stage_ack_check(&[rec(12, 0), rec(40, 0)], 40);
        assert_eq!(st, SandboxCheckState::Pass, "{detail}");
        let (st, detail) = stage_ack_check(&[rec(39, 0)], 40);
        assert_eq!(st, SandboxCheckState::Fail);
        assert!(detail.contains("39/40"), "{detail}");
        let (st, detail) = stage_ack_check(&[rec(40, 2)], 40);
        assert_eq!(st, SandboxCheckState::Fail);
        assert!(detail.contains("2 gaps"), "{detail}");
    }

    #[test]
    fn start_params_are_validated() {
        let p = |m: u32, chaos| SandboxStartParams {
            minutes: m,
            seed: None,
            chaos,
            judge: true,
            tone: None,
            record: false,
        };
        assert!(validate(&p(5, None)).is_ok());
        assert!(validate(&p(10, Some(SandboxChaos::Lost))).is_ok());
        for bad in [
            p(7, None),
            p(0, None),
            p(5, Some(SandboxChaos::Lost)),
            SandboxStartParams {
                tone: Some("../x".into()),
                ..p(5, None)
            },
        ] {
            assert_eq!(validate(&bad).unwrap_err().code, ErrorCode::InvalidParams);
        }
    }

    // ── hosting with fakes ──────────────────────────────────────────────

    pub(crate) fn params(judge: bool) -> SandboxStartParams {
        SandboxStartParams {
            minutes: 5,
            seed: Some(3),
            chaos: None,
            judge,
            tone: None,
            record: false,
        }
    }

    fn sim_checks(spec: &SandboxStartParams) -> Vec<SandboxCheck> {
        SANDBOX_CHECKS
            .iter()
            .map(|id| SandboxCheck {
                id: id.to_string(),
                state: if matches!(*id, "stage_ack" | "judge")
                    || shadowrunr_types::sandbox::SANDBOX_WATCHED_CHECKS.contains(id)
                {
                    SandboxCheckState::Unmeasured
                } else {
                    SandboxCheckState::Pass
                },
                detail: format!("fake {} minutes", spec.minutes),
            })
            .collect()
    }

    /// Emits `events` events, then waits until `hold` passes or a stop; `ack` makes a fake
    /// stage ack the final seq on `acks`.
    pub(crate) fn fake_run(
        events: u64,
        hold: Duration,
        ack: Option<(Arc<AppState>, u32)>,
        fail: bool,
    ) -> RunFn {
        Arc::new(move |run, _engine, _opts, sink, mut stop, progress| {
            let ack = ack.clone();
            Box::pin(async move {
                for seq in 1..=events {
                    let ev = if seq == events {
                        StreamEvent::StreamEnd {
                            ts_ms: seq as i64,
                            reason: shadowrunr_types::EndReason::Scheduled,
                        }
                    } else {
                        StreamEvent::Health {
                            ts_ms: seq as i64,
                            state: shadowrunr_types::FleetHealth::Healthy,
                            detail: None,
                        }
                    };
                    sink(seq, &ev);
                    progress.send_modify(|p| p.events = seq);
                }
                if fail {
                    return Err(SimError::Spec("fake engine fell over".into()));
                }
                let _ = tokio::time::timeout(hold, stop.wait_for(|s| *s)).await;
                progress.send_modify(|p| p.closing = true);
                if let Some((state, gaps)) = ack {
                    let conn = state.acks.connection();
                    state.acks.record(
                        &run.session_id,
                        conn,
                        StageAck {
                            seq: events,
                            gaps,
                            stalled_ms: 0,
                        },
                        1,
                    );
                }
                Ok(SandboxReport {
                    session_id: run.session_id.clone(),
                    spec: run.params.clone(),
                    started_ms: 1_000,
                    ended_ms: 61_000,
                    pass: false,
                    checks: sim_checks(&run.params),
                    max_say_gap_s: 4.5,
                    say_gap_p95_s: 3.0,
                    beat_ms_p50: 900,
                    beat_ms_p95: 2_000,
                    core_errors: 0,
                    stall_lines: 0,
                    health_transitions: 0,
                    judge: None,
                })
            })
        })
    }

    pub(crate) fn fake_judge(rule_fails: Option<u32>) -> JudgeFn {
        Arc::new(move |_dir, _engine, _opts| {
            Box::pin(async move {
                match rule_fails {
                    Some(rule_fails) => Ok(SandboxJudge {
                        verdict_path: "verdict.json".into(),
                        rule_fails,
                        quality_mean: Some(3.5),
                    }),
                    None => Err(SimError::Spec("judge timed out".into())),
                }
            })
        })
    }

    pub(crate) fn hosting(run: RunFn, judge: JudgeFn) -> Hosting {
        Hosting {
            planned_s: Some(60),
            ack_grace: Duration::from_millis(300),
            run,
            judge,
        }
    }

    fn state(tmp: &Path) -> Arc<AppState> {
        AppState::new(
            test_config(tmp, repo().join("characters")),
            EngineHandle::scripted(),
        )
    }

    async fn until_ended(state: &Arc<AppState>, id: &str) -> SandboxStatus {
        let p = SandboxStatusParams {
            session_id: Some(id.to_string()),
        };
        for _ in 0..200 {
            let st = status(state, p.clone()).await.unwrap();
            if is_ended(st.state) {
                return st;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("session {id} did not end");
    }

    #[tokio::test]
    async fn a_session_with_an_acking_stage_and_a_clean_judge_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        let h = hosting(
            fake_run(5, Duration::ZERO, Some((st.clone(), 0)), false),
            fake_judge(Some(0)),
        );
        let started = start_with(st.clone(), params(true), h).await.unwrap();
        assert!(started.session_id.starts_with("sbx-"));
        assert!(started.session_id.ends_with("-5m"));
        assert_eq!(
            started.stage_url,
            "http://localhost:5178/?run=live&broadcast=1"
        );
        let end = until_ended(&st, &started.session_id).await;
        assert_eq!(end.state, SandboxState::Done);
        assert_eq!(end.stage_ack_seq, Some(5));
        let report = end.report.unwrap();
        assert!(report.pass, "{:?}", report.checks);
        let ids: Vec<&str> = report.checks.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, SANDBOX_CHECKS);
        assert_eq!(report.judge.as_ref().unwrap().rule_fails, 0);
        assert_eq!(end.elapsed_s, 60);

        // session.json holds the completed report; the live run is gone.
        let disk: SandboxReport = serde_json::from_str(
            &std::fs::read_to_string(Path::new(&started.dir).join("session.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(disk, report);
        assert!(st.live.get(&started.session_id).is_none());
        assert!(st.sandbox.running().is_none());
        let latest = status(&st, SandboxStatusParams::default()).await.unwrap();
        assert_eq!(latest.session_id, started.session_id);
    }

    #[tokio::test]
    async fn a_recorded_session_is_gated_by_the_watched_checks_the_daemon_fills_from_voice_jsonl() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        let inner = fake_run(5, Duration::ZERO, Some((st.clone(), 0)), false);
        // The run leaves its transcript and the voice's log: two lines, both played, the rig
        // drawn from the start.
        let run: RunFn = Arc::new(move |run, engine, opts, sink, stop, progress| {
            let dir = run.dir.clone();
            let fut = inner(run, engine, opts, sink, stop, progress);
            Box::pin(async move {
                let say = |t: i64| {
                    format!(r#"{{"type":"say","ts_ms":{t},"speaker":"malakor","text":"Kneel."}}"#)
                };
                std::fs::write(
                    dir.join("transcript.jsonl"),
                    [say(900), say(5_000)].join(
                        "
",
                    ),
                )
                .unwrap();
                let voice = [
                    r#"{"ts_ms":0,"seq":0,"event":"face","reason":"rig"}"#,
                    r#"{"ts_ms":950,"seq":1,"event":"rendered","part":0,"duration_ms":900}"#,
                    r#"{"ts_ms":990,"seq":1,"event":"played","part":0}"#,
                    r#"{"ts_ms":5050,"seq":2,"event":"rendered","part":0,"duration_ms":900}"#,
                    r#"{"ts_ms":5090,"seq":2,"event":"played","part":0}"#,
                ];
                std::fs::write(
                    dir.join("voice.jsonl"),
                    voice.join(
                        "
",
                    ),
                )
                .unwrap();
                fut.await
            })
        });
        let h = hosting(run, fake_judge(Some(0)));
        let p = SandboxStartParams {
            record: true,
            ..params(true)
        };
        let started = start_with(st.clone(), p, h).await.unwrap();
        let end = until_ended(&st, &started.session_id).await;
        let report = end.report.clone().unwrap();
        let check = |id: &str| report.checks.iter().find(|c| c.id == id).unwrap().clone();
        assert_eq!(
            check("voiced").state,
            SandboxCheckState::Pass,
            "{:?}",
            check("voiced")
        );
        assert_eq!(
            check("face").state,
            SandboxCheckState::Pass,
            "{:?}",
            check("face")
        );
        // The recording is not in the run dir yet: unmeasured, and a recorded session's gate
        // fails on it.
        assert_eq!(check("av_sync").state, SandboxCheckState::Unmeasured);
        assert_eq!(check("dead_air").detail, "no recording.json");
        assert!(!report.pass);

        // The recorder merges its checks into session.json; the status reads the file.
        let mut merged = report.clone();
        for c in merged.checks.iter_mut() {
            c.state = SandboxCheckState::Pass;
        }
        merged.pass = true;
        write_durable(&Path::new(&started.dir).join("session.json"), &merged).unwrap();
        let again = status(
            &st,
            SandboxStatusParams {
                session_id: Some(started.session_id.clone()),
            },
        )
        .await
        .unwrap();
        assert_eq!(again.report, Some(merged));
    }

    #[tokio::test]
    async fn no_subscriber_leaves_stage_ack_unmeasured_and_no_judge_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        let h = hosting(
            fake_run(3, Duration::ZERO, None, false),
            fake_judge(Some(0)),
        );
        let started = start_with(st.clone(), params(false), h).await.unwrap();
        let report = until_ended(&st, &started.session_id).await.report.unwrap();
        let check = |id: &str| report.checks.iter().find(|c| c.id == id).unwrap().clone();
        assert_eq!(check("stage_ack").state, SandboxCheckState::Unmeasured);
        assert_eq!(check("judge").state, SandboxCheckState::Unmeasured);
        assert_eq!(check("judge").detail, "skipped");
        assert!(!report.pass);
        assert!(report.judge.is_none());
    }

    #[tokio::test]
    async fn gaps_fail_stage_ack_and_judge_failures_fail_the_judge() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        let h = hosting(
            fake_run(4, Duration::ZERO, Some((st.clone(), 1)), false),
            fake_judge(Some(2)),
        );
        let id = start_with(st.clone(), params(true), h)
            .await
            .unwrap()
            .session_id;
        let report = until_ended(&st, &id).await.report.unwrap();
        let check = |id: &str| report.checks.iter().find(|c| c.id == id).unwrap().clone();
        assert_eq!(check("stage_ack").state, SandboxCheckState::Fail);
        assert_eq!(check("judge").state, SandboxCheckState::Fail);
        assert!(!report.pass);

        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        let h = hosting(fake_run(2, Duration::ZERO, None, false), fake_judge(None));
        let id = start_with(st.clone(), params(true), h)
            .await
            .unwrap()
            .session_id;
        let report = until_ended(&st, &id).await.report.unwrap();
        let judge = report.checks.iter().find(|c| c.id == "judge").unwrap();
        assert_eq!(judge.state, SandboxCheckState::Fail);
        assert!(judge.detail.contains("judge timed out"), "{}", judge.detail);
    }

    #[tokio::test]
    async fn one_session_at_a_time_and_stop_ends_it_early() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        let long = || {
            hosting(
                fake_run(2, Duration::from_secs(60), None, false),
                fake_judge(Some(0)),
            )
        };
        let id = start_with(st.clone(), params(false), long())
            .await
            .unwrap()
            .session_id;
        assert_eq!(st.sandbox.running().as_deref(), Some(id.as_str()));
        let again = start_with(st.clone(), params(false), long()).await;
        assert_eq!(again.unwrap_err().code, ErrorCode::Conflict);
        let sim = crate::live::start_sim(
            st.clone(),
            shadowrunr_types::SimRunSpec {
                run_id: "beside".into(),
                character: "malakor".into(),
                tone_variant: "theatrical".into(),
                core_tier: shadowrunr_types::ModelTier::Haiku,
                core_effort: shadowrunr_types::Effort::Low,
                segment: shadowrunr_types::SegmentId::Seg01ImprovisedRoom,
                audience_size: 3,
                seed: 1,
                duration_s: 60,
                population: "small_new_channel".into(),
            },
            false,
        )
        .await;
        assert_eq!(sim.unwrap_err().code, ErrorCode::Conflict);

        let running = status(&st, SandboxStatusParams::default()).await.unwrap();
        assert_eq!(running.state, SandboxState::Running);
        assert_eq!(running.events, 2);
        assert_eq!(running.planned_s, 60);
        assert!(running.report.is_none());

        let stopping = stop(
            &st,
            SandboxIdParams {
                session_id: id.clone(),
            },
        )
        .await
        .unwrap();
        assert!(!is_ended(stopping.state));
        let end = until_ended(&st, &id).await;
        assert_eq!(end.state, SandboxState::Stopped);
        // Stopping an ended session returns it unchanged.
        let again = stop(
            &st,
            SandboxIdParams {
                session_id: id.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(again, end);
        let unknown = stop(
            &st,
            SandboxIdParams {
                session_id: "sbx-19700101-000000-5m".into(),
            },
        )
        .await;
        assert_eq!(unknown.unwrap_err().code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn a_failed_run_is_failed_with_error_json_and_frees_the_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        let h = hosting(fake_run(2, Duration::ZERO, None, true), fake_judge(Some(0)));
        let started = start_with(st.clone(), params(true), h).await.unwrap();
        let live_run = st.live.get(&started.session_id);
        let end = until_ended(&st, &started.session_id).await;
        assert_eq!(end.state, SandboxState::Failed);
        assert!(end.report.is_none());
        let err = std::fs::read_to_string(Path::new(&started.dir).join("error.json")).unwrap();
        assert!(err.contains("fake engine fell over"), "{err}");
        if let Some(r) = live_run {
            assert!(r.is_done());
            assert!(r.attach(0).error.is_some());
        }
        assert!(st.sandbox.running().is_none());
        assert!(!st.live.any_in_flight());

        // A later daemon reads the same session from disk: no session.json is failed.
        let fresh = state(tmp.path());
        let from_disk = status(&fresh, SandboxStatusParams::default())
            .await
            .unwrap();
        assert_eq!(from_disk.session_id, started.session_id);
        assert_eq!(from_disk.state, SandboxState::Failed);
        assert_eq!(from_disk.events, 0);
    }

    #[tokio::test]
    async fn status_and_start_refuse_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let st = state(tmp.path());
        let none = status(&st, SandboxStatusParams::default()).await;
        assert_eq!(none.unwrap_err().code, ErrorCode::NotFound);
        let bad = status(
            &st,
            SandboxStatusParams {
                session_id: Some("../x".into()),
            },
        )
        .await;
        assert_eq!(bad.unwrap_err().code, ErrorCode::InvalidParams);
        let tone = start_with(
            st.clone(),
            SandboxStartParams {
                tone: Some("no-such-tone".into()),
                ..params(true)
            },
            Hosting::default(),
        )
        .await;
        assert_eq!(tone.unwrap_err().code, ErrorCode::InvalidParams);

        let red = AppState::new(
            test_config(tmp.path(), tmp.path().join("no-characters")),
            EngineHandle::scripted(),
        );
        let refused = start_with(red, params(true), Hosting::default()).await;
        assert_eq!(refused.unwrap_err().code, ErrorCode::PreflightRed);
    }
}
