//! Sandbox sessions: the simulated room and the real brain on the wall clock. Stream time
//! is wall time since the session started, so a slow engine call shows up as a silence
//! (`say-gap-ceiling`). The daemon hosts a session (`sandbox.start`), hands every event to
//! its live broadcast through the [`EventSink`], and completes the report's `stage_ack` and
//! `judge` checks; this module fills in `say_gap`, `core_errors`, `sign_off` and `chaos`.
//!
//! - The host beats every `tick_s` on a tokio interval. A beat that overruns starts the next
//!   one at once and never bursts to catch up; each event is stamped when it happens.
//! - The chat room runs as its own task on its own beat. Bulk chat and the featured-chatter
//!   call are made off the host's critical path and reach the brain through a channel it
//!   drains every beat; the room reads the show back from a shared transcript tail.
//! - Every event is appended to `transcript.jsonl`, flushed at once and handed to the sink.
//! - The last [`CLOSING_S`] seconds (or the moment `stop` turns true) are the closing window:
//!   the host is asked to sign off, and the driver ends the stream at the deadline anyway.
//! - Chaos wraps the brain's engine only ([`ChaosEngine`]); the room keeps talking.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use shadowrunr_brain::BrainTuning;
use shadowrunr_segments::{segment_table, HandoverRule, SegmentConfig, SegmentRuntime};
use shadowrunr_types::sandbox::{
    SandboxChaos, SandboxCheck, SandboxCheckState, SandboxJudge, SandboxReport, SandboxStartParams,
    SANDBOX_CHECKS, SANDBOX_MINUTES,
};
use shadowrunr_types::{
    ChatMsg, CheckState, EndReason, Engine, EngineError, EngineReply, EngineRequest, FleetHealth,
    HealthReport, ModelTier, Op, Role, SegmentId, SimRunSpec, Speaker, StreamEvent, TsMs,
    WarmSession,
};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};

use crate::chat::{ChatRoom, ChatStats, Directives, RoomConfig};
use crate::judge::{self, JudgeInputs, JudgeOutcome, Rubric};
use crate::ledger::{self, LedgerEngine};
use crate::runner::{self, Mind, Stage, JUDGE_WINDOWS_DIR};
use crate::situations::{Injection, SituationBank};
use crate::transcript::{self, TranscriptWriter};
use crate::{population, read_json, stats, write_json, SimError, SimOptions};

/// Audience size of a sandbox session's simulated room.
pub const SANDBOX_AUDIENCE: u32 = 30;
/// The character a sandbox session runs.
pub const SANDBOX_CHARACTER: &str = "malakor";
/// The calibration population mix of the room (as a 30-viewer tournament cell).
pub const SANDBOX_POPULATION: &str = "growing_channel";
/// The closing window: the last this many seconds of a session.
pub const CLOSING_S: u32 = 60;
/// A sign-off counts when it lands in the last this many seconds.
pub const SIGN_OFF_WINDOW_S: u32 = 90;
/// After a stop request, the host has this long to sign off.
pub const STOP_SIGN_OFF_S: u32 = 10;
/// The say-gap ceiling (`say-gap-ceiling`): the longest silence a session may hold.
pub const SAY_GAP_CEILING_S: f64 = 15.0;
/// Length of the `outage` chaos window.
pub const OUTAGE_S: u32 = 90;
/// Shortest session `lost` chaos accepts.
pub const LOST_MIN_MINUTES: u32 = 10;
/// A `lost` sign-off counts within the fleet grace plus this many seconds of the outage start.
pub const LOST_SIGN_OFF_SLACK_S: u32 = 15;
/// The handover of a test-length session (`planned_s` given), percent of the session.
pub const TEST_HANDOVER_PCT: u32 = 40;
/// The moderation rule on a stall line (the brain speaks it while the fleet is down).
pub const STALL_RULE: &str = "engine_stall";
/// `<dir>/room.json`: the room's featured chatters, stimuli and stats (the judge reads it).
pub const ROOM_FILE: &str = "room.json";
/// `<dir>/session.json`: the session's report.
pub const SESSION_FILE: &str = "session.json";

/// Transcript events the room can read back (it shows the chatter model the last 30).
const SHARED_TAIL: usize = 400;
/// The room beats this long before the host, so its chat is waiting when the host drains.
const ROOM_LEAD_MS: u64 = 500;

/// One session to run.
#[derive(Debug, Clone)]
pub struct SandboxRun {
    pub session_id: String,
    pub params: SandboxStartParams,
    /// `runs/sandbox/<session_id>`: transcript.jsonl, ledger.jsonl, spec.json, session.json.
    pub dir: PathBuf,
    pub core_tier: ModelTier,
    /// Tone when `params.tone` is absent (the character's active tone).
    pub default_tone: String,
    /// Planned length in seconds; `None` = `params.minutes * 60`. Tests only.
    pub planned_s: Option<u32>,
}

/// Called once per transcript event, in order, as it happens; `seq` is the event's 1-based
/// line in transcript.jsonl (the `/events` envelope seq).
pub type EventSink = Arc<dyn Fn(u64, &StreamEvent) + Send + Sync>;

/// Live progress, published on every beat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SandboxProgress {
    pub elapsed_s: u32,
    pub planned_s: u32,
    pub events: u64,
    pub max_say_gap_s: f64,
    pub core_errors: u32,
    /// The closing window has started (planned end or a stop request).
    pub closing: bool,
}

/// `spec.json`: the run spec (as a tournament run's, so run tooling reads it) plus the
/// session's params and the programme they resolved to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSpec {
    #[serde(flatten)]
    pub run: SimRunSpec,
    pub params: SandboxStartParams,
    pub planned_s: u32,
    pub tick_s: u32,
    /// The segments in order: SEG-01, then SEG-02A on the fictional demo case.
    pub programme: Vec<SegmentId>,
    /// When SEG-01's handover clock fires (its `max_elapsed_s`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handover_s: Option<u32>,
    /// The chaos window `[from, to)` in stream seconds; `to` is the planned end for `lost`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chaos_window_s: Option<(u32, u32)>,
}

/// `room.json`: what the room did, besides the transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxRoom {
    pub featured: Vec<String>,
    /// Every scheduled situation and what became of it.
    pub injections: Vec<Injection>,
    pub chat: ChatStats,
    /// Viewers removed from chat (`crisis_chat_ban`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub banned: Vec<String>,
    /// Beats the brain failed (not engine fleet errors, which it rides out).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub brain_errors: Vec<String>,
}

/// What a session's params resolve to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Programme {
    pub planned_s: u32,
    /// SEG-01's handover; `None` keeps SEG-01 for the whole session.
    pub handover_s: Option<u32>,
    pub chaos: Option<SandboxChaos>,
    /// The chaos window in stream ms, `[from, to)`.
    pub chaos_ms: Option<(TsMs, TsMs)>,
}

impl Programme {
    pub(crate) fn resolve(run: &SandboxRun) -> Result<Self, SimError> {
        let minutes = run.params.minutes;
        if !SANDBOX_MINUTES.contains(&minutes) {
            return Err(SimError::Spec(format!(
                "minutes must be one of {SANDBOX_MINUTES:?}, not {minutes}"
            )));
        }
        let planned_s = run.planned_s.unwrap_or(minutes * 60);
        if planned_s == 0 {
            return Err(SimError::Spec("planned_s must be positive".into()));
        }
        let chaos = run.params.chaos;
        if chaos == Some(SandboxChaos::Lost) && minutes < LOST_MIN_MINUTES {
            return Err(SimError::Spec(format!(
                "chaos lost needs a session of {LOST_MIN_MINUTES} minutes or more"
            )));
        }
        let handover_s = match (minutes, run.planned_s) {
            (5, _) => None,
            (_, Some(p)) => Some((p * TEST_HANDOVER_PCT / 100).max(1)),
            (10, None) => Some(240),
            _ => Some(600),
        };
        let half = i64::from(planned_s) * 500;
        let chaos_ms = chaos.map(|c| match c {
            SandboxChaos::Outage => (half, half + i64::from(OUTAGE_S) * 1000),
            SandboxChaos::Lost => (half, i64::from(planned_s) * 1000),
        });
        Ok(Self {
            planned_s,
            handover_s,
            chaos,
            chaos_ms,
        })
    }

    pub(crate) fn segments(&self) -> Vec<SegmentId> {
        match self.handover_s {
            Some(_) => vec![SegmentId::Seg01ImprovisedRoom, SegmentId::Seg02aHexmap],
            None => vec![SegmentId::Seg01ImprovisedRoom],
        }
    }

    /// SEG-01's handover rule for this session: the clock alone decides. The audience
    /// threshold is put out of reach, as a 30-viewer room would otherwise hand over on its
    /// sustained ccv after 180 s, before the programme's handover time.
    pub(crate) fn handover_rule(&self) -> Result<HandoverRule, SimError> {
        let table = segment_table(SegmentId::Seg01ImprovisedRoom)
            .map_err(|e| SimError::Spec(format!("segment table: {e}")))?;
        let base = table
            .clock
            .handover
            .ok_or_else(|| SimError::Spec("SEG-01 has no handover rule".into()))?;
        Ok(HandoverRule {
            ccv_min: u32::MAX,
            max_elapsed_s: self.handover_s.unwrap_or(u32::MAX),
            ..base
        })
    }

