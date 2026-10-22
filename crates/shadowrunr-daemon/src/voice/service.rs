//! The voice service: one per daemon ([`VoiceHub`], in `AppState`). It voices the live runs
//! that `/voice` clients follow, and only those:
//!
//! - **Feeders.** The first client to follow a run starts a feeder on it: the same
//!   `LiveRun::attach` that `/events` uses. Each `say` with `speaker: malakor` becomes a line
//!   `<run_id>:<seq>`. A feeder reads the run's broadcast and never waits on anything, so the
//!   brain never waits on the voice. A feeder that falls behind the broadcast reads what it
//!   missed from the run's history, so no line is lost. When the run's last follower leaves,
//!   its feeder stops and its waiting lines leave the queue.
//! - **The ledger.** Each line is logged `queued` in its run's `voice.jsonl` as it enters the
//!   queue, and then ends there in its `rendered` parts or one `skipped` (`unfollowed` for a
//!   line whose run nobody follows any more, which no frame carries).
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
//! - **Per stream.** With [`Voices`] attached (the daemon), each run's `stream_start` resolves
//!   the streaming character's voice (`super::settings`: package, daemon, local override). A
//!   voice that differs from the one speaking replaces its renderer before the run's first line:
//!   the old one stops (its lines in flight end with it) and the new one starts. The status names
//!   the provider and the voice. Nothing ever falls back to another provider.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use shadowrunr_types::event::Speaker;
use shadowrunr_types::voice::{VoiceFrame, VoiceSkip, VoiceState};
use shadowrunr_types::voice_choice::{ProviderStatus, VoiceProvider};
use shadowrunr_types::StreamEvent;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, oneshot, watch, Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

use super::log::{Usage, VoiceLogs};
use super::renderer::{duration_ms, Part, Renderer, RendererState, Utter};
use super::VoiceConfig;
use crate::live::{Frame, LiveItem, LiveRun};

/// Frames a run's broadcast holds for a follower that has not read them.
pub const RUN_BROADCAST: usize = 64;
/// Failed lines in a row before the status says `degraded`.
pub const FAILING_AFTER: u32 = 3;

/// A voice the hub can speak with: what the status names, and how it is told apart.
#[derive(Debug, Clone)]
pub struct Spoken {
    pub provider: VoiceProvider,
    /// The voice's name on the status frame.
    pub voice: String,
    /// Two voices with the same key speak alike: a stream start keeps the running renderer.
    pub key: String,
    /// What the renderer is built from.
    pub spec: super::settings::Spec,
}

/// Where the hub's renderers come from, per stream (the daemon's is
/// `super::settings::DaemonVoices`; tests use a fake).
pub trait Voices: Send + Sync + 'static {
    /// The voice `character` speaks with now; `None` keeps the running renderer.
    fn resolve(&self, character: &str) -> Option<Spoken>;
    /// Start a renderer for `spoken`. It stops once `stop` is set or dropped; the handle, when
    /// there is one, ends after it has stopped.
    fn start(
        &self,
        spoken: &Spoken,
        stop: watch::Receiver<bool>,
    ) -> (Arc<dyn Renderer>, Option<JoinHandle<()>>);
}

