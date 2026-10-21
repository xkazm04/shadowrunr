//! The voice service: one per daemon ([`VoiceHub`], in `AppState`). It voices the live runs
//! that `/voice` clients follow, and only those:
//!
//! - **Feeders.** The first client to follow a run starts a feeder on it: the same
//!   `LiveRun::attach` that `/events` uses. Each `say` with `speaker: malakor` becomes a line
//!   `<run_id>:<seq>`. A feeder reads the run's broadcast and never waits on anything, so the
//!   brain never waits on the voice. When the run's last follower leaves, its feeder stops.
//! - **The queue.** At most `max_queue` lines wait; a new line past that pushes out the oldest,
//!   which is `skipped` with `queue_full`.
//! - **The worker.** Up to `lines_in_flight` lines are with the [`Renderer`] at once, so it can
//!   render ahead into the next line while the current one is still going out. Each line's
//!   frames still go out in turn: a line's frames wait until the line before it is done, so a
//!   run's utterances stay in seq order. A line whose first audio has not reached the daemon
//!   within `max_lag_ms` of its `say` reaching the daemon is cancelled and `skipped` with `late`
//!   (also when it is already that old as it leaves the queue), whether or not its turn has
//!   come; the sidecar sends lines in order, so a waiting line's clock includes the time the
//!   lines ahead of it take. A part that fails, or does not arrive within `render_timeout_ms` of
//!   the part before (or of the line's turn), skips the rest of the line with `error`. Three
//!   failed lines in a row turn the status `degraded` until a line speaks again.
//! - **Fan-out.** Frames go out on a broadcast per run; the status is one watch, so a new client
//!   gets the current status first.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use shadowrunr_types::event::Speaker;
use shadowrunr_types::voice::{VoiceFrame, VoiceSkip, VoiceState};
use shadowrunr_types::StreamEvent;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, oneshot, watch, Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

use super::log::VoiceLogs;
use super::renderer::{duration_ms, Part, Renderer, RendererState};
use super::VoiceConfig;
use crate::live::{Frame, LiveItem, LiveRun};

/// Frames a run's broadcast holds for a follower that has not read them.
pub const RUN_BROADCAST: usize = 64;
/// Failed lines in a row before the status says `degraded`.
pub const FAILING_AFTER: u32 = 3;

/// A frame for `/voice` clients, serialised once.
#[derive(Debug, Clone)]
pub struct VoiceItem {
    pub frame: Arc<VoiceFrame>,
    pub json: Arc<str>,
}

impl VoiceItem {
    pub fn new(frame: VoiceFrame) -> Self {
        let json = Arc::from(serde_json::to_string(&frame).unwrap_or_default());
        Self {
            frame: Arc::new(frame),
            json,
        }
    }
}

/// One `say` waiting to be spoken.
#[derive(Debug, Clone)]
pub struct Line {
    pub run_id: String,
    pub seq: u64,
    pub text: String,
    /// When the `say` reached the daemon: the lag clock starts here.
    pub arrived: Instant,
}

