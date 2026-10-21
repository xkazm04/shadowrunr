//! `<run dir>/voice.jsonl`: what the voice actually did, one [`VoiceLogLine`] per event, in the
//! order the events happen (contract § run-directory artifacts).
//!
//! - `rendered` and `skipped` come from the voice service as its frames go out;
//!   `played`, `stage_dropped` and `face` come from the stage's client frames on `/voice`.
//! - Times are on the session's stream clock: wall epoch ms minus the session's `started_ms`.
//!   The session's clock is only known exactly when it ends (its report), so while it runs the
//!   log estimates it from the events the daemon sees ([`VoiceLog::observe`]: an event stamped
//!   `ts_ms` that reaches the daemon at wall `w` puts `started_ms` at or before `w - ts_ms`,
//!   and the smallest such bound is kept). [`VoiceLog::started`] then rewrites the file on the
//!   exact clock.
//! - **It never blocks the stream.** Lines go through a bounded queue ([`LOG_QUEUE`]) to one
//!   writer thread per log; a full queue drops the line, and a write that fails is logged once
//!   (one stderr line per log), and the show goes on.
//! - A log exists only for a run that opened one (a sandbox session opens its own); events for
//!   any other run are dropped. The book keeps the newest [`KEEP`] logs, so the stage's last
//!   `played` frames still land after the session has ended.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

use shadowrunr_types::voice::{
    VoiceClientFrame, VoiceFrame, VoiceLogEvent, VoiceLogLine, VoiceSkip,
};

use crate::runs;

/// The file in the run directory.
pub const FILE: &str = "voice.jsonl";
/// Lines waiting for the writer, at most; past it a line is dropped.
pub const LOG_QUEUE: usize = 1024;
/// Logs the book keeps open; opening another closes the oldest.
pub const KEEP: usize = 4;
/// Lines a log remembers for its rewrite on the exact clock (a 30-minute session writes a few
/// thousand).
const REMEMBER: usize = 100_000;
/// The clock is not estimated yet.
const UNKNOWN: i64 = i64::MAX;

/// A line before its stream-clock time is known: the wall time it happened at.
#[derive(Debug, Clone)]
struct Pending {
    wall_ms: i64,
    line: VoiceLogLine,
}

enum Msg {
    Line(Pending),
    /// The session's exact `started_ms`: rewrite everything on it.
    Started(i64),
    /// Answered once everything before it is written (tests and the end of a session).
    Sync(std::sync::mpsc::Sender<()>),
}

/// One run's log.
pub struct VoiceLog {
    pub run_id: String,
    pub path: PathBuf,
    tx: SyncSender<Msg>,
    /// The best estimate of the session's `started_ms` so far ([`UNKNOWN`] before any event).
    anchor: Arc<AtomicI64>,
    dropped: AtomicBool,
}

impl VoiceLog {
    fn open(run_id: &str, dir: &Path) -> Arc<Self> {
        let path = dir.join(FILE);
        let (tx, rx) = sync_channel(LOG_QUEUE);
        let anchor = Arc::new(AtomicI64::new(UNKNOWN));
        let writer = Writer {
            path: path.clone(),
            file: None,
            anchor: anchor.clone(),
            fallback: runs::now_ms(),
            exact: None,
            lines: Vec::new(),
            failed: false,
        };
        let name = format!("voice-log {run_id}");
        let spawned = std::thread::Builder::new()
            .name(name)
            .spawn(move || writer.run(rx));
        if let Err(e) = spawned {
            eprintln!("voice log {}: no writer thread: {e}", path.display());
        }
        Arc::new(Self {
            run_id: run_id.to_string(),
            path,
            tx,
            anchor,
            dropped: AtomicBool::new(false),
        })
    }

    /// A stream event stamped `ts_ms` reached the daemon now.
    pub fn observe(&self, ts_ms: i64) {
        self.observe_at(runs::now_ms(), ts_ms);
    }

    /// A stream event stamped `ts_ms` reached the daemon at `wall_ms`: the session started at
    /// or before `wall_ms - ts_ms`.
    pub fn observe_at(&self, wall_ms: i64, ts_ms: i64) {
        self.anchor
            .fetch_min(wall_ms.saturating_sub(ts_ms), Ordering::Relaxed);
    }

    /// The session's estimated `started_ms`, once any event has been seen.
    pub fn estimate(&self) -> Option<i64> {
        Some(self.anchor.load(Ordering::Relaxed)).filter(|a| *a != UNKNOWN)
    }

