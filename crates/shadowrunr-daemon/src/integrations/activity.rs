//! Activity into Personas: matching events become HMAC-signed calls to a Personas webhook
//! trigger (`POST <webhook_url>/webhook/<trigger_id>`), opt-in under `[personas.activity]`.
//!
//! **Never fatal, never blocking.** [`Forwarder::forward`] is a `try_send` onto a bounded
//! queue: a full queue drops the event and counts it. One background task delivers in order
//! with retry and exponential backoff (honouring `Retry-After`). An event that exhausts its
//! attempts marks Personas down for `down_cooldown_ms`, and events arriving meanwhile are
//! dropped and counted without a network attempt. A call Personas refuses outright (a bad
//! signature, an unknown or disabled trigger) is counted as rejected, not retried. Nothing
//! the stream or sim loop awaits ever waits on Personas.
//!
//! What is forwarded: `stream_end`, `segment_end`, `handover`, sim `tournament_complete` and
//! reconcile `report_ready` by default (`events`), plus, always, moderation events with
//! action `priority:safety` or rule `character_break` / `persona_leak`. The body is JSON:
//! `{source, kind, ts_ms, summary, detail, dropped, project_id?, run_id?, seq?,
//! tournament_id?, report_id?}`. No chat text, prompt, token or secret is ever included.

use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use shadowrunr_types::{HealthReport, HealthState};
use tokio::sync::broadcast::error::RecvError;

pub use super::forwarder::{Forwarded, Forwarder, ForwarderConfig, StatsSnapshot};
use super::settings::PersonasSettings;
use super::signing::Secret;
use crate::config::Config;
use crate::live::{LiveItem, LiveRun};
use crate::server::AppState;

/// The pre-flight line this integration adds when activity is configured.
pub const PREFLIGHT_CHECK: &str = "PF-09";

/// How long a one-shot CLI notification may hold the command's exit.
pub const CLI_FLUSH: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    StreamEnd,
    SegmentEnd,
    Handover,
    TournamentComplete,
    ReportReady,
    Moderation,
}

impl ActivityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StreamEnd => "stream_end",
            Self::SegmentEnd => "segment_end",
            Self::Handover => "handover",
            Self::TournamentComplete => "tournament_complete",
            Self::ReportReady => "report_ready",
            Self::Moderation => "moderation",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::StreamEnd,
            Self::SegmentEnd,
            Self::Handover,
            Self::TournamentComplete,
            Self::ReportReady,
            Self::Moderation,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

/// One thing worth telling Personas about.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityEvent {
    pub kind: ActivityKind,
    pub run_id: Option<String>,
    pub seq: Option<u64>,
    pub tournament_id: Option<String>,
    pub report_id: Option<String>,
    pub summary: String,
    pub detail: Value,
}

impl ActivityEvent {
    fn new(kind: ActivityKind, summary: String, detail: Value) -> Self {
        Self {
            kind,
            run_id: None,
            seq: None,
            tournament_id: None,
            report_id: None,
            summary,
            detail,
        }
    }

    /// A transcript event (`StreamEvent` JSON) of a forwardable type, else `None`.
    pub fn from_stream_event(run_id: &str, seq: u64, ev: &Value) -> Option<Self> {
        let s = |k: &str| ev.get(k).and_then(Value::as_str).unwrap_or("?");
        let (kind, summary) = match ev.get("type").and_then(Value::as_str)? {
            "stream_end" => (
                ActivityKind::StreamEnd,
                format!("stream ended ({})", s("reason")),
            ),
            "segment_end" => (
                ActivityKind::SegmentEnd,
                format!("segment {} ended", s("segment")),
            ),
            "handover" => (
                ActivityKind::Handover,
                format!("handover {} -> {} ({})", s("from"), s("to"), s("reason")),
            ),
            "moderation" => (
                ActivityKind::Moderation,
                format!(
                    "moderation {} on {}: {}",
                    s("rule"),
                    s("stage"),
                    s("action")
                ),
            ),
            _ => return None,
        };
        let mut e = Self::new(kind, format!("{run_id}: {summary}"), ev.clone());
        e.run_id = Some(run_id.to_string());
        e.seq = Some(seq);
        Some(e)
    }