struct RunVoice {
    tx: broadcast::Sender<VoiceItem>,
    followers: usize,
    feeder: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Health {
    renderer: Option<RendererState>,
    /// Lines failed in a row, and the last reason.
    failed: u32,
    failing: Option<String>,
}

pub struct VoiceHub {
    cfg: VoiceConfig,
    status: watch::Sender<VoiceItem>,
    health: Mutex<Health>,
    runs: Mutex<HashMap<String, RunVoice>>,
    queue: Mutex<VecDeque<Line>>,
    queued: Notify,
    renderer: OnceLock<Arc<dyn Renderer>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    /// Each run's `voice.jsonl` (`log`): what went out, and what the stage reported.
    logs: VoiceLogs,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl VoiceHub {
    /// The hub, idle: `off` when disabled, else `starting` until a renderer is attached.
    pub fn new(cfg: VoiceConfig) -> Arc<Self> {
        let status = if cfg.enabled {
            VoiceFrame::Status {
                state: VoiceState::Starting,
                reason: Some("the voice service is starting".into()),
                voice: Some(cfg.voice.clone()),
                visemes: false,
            }
        } else {
            VoiceFrame::Status {
                state: VoiceState::Off,
                reason: Some("[voice] enabled = false".into()),
                voice: None,
                visemes: false,
            }
        };
        Arc::new(Self {
            cfg,
            status: watch::Sender::new(VoiceItem::new(status)),
            health: Mutex::new(Health::default()),
            runs: Mutex::new(HashMap::new()),
            queue: Mutex::new(VecDeque::new()),
            queued: Notify::new(),
            renderer: OnceLock::new(),
            tasks: Mutex::new(Vec::new()),
            logs: VoiceLogs::default(),
        })
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    pub fn config(&self) -> &VoiceConfig {
        &self.cfg
    }

    /// The runs' voice logs.
    pub fn logs(&self) -> &VoiceLogs {
        &self.logs
    }

    pub fn status(&self) -> watch::Receiver<VoiceItem> {
        self.status.subscribe()
    }

    /// Start speaking through `renderer`: its state drives the status, and the worker takes
    /// lines until `shutdown`. Only the first attach counts.
    pub fn attach(
        self: &Arc<Self>,
        renderer: Arc<dyn Renderer>,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        if !self.cfg.enabled || self.renderer.set(renderer.clone()).is_err() {
            return;
        }
        let watcher = tokio::spawn(watch_renderer(Arc::downgrade(self), renderer.state()));
        let worker = tokio::spawn(worker(self.clone(), renderer, shutdown));
        lock(&self.tasks).extend([watcher, worker]);
    }

    /// Stop every task (feeders, the worker and the status watcher).
    pub fn stop(&self) {
        for t in lock(&self.tasks).drain(..) {
            t.abort();
        }
        for r in lock(&self.runs).values_mut() {
            if let Some(f) = r.feeder.take() {
                f.abort();
            }
        }
    }

    fn publish_status(&self) {
        let h = lock(&self.health);
        let voice = Some(self.cfg.voice.clone());
        let frame = match (&h.renderer, &h.failing) {
            (None, _) | (Some(RendererState::Starting), _) => VoiceFrame::Status {
                state: VoiceState::Starting,
                reason: None,
                voice,
                visemes: false,
            },
            (Some(RendererState::Degraded(why)), _) => VoiceFrame::Status {
                state: VoiceState::Degraded,
                reason: Some(why.clone()),
                voice,
                visemes: false,
            },
            (Some(RendererState::Ready { visemes, .. }), Some(why)) => VoiceFrame::Status {
                state: VoiceState::Degraded,
                reason: Some(format!("renders failing: {why}")),
                voice,
                visemes: *visemes,
            },
            (Some(RendererState::Ready { visemes, reason }), None) => VoiceFrame::Status {
                state: VoiceState::Ready,
                reason: reason.clone(),
                voice,
                visemes: *visemes,
            },
        };
        drop(h);
        self.status.send_if_modified(|cur| {
            if *cur.frame == frame {
                return false;
            }
            *cur = VoiceItem::new(frame);
            true
        });
    }

    fn line_spoke(&self) {
        let mut h = lock(&self.health);
        let changed = h.failing.is_some();
        h.failed = 0;
        h.failing = None;
        drop(h);
        if changed {
            self.publish_status();
        }
    }

    fn line_failed(&self, why: &str) {
        let mut h = lock(&self.health);
        h.failed += 1;
        if h.failed < FAILING_AFTER {
            return;
        }
        h.failing = Some(why.to_string());
        drop(h);
        self.publish_status();
    }

    /// Follow `run`: its frames, and a feeder while anyone follows it. `from_start` voices the
    /// run's earlier lines too (a run that has just started); otherwise only what follows.
    pub fn follow(self: &Arc<Self>, run: &Arc<LiveRun>, from_start: bool) -> Following {
        let mut runs = lock(&self.runs);
        let entry = runs.entry(run.run_id.clone()).or_insert_with(|| RunVoice {
            tx: broadcast::Sender::new(RUN_BROADCAST),
            followers: 0,
            feeder: None,
        });
        entry.followers += 1;
        let live = entry.feeder.as_ref().is_some_and(|f| !f.is_finished());
        if !live && self.renderer.get().is_some() && !run.is_done() {
            let since = if from_start { 0 } else { run.len() };
            entry.feeder = Some(tokio::spawn(feed(Arc::downgrade(self), run.clone(), since)));
        }
        Following {
            run_id: run.run_id.clone(),
            rx: entry.tx.subscribe(),
            hub: Arc::downgrade(self),
        }
    }

    fn unfollow(&self, run_id: &str) {
        let mut runs = lock(&self.runs);
        let Some(r) = runs.get_mut(run_id) else {
            return;
        };
        r.followers = r.followers.saturating_sub(1);
        if r.followers == 0 {
            if let Some(f) = r.feeder.take() {
                f.abort();
            }
            runs.remove(run_id);
        }
    }

    pub(crate) fn followed(&self, run_id: &str) -> bool {
        lock(&self.runs).contains_key(run_id)
    }

    fn send(&self, run_id: &str, frame: VoiceFrame) {
        self.logs.service(&frame);
        if let Some(r) = lock(&self.runs).get(run_id) {
            let _ = r.tx.send(VoiceItem::new(frame));
        }
    }

    fn skip(&self, line: &Line, part: u32, reason: VoiceSkip) {
        self.send(
            &line.run_id,
            VoiceFrame::Skipped {
                run_id: line.run_id.clone(),
                seq: line.seq,
                part,
                reason,
            },
        );
    }

    /// Queue a line; past `max_queue` the oldest waiting line is skipped (`queue_full`).
    pub fn enqueue(&self, line: Line) {
        let dropped = {
            let mut q = lock(&self.queue);
            q.push_back(line);
            let mut dropped = Vec::new();
            while q.len() > self.cfg.max_queue.max(1) {
                dropped.extend(q.pop_front());
            }
            dropped
        };
        for d in dropped {
            self.skip(&d, 0, VoiceSkip::QueueFull);
        }
        self.queued.notify_one();
    }

    async fn next_line(&self) -> Line {
        loop {
            let notified = self.queued.notified();
            if let Some(l) = lock(&self.queue).pop_front() {
                return l;
            }
            notified.await;
        }
    }

    /// Lines waiting (for tests and the status line).
    pub fn queued(&self) -> usize {
        lock(&self.queue).len()
    }
}

/// A client's hold on one run's frames; dropping it lets the feeder go.
pub struct Following {
    pub run_id: String,
    pub rx: broadcast::Receiver<VoiceItem>,
    hub: Weak<VoiceHub>,
}

impl Drop for Following {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            hub.unfollow(&self.run_id);
        }
    }
}

async fn watch_renderer(hub: Weak<VoiceHub>, mut rx: watch::Receiver<RendererState>) {
    loop {
        let st = rx.borrow_and_update().clone();
        let Some(h) = hub.upgrade() else { return };
        lock(&h.health).renderer = Some(st);
        h.publish_status();
        drop(h);
        if rx.changed().await.is_err() {
            return;
        }
    }
}

/// Malakor's `say` text, when the frame is one.
pub(crate) fn malakor_say(frame: &Frame) -> Option<String> {
    if !frame.contains("\"say\"") {
        return None;
    }
    match serde_json::from_str::<StreamEvent>(frame).ok()? {
        StreamEvent::Say {
            speaker: Speaker::Malakor,
            text,
            ..
        } if !text.trim().is_empty() => Some(text),
        _ => None,
    }
}

/// Queue each of the run's Malakor lines after `since`, until the run ends.
async fn feed(hub: Weak<VoiceHub>, run: Arc<LiveRun>, since: u64) {
    let attach = run.attach(since);
    let push = |seq: u64, f: &Frame| {
        let Some(text) = malakor_say(f) else {
            return true;
        };
        let Some(h) = hub.upgrade() else { return false };
        h.enqueue(Line {
            run_id: run.run_id.clone(),
            seq,
            text,
            arrived: Instant::now(),
        });
        true
    };
    for (seq, f) in &attach.history {
        if !push(*seq, f) {
            return;
        }
    }
    let Some(mut rx) = attach.rx else { return };
    let mut last = attach.history.last().map_or(since, |(s, _)| *s);
    loop {
        match rx.recv().await {
            Ok(LiveItem::Event(seq, f)) => {
                if seq <= last {
                    continue;
                }
                last = seq;
                if !push(seq, &f) {
                    return;
                }
            }
            Ok(LiveItem::End(_)) | Err(RecvError::Closed) => return,
            Err(RecvError::Lagged(n)) => {
                eprintln!(
                    "voice: {} fell {n} events behind; those lines are not voiced",
                    run.run_id
                );
            }
        }
    }
}

/// Hand queued lines to the renderer, up to `lines_in_flight` at once, until `shutdown`. Each
/// line in flight is its own task, so the renderer can work ahead; their frames still go out one
/// line after another, in the order the lines left the queue.
async fn worker(
    hub: Arc<VoiceHub>,
    renderer: Arc<dyn Renderer>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) {
    tokio::pin!(shutdown);
    let slots = Arc::new(Semaphore::new(hub.cfg.lines_in_flight.max(1)));
    // Dropped on return: every line still in flight stops with the worker.
    let mut flights = JoinSet::new();
    let mut prev: Option<oneshot::Receiver<()>> = None;
    loop {
        let slot = tokio::select! {
            s = slots.clone().acquire_owned() => match s {
                Ok(s) => s,
                Err(_) => return,
            },
            () = &mut shutdown => return,
        };
        let line = tokio::select! {
            l = hub.next_line() => l,
            () = &mut shutdown => return,
        };
        while flights.try_join_next().is_some() {}
        let (done, next) = oneshot::channel();
        let order = Order {
            prev: prev.replace(next),
            since: None,
            held: Vec::new(),
        };
        flights.spawn(fly(hub.clone(), renderer.clone(), line, slot, order, done));
    }
}

/// One line's place in the output: its frames wait until the line before it is done.
struct Order {
    /// Resolves when the line before is done; `None` once it is (or there was none).
    prev: Option<oneshot::Receiver<()>>,
    /// When this line's turn came.
    since: Option<Instant>,
    /// Frames made before the turn came.
    held: Vec<VoiceFrame>,
}

impl Order {
    fn emit(&mut self, hub: &VoiceHub, run_id: &str, frame: VoiceFrame) {
        if self.prev.is_some() {
            self.held.push(frame);
        } else {
            hub.send(run_id, frame);
        }
    }