    /// The session's exact `started_ms` (its report): the file is rewritten on it, and later
    /// lines use it.
    pub fn started(&self, started_ms: i64) {
        self.send(Msg::Started(started_ms));
    }

    /// Log `line` as happening at `wall_ms` (its `ts_ms` is filled in by the writer).
    pub fn record(&self, wall_ms: i64, line: VoiceLogLine) {
        self.send(Msg::Line(Pending { wall_ms, line }));
    }

    /// Wait until everything logged so far is written.
    pub fn sync(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.tx.send(Msg::Sync(tx)).is_ok() {
            let _ = rx.recv();
        }
    }

    fn send(&self, msg: Msg) {
        match self.tx.try_send(msg) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                if !self.dropped.swap(true, Ordering::Relaxed) {
                    eprintln!(
                        "voice log {}: the writer is {LOG_QUEUE} lines behind; lines are lost",
                        self.path.display()
                    );
                }
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

/// The writer thread's side of one log.
struct Writer {
    path: PathBuf,
    file: Option<File>,
    anchor: Arc<AtomicI64>,
    /// When the log was opened: the clock until an event has been seen.
    fallback: i64,
    exact: Option<i64>,
    lines: Vec<Pending>,
    failed: bool,
}

impl Writer {
    fn run(mut self, rx: Receiver<Msg>) {
        while let Ok(msg) = rx.recv() {
            match msg {
                Msg::Line(p) => {
                    let mut line = p.line.clone();
                    line.ts_ms = p.wall_ms - self.clock();
                    let r = self.append(&line);
                    self.check(r);
                    if self.lines.len() < REMEMBER {
                        self.lines.push(p);
                    }
                }
                Msg::Started(ms) => {
                    self.exact = Some(ms);
                    if !self.lines.is_empty() {
                        let r = self.rewrite();
                        self.check(r);
                    }
                }
                Msg::Sync(tx) => {
                    let _ = tx.send(());
                }
            }
        }
    }

    fn clock(&self) -> i64 {
        self.exact.unwrap_or_else(|| {
            let a = self.anchor.load(Ordering::Relaxed);
            if a == UNKNOWN {
                self.fallback
            } else {
                a
            }
        })
    }

    fn check(&mut self, r: std::io::Result<()>) {
        if let Err(e) = r {
            self.file = None;
            if !self.failed {
                self.failed = true;
                eprintln!(
                    "voice log {}: {e}; the show goes on without it",
                    self.path.display()
                );
            }
        }
    }

    fn append(&mut self, line: &VoiceLogLine) -> std::io::Result<()> {
        if self.file.is_none() {
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?,
            );
        }
        let mut text = serde_json::to_string(line).map_err(std::io::Error::other)?;
        text.push('\n');
        self.file
            .as_mut()
            .expect("opened above")
            .write_all(text.as_bytes())
    }

    /// Every remembered line again, on the exact clock, through a temp file and a rename.
    fn rewrite(&mut self) -> std::io::Result<()> {
        let clock = self.clock();
        let mut text = String::new();
        for p in &self.lines {
            let mut line = p.line.clone();
            line.ts_ms = p.wall_ms - clock;
            text.push_str(&serde_json::to_string(&line).map_err(std::io::Error::other)?);
            text.push('\n');
        }
        self.file = None;
        let tmp = self.path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)
    }
}

/// The open logs, by run id.
#[derive(Default)]
pub struct VoiceLogs {
    logs: Mutex<VecDeque<Arc<VoiceLog>>>,
}