    /// A tournament finished; `detail` is a small summary (counts, directory).
    pub fn tournament_complete(tournament_id: &str, detail: Value) -> Self {
        let mut e = Self::new(
            ActivityKind::TournamentComplete,
            format!("tournament {tournament_id} complete"),
            detail,
        );
        e.tournament_id = Some(tournament_id.to_string());
        e
    }

    /// A Reconcile & Plan report was stored.
    pub fn report_ready(report_id: &str, detail: Value) -> Self {
        let mut e = Self::new(
            ActivityKind::ReportReady,
            format!("reconcile report {report_id} ready"),
            detail,
        );
        e.report_id = Some(report_id.to_string());
        e
    }

    /// Moderation that is always forwarded: a safety priority, a character break, a leak.
    pub fn is_safety(&self) -> bool {
        self.kind == ActivityKind::Moderation
            && (self.detail.get("action").and_then(Value::as_str) == Some("priority:safety")
                || matches!(
                    self.detail.get("rule").and_then(Value::as_str),
                    Some("character_break" | "persona_leak")
                ))
    }
}

/// Which events are forwarded.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter(Vec<ActivityKind>);

impl Filter {
    /// Unknown names are skipped here; `ActivitySettings::problems` reports them.
    pub fn from_names(names: &[String]) -> Self {
        Self(
            names
                .iter()
                .filter_map(|n| ActivityKind::parse(n))
                .collect(),
        )
    }

    pub fn wants(&self, e: &ActivityEvent) -> bool {
        e.is_safety() || self.0.contains(&e.kind)
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    source: &'static str,
    kind: &'static str,
    ts_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tournament_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    report_id: Option<&'a str>,
    summary: &'a str,
    detail: &'a Value,
    /// Events this forwarder has dropped so far, so gaps are visible on the Personas side.
    dropped: u64,
}

/// The exact bytes that are signed and sent.
pub fn payload(e: &ActivityEvent, project_id: Option<&str>, dropped: u64, ts_ms: i64) -> Vec<u8> {
    let p = Payload {
        source: "shadowrunr",
        kind: e.kind.as_str(),
        ts_ms,
        project_id,
        run_id: e.run_id.as_deref(),
        seq: e.seq,
        tournament_id: e.tournament_id.as_deref(),
        report_id: e.report_id.as_deref(),
        summary: &e.summary,
        detail: &e.detail,
        dropped,
    };
    serde_json::to_vec(&p).unwrap_or_else(|_| b"{}".to_vec())
}

/// Build a forwarder from `cfg`'s `[personas.activity]`: `Ok(None)` when it is off.
pub fn from_config(cfg: &Config) -> Result<Option<Forwarder>, String> {
    let settings = PersonasSettings::load(cfg)?;
    let Some(a) = settings.activity_on() else {
        return Ok(None);
    };
    let project_id = super::door::read_project_file(&cfg.root).ok().map(|p| p.id);
    Forwarder::spawn(ForwarderConfig::from_settings(a, project_id)?).map(Some)
}

static GLOBAL: OnceLock<Arc<Forwarder>> = OnceLock::new();

/// The process-wide forwarder installed by [`start_for_daemon`], if any.
pub fn global() -> Option<&'static Arc<Forwarder>> {
    GLOBAL.get()
}

/// Forward through the process-wide forwarder; a no-op when activity is off.
pub fn emit(e: ActivityEvent) -> Forwarded {
    match GLOBAL.get() {
        Some(f) => f.forward(e),
        None => Forwarded::Filtered,
    }
}

/// Daemon hook: when activity is configured, install the process-wide forwarder and follow
/// every live run's events. Problems are printed once and leave the daemon untouched.
pub fn start_for_daemon(state: &Arc<AppState>) {
    match from_config(&state.cfg) {
        Ok(None) => {}
        Ok(Some(f)) => {
            let f = Arc::new(f);
            let _ = GLOBAL.set(f.clone());
            tokio::spawn(watch_live(state.clone(), f));
        }
        Err(e) => eprintln!("personas activity is off: {e}"),
    }
}

