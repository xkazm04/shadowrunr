//! The sidecar process behind [`Renderer`]: `python -m shadowrunr_voice` in `[voice] sidecar`,
//! JSON lines on stdio (contract appendix (voice)). One process per daemon, supervised:
//!
//! - Before anything is spawned, the engine's files are probed; a missing engine is
//!   `degraded` with the file's name, and nothing is retried (the config has to change).
//! - The process is `starting` until its `hello`, then `ready` (with or without mouth shapes).
//! - When it exits, every line in flight fails from the part it reached, the state is
//!   `degraded` with the exit status and the last stderr line, and it restarts with backoff:
//!   1 s, doubling to 60 s, and at most [`MAX_RESTARTS`] restarts in any [`RESTART_WINDOW`].
//! - It never outlives the daemon: the supervisor holds its stdin, so a shutdown closes stdin
//!   (the sidecar exits on EOF), waits [`STOP_GRACE`], then kills it; the child is also
//!   `kill_on_drop`, and a daemon that dies outright closes the pipe the sidecar reads.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use shadowrunr_types::voice::{Viseme, VisemeSpan};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::probe;
use super::renderer::{Part, Rendered, Renderer, RendererState};
use super::VoiceConfig;

/// `CREATE_NO_WINDOW`: the sidecar never flashes a console window on Windows.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// The protocol this daemon speaks.
pub const PROTOCOL: u32 = 1;
/// Restarts allowed in any [`RESTART_WINDOW`].
pub const MAX_RESTARTS: usize = 3;
pub const RESTART_WINDOW: Duration = Duration::from_secs(300);
const FIRST_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// From spawn to `hello`: the first load warms the model (and the durations model).
const HELLO_TIMEOUT: Duration = Duration::from_secs(120);
/// From closing stdin to killing the process.
pub const STOP_GRACE: Duration = Duration::from_secs(2);
/// Stderr lines kept for the exit reason.
const STDERR_TAIL: usize = 5;

/// Lines the sidecar writes.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SidecarLine {
    Hello {
        protocol: u32,
        #[serde(default)]
        visemes: bool,
        #[serde(default)]
        reason: Option<String>,
    },
    Audio {
        id: String,
        part: u32,
        last: bool,
        #[serde(default)]
        text: String,
        sample_rate: u32,
        pcm: String,
        #[serde(default)]
        visemes: Vec<(String, f64, f64)>,
    },
    Error {
        id: String,
        #[serde(default)]
        part: Option<u32>,
        #[serde(default)]
        reason: String,
    },
}

/// `[[v, start_ms, end_ms]]` as spans; an unknown shape is dropped, not the sentence.
fn spans(raw: Vec<(String, f64, f64)>) -> Vec<VisemeSpan> {
    raw.into_iter()
        .filter_map(|(v, s, e)| {
            let v: Viseme = serde_json::from_value(serde_json::Value::String(v)).ok()?;
            let ms = |x: f64| x.max(0.0).round().min(f64::from(u32::MAX)) as u32;
            Some(VisemeSpan {
                v,
                start_ms: ms(s),
                end_ms: ms(e),
            })
        })
        .collect()
}

/// The sidecar's argument list after `-m shadowrunr_voice`, from `[voice]`.
pub fn sidecar_args(cfg: &VoiceConfig) -> Vec<String> {
    let mut a = vec!["-m".to_string(), "shadowrunr_voice".to_string()];
    if let Some(dir) = &cfg.engine_dir {
        a.extend(["--engine-dir".into(), dir.clone()]);
    }
    if let Some(d) = &cfg.durations_model {
        a.extend(["--durations".into(), d.clone()]);
    }
    a.extend([
        "--sid".into(),
        cfg.sid.to_string(),
        "--voice".into(),
        cfg.voice.clone(),
        "--render-timeout-ms".into(),
        cfg.render_timeout_ms.to_string(),
        "--workers".into(),
        cfg.render_workers.max(1).to_string(),
    ]);
    if let Some(t) = cfg.render_threads {
        a.extend(["--threads".into(), t.max(1).to_string()]);
    }
    a
}