impl VoiceLogs {
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<Arc<VoiceLog>>> {
        self.logs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Start `run_id`'s log in `dir` (replacing an earlier one for the same run). The file is
    /// created with its first line.
    pub fn open(&self, run_id: &str, dir: &Path) -> Arc<VoiceLog> {
        let log = VoiceLog::open(run_id, dir);
        let mut logs = self.lock();
        logs.retain(|l| l.run_id != run_id);
        if logs.len() >= KEEP {
            logs.pop_front();
        }
        logs.push_back(log.clone());
        log
    }

    pub fn get(&self, run_id: &str) -> Option<Arc<VoiceLog>> {
        self.lock().iter().find(|l| l.run_id == run_id).cloned()
    }

    /// Log `line` for `run_id` at `wall_ms`; nothing when the run has no log.
    pub fn record(&self, run_id: &str, wall_ms: i64, line: VoiceLogLine) {
        if let Some(log) = self.get(run_id) {
            log.record(wall_ms, line);
        }
    }

    /// The service sent `frame`: `rendered` for an utterance, `skipped` for a skip.
    pub fn service(&self, frame: &VoiceFrame) {
        let (run_id, line) = match frame {
            VoiceFrame::Utterance {
                run_id,
                seq,
                part,
                text,
                duration_ms,
                ..
            } => (
                run_id,
                VoiceLogLine {
                    ts_ms: 0,
                    seq: *seq,
                    event: VoiceLogEvent::Rendered,
                    part: Some(*part),
                    reason: None,
                    duration_ms: Some(u64::from(*duration_ms)),
                    text: Some(text.clone()),
                },
            ),
            VoiceFrame::Skipped {
                run_id,
                seq,
                part,
                reason,
            } => (
                run_id,
                VoiceLogLine {
                    ts_ms: 0,
                    seq: *seq,
                    event: VoiceLogEvent::Skipped,
                    part: Some(*part),
                    reason: Some(skip_reason(*reason).into()),
                    duration_ms: None,
                    text: None,
                },
            ),
            VoiceFrame::Status { .. } => return,
        };
        self.record(run_id, runs::now_ms(), line);
    }

    /// A stage report from `/voice`. `follow` frames are not reports and log nothing.
    pub fn stage(&self, frame: &VoiceClientFrame) {
        let (run_id, wall_ms, line) = match frame {
            VoiceClientFrame::Follow(_) => return,
            VoiceClientFrame::Played(f) => (
                &f.played.run_id,
                f.played.at_wall_ms,
                VoiceLogLine {
                    ts_ms: 0,
                    seq: f.played.seq,
                    event: VoiceLogEvent::Played,
                    part: Some(f.played.part),
                    reason: None,
                    duration_ms: None,
                    text: None,
                },
            ),
            VoiceClientFrame::StageDropped(f) => (
                &f.stage_dropped.run_id,
                runs::now_ms(),
                VoiceLogLine {
                    ts_ms: 0,
                    seq: f.stage_dropped.seq,
                    event: VoiceLogEvent::StageDropped,
                    part: None,
                    reason: Some(wire(&f.stage_dropped.reason)),
                    duration_ms: None,
                    text: None,
                },
            ),
            VoiceClientFrame::Face(f) => (
                &f.face.run_id,
                f.face.at_wall_ms,
                VoiceLogLine {
                    ts_ms: 0,
                    seq: 0,
                    event: VoiceLogEvent::Face,
                    part: None,
                    reason: Some(wire(&f.face.state)),
                    duration_ms: None,
                    text: None,
                },
            ),
        };
        self.record(run_id, wall_ms, line);
    }
}

fn skip_reason(r: VoiceSkip) -> &'static str {
    match r {
        VoiceSkip::Late => "late",
        VoiceSkip::QueueFull => "queue_full",
        VoiceSkip::Error => "error",
    }
}