    /// Stream ms the driver ends the stream by, signed off or not. With `lost` the fleet's
    /// sign-off may come up to the grace (plus slack) after the outage starts, which in a
    /// 10-minute session is at the planned end.
    fn deadline_ms(&self, grace_s: u64) -> TsMs {
        let planned = i64::from(self.planned_s) * 1000;
        match (self.chaos, self.chaos_ms) {
            (Some(SandboxChaos::Lost), Some((from, _))) => {
                planned.max(from + (grace_s as i64 + i64::from(LOST_SIGN_OFF_SLACK_S)) * 1000)
            }
            _ => planned,
        }
    }
}

/// A seed from the session id, when the params give none.
fn seed_of(session_id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in session_id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn now_epoch_ms() -> TsMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as TsMs)
        .unwrap_or(0)
}

fn since_ms(start: Instant) -> TsMs {
    start.elapsed().as_millis() as TsMs
}

// ----- chaos --------------------------------------------------------------------------

/// What the chaos wrapper saw: calls it refused, and real core-thread errors.
#[derive(Debug, Default)]
pub struct ChaosCounters {
    /// Calls refused inside the window, every role.
    pub injected: AtomicU32,
    /// Core-thread calls refused inside the window.
    pub injected_core: AtomicU32,
    /// Core-thread calls the engine itself failed (outside the window).
    pub core_errors: AtomicU32,
}

/// The brain's engine with an injected fleet failure: inside the window every call (and every
/// warm-session turn) fails with `unavailable`, which the brain classifies as fleet loss
/// (`fleet_class` in the brain: rate_limited, unavailable, timeout and io). Outside it, calls
/// pass through. With no window it only counts the core thread's own errors.
pub struct ChaosEngine {
    inner: Arc<dyn Engine>,
    start: Instant,
    window: Option<(TsMs, TsMs)>,
    pub counters: Arc<ChaosCounters>,
}

/// The error a chaos window returns.
pub fn chaos_error() -> EngineError {
    EngineError::Unavailable {
        reason: "sandbox chaos: the engine fleet is down".into(),
    }
}

fn in_window(start: Instant, window: Option<(TsMs, TsMs)>) -> bool {
    let t = since_ms(start);
    window.is_some_and(|(from, to)| from <= t && t < to)
}

impl ChaosEngine {
    /// `window` is `[from, to)` in ms since `start`.
    pub fn new(inner: Arc<dyn Engine>, start: Instant, window: Option<(TsMs, TsMs)>) -> Self {
        Self {
            inner,
            start,
            window,
            counters: Arc::new(ChaosCounters::default()),
        }
    }
}

fn refuse(counters: &ChaosCounters, role: Role) -> EngineError {
    counters.injected.fetch_add(1, Ordering::Relaxed);
    if role == Role::CoreThread {
        counters.injected_core.fetch_add(1, Ordering::Relaxed);
    }
    chaos_error()
}

fn count_core_error(counters: &ChaosCounters, role: Role, res: &Result<EngineReply, EngineError>) {
    if role == Role::CoreThread && res.is_err() {
        counters.core_errors.fetch_add(1, Ordering::Relaxed);
    }
}

#[async_trait]
impl Engine for ChaosEngine {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
        if in_window(self.start, self.window) {
            return Err(refuse(&self.counters, req.role));
        }
        let role = req.role;
        let res = self.inner.complete(req).await;
        count_core_error(&self.counters, role, &res);
        res
    }

    async fn open_session(
        &self,
        template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError> {
        let role = template.role;
        if in_window(self.start, self.window) {
            return Err(refuse(&self.counters, role));
        }
        let inner = self.inner.open_session(template).await?;
        Ok(Box::new(ChaosSession {
            inner,
            role,
            start: self.start,
            window: self.window,
            counters: self.counters.clone(),
        }))
    }

    async fn health(&self) -> Vec<HealthReport> {
        self.inner.health().await
    }
}

struct ChaosSession {
    inner: Box<dyn WarmSession>,
    role: Role,
    start: Instant,
    window: Option<(TsMs, TsMs)>,
    counters: Arc<ChaosCounters>,
}

#[async_trait]
impl WarmSession for ChaosSession {
    async fn send(&mut self, prompt: String) -> Result<EngineReply, EngineError> {
        if in_window(self.start, self.window) {
            return Err(refuse(&self.counters, self.role));
        }
        let res = self.inner.send(prompt).await;
        count_core_error(&self.counters, self.role, &res);
        res
    }
    async fn interrupt(&mut self) -> Result<(), EngineError> {
        self.inner.interrupt().await
    }
    async fn close(self: Box<Self>) -> Result<(), EngineError> {
        self.inner.close().await
    }
}

// ----- the shared transcript ------------------------------------------------------------

/// What the host shares with the room: the transcript's tail and the stage's view.
#[derive(Default)]
struct Shared {
    tail: VecDeque<StreamEvent>,
    /// Events ever pushed (the tail's last one is number `total`).
    total: u64,
    open_poll: Option<Option<(String, Vec<String>)>>,
    ccv: u32,
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// The session's transcript: every event written, flushed, shared and handed to the sink in
/// order, on a stream clock that never runs backwards.
struct Log {
    w: TranscriptWriter,
    sink: EventSink,
    shared: Arc<Mutex<Shared>>,
    last_ts: TsMs,
    last_say: TsMs,
    max_gap_ms: TsMs,
    segment: SegmentId,
}

impl Log {
    fn push(&mut self, mut ev: StreamEvent) -> Result<(), SimError> {
        let ts = transcript::ts(&ev);
        if ts < self.last_ts {
            transcript::set_ts(&mut ev, self.last_ts);
        }
        let ts = ts.max(self.last_ts);
        self.last_ts = ts;
        match &ev {
            StreamEvent::Say {
                speaker: Speaker::Malakor,
                ..
            } => {
                self.max_gap_ms = self.max_gap_ms.max(ts - self.last_say);
                self.last_say = ts;
            }
            StreamEvent::SegmentStart { segment, .. } => self.segment = *segment,
            _ => {}
        }
        let seq = self.w.push(ev)? as u64;
        let ev = &self.w.events()[seq as usize - 1];
        (self.sink)(seq, ev);
        let mut sh = lock(&self.shared);
        sh.tail.push_back(ev.clone());
        if sh.tail.len() > SHARED_TAIL {
            sh.tail.pop_front();
        }
        sh.total = seq;
        Ok(())
    }

    fn push_all(&mut self, evs: impl IntoIterator<Item = StreamEvent>) -> Result<(), SimError> {
        for ev in evs {
            self.push(ev)?;
        }
        Ok(())
    }

    fn events(&self) -> &[StreamEvent] {
        self.w.events()
    }
}

// ----- the room's task --------------------------------------------------------------------

struct RoomTask {
    room: ChatRoom,
    engine: Arc<dyn Engine>,
    shared: Arc<Mutex<Shared>>,
    tx: mpsc::UnboundedSender<Vec<ChatMsg>>,
    start: Instant,
    tick: Duration,
    featured_every: u32,
    correlation_id: String,
}

type FeaturedCall = (JoinHandle<Result<EngineReply, EngineError>>, Directives);

impl RoomTask {
    fn t_s(&self) -> u32 {
        (since_ms(self.start) / 1000) as u32
    }

    fn send(&self, msgs: Vec<ChatMsg>) {
        if !msgs.is_empty() {
            let _ = self.tx.send(msgs);
        }
    }