/// When the next start may happen: backoff after each failure, and the restart cap.
#[derive(Debug, Default)]
pub struct RestartBudget {
    starts: VecDeque<Instant>,
    failures: u32,
}

impl RestartBudget {
    pub fn started(&mut self, now: Instant) {
        self.starts.push_back(now);
    }

    /// The process reached `hello`: the backoff starts over.
    pub fn healthy(&mut self) {
        self.failures = 0;
    }

    /// After an exit: how long to wait, and whether the cap (not the backoff) set it.
    pub fn after_exit(&mut self, now: Instant) -> (Duration, bool) {
        self.failures += 1;
        let backoff = FIRST_BACKOFF
            .saturating_mul(1 << (self.failures - 1).min(16))
            .min(MAX_BACKOFF);
        while self
            .starts
            .front()
            .is_some_and(|t| now.duration_since(*t) >= RESTART_WINDOW)
        {
            self.starts.pop_front();
        }
        // The first start plus MAX_RESTARTS restarts fit in one window.
        if self.starts.len() > MAX_RESTARTS {
            let free_at = self.starts[self.starts.len() - 1 - MAX_RESTARTS] + RESTART_WINDOW;
            let wait = free_at.saturating_duration_since(now);
            if wait > backoff {
                return (wait, true);
            }
        }
        (backoff, false)
    }
}

/// Resolves once the stop flag is set.
async fn stopped(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|d| *d).await;
}

struct Pending {
    tx: mpsc::UnboundedSender<Part>,
    next_part: u32,
}

#[derive(Default)]
struct Shared {
    pending: HashMap<String, Pending>,
    /// The running process's stdin, as lines; `None` when there is no process.
    stdin: Option<mpsc::UnboundedSender<String>>,
}

pub struct SidecarRenderer {
    cfg: VoiceConfig,
    state: watch::Sender<RendererState>,
    shared: Mutex<Shared>,
}

/// How one process ended.
enum Ended {
    /// It exited or was killed: restart.
    Exited(String),
    /// It cannot work with this daemon: stay degraded.
    Fatal(String),
    Shutdown,
}