/// The renderer speaking now.
#[derive(Default)]
struct Current {
    provider: Option<VoiceProvider>,
    voice: Option<String>,
    /// `None` for a renderer attached directly (tests, replay): never swapped.
    key: Option<String>,
    renderer: Option<Arc<dyn Renderer>>,
    /// Dropping it stops the renderer.
    stop: Option<watch::Sender<bool>>,
    watcher: Option<JoinHandle<()>>,
    supervisor: Option<JoinHandle<()>>,
}

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
    /// The text and its delivery cue, as the `say` carried them.
    pub utter: Utter,
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
    current: Mutex<Current>,
    voices: OnceLock<Arc<dyn Voices>>,
    started: AtomicBool,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    /// The preview renderer (`voice.preview`, `voice.bench`), apart from the show's; its lock
    /// is one preview at a time.
    pub(crate) preview: tokio::sync::Mutex<Option<super::settings::PreviewVoice>>,
    /// The last `voice.verify` verdict per provider.
    pub(crate) verdicts: Mutex<HashMap<VoiceProvider, ProviderStatus>>,
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
                provider: Some(VoiceProvider::Kokoro),
            }
        } else {
            VoiceFrame::Status {
                state: VoiceState::Off,
                reason: Some("[voice] enabled = false".into()),
                voice: None,
                visemes: false,
                provider: None,
            }
        };
        Arc::new(Self {
            cfg,
            status: watch::Sender::new(VoiceItem::new(status)),
            health: Mutex::new(Health::default()),
            runs: Mutex::new(HashMap::new()),
            queue: Mutex::new(VecDeque::new()),
            queued: Notify::new(),
            current: Mutex::new(Current::default()),
            voices: OnceLock::new(),
            started: AtomicBool::new(false),
            tasks: Mutex::new(Vec::new()),
            preview: tokio::sync::Mutex::new(None),
            verdicts: Mutex::new(HashMap::new()),
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

    /// Start speaking through `renderer`, for good: its state drives the status, and the
    /// worker takes lines until `shutdown`. Only the first attach counts.
    pub fn attach(
        self: &Arc<Self>,
        renderer: Arc<dyn Renderer>,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        if !self.cfg.enabled || self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        self.install(Installed {
            renderer,
            provider: VoiceProvider::Kokoro,
            voice: self.cfg.voice.clone(),
            key: None,
            stop: None,
            supervisor: None,
        });
        let worker = tokio::spawn(worker(self.clone(), shutdown));
        lock(&self.tasks).push(worker);
    }

    /// Start speaking with `first`, then with whatever each stream start resolves (`voices`),
    /// until `shutdown`. Only the first attach counts.
    pub fn attach_voices(
        self: &Arc<Self>,
        voices: Arc<dyn Voices>,
        first: &Spoken,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        if !self.cfg.enabled || self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.voices.set(voices);
        self.speak_with(first);
        let worker = tokio::spawn(worker(self.clone(), shutdown));
        lock(&self.tasks).push(worker);
    }

    /// A run streaming `character` started: resolve its voice, and replace the renderer when
    /// the voice differs from the one speaking.
    pub fn stream_start(self: &Arc<Self>, character: &str) {
        let Some(voices) = self.voices.get() else {
            return;
        };
        let Some(spoken) = voices.resolve(character) else {
            return;
        };
        if lock(&self.current).key.as_deref() == Some(spoken.key.as_str()) {
            return;
        }
        self.speak_with(&spoken);
    }

    fn speak_with(self: &Arc<Self>, spoken: &Spoken) {
        let Some(voices) = self.voices.get() else {
            return;
        };
        let (stop, stop_rx) = watch::channel(false);
        let (renderer, supervisor) = voices.start(spoken, stop_rx);
        self.install(Installed {
            renderer,
            provider: spoken.provider,
            voice: spoken.voice.clone(),
            key: Some(spoken.key.clone()),
            stop: Some(stop),
            supervisor,
        });
    }

    /// Make a renderer the one speaking. The one before it stops (its stop is dropped), and the
    /// health starts over.
    fn install(self: &Arc<Self>, i: Installed) {
        let watcher = tokio::spawn(watch_renderer(
            Arc::downgrade(self),
            i.renderer.state(),
            i.key.clone(),
        ));
        let old = {
            let mut c = lock(&self.current);
            *lock(&self.health) = Health::default();
            std::mem::replace(
                &mut *c,
                Current {
                    provider: Some(i.provider),
                    voice: Some(i.voice),
                    key: i.key,
                    renderer: Some(i.renderer),
                    stop: i.stop,
                    watcher: Some(watcher),
                    supervisor: i.supervisor,
                },
            )
        };
        if let Some(w) = old.watcher {
            w.abort();
        }
        drop(old.stop);
        self.publish_status();
    }

    /// The renderer speaking now, and its provider.
    fn speaking(&self) -> Option<(Arc<dyn Renderer>, Option<VoiceProvider>)> {
        let c = lock(&self.current);
        c.renderer.clone().map(|r| (r, c.provider))
    }

    /// The provider speaking now (`None` before any renderer).
    pub fn provider(&self) -> Option<VoiceProvider> {
        lock(&self.current).provider
    }

    /// Stop the renderer speaking now, and hand back its supervisor to wait on.
    pub fn stop_renderer(&self) -> Option<JoinHandle<()>> {
        let mut c = lock(&self.current);
        if let Some(stop) = c.stop.take() {
            stop.send_replace(true);
        }
        c.supervisor.take()
    }

    /// Stop every task (feeders, the worker and the status watcher).
    pub fn stop(&self) {
        for t in lock(&self.tasks).drain(..) {
            t.abort();
        }
        if let Some(w) = lock(&self.current).watcher.take() {
            w.abort();
        }
        for r in lock(&self.runs).values_mut() {
            if let Some(f) = r.feeder.take() {
                f.abort();
            }
        }
    }

    fn publish_status(&self) {
        let (voice, provider) = {
            let c = lock(&self.current);
            (
                c.voice.clone().or_else(|| Some(self.cfg.voice.clone())),
                Some(c.provider.unwrap_or(VoiceProvider::Kokoro)),
            )
        };
        let h = lock(&self.health);
        let frame = match (&h.renderer, &h.failing) {
            (None, _) | (Some(RendererState::Starting), _) => VoiceFrame::Status {
                state: VoiceState::Starting,
                reason: None,
                voice,
                visemes: false,
                provider,
            },
            (Some(RendererState::Degraded(why)), _) => VoiceFrame::Status {
                state: VoiceState::Degraded,
                reason: Some(why.clone()),
                voice,
                visemes: false,
                provider,
            },
            (Some(RendererState::Ready { visemes, .. }), Some(why)) => VoiceFrame::Status {
                state: VoiceState::Degraded,
                reason: Some(format!("renders failing: {why}")),
                voice,
                visemes: *visemes,
                provider,
            },
            (Some(RendererState::Ready { visemes, reason }), None) => VoiceFrame::Status {
                state: VoiceState::Ready,
                reason: reason.clone(),
                voice,
                visemes: *visemes,
                provider,
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
        if !live && self.started.load(Ordering::SeqCst) && !run.is_done() {
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
            drop(runs);
            // Its waiting lines leave the queue, each with its end in the log.
            let gone = {
                let mut q = lock(&self.queue);
                let (gone, keep): (VecDeque<Line>, VecDeque<Line>) =
                    q.drain(..).partition(|l| l.run_id == run_id);
                *q = keep;
                gone
            };
            for l in gone {
                self.logs.unfollowed(&l.run_id, l.seq);
            }
        }
    }

    pub(crate) fn followed(&self, run_id: &str) -> bool {
        lock(&self.runs).contains_key(run_id)
    }

    fn send(&self, run_id: &str, frame: VoiceFrame, usage: Option<Usage>) {
        self.logs.service(&frame, usage);
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
            None,
        );
    }

    /// Queue a line (logged `queued`); past `max_queue` the oldest waiting line is skipped
    /// (`queue_full`).
    pub fn enqueue(&self, line: Line) {
        self.logs.queued(
            &line.run_id,
            line.seq,
            &line.utter.text,
            line.utter.delivery,
            self.provider(),
        );
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

async fn watch_renderer(
    hub: Weak<VoiceHub>,
    mut rx: watch::Receiver<RendererState>,
    key: Option<String>,
) {
    loop {
        let st = rx.borrow_and_update().clone();
        let Some(h) = hub.upgrade() else { return };
        if lock(&h.current).key != key {
            // Replaced: the renderer speaking now has its own watcher.
            return;
        }
        lock(&h.health).renderer = Some(st);
        h.publish_status();
        drop(h);
        if rx.changed().await.is_err() {
            return;
        }
    }
}

/// Malakor's `say` line (its text and delivery), when the frame is one.
pub(crate) fn malakor_say(frame: &Frame) -> Option<Utter> {
    if !frame.contains("\"say\"") {
        return None;
    }
    match serde_json::from_str::<StreamEvent>(frame).ok()? {
        StreamEvent::Say {
            speaker: Speaker::Host,
            text,
            delivery,
            ..
        } if !text.trim().is_empty() => Some(Utter { text, delivery }),
        _ => None,
    }
}

/// The character a `stream_start` frame names.
pub(crate) fn stream_character(frame: &Frame) -> Option<String> {
    if !frame.contains("\"stream_start\"") {
        return None;
    }
    match serde_json::from_str::<StreamEvent>(frame).ok()? {
        StreamEvent::StreamStart { character, .. } => Some(character),
        _ => None,
    }
}

/// Queue each of the run's Malakor lines after `since`, until the run ends. The run's
/// `stream_start` resolves its voice first (read from the run when it came before `since`).
async fn feed(hub: Weak<VoiceHub>, run: Arc<LiveRun>, since: u64) {
    if since > 0 {
        if let (Some(h), Some(character)) = (hub.upgrade(), run.character()) {
            h.stream_start(&character);
        }
    }
    let attach = run.attach(since);
    let push = |seq: u64, f: &Frame| {
        if let Some(character) = stream_character(f) {
            if let Some(h) = hub.upgrade() {
                h.stream_start(&character);
            }
            return true;
        }
        let Some(utter) = malakor_say(f) else {
            return true;
        };
        let Some(h) = hub.upgrade() else { return false };
        h.enqueue(Line {
            run_id: run.run_id.clone(),
            seq,
            utter,
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
            Err(RecvError::Lagged(_)) => {
                // The broadcast went past the feeder, but the run keeps its whole history:
                // read what was missed from it, then follow on from there.
                let again = run.attach(last);
                for (seq, f) in &again.history {
                    last = *seq;
                    if !push(*seq, f) {
                        return;
                    }
                }
                match again.rx {
                    Some(r) => rx = r,
                    None => return,
                }
            }
        }
    }
}

/// Hand queued lines to the renderer, up to `lines_in_flight` at once, until `shutdown`. Each
/// line in flight is its own task, so the renderer can work ahead; their frames still go out one
/// line after another, in the order the lines left the queue.
async fn worker(
    hub: Arc<VoiceHub>,
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
        // The renderer speaking when the line leaves the queue speaks all of it.
        let Some((renderer, provider)) = hub.speaking() else {
            continue;
        };
        let (done, next) = oneshot::channel();
        let order = Order {
            prev: prev.replace(next),
            since: None,
            held: Vec::new(),
        };
        let voice = Voice { renderer, provider };
        flights.spawn(fly(hub.clone(), voice, line, slot, order, done));
    }
}

/// One line's place in the output: its frames wait until the line before it is done.
struct Order {
    /// Resolves when the line before is done; `None` once it is (or there was none).
    prev: Option<oneshot::Receiver<()>>,
    /// When this line's turn came.
    since: Option<Instant>,
    /// Frames made before the turn came, with their usage.
    held: Vec<(VoiceFrame, Option<Usage>)>,
}

/// A renderer to make the one speaking.
struct Installed {
    renderer: Arc<dyn Renderer>,
    provider: VoiceProvider,
    voice: String,
    key: Option<String>,
    stop: Option<watch::Sender<bool>>,
    supervisor: Option<JoinHandle<()>>,
}

/// The renderer a line speaks through, and its provider (for the log).
struct Voice {
    renderer: Arc<dyn Renderer>,
    provider: Option<VoiceProvider>,
}

impl Order {
    fn emit(&mut self, hub: &VoiceHub, run_id: &str, frame: VoiceFrame) {
        self.emit_with(hub, run_id, frame, None);
    }

    fn emit_with(&mut self, hub: &VoiceHub, run_id: &str, frame: VoiceFrame, usage: Option<Usage>) {
        if self.prev.is_some() {
            self.held.push((frame, usage));
        } else {
            hub.send(run_id, frame, usage);
        }
    }

    /// The line before is done (or gone): send what was held, and send directly from now on.
    fn take_turn(&mut self, hub: &VoiceHub, run_id: &str) {
        self.prev = None;
        self.since.get_or_insert_with(Instant::now);
        for (f, usage) in self.held.drain(..) {
            hub.send(run_id, f, usage);
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
    voice: Voice,
    line: Line,
    slot: OwnedSemaphorePermit,
    mut order: Order,
    done: oneshot::Sender<()>,
) {
    if order.prev.is_none() {
        order.take_turn(&hub, &line.run_id);
    }
    speak(&hub, &voice, &line, &mut order).await;
    drop(slot);
    order.wait_turn(&hub, &line.run_id).await;
    let _ = done.send(());
}

async fn speak(hub: &VoiceHub, voice: &Voice, line: &Line, order: &mut Order) {
    let renderer = voice.renderer.as_ref();
    if !hub.followed(&line.run_id) {
        hub.logs.unfollowed(&line.run_id, line.seq);
        return;
    }
    let lag_by = line.arrived + Duration::from_millis(hub.cfg.max_lag_ms);
    if Instant::now() >= lag_by {
        order.emit(hub, &line.run_id, skipped(line, 0, VoiceSkip::Late));
        return;
    }
    // A renderer just started (a stream start swapped it) takes the line once it is up.
    let mut st = renderer.state();
    let up = st.wait_for(|s| !matches!(s, RendererState::Starting));
    if tokio::time::timeout_at(lag_by, up).await.is_err() {
        order.emit(hub, &line.run_id, skipped(line, 0, VoiceSkip::Late));
        return;
    }
    let render = Duration::from_millis(hub.cfg.render_timeout_ms.max(1));
    let id = format!("{}:{}", line.run_id, line.seq);
    let mut parts = renderer.say(&id, &line.utter);
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
                let usage = voice.provider.map(|provider| Usage {
                    provider,
                    chars: r.chars,
                    timings: r.timings,
                });
                order.emit_with(
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
                    usage,
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