    /// The room's beat: catch up with the show, then chat. The featured call runs on its
    /// own task, so a slow chatter model delays neither the host nor the room's bulk chat.
    async fn run(mut self, mut done: watch::Receiver<bool>) -> ChatRoom {
        let route = shadowrunr_engine::route(Role::Chatter);
        let lead = Duration::from_millis(ROOM_LEAD_MS).min(self.tick / 2);
        let mut iv = tokio::time::interval_at(self.start + self.tick - lead, self.tick);
        iv.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut seen = 0u64;
        let mut beats = 0u32;
        let mut pending: Option<FeaturedCall> = None;
        loop {
            tokio::select! {
                _ = done.changed() => break,
                res = async { pending.as_mut().map(|(h, _)| h).expect("guarded").await },
                    if pending.is_some() =>
                {
                    let dirs = pending.take().map(|(_, d)| d).unwrap_or_default();
                    let t = self.t_s();
                    match res {
                        Ok(Ok(reply)) => {
                            let msgs = self.room.accept_featured(t, &reply.text, &dirs);
                            self.send(msgs);
                        }
                        _ => self.room.stats.featured_engine_errors += 1,
                    }
                }
                _ = iv.tick() => {
                    let t = self.t_s();
                    let (new, poll, recent) = {
                        let sh = lock(&self.shared);
                        let first = sh.total - sh.tail.len() as u64;
                        let skip = seen.saturating_sub(first) as usize;
                        let new: Vec<StreamEvent> = sh.tail.iter().skip(skip).cloned().collect();
                        seen = sh.total;
                        (new, sh.open_poll.clone(), sh.tail.iter().cloned().collect::<Vec<_>>())
                    };
                    self.room.observe(t, &new);
                    self.room.sync_poll(t, poll);
                    self.room.begin_tick(t);
                    if self.room.silent(t) {
                        self.room.stats.silent_ticks += 1;
                    }
                    let msgs = self.room.bulk(t);
                    lock(&self.shared).ccv = self.room.ccv_at(t);
                    self.send(msgs);
                    if pending.is_none() && beats.is_multiple_of(self.featured_every) {
                        if let Some((req, dirs)) =
                            self.room.featured_request(t, &recent, &self.correlation_id, route)
                        {
                            let engine = self.engine.clone();
                            let call = tokio::spawn(async move { engine.complete(req).await });
                            pending = Some((call, dirs));
                        }
                    }
                    beats += 1;
                }
            }
        }
        if let Some((call, _)) = pending {
            call.abort();
        }
        self.room
    }
}

// ----- the session --------------------------------------------------------------------

/// The closing request the host sees in its state summary.
fn closing_line(now: TsMs, deadline: TsMs, stopped: bool) -> String {
    if stopped {
        "CLOSING NOW: the operator is ending the stream. Say goodbye to the minions in \
         character and emit sign_off this turn."
            .into()
    } else {
        format!(
            "CLOSING: the stream ends in {} s. Wrap up, thank the minions in character, then \
             emit sign_off.",
            ((deadline - now).max(0) + 999) / 1000
        )
    }
}

/// Everything a session's loop needs besides its mind and stage.
struct Session {
    run: SandboxRun,
    programme: Programme,
    tick_s: u32,
    fleet_grace_s: u64,
    started_ms: TsMs,
    start: Instant,
    log: Log,
    chat_rx: mpsc::UnboundedReceiver<Vec<ChatMsg>>,
    room: JoinHandle<ChatRoom>,
    room_done: watch::Sender<bool>,
    chaos: Arc<ChaosCounters>,
    ledger_path: PathBuf,
    stop: watch::Receiver<bool>,
    progress: watch::Sender<SandboxProgress>,
}

/// How a session's stream ended.
struct Ended {
    end_ms: TsMs,
    stopped_ms: Option<TsMs>,
    beat_ms: Vec<u64>,
    brain_errors: Vec<String>,
    room: ChatRoom,
}

impl Session {
    fn progress(&self, closing: bool, events: u64) {
        let p = SandboxProgress {
            elapsed_s: (since_ms(self.start) / 1000) as u32,
            planned_s: self.programme.planned_s,
            events,
            max_say_gap_s: self.log.max_gap_ms as f64 / 1000.0,
            core_errors: self.chaos.core_errors.load(Ordering::Relaxed),
            closing,
        };
        self.progress.send_replace(p);
    }

    async fn drive<M: Mind, S: Stage>(
        &mut self,
        mut mind: M,
        mut stage: S,
    ) -> Result<Ended, SimError> {
        let tick = Duration::from_secs(u64::from(self.tick_s));
        let planned_ms = i64::from(self.programme.planned_s) * 1000;
        let closing_ms = planned_ms - i64::from(CLOSING_S) * 1000;
        let mut deadline = self.programme.deadline_ms(self.fleet_grace_s);
        let mut iv = tokio::time::interval_at(self.start + tick, tick);
        iv.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut stop_open = true;
        let mut stopped_ms: Option<TsMs> = None;
        let mut beat_ms: Vec<u64> = Vec::new();
        let mut brain_errors: Vec<String> = Vec::new();
        let mut signed_off = false;
        let mut fleet_lost = false;
        let mut closing = false;
        let mut stop_now = *self.stop.borrow_and_update();
        loop {
            if !stop_now {
                let deadline_at = self.start + Duration::from_millis(deadline.max(0) as u64);
                tokio::select! {
                    biased;
                    changed = self.stop.changed(), if stop_open => {
                        match changed {
                            Ok(()) => stop_now = *self.stop.borrow_and_update(),
                            Err(_) => stop_open = false,
                        }
                        if !stop_now {
                            continue;
                        }
                    }
                    _ = tokio::time::sleep_until(deadline_at) => break,
                    _ = iv.tick() => {}
                }
            }
            if stop_now {
                // The closing window starts at once, with a short deadline for the sign-off.
                let at = since_ms(self.start);
                stopped_ms = Some(at);
                stop_open = false;
                deadline = deadline.min(at + i64::from(STOP_SIGN_OFF_S) * 1000);
                iv.reset();
            }
            stop_now = false;

            let beat_start = Instant::now();
            let now = since_ms(self.start);
            closing |= now >= closing_ms || stopped_ms.is_some();
            let mut msgs: Vec<ChatMsg> = Vec::new();
            while let Ok(batch) = self.chat_rx.try_recv() {
                msgs.extend(batch);
            }
            msgs.sort_by_key(|m| m.ts_ms);
            for m in &mut msgs {
                // Chat waiting in the channel is stamped no earlier than the transcript.
                m.ts_ms = m.ts_ms.clamp(self.log.last_ts, now.max(self.log.last_ts));
            }
            for m in &msgs {
                self.log.push(StreamEvent::Chat {
                    ts_ms: m.ts_ms,
                    msg: m.clone(),
                })?;
            }
            let ccv = lock(&self.log.shared).ccv;
            let ev = stage.ingest_chat(now, &msgs);
            self.log.push_all(ev)?;

            let mut ctx = stage.context(ccv, (now / 1000) as u32);
            if closing {
                ctx.state_summary.push('\n');
                ctx.state_summary
                    .push_str(&closing_line(now, deadline, stopped_ms.is_some()));
            }
            let mut done = now;
            match mind.tick(now, msgs, &ctx).await {
                Ok(mut evs) => {
                    // The beat's events happen when the brain is done with it.
                    done = since_ms(self.start).max(now);
                    for e in &mut evs {
                        transcript::set_ts(e, done);
                    }
                    let ops: Vec<Op> = evs
                        .iter()
                        .filter_map(|e| match e {
                            StreamEvent::Op { op, .. } => Some(op.clone()),
                            _ => None,
                        })
                        .collect();
                    let sign_off = ops.iter().find_map(|o| match o {
                        Op::SignOff { reason } => Some(reason.clone()),
                        _ => None,
                    });
                    self.log.push_all(evs)?;
                    if !ops.is_empty() {
                        let ev = runner::drop_op_echo(&ops, stage.apply_ops(done, &ops));
                        self.log.push_all(ev)?;
                    }
                    if let Some(reason) = sign_off {
                        if stage.is_signed_off().unwrap_or(true) {
                            signed_off = true;
                            fleet_lost = reason.starts_with(crate::outcome::FLEET_LOST_PREFIX);
                        }
                    }
                }
                Err(e) => brain_errors.push(format!("{}s: {e}", now / 1000)),
            }
            let ev = stage.tick(done, ccv);
            self.log.push_all(ev)?;
            lock(&self.log.shared).open_poll = stage.open_poll();
            beat_ms.push(beat_start.elapsed().as_millis() as u64);
            self.progress(closing, self.log.events().len() as u64);
            if signed_off || since_ms(self.start) >= deadline {
                break;
            }
        }

        let end_ms = since_ms(self.start).max(self.log.last_ts);
        let reason = if stopped_ms.is_some() {
            EndReason::Operator
        } else if fleet_lost {
            EndReason::FleetLost
        } else {
            EndReason::Scheduled
        };
        let segment = self.log.segment;
        self.log.push(StreamEvent::SegmentEnd {
            ts_ms: end_ms,
            segment,
            ses: None,
        })?;
        self.log.push(StreamEvent::StreamEnd {
            ts_ms: end_ms,
            reason,
        })?;
        self.progress(true, self.log.events().len() as u64);
        let _ = self.room_done.send(true);
        let room = (&mut self.room)
            .await
            .map_err(|e| SimError::Io(format!("chat room task: {e}")))?;
        if let Err(e) = mind.end_stream(end_ms).await {
            brain_errors.push(format!("end_stream: {e}"));
        }
        Ok(Ended {
            end_ms,
            stopped_ms,
            beat_ms,
            brain_errors,
            room,
        })
    }