/// A unit enum's wire name.
fn wire<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use shadowrunr_types::voice::{
        FaceState, StageDropReason, VoiceFace, VoiceFaceFrame, VoicePlayed, VoicePlayedFrame,
        VoiceStageDropped, VoiceStageDroppedFrame,
    };

    use super::*;
    use crate::live::{Frame, LiveRun};
    use crate::voice::renderer::fake::{FakeRenderer, Script};
    use crate::voice::{VoiceConfig, VoiceHub};

    fn read(path: &Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn played(run: &str, seq: u64, part: u32, at: i64) -> VoiceClientFrame {
        VoiceClientFrame::Played(VoicePlayedFrame {
            played: VoicePlayed {
                run_id: run.into(),
                seq,
                part,
                at_wall_ms: at,
            },
        })
    }

    #[test]
    fn stage_reports_land_on_the_stream_clock_and_are_rewritten_on_the_exact_one() {
        let d = tempfile::tempdir().unwrap();
        let book = VoiceLogs::default();
        let log = book.open("r", d.path());
        // Events seen at wall 10_500 (ts 400) and 10_300 (ts 250): started at or before 10_050.
        log.observe_at(10_500, 400);
        log.observe_at(10_300, 250);
        assert_eq!(log.estimate(), Some(10_050));
        book.stage(&played("r", 7, 0, 12_050));
        book.stage(&VoiceClientFrame::Face(VoiceFaceFrame {
            face: VoiceFace {
                run_id: "r".into(),
                state: FaceState::Silhouette,
                at_wall_ms: 10_060,
            },
        }));
        // Another run's report and a follow frame log nothing here.
        book.stage(&played("other", 1, 0, 12_000));
        book.stage(&serde_json::from_str(r#"{"follow":{}}"#).unwrap());
        log.sync();
        assert_eq!(
            read(&log.path),
            [
                serde_json::json!({"ts_ms":2000,"seq":7,"event":"played","part":0}),
                serde_json::json!({"ts_ms":10,"seq":0,"event":"face","reason":"silhouette"}),
            ]
        );
        // The report's exact clock is 50 ms earlier than the estimate.
        log.started(10_000);
        book.stage(&played("r", 7, 1, 13_000));
        log.sync();
        let ts: Vec<i64> = read(&log.path)
            .iter()
            .map(|v| v["ts_ms"].as_i64().unwrap())
            .collect();
        assert_eq!(ts, [2050, 60, 3000]);
        assert!(!d.path().join("voice.jsonl.tmp").exists());
    }

    #[test]
    fn a_dropped_line_is_logged_with_its_reason() {
        let d = tempfile::tempdir().unwrap();
        let book = VoiceLogs::default();
        let log = book.open("r", d.path());
        log.observe_at(1_000, 0);
        book.stage(&VoiceClientFrame::StageDropped(VoiceStageDroppedFrame {
            stage_dropped: VoiceStageDropped {
                run_id: "r".into(),
                seq: 9,
                reason: StageDropReason::StageCap,
            },
        }));
        log.sync();
        let v = read(&log.path);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0]["event"], "stage_dropped");
        assert_eq!(v[0]["reason"], "stage_cap");
        assert_eq!(v[0]["seq"], 9);
        assert!(v[0].get("part").is_none());
    }

    #[test]
    fn a_log_that_cannot_be_written_never_stops_the_caller() {
        let d = tempfile::tempdir().unwrap();
        // The "run dir" is a file: every write fails.
        let not_a_dir = d.path().join("plain-file");
        std::fs::write(&not_a_dir, b"x").unwrap();
        let book = VoiceLogs::default();
        let log = book.open("r", &not_a_dir);
        for i in 0..(LOG_QUEUE as u64 * 3) {
            book.stage(&played("r", i, 0, 5));
        }
        log.started(0);
        log.sync();
        assert!(!log.path.exists());
    }

    #[test]
    fn the_book_keeps_the_newest_logs() {
        let d = tempfile::tempdir().unwrap();
        let book = VoiceLogs::default();
        for i in 0..=KEEP {
            book.open(&format!("r{i}"), d.path());
        }
        assert!(book.get("r0").is_none());
        assert!(book.get(&format!("r{KEEP}")).is_some());
    }

    #[tokio::test]
    async fn the_service_logs_what_it_rendered_and_what_it_skipped() {
        let d = tempfile::tempdir().unwrap();
        let hub = VoiceHub::new(VoiceConfig {
            enabled: true,
            ..VoiceConfig::default()
        });
        let fake = Arc::new(FakeRenderer::new());
        hub.attach(fake.clone(), std::future::pending());
        fake.script(
            "Kneel. Now.",
            Script::Speak(
                vec!["Kneel.".into(), "Now.".into()],
                Duration::from_millis(5),
            ),
        );
        fake.script("Broken.", Script::FailAfter(Vec::new(), 0));
        let log = hub.logs().open("r", d.path());
        let run = Arc::new(LiveRun::new("r".into()));
        let mut f = hub.follow(&run, false);
        let say = |t: &str| {
            Frame::from(
                serde_json::json!({"type":"say","ts_ms":1,"speaker":"malakor","text":t})
                    .to_string(),
            )
        };
        run.push(say("Kneel. Now."));
        run.push(say("Broken."));
        let mut seen = 0;
        while seen < 3 {
            tokio::time::timeout(Duration::from_secs(5), f.rx.recv())
                .await
                .expect("a frame in time")
                .expect("open");
            seen += 1;
        }
        log.sync();
        let v = read(&log.path);
        let events: Vec<(String, u64, u64)> = v
            .iter()
            .map(|l| {
                (
                    l["event"].as_str().unwrap().to_string(),
                    l["seq"].as_u64().unwrap(),
                    l["part"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            events,
            [
                ("rendered".into(), 1, 0),
                ("rendered".into(), 1, 1),
                ("skipped".into(), 2, 0),
            ]
        );
        assert_eq!(v[0]["text"], "Kneel.");
        assert!(v[0]["duration_ms"].is_u64());
        assert_eq!(v[2]["reason"], "error");
    }
}