impl SidecarRenderer {
    /// Start the supervisor. It stops (and the process with it) when `shutdown` resolves.
    pub fn start(
        cfg: VoiceConfig,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> (Arc<Self>, JoinHandle<()>) {
        let me = Arc::new(Self {
            cfg,
            state: watch::Sender::new(RendererState::Starting),
            shared: Mutex::new(Shared::default()),
        });
        let (stop_tx, stop_rx) = watch::channel(false);
        tokio::spawn(async move {
            shutdown.await;
            stop_tx.send_replace(true);
        });
        let task = tokio::spawn(me.clone().supervise(stop_rx));
        (me, task)
    }

    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|p| p.into_inner())
    }

    async fn supervise(self: Arc<Self>, mut stop: watch::Receiver<bool>) {
        if let Some(why) = probe::engine_missing(&self.cfg) {
            self.state.send_replace(RendererState::Degraded(why));
            return;
        }
        let mut budget = RestartBudget::default();
        loop {
            budget.started(Instant::now());
            let ended = self.run_once(&mut stop, &mut budget).await;
            self.fail_pending("the voice sidecar stopped");
            let why = match ended {
                Ended::Shutdown => return,
                Ended::Fatal(why) => {
                    eprintln!("voice: {why}");
                    self.state.send_replace(RendererState::Degraded(why));
                    return;
                }
                Ended::Exited(why) => why,
            };
            let (wait, capped) = budget.after_exit(Instant::now());
            let reason = if capped {
                format!(
                    "{why}; restarted {MAX_RESTARTS} times in {} min, next try in {} s",
                    RESTART_WINDOW.as_secs() / 60,
                    wait.as_secs()
                )
            } else {
                format!("{why}; restarting in {} s", wait.as_secs().max(1))
            };
            eprintln!("voice: {reason}");
            self.state.send_replace(RendererState::Degraded(reason));
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = stopped(&mut stop) => return,
            }
        }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.cfg.python);
        cmd.args(sidecar_args(&self.cfg))
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if Path::new(&self.cfg.sidecar).is_dir() {
            cmd.current_dir(&self.cfg.sidecar);
        }
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }

    async fn run_once(
        &self,
        stop: &mut watch::Receiver<bool>,
        budget: &mut RestartBudget,
    ) -> Ended {
        self.state.send_replace(RendererState::Starting);
        let mut child = match self.command().spawn() {
            Ok(c) => c,
            Err(e) => {
                return Ended::Exited(format!(
                    "cannot start the voice sidecar with {}: {e}",
                    self.cfg.python
                ))
            }
        };
        let (Some(mut stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Ended::Exited("the sidecar's stdio is not piped".into());
        };
        let (in_tx, mut in_rx) = mpsc::unbounded_channel::<String>();
        self.shared().stdin = Some(in_tx);
        // The writer owns stdin: when the channel closes, stdin closes and the sidecar exits.
        tokio::spawn(async move {
            while let Some(line) = in_rx.recv().await {
                let line = format!("{line}\n");
                if stdin.write_all(line.as_bytes()).await.is_err() || stdin.flush().await.is_err() {
                    break;
                }
            }
        });
        let tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
        let tail_w = tail.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let l = l.trim().to_string();
                if l.is_empty() {
                    continue;
                }
                let mut t = tail_w.lock().unwrap_or_else(|p| p.into_inner());
                if t.len() == STDERR_TAIL {
                    t.pop_front();
                }
                t.push_back(l);
            }
        });
        let last_err = move || {
            tail.lock()
                .unwrap_or_else(|p| p.into_inner())
                .back()
                .cloned()
        };

        let mut lines = BufReader::new(stdout).lines();
        let hello_by = Instant::now() + HELLO_TIMEOUT;
        let mut hello = false;
        loop {
            tokio::select! {
                line = lines.next_line() => match line {
                    Ok(Some(l)) => match self.on_line(&l, &mut hello) {
                        Ok(true) => budget.healthy(),
                        Ok(false) => {}
                        Err(fatal) => {
                            self.stop_child(&mut child).await;
                            return Ended::Fatal(fatal);
                        }
                    },
                    Ok(None) | Err(_) => break,
                },
                () = tokio::time::sleep_until(hello_by), if !hello => {
                    self.stop_child(&mut child).await;
                    return Ended::Exited(format!(
                        "the voice sidecar sent no hello within {} s",
                        HELLO_TIMEOUT.as_secs()
                    ));
                }
                () = stopped(stop) => {
                    self.stop_child(&mut child).await;
                    return Ended::Shutdown;
                }
            }
        }
        // Stdout closed: the process is exiting.
        let status = self.stop_child(&mut child).await;
        let mut why = format!("the voice sidecar exited ({status})");
        if let Some(e) = last_err() {
            why.push_str(&format!(": {e}"));
        }
        Ended::Exited(why)
    }

    /// Close stdin, give the process [`STOP_GRACE`], then kill it. Returns how it ended.
    async fn stop_child(&self, child: &mut Child) -> String {
        self.shared().stdin = None;
        match tokio::time::timeout(STOP_GRACE, child.wait()).await {
            Ok(Ok(s)) => s.to_string(),
            Ok(Err(e)) => e.to_string(),
            Err(_) => {
                let _ = child.kill().await;
                "killed after the stop grace".into()
            }
        }
    }

    /// Handle one stdout line. Ok(true) on `hello`; Err for a protocol this daemon cannot speak.
    fn on_line(&self, line: &str, hello: &mut bool) -> Result<bool, String> {
        let msg: SidecarLine = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(e) => {
                let short: String = line.chars().take(120).collect();
                eprintln!("voice: ignoring a sidecar line that is not the protocol ({e}): {short}");
                return Ok(false);
            }
        };
        match msg {
            SidecarLine::Hello {
                protocol,
                visemes,
                reason,
            } => {
                if protocol != PROTOCOL {
                    return Err(format!(
                        "the voice sidecar speaks protocol {protocol}; this daemon speaks {PROTOCOL}"
                    ));
                }
                *hello = true;
                self.state
                    .send_replace(RendererState::Ready { visemes, reason });
                Ok(true)
            }
            SidecarLine::Audio {
                id,
                part,
                last,
                text,
                sample_rate,
                pcm,
                visemes,
            } => {
                let mut sh = self.shared();
                if let Some(p) = sh.pending.get_mut(&id) {
                    p.next_part = part + 1;
                    let sent =
                        p.tx.send(Part::Audio(Rendered {
                            part,
                            last,
                            text,
                            sample_rate,
                            pcm,
                            visemes: spans(visemes),
                        }))
                        .is_ok();
                    if last || !sent {
                        sh.pending.remove(&id);
                    }
                }
                Ok(false)
            }
            SidecarLine::Error { id, part, reason } => {
                if let Some(p) = self.shared().pending.remove(&id) {
                    let _ = p.tx.send(Part::Error {
                        part: part.unwrap_or(p.next_part),
                        reason,
                    });
                }
                Ok(false)
            }
        }
    }

    fn fail_pending(&self, reason: &str) {
        let pending = std::mem::take(&mut self.shared().pending);
        for (_, p) in pending {
            let _ = p.tx.send(Part::Error {
                part: p.next_part,
                reason: reason.into(),
            });
        }
    }
}

