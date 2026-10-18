//! Live `sim.start` runs: the simulation runs on a blocking thread (its brain need not be
//! `Send`), while a tailer follows its `transcript.jsonl` and fans each complete line out to
//! `/events` subscribers. Subscribers that attach late get the run's history first, then the
//! new events, with nothing lost or duplicated.
//!
//! The sim writes its transcript through a buffered writer, so events reach subscribers in
//! buffer-sized batches rather than one by one (see the WP5 report: an event sink in
//! `SimOptions` would make this exact).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use shadowrunr_types::{SegmentId, SimRunSpec, StreamEvent};
use tokio::sync::{mpsc, watch};

use crate::preflight;
use crate::rpc::RpcError;
use crate::runs;
use crate::server::AppState;

/// A frame for `/events` subscribers: a raw `StreamEvent` JSON line.
pub type Frame = Arc<str>;

const TAIL_EVERY: Duration = Duration::from_millis(100);

struct LiveInner {
    history: Vec<Frame>,
    subscribers: Vec<mpsc::UnboundedSender<Frame>>,
    done: bool,
}

pub struct LiveRun {
    pub run_id: String,
    inner: Mutex<LiveInner>,
}

impl LiveRun {
    fn new(run_id: String) -> Self {
        Self {
            run_id,
            inner: Mutex::new(LiveInner {
                history: Vec::new(),
                subscribers: Vec::new(),
                done: false,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LiveInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn push(&self, frame: Frame) {
        let mut g = self.lock();
        g.history.push(frame.clone());
        g.subscribers.retain(|s| s.send(frame.clone()).is_ok());
    }

    /// End the run. `error` is sent to current subscribers as a final
    /// `{"error": {...}}` frame (not a `StreamEvent`; clients that only read events ignore it).
    fn finish(&self, error: Option<RpcError>) {
        let mut g = self.lock();
        g.done = true;
        if let Some(e) = error {
            let frame: Frame = Arc::from(e.body().to_string());
            for s in &g.subscribers {
                let _ = s.send(frame.clone());
            }
        }
        g.subscribers.clear();
    }

    /// The history so far plus a receiver for what follows (`None` once the run is done).
    pub fn attach(&self) -> (Vec<Frame>, Option<mpsc::UnboundedReceiver<Frame>>) {
        let mut g = self.lock();
        let history = g.history.clone();
        if g.done {
            return (history, None);
        }
        let (tx, rx) = mpsc::unbounded_channel();
        g.subscribers.push(tx);
        (history, Some(rx))
    }

    pub fn is_done(&self) -> bool {
        self.lock().done
    }
}

/// Runs in flight, plus the most recent one for `{"subscribe": "live"}`.
pub struct LiveRegistry {
    runs: Mutex<HashMap<String, Arc<LiveRun>>>,
    latest: watch::Sender<Option<Arc<LiveRun>>>,
}

impl Default for LiveRegistry {
    fn default() -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
            latest: watch::Sender::new(None),
        }
    }
}

impl LiveRegistry {
    fn runs(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<LiveRun>>> {
        self.runs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// An active run by id.
    pub fn get(&self, run_id: &str) -> Option<Arc<LiveRun>> {
        self.runs().get(run_id).cloned()
    }

    pub fn latest(&self) -> watch::Receiver<Option<Arc<LiveRun>>> {
        self.latest.subscribe()
    }

    fn register(&self, run_id: &str, max: usize) -> Result<Arc<LiveRun>, RpcError> {
        let mut runs = self.runs();
        if runs.contains_key(run_id) {
            return Err(RpcError::new(
                "busy",
                format!("run {run_id} is already running"),
            ));
        }
        if runs.len() >= max.max(1) {
            return Err(RpcError::new(
                "busy",
                format!("{} live runs already in flight (max_live_runs)", runs.len()),
            ));
        }
        let run = Arc::new(LiveRun::new(run_id.to_string()));
        runs.insert(run_id.to_string(), run.clone());
        drop(runs);
        self.latest.send_replace(Some(run.clone()));
        Ok(run)
    }

    fn remove(&self, run_id: &str) {
        self.runs().remove(run_id);
    }
}

/// Follows a growing JSONL file, yielding only complete, parseable lines.
struct Tail {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
}

impl Tail {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            partial: Vec::new(),
        }
    }

    fn drain(&mut self, run: &LiveRun) {
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return;
        };
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        let Ok(n) = f.read_to_end(&mut buf) else {
            return;
        };
        self.offset += n as u64;
        self.partial.extend_from_slice(&buf);
        while let Some(pos) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line);
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            if serde_json::from_str::<StreamEvent>(text).is_ok() {
                run.push(Arc::from(text));
            }
        }
    }
}