    fn report(&self, ended: &Ended) -> SandboxReport {
        let events = self.log.events();
        let says = malakor_says(events);
        let gaps = say_gaps(&says, ended.end_ms);
        let (max_gap, max_at) = gaps
            .iter()
            .copied()
            .max_by_key(|(g, _)| *g)
            .unwrap_or((0, 0));
        let gap_ms: Vec<u64> = gaps.iter().map(|(g, _)| *g as u64).collect();
        let max_say_gap_s = max_gap as f64 / 1000.0;
        let say_gap = check(
            "say_gap",
            max_say_gap_s <= SAY_GAP_CEILING_S,
            if says.is_empty() {
                format!(
                    "Malakor never spoke in {} s (ceiling {SAY_GAP_CEILING_S} s)",
                    ended.end_ms / 1000
                )
            } else {
                format!(
                    "max {max_say_gap_s:.1} s, from {} to {} (ceiling {SAY_GAP_CEILING_S} s)",
                    transcript::clock(max_at - max_gap),
                    transcript::clock(max_at)
                )
            },
        );

        let injected_core = self.chaos.injected_core.load(Ordering::Relaxed);
        let rows = ledger::read(&self.ledger_path).unwrap_or_default();
        let mut kinds: BTreeMap<String, u32> = BTreeMap::new();
        for r in rows.iter().filter(|r| r.role == Role::CoreThread) {
            if let Some(k) = &r.error_kind {
                *kinds.entry(k.clone()).or_default() += 1;
            }
        }
        let core_errors = kinds.values().sum::<u32>().saturating_sub(injected_core);
        let core = check(
            "core_errors",
            core_errors == 0,
            if core_errors == 0 {
                "no core-thread engine errors outside the chaos window".into()
            } else {
                format!(
                    "{core_errors} core-thread engine errors outside the chaos window ({})",
                    kinds
                        .iter()
                        .map(|(k, n)| format!("{k} {n}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
        );

        let sign_off = self.sign_off_check(events, ended);
        let chaos = self.chaos_check(events, ended);
        let daemon = |id: &str| SandboxCheck {
            id: id.into(),
            state: SandboxCheckState::Unmeasured,
            detail: "filled by the daemon".into(),
        };
        let checks = vec![
            say_gap,
            core,
            sign_off,
            chaos,
            daemon(SANDBOX_CHECKS[4]),
            daemon(SANDBOX_CHECKS[5]),
        ];
        debug_assert!(checks.iter().map(|c| c.id.as_str()).eq(SANDBOX_CHECKS));
        SandboxReport {
            session_id: self.run.session_id.clone(),
            spec: self.run.params.clone(),
            started_ms: self.started_ms,
            ended_ms: self.started_ms + ended.end_ms,
            pass: checks.iter().all(|c| c.state == SandboxCheckState::Pass),
            checks,
            max_say_gap_s,
            say_gap_p95_s: stats::percentile(&gap_ms, 95.0).unwrap_or(0) as f64 / 1000.0,
            beat_ms_p50: stats::percentile(&ended.beat_ms, 50.0).unwrap_or(0),
            beat_ms_p95: stats::percentile(&ended.beat_ms, 95.0).unwrap_or(0),
            core_errors,
            stall_lines: stall_lines(events),
            health_transitions: events
                .iter()
                .filter(|e| matches!(e, StreamEvent::Health { .. }))
                .count() as u32,
            judge: None,
        }
    }

    fn sign_off_check(&self, events: &[StreamEvent], ended: &Ended) -> SandboxCheck {
        let sign_offs: Vec<(TsMs, &str)> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Op {
                    ts_ms,
                    op: Op::SignOff { reason },
                } => Some((*ts_ms, reason.as_str())),
                _ => None,
            })
            .collect();
        let fleet_lost = sign_offs
            .iter()
            .find(|(_, r)| r.starts_with(crate::outcome::FLEET_LOST_PREFIX));
        let stopped = ended
            .stopped_ms
            .map(|s| format!("stopped at {}; ", transcript::clock(s)))
            .unwrap_or_default();
        if self.programme.chaos == Some(SandboxChaos::Lost) {
            if let Some((t, _)) = fleet_lost {
                return check(
                    "sign_off",
                    true,
                    format!("{stopped}fleet_lost sign-off at {}", transcript::clock(*t)),
                );
            }
        }
        let (from, to, window) = match ended.stopped_ms {
            Some(s) => (
                s,
                s + i64::from(STOP_SIGN_OFF_S) * 1000,
                format!("within {STOP_SIGN_OFF_S} s of the stop"),
            ),
            None => (
                i64::from(self.programme.planned_s.saturating_sub(SIGN_OFF_WINDOW_S)) * 1000,
                TsMs::MAX,
                format!("in the last {SIGN_OFF_WINDOW_S} s"),
            ),
        };
        match sign_offs.first() {
            Some((t, _)) if (from..=to).contains(t) => check(
                "sign_off",
                true,
                format!("{stopped}sign-off at {}, {window}", transcript::clock(*t)),
            ),
            Some((t, _)) => check(
                "sign_off",
                false,
                format!(
                    "{stopped}sign-off at {}, not {window}",
                    transcript::clock(*t)
                ),
            ),
            None => check(
                "sign_off",
                false,
                format!(
                    "{stopped}no sign-off {window}: the driver ended the stream at {}",
                    transcript::clock(ended.end_ms)
                ),
            ),
        }
    }

    fn chaos_check(&self, events: &[StreamEvent], ended: &Ended) -> SandboxCheck {
        let (Some(kind), Some((from, to))) = (self.programme.chaos, self.programme.chaos_ms) else {
            return SandboxCheck {
                id: "chaos".into(),
                state: SandboxCheckState::Unmeasured,
                detail: "no chaos".into(),
            };
        };
        let stalls = stall_lines(events);
        if ended.end_ms < from {
            return SandboxCheck {
                id: "chaos".into(),
                state: SandboxCheckState::Unmeasured,
                detail: format!(
                    "the stream ended at {}, before the chaos window",
                    transcript::clock(ended.end_ms)
                ),
            };
        }
        match kind {
            SandboxChaos::Outage => {
                let down = events.iter().position(|e| {
                    matches!(
                        e,
                        StreamEvent::Health {
                            state: FleetHealth::Down,
                            ..
                        }
                    )
                });
                let recovered = down.is_some_and(|i| {
                    events[i..].iter().any(|e| {
                        matches!(
                            e,
                            StreamEvent::Health {
                                state: FleetHealth::Healthy,
                                ..
                            }
                        )
                    })
                });
                let resumed = resumed_say(events, to);
                let pass = down.is_some() && recovered && stalls > 0 && resumed.is_some();
                check(
                    "chaos",
                    pass,
                    format!(
                        "outage {}-{}: health down {}, healthy after {}, {stalls} stall lines, \
                         resumed {}",
                        transcript::clock(from),
                        transcript::clock(to),
                        yes_no(down.is_some()),
                        yes_no(recovered),
                        resumed.map_or("no".into(), transcript::clock)
                    ),
                )
            }
            SandboxChaos::Lost => {
                let by =
                    from + (self.fleet_grace_s as i64 + i64::from(LOST_SIGN_OFF_SLACK_S)) * 1000;
                let sign_off = events.iter().find_map(|e| match e {
                    StreamEvent::Op {
                        ts_ms,
                        op: Op::SignOff { reason },
                    } if reason.starts_with(crate::outcome::FLEET_LOST_PREFIX) => Some(*ts_ms),
                    _ => None,
                });
                let in_time = sign_off.is_some_and(|t| (from..=by).contains(&t));
                check(
                    "chaos",
                    stalls > 0 && in_time,
                    format!(
                        "lost from {}: {stalls} stall lines, fleet_lost sign-off {} (due by {})",
                        transcript::clock(from),
                        sign_off.map_or("none".into(), transcript::clock),
                        transcript::clock(by)
                    ),
                )
            }
        }
    }
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn check(id: &str, pass: bool, detail: String) -> SandboxCheck {
    SandboxCheck {
        id: id.into(),
        state: if pass {
            SandboxCheckState::Pass
        } else {
            SandboxCheckState::Fail
        },
        detail,
    }
}

fn malakor_says(events: &[StreamEvent]) -> Vec<TsMs> {
    events
        .iter()
        .filter(|e| {
            matches!(
                e,
                StreamEvent::Say {
                    speaker: Speaker::Malakor,
                    ..
                }
            )
        })
        .map(transcript::ts)
        .collect()
}

/// Every silence as `(length_ms, ended_at_ms)`: session start to the first say, then between
/// consecutive says. With no say at all, the whole session.
fn say_gaps(says: &[TsMs], end_ms: TsMs) -> Vec<(TsMs, TsMs)> {
    if says.is_empty() {
        return vec![(end_ms, end_ms)];
    }
    let mut prev = 0;
    says.iter()
        .map(|&t| {
            let g = (t - prev, t);
            prev = t;
            g
        })
        .collect()
}

fn is_stall(e: &StreamEvent) -> bool {
    matches!(e, StreamEvent::Moderation { rule, .. } if rule == STALL_RULE)
}

fn stall_lines(events: &[StreamEvent]) -> u32 {
    events.iter().filter(|e| is_stall(e)).count() as u32
}

/// The first Malakor line at or after `at_ms` that is not a stall line (a stall `say` is
/// followed by its `engine_stall` moderation).
fn resumed_say(events: &[StreamEvent], at_ms: TsMs) -> Option<TsMs> {
    events.iter().enumerate().find_map(|(i, e)| match e {
        StreamEvent::Say {
            ts_ms,
            speaker: Speaker::Malakor,
            ..
        } if *ts_ms >= at_ms && !events.get(i + 1).is_some_and(is_stall) => Some(*ts_ms),
        _ => None,
    })
}

/// Run one session on the wall clock until its planned end or until `stop` turns true, then
/// write `session.json` and return its report. `stage_ack` and `judge` are left
/// `unmeasured` for the caller to complete.
pub async fn run_sandbox(
    run: SandboxRun,
    engine: Arc<dyn Engine>,
    opts: &SimOptions,
    sink: EventSink,
    stop: watch::Receiver<bool>,
    progress: watch::Sender<SandboxProgress>,
) -> Result<SandboxReport, SimError> {
    let programme = Programme::resolve(&run)?;
    let seed = run.params.seed.unwrap_or_else(|| seed_of(&run.session_id));
    let tone = run
        .params
        .tone
        .clone()
        .unwrap_or_else(|| run.default_tone.clone());
    let spec = SimRunSpec {
        run_id: run.session_id.clone(),
        character: SANDBOX_CHARACTER.into(),
        tone_variant: tone,
        core_tier: run.core_tier,
        core_effort: shadowrunr_engine::route(Role::CoreThread).1,
        segment: SegmentId::Seg01ImprovisedRoom,
        audience_size: SANDBOX_AUDIENCE,
        seed,
        duration_s: programme.planned_s,
        population: SANDBOX_POPULATION.into(),
    };
    // The session is judged afterwards (`judge_session`), on the fictional demo case.
    let run_opts = SimOptions {
        judge: false,
        case: None,
        ..opts.clone()
    };
    let tick_s = run_opts.tick_s.max(1);
    let prep = runner::prepare_in(&spec, &run.dir, engine.clone(), &run_opts)?;
    write_json(
        &run.dir.join("spec.json"),
        &SandboxSpec {
            run: spec.clone(),
            params: run.params.clone(),
            planned_s: programme.planned_s,
            tick_s,
            programme: programme.segments(),
            handover_s: programme.handover_s,
            chaos_window_s: programme
                .chaos_ms
                .map(|(a, b)| ((a / 1000) as u32, (b / 1000) as u32)),
        },
    )?;

    let memory_dir = runner::temp_memory_dir(&spec.run_id);
    let res = async {
        std::fs::create_dir_all(&memory_dir)
            .map_err(|e| SimError::Io(format!("{}: {e}", memory_dir.display())))?;
        let stage = SegmentRuntime::with_config(
            SegmentId::Seg01ImprovisedRoom,
            SegmentConfig {
                foils: prep.foils.clone(),
                start_ms: Some(0),
                planned_s: Some(programme.planned_s),
                handover: Some(programme.handover_rule()?),
                ..SegmentConfig::default()
            },
        )
        .map_err(|e| SimError::Spec(format!("segment runtime: {e}")))?;

        let start = Instant::now();
        let started_ms = now_epoch_ms();
        // Chaos wraps the brain's engine inside the ledger, so the ledger shows what the
        // brain saw; the room calls the ledgered engine directly.
        let chaos = Arc::new(ChaosEngine::new(engine, start, programme.chaos_ms));
        let counters = chaos.counters.clone();
        let brain_engine: Arc<dyn Engine> = Arc::new(prep.ledger.sibling(chaos));
        // The memory is thrown away after the session: no sleep cycle at its end.
        let tuning = BrainTuning {
            consolidate_at_end: false,
            ..runner::brain_tuning(&run_opts)
        };
        let fleet_grace_s = tuning.fleet_grace_s;
        let brain =
            runner::build_brain(&prep, &run_opts, &memory_dir, brain_engine, &stage, tuning)
                .await?;

        let pop = population::generate(
            &prep.cal,
            &spec.population,
            spec.audience_size,
            spec.duration_s,
            spec.seed,
            run_opts.hooks.rank,
        )?;
        let room = ChatRoom::new(
            pop,
            RoomConfig {
                cal: &prep.cal,
                bank: &prep.bank,
                hooks: run_opts.hooks,
                seed: spec.seed,
                tick_s,
                duration_s: spec.duration_s,
                segment: spec.segment,
                foils: prep.foils.clone(),
                hex_ids: prep.hex_ids.clone(),
                max_featured: run_opts.max_featured,
                streamer_name: prep.streamer.clone(),
            },
        );

        let shared = Arc::new(Mutex::new(Shared {
            ccv: room.ccv_at(0),
            ..Shared::default()
        }));
        let mut log = Log {
            w: TranscriptWriter::create_live(&prep.run_dir.join("transcript.jsonl"))?,
            sink,
            shared: shared.clone(),
            last_ts: 0,
            last_say: 0,
            max_gap_ms: 0,
            segment: spec.segment,
        };
        log.push(StreamEvent::StreamStart {
            ts_ms: 0,
            stream_id: spec.run_id.clone(),
            character: spec.character.clone(),
            tone_variant: spec.tone_variant.clone(),
            core_tier: spec.core_tier,
            audience_size: spec.audience_size,
            brief: prep.brief.clone(),
        })?;
        let mut stage = stage;
        log.push_all(Stage::start(&mut stage, 0, spec.segment))?;

        let (tx, chat_rx) = mpsc::unbounded_channel();
        let (room_done, done_rx) = watch::channel(false);
        let room_task = RoomTask {
            room,
            engine: prep.engine.clone(),
            shared,
            tx,
            start,
            tick: Duration::from_secs(u64::from(tick_s)),
            featured_every: run_opts.featured_every_ticks.max(1),
            correlation_id: spec.run_id.clone(),
        };
        let room = tokio::spawn(room_task.run(done_rx));

        let mut session = Session {
            run: run.clone(),
            programme: programme.clone(),
            tick_s,
            fleet_grace_s,
            started_ms,
            start,
            log,
            chat_rx,
            room,
            room_done,
            chaos: counters,
            ledger_path: prep.run_dir.join("ledger.jsonl"),
            stop,
            progress,
        };
        let ended = session.drive(brain, stage).await?;
        write_json(
            &prep.run_dir.join(ROOM_FILE),
            &SandboxRoom {
                featured: ended.room.featured_ids().to_vec(),
                injections: ended.room.injections.clone(),
                chat: ended.room.stats.clone(),
                banned: ended
                    .room
                    .banned_ids()
                    .into_iter()
                    .map(String::from)
                    .collect(),
                brain_errors: ended.brain_errors.clone(),
            },
        )?;
        let report = session.report(&ended);
        write_json(&prep.run_dir.join(SESSION_FILE), &report)?;
        Ok(report)
    }
    .await;
    if !opts.keep_memory {
        let _ = std::fs::remove_dir_all(&memory_dir);
    }
    res
}

/// Mean of the per-dimension rates of a verdict's quality facts (as the tournament summary
/// takes it); `None` without quality facts.
fn quality_mean(v: &shadowrunr_types::JudgeVerdict) -> Option<f64> {
    let mut dims: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    for q in &v.quality {
        let e = dims.entry(q.dimension.as_str()).or_default();
        e.1 += 1;
        if q.value {
            e.0 += 1;
        }
    }
    if dims.is_empty() {
        return None;
    }
    let rates: Vec<f64> = dims
        .values()
        .map(|(k, n)| f64::from(*k) / f64::from(*n))
        .collect();
    Some(rates.iter().sum::<f64>() / rates.len() as f64)
}

/// Judge a finished session in `dir` with the windowed judge (as a tournament run is), write
/// `verdict.json` and `judge.json` there, and summarise the verdict for the report.
pub async fn judge_session(
    dir: &Path,
    engine: Arc<dyn Engine>,
    opts: &SimOptions,
) -> Result<SandboxJudge, SimError> {
    let spec: SandboxSpec = read_json(&dir.join("spec.json"))?;
    let room: SandboxRoom = read_json(&dir.join(ROOM_FILE))?;
    let run = &spec.run;
    let tier = judge::resolve_judge_tier(opts.judge_tier, &opts.judge_tiers, run.core_tier)?;
    if let Some(block) = opts.quota.as_ref().and_then(|q| q.0.blocked()) {
        return Err(SimError::Spec(format!(
            "{}quota ({})",
            crate::outcome::JUDGE_DEFERRED_PREFIX,
            block.accounts.join(", ")
        )));
    }
    let events = transcript::read(&dir.join("transcript.jsonl"))?;
    let bank =
        SituationBank::load(&runner::character_dir(opts, &run.character).join("situations.json"))?;
    let rubric = Rubric::load(&runner::rubric_path(opts))?;
    let brief = runner::run_brief(dir)?;
    let engine: Arc<dyn Engine> = Arc::new(LedgerEngine::create(
        engine,
        &dir.join("ledger.jsonl"),
        &run.run_id,
    )?);
    let featured = room.featured.iter().cloned().collect();
    let window_cache = dir.join(JUDGE_WINDOWS_DIR);
    let inp = JudgeInputs {
        run_id: &run.run_id,
        events: &events,
        bank: &bank,
        injections: &room.injections,
        featured_viewers: &featured,
        rubric: &rubric,
        judge_tier: tier,
        core_tier: run.core_tier,
        brief: brief.as_ref(),
        window_cache: Some(&window_cache),
    };
    match judge::judge_run(&inp, engine.as_ref()).await? {
        JudgeOutcome::Verdict(b) => {
            let (verdict, audit) = *b;
            write_json(&dir.join("judge.json"), &audit)?;
            write_json(&dir.join("verdict.json"), &verdict)?;
            Ok(SandboxJudge {
                verdict_path: "verdict.json".into(),
                rule_fails: verdict
                    .rules
                    .iter()
                    .filter(|r| r.state == CheckState::Fail)
                    .count() as u32,
                quality_mean: quality_mean(&verdict),
            })
        }
        JudgeOutcome::Void(audit, why) => {
            write_json(&dir.join("judge.json"), &*audit)?;
            Err(SimError::Spec(format!("judge void: {why}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::tests::{reply, test_opts, FakeEngine};

    /// Plays the host for the real brain: a line every beat, the switch when the handover is
    /// due, and a sign-off once the closing request says 5 s or less (or the stop). Chatter
    /// and judge calls go to [`FakeEngine`].
    struct HostEngine {
        fake: FakeEngine,
        sign_off: bool,
        /// The nth core call (0-based) takes this long.
        slow_core: Option<(usize, Duration)>,
        chatter_delay: Duration,
        core_calls: Mutex<Vec<Instant>>,
        chatter_calls: Mutex<Vec<Instant>>,
    }

    impl HostEngine {
        fn new() -> Self {
            Self {
                fake: FakeEngine::new(""),
                sign_off: true,
                slow_core: None,
                chatter_delay: Duration::ZERO,
                core_calls: Mutex::new(Vec::new()),
                chatter_calls: Mutex::new(Vec::new()),
            }
        }

        fn core_text(&self, prompt: &str) -> String {
            let mut t = "Hear me, minions: the Spire listens.".to_string();
            if prompt.contains("HANDOVER DUE") {
                t.push_str("\nOP: {\"op\":\"switch_segment\",\"to\":\"SEG-02A\"}");
            }
            let ends_in = prompt
                .split("CLOSING: the stream ends in ")
                .nth(1)
                .and_then(|r| r.split(' ').next())
                .and_then(|n| n.parse::<u32>().ok());
            if self.sign_off && (prompt.contains("CLOSING NOW") || ends_in.is_some_and(|s| s <= 5))
            {
                t.push_str("\nOP: {\"op\":\"sign_off\",\"reason\":\"the hour is done\"}");
            }
            t
        }

        /// Gaps between consecutive core calls, ms.
        fn core_gaps(&self) -> Vec<u64> {
            let calls = self.core_calls.lock().unwrap();
            calls
                .windows(2)
                .map(|w| (w[1] - w[0]).as_millis() as u64)
                .collect()
        }
    }

    #[async_trait]
    impl Engine for HostEngine {
        async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
            match req.role {
                Role::CoreThread => {
                    let n = {
                        let mut c = self.core_calls.lock().unwrap();
                        c.push(Instant::now());
                        c.len() - 1
                    };
                    if let Some((at, d)) = self.slow_core {
                        if at == n {
                            tokio::time::sleep(d).await;
                        }
                    }
                    Ok(reply(self.core_text(&req.prompt)))
                }
                Role::Chatter => {
                    self.chatter_calls.lock().unwrap().push(Instant::now());
                    if !self.chatter_delay.is_zero() {
                        tokio::time::sleep(self.chatter_delay).await;
                    }
                    self.fake.complete(req).await
                }
                _ => self.fake.complete(req).await,
            }
        }
        async fn open_session(
            &self,
            t: EngineRequest,
        ) -> Result<Box<dyn WarmSession>, EngineError> {
            self.fake.open_session(t).await
        }
        async fn health(&self) -> Vec<HealthReport> {
            Vec::new()
        }
    }

    fn sandbox_run(dir: &Path, minutes: u32, planned_s: u32) -> SandboxRun {
        SandboxRun {
            session_id: "sbx-test".into(),
            params: SandboxStartParams {
                minutes,
                seed: Some(7),
                chaos: None,
                judge: false,
                tone: None,
            },
            dir: dir.join("sbx-test"),
            core_tier: ModelTier::Haiku,
            default_tone: "theatrical".into(),
            planned_s: Some(planned_s),
        }
    }

    fn opts(tmp: &Path) -> SimOptions {
        let mut o = test_opts(&tmp.join("sim"));
        o.hooks = crate::Hooks::default();
        o.judge = false;
        o
    }

    fn no_sink() -> EventSink {
        Arc::new(|_, _| {})
    }

    struct Outcome {
        report: SandboxReport,
        events: Vec<StreamEvent>,
        dir: PathBuf,
        _tmp: Option<tempfile::TempDir>,
    }

    async fn session(
        run: SandboxRun,
        engine: Arc<dyn Engine>,
        opts: &SimOptions,
        sink: EventSink,
        stop: watch::Receiver<bool>,
    ) -> Outcome {
        let (ptx, _prx) = watch::channel(SandboxProgress::default());
        let dir = run.dir.clone();
        let report = run_sandbox(run, engine, opts, sink, stop, ptx)
            .await
            .expect("the session runs");
        let events = transcript::read(&dir.join("transcript.jsonl")).unwrap();
        Outcome {
            report,
            events,
            dir,
            _tmp: None,
        }
    }

    fn state(r: &SandboxReport, id: &str) -> SandboxCheckState {
        r.checks.iter().find(|c| c.id == id).unwrap().state
    }

    fn detail<'a>(r: &'a SandboxReport, id: &str) -> &'a str {
        &r.checks.iter().find(|c| c.id == id).unwrap().detail
    }

    fn sign_off_at(events: &[StreamEvent]) -> Option<TsMs> {
        events.iter().find_map(|e| match e {
            StreamEvent::Op {
                ts_ms,
                op: Op::SignOff { .. },
            } => Some(*ts_ms),
            _ => None,
        })
    }

    #[tokio::test(start_paused = true)]
    async fn a_clean_session_beats_on_the_wall_clock_and_flushes_every_event_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = opts(tmp.path());
        let run = sandbox_run(tmp.path(), 5, 60);
        let path = run.dir.join("transcript.jsonl");
        let seen: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
        let sink: EventSink = {
            let seen = seen.clone();
            Arc::new(move |seq, ev| {
                // The line is on disk when the sink hears of it.
                let text = std::fs::read_to_string(&path).unwrap();
                let lines: Vec<&str> = text.lines().collect();
                assert_eq!(lines.len() as u64, seq, "flushed before the sink");
                assert_eq!(lines[seq as usize - 1], serde_json::to_string(ev).unwrap());
                seen.lock().unwrap().push(seq);
            })
        };
        let (_stop_tx, stop) = watch::channel(false);
        let engine = Arc::new(HostEngine::new());
        let started = Instant::now();
        let out = session(run, engine.clone(), &opts, sink, stop).await;
        let r = &out.report;

        // Wall clock: a beat every 5 s from 5 s, until the sign-off at 55 s.
        let gaps = engine.core_gaps();
        assert_eq!(gaps.len(), 10, "{gaps:?}");
        assert!(gaps.iter().all(|g| *g == 5_000), "{gaps:?}");
        assert_eq!(started.elapsed().as_secs(), 55);

        // The sink saw every line once, in order, on a clock that never runs backwards.
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen, (1..=out.events.len() as u64).collect::<Vec<_>>());
        assert!(out
            .events
            .windows(2)
            .all(|w| transcript::ts(&w[0]) <= transcript::ts(&w[1])));
        assert!(matches!(out.events[0], StreamEvent::StreamStart { .. }));
        assert!(matches!(
            out.events.last(),
            Some(StreamEvent::StreamEnd {
                reason: EndReason::Scheduled,
                ts_ms: 55_000
            })
        ));

        // The gate: the health checks pass, the daemon's are left for the daemon.
        let ids: Vec<&str> = r.checks.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, SANDBOX_CHECKS);
        assert_eq!(state(r, "say_gap"), SandboxCheckState::Pass, "{r:?}");
        assert_eq!(state(r, "core_errors"), SandboxCheckState::Pass);
        assert_eq!(state(r, "sign_off"), SandboxCheckState::Pass, "{r:?}");
        assert_eq!(detail(r, "sign_off"), "sign-off at 00:55, in the last 90 s");
        assert_eq!(state(r, "chaos"), SandboxCheckState::Unmeasured);
        assert_eq!(detail(r, "chaos"), "no chaos");
        for id in ["stage_ack", "judge"] {
            assert_eq!(state(r, id), SandboxCheckState::Unmeasured);
            assert_eq!(detail(r, id), "filled by the daemon");
        }
        assert!(!r.pass, "an unmeasured check is not a pass");
        assert_eq!(r.max_say_gap_s, 5.0);
        assert_eq!(r.say_gap_p95_s, 5.0);
        assert_eq!(r.core_errors, 0);
        assert_eq!((r.stall_lines, r.health_transitions), (0, 0));
        assert_eq!(r.ended_ms - r.started_ms, 55_000);

        // The session dir.
        let stored: SandboxReport = read_json(&out.dir.join(SESSION_FILE)).unwrap();
        assert_eq!(&stored, r);
        let spec: SandboxSpec = read_json(&out.dir.join("spec.json")).unwrap();
        assert_eq!(spec.run.tone_variant, "theatrical");
        assert_eq!(spec.run.seed, 7);
        assert_eq!(spec.run.audience_size, SANDBOX_AUDIENCE);
        assert_eq!(spec.planned_s, 60);
        assert_eq!(spec.programme, vec![SegmentId::Seg01ImprovisedRoom]);
        assert!(out.dir.join("ledger.jsonl").is_file());
        for f in ["spec.json", SESSION_FILE, ROOM_FILE, "transcript.jsonl"] {
            let text = std::fs::read_to_string(out.dir.join(f)).unwrap();
            assert!(!text.contains(":null") && !text.contains(": null"), "{f}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_overrunning_beat_shows_as_a_gap_and_the_next_starts_at_once_without_a_burst() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = opts(tmp.path());
        let engine = Arc::new(HostEngine {
            slow_core: Some((3, Duration::from_secs(12))),
            ..HostEngine::new()
        });
        let (_stop_tx, stop) = watch::channel(false);
        let out = session(
            sandbox_run(tmp.path(), 5, 60),
            engine.clone(),
            &opts,
            no_sink(),
            stop,
        )
        .await;
        // Beats at 5, 10, 15, 20 (12 s long), at once at 32, then every 5 s again.
        let gaps = engine.core_gaps();
        assert_eq!(
            &gaps[..5],
            &[5_000, 5_000, 5_000, 12_000, 5_000],
            "{gaps:?}"
        );
        assert!(gaps.iter().all(|g| *g >= 5_000), "no burst: {gaps:?}");
        // The slow line is stamped when it was spoken: a 17 s silence fails the gate.
        let r = &out.report;
        assert_eq!(r.max_say_gap_s, 17.0, "{r:?}");
        assert_eq!(state(r, "say_gap"), SandboxCheckState::Fail);
        assert_eq!(
            detail(r, "say_gap"),
            "max 17.0 s, from 00:15 to 00:32 (ceiling 15 s)"
        );
        assert!(r.beat_ms_p95 >= 12_000, "{}", r.beat_ms_p95);
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_chatter_model_never_delays_the_host() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = opts(tmp.path());
        let engine = Arc::new(HostEngine {
            chatter_delay: Duration::from_secs(6),
            ..HostEngine::new()
        });
        let (_stop_tx, stop) = watch::channel(false);
        // Five minutes: the room's empty-room silence leaves time to talk around it.
        let out = session(
            sandbox_run(tmp.path(), 5, 300),
            engine.clone(),
            &opts,
            no_sink(),
            stop,
        )
        .await;
        let gaps = engine.core_gaps();
        assert!(gaps.iter().all(|g| *g == 5_000), "{gaps:?}");
        let r = &out.report;
        assert_eq!(state(r, "say_gap"), SandboxCheckState::Pass);
        assert!(r.beat_ms_p95 < 1_000, "{}", r.beat_ms_p95);
        // The featured lines still came, one call at a time.
        let room: SandboxRoom = read_json(&out.dir.join(ROOM_FILE)).unwrap();
        assert!(
            room.chat.featured_msgs > 0 && room.chat.bulk_msgs > 0,
            "{:?}",
            room.chat
        );
        let calls = engine.chatter_calls.lock().unwrap();
        assert!(calls.len() > 1);
        assert!(calls
            .windows(2)
            .all(|w| w[1] - w[0] >= Duration::from_secs(6)));
    }

    async fn stopped_session(sign_off: bool) -> Outcome {
        let tmp = tempfile::tempdir().unwrap();
        let opts = opts(tmp.path());
        let engine = Arc::new(HostEngine {
            sign_off,
            ..HostEngine::new()
        });
        let (stop_tx, stop) = watch::channel(false);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(31_500)).await;
            let _ = stop_tx.send(true);
            // Keep the sender until the session is over.
            tokio::time::sleep(Duration::from_secs(600)).await;
        });
        let mut out = session(
            sandbox_run(tmp.path(), 5, 120),
            engine,
            &opts,
            no_sink(),
            stop,
        )
        .await;
        out._tmp = Some(tmp);
        out
    }

    #[tokio::test(start_paused = true)]
    async fn a_stop_starts_the_closing_at_once_and_the_host_signs_off_within_10_s() {
        let out = stopped_session(true).await;
        let r = &out.report;
        assert_eq!(state(r, "sign_off"), SandboxCheckState::Pass, "{r:?}");
        assert_eq!(
            detail(r, "sign_off"),
            "stopped at 00:31; sign-off at 00:31, within 10 s of the stop"
        );
        assert_eq!(
            sign_off_at(&out.events),
            Some(31_500),
            "the stop beat runs at once"
        );
        assert!(matches!(
            out.events.last(),
            Some(StreamEvent::StreamEnd {
                reason: EndReason::Operator,
                ts_ms: 31_500
            })
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn a_stop_without_a_sign_off_ends_the_stream_10_s_later_and_fails_the_check() {
        let out = stopped_session(false).await;
        let r = &out.report;
        assert_eq!(state(r, "sign_off"), SandboxCheckState::Fail);
        assert!(
            detail(r, "sign_off").starts_with("stopped at 00:31; no sign-off"),
            "{}",
            detail(r, "sign_off")
        );
        assert!(matches!(
            out.events.last(),
            Some(StreamEvent::StreamEnd {
                reason: EndReason::Operator,
                ts_ms: 41_500
            })
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn the_planned_end_ends_an_unsigned_stream_and_fails_the_check() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = opts(tmp.path());
        let engine = Arc::new(HostEngine {
            sign_off: false,
            ..HostEngine::new()
        });
        let (_stop_tx, stop) = watch::channel(false);
        let out = session(
            sandbox_run(tmp.path(), 5, 60),
            engine,
            &opts,
            no_sink(),
            stop,
        )
        .await;
        let r = &out.report;
        assert_eq!(state(r, "sign_off"), SandboxCheckState::Fail);
        assert_eq!(
            detail(r, "sign_off"),
            "no sign-off in the last 90 s: the driver ended the stream at 01:00"
        );
        assert!(matches!(
            out.events.last(),
            Some(StreamEvent::StreamEnd {
                reason: EndReason::Scheduled,
                ts_ms: 60_000
            })
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn the_chaos_engine_fails_every_brain_call_inside_its_window_only() {
        let start = Instant::now();
        let chaos = ChaosEngine::new(Arc::new(FakeEngine::new("")), start, Some((10_000, 20_000)));
        let req = |role| EngineRequest {
            role,
            tier: ModelTier::Haiku,
            effort: shadowrunr_types::Effort::Low,
            system: String::new(),
            prompt: String::new(),
            allowed_tools: Vec::new(),
            timeout_ms: 1_000,
            json_schema: None,
            correlation_id: None,
        };
        assert!(chaos.complete(req(Role::CoreThread)).await.is_ok());
        tokio::time::sleep(Duration::from_secs(10)).await;
        for role in [Role::CoreThread, Role::ChatPicker, Role::Researcher] {
            let e = chaos.complete(req(role)).await.unwrap_err();
            // The brain's fleet class: rate_limited, unavailable, timeout, io.
            assert!(matches!(e, EngineError::Unavailable { .. }), "{e}");
            assert!(crate::outcome::is_engine_error(&e));
        }
        assert!(chaos.open_session(req(Role::CoreThread)).await.is_err());
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert!(chaos.complete(req(Role::CoreThread)).await.is_ok());
        assert_eq!(chaos.counters.injected.load(Ordering::Relaxed), 4);
        assert_eq!(chaos.counters.injected_core.load(Ordering::Relaxed), 2);
        assert_eq!(chaos.counters.core_errors.load(Ordering::Relaxed), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn an_outage_fails_the_brain_in_its_window_while_the_room_keeps_talking() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = opts(tmp.path());
        let mut run = sandbox_run(tmp.path(), 10, 600);
        run.params.chaos = Some(SandboxChaos::Outage);
        let (_stop_tx, stop) = watch::channel(false);
        let out = session(run, Arc::new(HostEngine::new()), &opts, no_sink(), stop).await;
        let spec: SandboxSpec = read_json(&out.dir.join("spec.json")).unwrap();
        assert_eq!(spec.chaos_window_s, Some((300, 390)));

        // The ledger: every core call in the window failed as fleet loss; the room's never did.
        let rows = ledger::read(&out.dir.join("ledger.jsonl")).unwrap();
        let core_failed = rows
            .iter()
            .filter(|r| {
                r.role == Role::CoreThread && r.error_kind.as_deref() == Some("unavailable")
            })
            .count();
        assert_eq!(core_failed, 18, "one per beat from 300 s to 385 s");
        assert!(rows
            .iter()
            .filter(|r| r.role == Role::Chatter)
            .all(|r| r.error_kind.is_none()));
        // The room talks through the window (outside its own empty-room silence), and the
        // host speaks again after it. WP1's stall lines fill the window itself; the integrated
        // test covers them.
        let room: SandboxRoom = read_json(&out.dir.join(ROOM_FILE)).unwrap();
        let (quiet_from, quiet_to) = room
            .injections
            .iter()
            .find_map(|i| i.silence_window)
            .map_or((0, 0), |(a, b)| (i64::from(a) * 1000, i64::from(b) * 1000));
        let chat_in_window = out
            .events
            .iter()
            .filter(|e| {
                matches!(e, StreamEvent::Chat { ts_ms, .. }
                if (300_000..390_000).contains(ts_ms) && !(quiet_from..quiet_to).contains(ts_ms))
            })
            .count();
        assert!(chat_in_window > 0, "silence {quiet_from}..{quiet_to}");
        assert!(out.events.iter().any(|e| matches!(e,
            StreamEvent::Say { ts_ms, speaker: Speaker::Malakor, .. } if *ts_ms >= 390_000)));
        // The injected errors are not core errors.
        let r = &out.report;
        assert_eq!(r.core_errors, 0);
        assert_eq!(state(r, "core_errors"), SandboxCheckState::Pass);
        assert!(
            detail(r, "chaos").starts_with("outage 05:00-06:30: "),
            "{}",
            detail(r, "chaos")
        );
        assert_ne!(state(r, "chaos"), SandboxCheckState::Unmeasured);
    }

    #[test]
    fn the_programme_follows_the_minutes() {
        let tmp = tempfile::tempdir().unwrap();
        let resolve = |minutes, planned: Option<u32>, chaos| {
            let mut run = sandbox_run(tmp.path(), minutes, 0);
            run.planned_s = planned;
            run.params.chaos = chaos;
            Programme::resolve(&run)
        };
        let five = resolve(5, None, None).unwrap();
        assert_eq!((five.planned_s, five.handover_s), (300, None));
        let rule = five.handover_rule().unwrap();
        assert_eq!((rule.ccv_min, rule.max_elapsed_s), (u32::MAX, u32::MAX));
        let ten = resolve(10, None, Some(SandboxChaos::Outage)).unwrap();
        assert_eq!(ten.handover_s, Some(240));
        assert_eq!(ten.chaos_ms, Some((300_000, 390_000)));
        let rule = ten.handover_rule().unwrap();
        assert_eq!(rule.to, SegmentId::Seg02aHexmap);
        assert_eq!(rule.max_elapsed_s, 240);
        let thirty = resolve(30, None, Some(SandboxChaos::Lost)).unwrap();
        assert_eq!(thirty.handover_s, Some(600));
        assert_eq!(thirty.chaos_ms, Some((900_000, 1_800_000)));
        assert_eq!(thirty.deadline_ms(300), 1_800_000);
        // In ten minutes the fleet's sign-off is due at the planned end: the driver waits.
        let ten_lost = resolve(10, None, Some(SandboxChaos::Lost)).unwrap();
        assert_eq!(ten_lost.deadline_ms(300), 615_000);
        // A test length scales the handover to 40%.
        assert_eq!(resolve(10, Some(120), None).unwrap().handover_s, Some(48));
        assert_eq!(resolve(30, Some(200), None).unwrap().handover_s, Some(80));
        // Lost needs ten minutes; only 5, 10 and 30 are sessions.
        for (m, c) in [(5, Some(SandboxChaos::Lost)), (7, None), (60, None)] {
            assert!(
                matches!(resolve(m, None, c), Err(SimError::Spec(_))),
                "{m} {c:?}"
            );
        }
    }

    /// Why the programme's handover rule puts the audience threshold out of reach: a
    /// 30-viewer room holds the table's `ccv_min` for its whole `sustain_s` from the start,
    /// so the table's rule would hand over at 180 s in every session, five minutes included.
    #[test]
    fn a_sandbox_room_would_meet_the_tables_ccv_rule_from_the_start() {
        let tmp = tempfile::tempdir().unwrap();
        let (cal, _, _) = crate::calibration::load(tmp.path()).unwrap();
        let table = segment_table(SegmentId::Seg01ImprovisedRoom)
            .unwrap()
            .clock
            .handover
            .unwrap();
        for seed in [1, 7, 42] {
            let pop = population::generate(
                &cal,
                SANDBOX_POPULATION,
                SANDBOX_AUDIENCE,
                300,
                seed,
                crate::population::tests::fake_rank,
            )
            .unwrap();
            assert!(
                (0..=table.sustain_s).all(|t| pop.ccv_at(t) >= table.ccv_min),
                "seed {seed}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn lost_under_ten_minutes_is_refused_before_anything_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = opts(tmp.path());
        let mut run = sandbox_run(tmp.path(), 5, 60);
        run.params.chaos = Some(SandboxChaos::Lost);
        let dir = run.dir.clone();
        let (_stop_tx, stop) = watch::channel(false);
        let (ptx, _) = watch::channel(SandboxProgress::default());
        let err = run_sandbox(
            run,
            Arc::new(HostEngine::new()),
            &opts,
            no_sink(),
            stop,
            ptx,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, SimError::Spec(_)), "{err}");
        assert!(!dir.exists());
    }

    #[tokio::test(start_paused = true)]
    async fn a_ten_minute_programme_hands_over_to_the_demo_case_at_40_percent() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = opts(tmp.path());
        let (_stop_tx, stop) = watch::channel(false);
        let out = session(
            sandbox_run(tmp.path(), 10, 120),
            Arc::new(HostEngine::new()),
            &opts,
            no_sink(),
            stop,
        )
        .await;
        let handover = out.events.iter().find_map(|e| match e {
            StreamEvent::Handover {
                ts_ms, to, reason, ..
            } => Some((*ts_ms, *to, reason.clone())),
            _ => None,
        });
        let (at, to, reason) = handover.expect("a handover");
        assert_eq!((at, to), (50_000, SegmentId::Seg02aHexmap));
        assert_eq!(reason, "elapsed >= 48 s");
        let i = out
            .events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    StreamEvent::SegmentStart {
                        segment: SegmentId::Seg02aHexmap,
                        ..
                    }
                )
            })
            .expect("SEG-02A started");
        assert!(matches!(&out.events[i + 1], StreamEvent::CaseOpen { case, .. } if case.fictional));
        // The 3-act arc is for plans of 60 minutes or more: no act cards here.
        assert!(!out.events.iter().any(|e| matches!(e,
            StreamEvent::Op { op: Op::ShowCard { card_id, .. }, .. } if card_id.starts_with("arc-"))));
        assert!(matches!(
            out.events[out.events.len() - 2],
            StreamEvent::SegmentEnd {
                segment: SegmentId::Seg02aHexmap,
                ..
            }
        ));
        let spec: SandboxSpec = read_json(&out.dir.join("spec.json")).unwrap();
        assert_eq!(
            spec.programme,
            vec![SegmentId::Seg01ImprovisedRoom, SegmentId::Seg02aHexmap]
        );
        assert_eq!(spec.handover_s, Some(48));
        assert_eq!(state(&out.report, "sign_off"), SandboxCheckState::Pass);
    }

    #[tokio::test(start_paused = true)]
    async fn a_session_is_judged_like_a_run() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = opts(tmp.path());
        let (_stop_tx, stop) = watch::channel(false);
        let out = session(
            sandbox_run(tmp.path(), 5, 60),
            Arc::new(HostEngine::new()),
            &opts,
            no_sink(),
            stop,
        )
        .await;
        let judge_engine = Arc::new(FakeEngine::new(""));
        let j = judge_session(&out.dir, judge_engine.clone(), &opts)
            .await
            .unwrap();
        assert_eq!(j.verdict_path, "verdict.json");
        assert_eq!(j.rule_fails, 0);
        assert!(j.quality_mean.is_some_and(|q| (0.0..=1.0).contains(&q)));
        let v: shadowrunr_types::JudgeVerdict = read_json(&out.dir.join("verdict.json")).unwrap();
        assert_eq!(v.run_id, "sbx-test");
        assert_eq!(v.judge_tier, ModelTier::Opus, "never the core tier");
        assert!(out.dir.join("judge.json").is_file());
        let calls = judge_engine.calls.lock().unwrap();
        assert!(!calls.is_empty());
        assert!(calls
            .iter()
            .all(|(r, t)| *r == Role::Judge && *t == ModelTier::Opus));
    }
}