    /// The line before is done (or gone): send what was held, and send directly from now on.
    fn take_turn(&mut self, hub: &VoiceHub, run_id: &str) {
        self.prev = None;
        self.since.get_or_insert_with(Instant::now);
        for f in self.held.drain(..) {
            hub.send(run_id, f);
        }
    }

    async fn wait_turn(&mut self, hub: &VoiceHub, run_id: &str) {
        if let Some(p) = self.prev.as_mut() {
            let _ = p.await;
        }
        self.take_turn(hub, run_id);
    }
}

/// A line in flight: render it within the bounds, give its slot back as soon as the renderer is
/// done with it, then send its frames in its turn.
async fn fly(
    hub: Arc<VoiceHub>,
    renderer: Arc<dyn Renderer>,
    line: Line,
    slot: OwnedSemaphorePermit,
    mut order: Order,
    done: oneshot::Sender<()>,
) {
    if order.prev.is_none() {
        order.take_turn(&hub, &line.run_id);
    }
    speak(&hub, renderer.as_ref(), &line, &mut order).await;
    drop(slot);
    order.wait_turn(&hub, &line.run_id).await;
    let _ = done.send(());
}

async fn speak(hub: &VoiceHub, renderer: &dyn Renderer, line: &Line, order: &mut Order) {
    if !hub.followed(&line.run_id) {
        return;
    }
    let lag_by = line.arrived + Duration::from_millis(hub.cfg.max_lag_ms);
    if Instant::now() >= lag_by {
        order.emit(hub, &line.run_id, skipped(line, 0, VoiceSkip::Late));
        return;
    }
    let render = Duration::from_millis(hub.cfg.render_timeout_ms.max(1));
    let id = format!("{}:{}", line.run_id, line.seq);
    let mut parts = renderer.say(&id, &line.text);
    let mut next = 0u32;
    let mut last_at = Instant::now();
    loop {
        // The render clock runs from the later of the last part and the line's turn. A line
        // still waiting for its turn has only the lag bound on its first part: the renderer
        // sends lines in order, so its first part cannot come before the line ahead is done.
        let render_by = match order.since {
            Some(t) => Some(t.max(last_at) + render),
            None if next > 0 => Some(last_at + render),
            None => None,
        };
        // The first part races the lag bound as well as the render timeout.
        let (by, late) = match render_by {
            Some(r) if next > 0 || r <= lag_by => (r, false),
            _ => (lag_by, true),
        };
        let got = tokio::select! {
            biased;
            _ = async { order.prev.as_mut().expect("guarded").await }, if order.prev.is_some() => {
                order.take_turn(hub, &line.run_id);
                continue;
            }
            got = tokio::time::timeout_at(by, parts.recv()) => got,
        };
        match got {
            Ok(Some(Part::Audio(r))) => {
                next = r.part + 1;
                last_at = Instant::now();
                let last = r.last;
                order.emit(
                    hub,
                    &line.run_id,
                    VoiceFrame::Utterance {
                        id: format!("{id}:{}", r.part),
                        run_id: line.run_id.clone(),
                        seq: line.seq,
                        part: r.part,
                        last,
                        duration_ms: duration_ms(&r.pcm, r.sample_rate),
                        text: r.text,
                        sample_rate: r.sample_rate,
                        pcm: r.pcm,
                        visemes: r.visemes,
                    },
                );
                if last {
                    hub.line_spoke();
                    return;
                }
            }
            Ok(Some(Part::Error { part, reason })) => {
                order.emit(hub, &line.run_id, skipped(line, part, VoiceSkip::Error));
                hub.line_failed(&reason);
                return;
            }
            Ok(None) => {
                order.emit(hub, &line.run_id, skipped(line, next, VoiceSkip::Error));
                hub.line_failed("the renderer dropped the line");
                return;
            }
            Err(_) => {
                renderer.cancel(&id);
                if late {
                    order.emit(hub, &line.run_id, skipped(line, 0, VoiceSkip::Late));
                } else {
                    order.emit(hub, &line.run_id, skipped(line, next, VoiceSkip::Error));
                    hub.line_failed(&format!(
                        "a sentence took longer than {} ms",
                        hub.cfg.render_timeout_ms
                    ));
                }
                return;
            }
        }
    }
}

fn skipped(line: &Line, part: u32, reason: VoiceSkip) -> VoiceFrame {
    VoiceFrame::Skipped {
        run_id: line.run_id.clone(),
        seq: line.seq,
        part,
        reason,
    }
}

#[cfg(test)]
mod tests;