fn validate(spec: &mut SimRunSpec) -> Result<(), RpcError> {
    runs::check_id("character", &spec.character)?;
    runs::check_id("tone_variant", &spec.tone_variant)?;
    if matches!(
        spec.segment,
        SegmentId::Seg02bCartography | SegmentId::Seg02bDependency
    ) {
        return Err(RpcError::invalid(format!(
            "segment {} is descoped",
            shadowrunr_sim::wire(&spec.segment)
        )));
    }
    if spec.duration_s == 0 || spec.audience_size == 0 {
        return Err(RpcError::invalid(
            "duration_s and audience_size must be positive",
        ));
    }
    if spec.run_id.trim().is_empty() {
        spec.run_id = format!(
            "{}.{}",
            shadowrunr_sim::matrix::run_id(
                &spec.tone_variant,
                spec.core_tier,
                spec.segment,
                spec.audience_size,
                spec.seed
            ),
            runs::now_ms()
        );
    }
    runs::check_id("run_id", &spec.run_id)
}

/// `sim.start`: refuse while a pre-flight check is red, then run in the background and
/// stream on `/events`. The run lands in `<runs>/sim/<adhoc_dir>/<run_id>/`.
pub async fn start_sim(
    state: Arc<AppState>,
    mut spec: SimRunSpec,
    judge: bool,
) -> Result<String, RpcError> {
    validate(&mut spec)?;
    let reports = preflight::run_all(&state.cfg, &state.engine).await;
    if preflight::any_red(&reports) {
        let red: Vec<String> = reports
            .iter()
            .filter(|r| r.state == shadowrunr_types::HealthState::Red)
            .map(|r| format!("{} {}", r.check, r.detail))
            .collect();
        return Err(RpcError::new(
            "preflight_red",
            format!("pre-flight is red: {}", red.join("; ")),
        ));
    }
    let run_id = spec.run_id.clone();
    let run = state
        .live
        .register(&run_id, state.cfg.daemon.max_live_runs)?;
    let out_dir = state.cfg.paths.sim_runs().join(&state.cfg.sim.adhoc_dir);
    let run_dir = out_dir.join(&run_id);
    if let Err(e) = runs::move_aside(&run_dir, &format!("attempt-{}", runs::now_ms())) {
        state.live.remove(&run_id);
        run.finish(None);
        return Err(RpcError::io(format!("{}: {e}", run_dir.display())));
    }

    let engine = state.engine.engine.clone();
    let opts = state.cfg.sim_options(judge);
    let handle = tokio::runtime::Handle::current();
    let mut sim = tokio::task::spawn_blocking(move || {
        handle.block_on(shadowrunr_sim::run_one_with(spec, out_dir, engine, &opts))
    });
    let mut tail = Tail::new(run_dir.join("transcript.jsonl"));
    let registry = state.clone();
    tokio::spawn(async move {
        let result = loop {
            tokio::select! {
                res = &mut sim => break res,
                _ = tokio::time::sleep(TAIL_EVERY) => tail.drain(&run),
            }
        };
        tail.drain(&run);
        let error = match result {
            Ok(Ok(_)) => None,
            Ok(Err(e)) => Some(RpcError::new("sim_failed", e.to_string())),
            Err(e) => Some(RpcError::new("sim_failed", format!("sim task: {e}"))),
        };
        if let Some(e) = &error {
            eprintln!("sim.start {}: {e}", run.run_id);
        }
        registry.live.remove(&run.run_id);
        run.finish(error);
    });
    Ok(run_id)
}