/// CLI hook: send one event and wait (at most [`CLI_FLUSH`]) for its delivery. Silent when
/// activity is off; never an error.
pub async fn notify_once(cfg: &Config, e: ActivityEvent) -> Option<StatsSnapshot> {
    let f = match from_config(cfg) {
        Ok(Some(f)) => f,
        Ok(None) => return None,
        Err(err) => {
            eprintln!("personas activity is off: {err}");
            return None;
        }
    };
    f.forward(e);
    f.drain(CLI_FLUSH).await;
    Some(f.stats())
}

/// Follow the daemon's live runs (each newest run as it registers) until shutdown.
pub async fn watch_live(state: Arc<AppState>, f: Arc<Forwarder>) {
    let mut latest = state.live.latest();
    let shutdown = state.shutdown_signal();
    tokio::pin!(shutdown);
    let mut last: Weak<LiveRun> = Weak::new();
    loop {
        let run = latest.borrow_and_update().clone();
        if let Some(run) = run {
            if !last.upgrade().is_some_and(|l| Arc::ptr_eq(&l, &run)) {
                last = Arc::downgrade(&run);
                tokio::spawn(follow_run(run, f.clone()));
            }
        }
        tokio::select! {
            () = &mut shutdown => break,
            r = latest.changed() => if r.is_err() { break },
        }
    }
}

fn handle_frame(run_id: &str, seq: u64, frame: &str, f: &Forwarder) {
    let Ok(v) = serde_json::from_str::<Value>(frame) else {
        return;
    };
    if let Some(e) = ActivityEvent::from_stream_event(run_id, seq, &v) {
        f.forward(e);
    }
}

/// Forward one live run's matching events, history first, until it ends.
pub async fn follow_run(run: Arc<LiveRun>, f: Arc<Forwarder>) {
    let attach = run.attach(0);
    for (seq, frame) in &attach.history {
        handle_frame(&run.run_id, *seq, frame, &f);
    }
    let Some(mut rx) = attach.rx else { return };
    loop {
        match rx.recv().await {
            Ok(LiveItem::Event(seq, frame)) => handle_frame(&run.run_id, seq, &frame, &f),
            Ok(LiveItem::End(_)) | Err(RecvError::Closed) => break,
            Err(RecvError::Lagged(n)) => f.note_lagged(n),
        }
    }
}

/// PF-09 "integrations": `None` when activity is off. Amber when it is configured but
/// invalid, missing its secret, or unreachable; never red (Personas never blocks a sim).
pub async fn preflight_line(cfg: &Config) -> Option<HealthReport> {
    let rep = |state, detail: String| HealthReport {
        check: PREFLIGHT_CHECK.into(),
        state,
        detail,
    };
    let settings = match PersonasSettings::load(cfg) {
        Ok(s) => s,
        Err(e) => return Some(rep(HealthState::Amber, format!("personas: {e}"))),
    };
    let a = settings.activity_on()?;
    let problems = a.problems();
    if !problems.is_empty() {
        return Some(rep(
            HealthState::Amber,
            format!("personas activity misconfigured: {}", problems.join("; ")),
        ));
    }
    if let Err(e) = Secret::from_env(a.secret_env.trim()) {
        return Some(rep(HealthState::Amber, format!("personas activity: {e}")));
    }
    let counts = global()
        .map(|f| {
            let s = f.stats();
            format!("; {} delivered, {} dropped", s.delivered, s.dropped())
        })
        .unwrap_or_default();
    let endpoint = a.endpoint();
    let probe = async {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(800))
            .no_proxy()
            .build()
            .ok()?;
        // GET /webhook/{id} is the read-only "does this trigger exist" endpoint.
        http.get(&endpoint)
            .send()
            .await
            .ok()
            .map(|r| r.status().as_u16())
    };
    Some(match probe.await {
        Some(200) => rep(
            HealthState::Green,
            format!("personas activity -> {endpoint}: trigger answers{counts}"),
        ),
        Some(404) => rep(
            HealthState::Amber,
            format!("personas activity -> {endpoint}: trigger not found{counts}"),
        ),
        Some(s) => rep(
            HealthState::Amber,
            format!("personas activity -> {endpoint}: HTTP {s}{counts}"),
        ),
        None => rep(
            HealthState::Amber,
            format!("personas activity configured but unreachable ({endpoint}){counts}"),
        ),
    })
}

#[cfg(test)]
#[path = "activity_tests.rs"]
mod tests;