impl Renderer for SidecarRenderer {
    fn state(&self) -> watch::Receiver<RendererState> {
        self.state.subscribe()
    }

    fn say(&self, id: &str, text: &str) -> mpsc::UnboundedReceiver<Part> {
        let (tx, rx) = mpsc::unbounded_channel();
        let line = serde_json::json!({ "type": "say", "id": id, "text": text }).to_string();
        let mut sh = self.shared();
        match &sh.stdin {
            Some(stdin) if stdin.send(line).is_ok() => {
                sh.pending
                    .insert(id.to_string(), Pending { tx, next_part: 0 });
            }
            _ => {
                let _ = tx.send(Part::Error {
                    part: 0,
                    reason: "the voice sidecar is not running".into(),
                });
            }
        }
        rx
    }

    fn cancel(&self, id: &str) {
        let mut sh = self.shared();
        sh.pending.remove(id);
        if let Some(stdin) = &sh.stdin {
            let _ = stdin.send(serde_json::json!({ "type": "cancel", "ids": [id] }).to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> SidecarRenderer {
        SidecarRenderer {
            cfg: VoiceConfig::default(),
            state: watch::Sender::new(RendererState::Starting),
            shared: Mutex::new(Shared::default()),
        }
    }

    #[test]
    fn args_come_from_the_config_in_order() {
        let cfg = VoiceConfig {
            engine_dir: Some("E".into()),
            durations_model: Some("D.onnx".into()),
            ..VoiceConfig::default()
        };
        assert_eq!(
            sidecar_args(&cfg).join(" "),
            "-m shadowrunr_voice --engine-dir E --durations D.onnx --sid 3 --voice af_heart \
             --render-timeout-ms 30000 --workers 3"
        );
        let bare = sidecar_args(&VoiceConfig::default()).join(" ");
        assert!(!bare.contains("--durations") && !bare.contains("--engine-dir"));
        assert!(
            !bare.contains("--threads"),
            "threads default to the sidecar's share"
        );
        let pinned = VoiceConfig {
            render_workers: 0,
            render_threads: Some(2),
            ..VoiceConfig::default()
        };
        assert!(sidecar_args(&pinned)
            .join(" ")
            .ends_with("--workers 1 --threads 2"));
    }

    #[test]
    fn restarts_back_off_and_are_capped_per_window() {
        let t0 = Instant::now();
        let mut b = RestartBudget::default();
        b.started(t0);
        assert_eq!(b.after_exit(t0), (Duration::from_secs(1), false));
        b.started(t0 + Duration::from_secs(1));
        assert_eq!(
            b.after_exit(t0 + Duration::from_secs(1)),
            (Duration::from_secs(2), false)
        );
        b.started(t0 + Duration::from_secs(3));
        assert_eq!(
            b.after_exit(t0 + Duration::from_secs(3)),
            (Duration::from_secs(4), false)
        );
        // The fourth start (the third restart) used the window up.
        b.started(t0 + Duration::from_secs(7));
        let (wait, capped) = b.after_exit(t0 + Duration::from_secs(7));
        assert!(capped);
        assert_eq!(wait, RESTART_WINDOW - Duration::from_secs(7));
        // A process that reached hello resets the backoff, not the window.
        b.healthy();
        let later = t0 + RESTART_WINDOW + Duration::from_secs(10);
        assert_eq!(b.after_exit(later), (Duration::from_secs(1), false));
    }

    #[tokio::test]
    async fn the_protocol_routes_parts_by_id_and_fails_what_is_in_flight() {
        let r = renderer();
        // No process: a say fails at once.
        let mut dead = r.say("x:1", "hi");
        assert!(matches!(
            dead.recv().await,
            Some(Part::Error { part: 0, .. })
        ));

        let (in_tx, mut in_rx) = mpsc::unbounded_channel();
        r.shared().stdin = Some(in_tx);
        let mut rx = r.say("r:4", "Kneel. Now.");
        let sent: serde_json::Value = serde_json::from_str(&in_rx.recv().await.unwrap()).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({"type":"say","id":"r:4","text":"Kneel. Now."})
        );

        let mut hello = false;
        assert_eq!(
            r.on_line(r#"{"type":"hello","protocol":1,"voice":"af_heart","sample_rate":24000,"visemes":false,"reason":"no numpy"}"#, &mut hello),
            Ok(true)
        );
        assert!(hello);
        assert_eq!(
            *r.state.borrow(),
            RendererState::Ready {
                visemes: false,
                reason: Some("no numpy".into())
            }
        );
        r.on_line(r#"{"type":"audio","id":"r:4","part":0,"last":false,"text":"Kneel.","sample_rate":24000,"pcm":"AAAA","visemes":[["PP",0,40.4],["zz",40,80],["aa",40,90]]}"#, &mut hello).unwrap();
        r.on_line("not json", &mut hello).unwrap();
        r.on_line(
            r#"{"type":"audio","id":"other","part":0,"last":true,"sample_rate":24000,"pcm":""}"#,
            &mut hello,
        )
        .unwrap();
        match rx.recv().await.unwrap() {
            Part::Audio(a) => {
                assert_eq!((a.part, a.last, a.text.as_str()), (0, false, "Kneel."));
                assert_eq!(a.visemes.len(), 2, "the unknown shape is dropped");
                assert_eq!(a.visemes[0].end_ms, 40);
            }
            other => panic!("{other:?}"),
        }
        // The process dies mid-line: the line fails from the part it reached.
        r.fail_pending("gone");
        assert_eq!(
            rx.recv().await,
            Some(Part::Error {
                part: 1,
                reason: "gone".into()
            })
        );

        // A cancel forgets the id and tells the sidecar.
        let mut c = r.say("r:5", "x");
        in_rx.recv().await.unwrap();
        r.cancel("r:5");
        let cancel: serde_json::Value = serde_json::from_str(&in_rx.recv().await.unwrap()).unwrap();
        assert_eq!(cancel, serde_json::json!({"type":"cancel","ids":["r:5"]}));
        assert!(c.recv().await.is_none());

        assert!(r
            .on_line(
                r#"{"type":"hello","protocol":2,"visemes":true}"#,
                &mut hello
            )
            .is_err());
    }

    #[tokio::test]
    async fn a_missing_engine_is_degraded_without_spawning() {
        let d = tempfile::tempdir().unwrap();
        let cfg = VoiceConfig {
            enabled: true,
            python: "definitely-not-a-python-xyz".into(),
            engine_dir: Some(d.path().to_string_lossy().into_owned()),
            ..VoiceConfig::default()
        };
        let (r, task) = SidecarRenderer::start(cfg, std::future::pending());
        task.await.unwrap();
        match &*r.state().borrow() {
            RendererState::Degraded(why) => assert!(why.contains("voice engine missing"), "{why}"),
            other => panic!("{other:?}"),
        }
    }
}
