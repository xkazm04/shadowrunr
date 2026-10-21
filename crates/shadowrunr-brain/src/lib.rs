//! The character's mind: package loader, prompt composer, long-term memory, operational
//! memory, worker pool, chat picker, moderation gates and the fleet-loss policy.
//!
//! One `Brain::tick` is one beat of the core thread:
//! 1. fleet check (a `health` event when the fleet mode changed; sign off after the grace
//!    period);
//! 2. collect finished background jobs (never awaits one);
//! 3. pre-filter chat (viewers removed after a crisis line, term rules, safety-critical
//!    disclosures, room-scaled rate caps, injection tagging, flood/copypasta clusters); a
//!    disclosure puts its viewer in care ([`care`]);
//! 4. orchestrate and pick: [`orchestrate`] drops repeats of answered waves, decides the beat's
//!    mode (cold open, react, bit, bridge, mixed, hold) and writes its `beat_plan`; the picker
//!    then passes at most the plan's ceiling (safety-critical first, then one owed request;
//!    deterministic, or `Role::ChatPicker` above a volume threshold). A hold ends the beat there,
//!    and so does speech still queued on the speech clock past the lead ([`pace`]: a `speaking`
//!    hold; on a live host the lead covers the measured core latency, so the next beat is
//!    composed while the current line plays);
//! 5. recall and compose (stable prefix, with the Lord's brief when the stream has one, +
//!    per-turn block, with the beat's prepared material when it voices some and its speech
//!    budget);
//! 6. call the core thread (warm session when available, else `complete`); a fleet-class
//!    failure speaks a model-free stall line instead once nobody has spoken long enough
//!    ([`stall`]);
//! 7. parse spoken lines, `OP:` and `NOTE:`; drop whole lines past the beat's speech budget or
//!    queued past the lag ceiling;
//!    gate output; dispatch research jobs; once a crisis line went out, remove that viewer from
//!    chat when `crisis-chat-ban` is enabled (their name is withheld from his lines for the
//!    rest of the stream). Every line put on air advances the speech clock.

pub mod care;
pub mod character;
pub mod compose;
pub mod fleet;
pub mod grammar;
pub mod memory;
pub mod moderation;
pub mod orchestrate;
pub mod outgate;
pub mod pace;
pub mod picker;
pub mod pool;
pub mod rules;
pub mod stall;
pub mod util;
pub mod working;

#[cfg(test)]
mod tests;

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use shadowrunr_types::character::Tuning;
use shadowrunr_types::{
    Case, ChatMsg, Engine, EngineError, EngineKind, EngineReply, EngineRequest, EpisodeKind,
    FactScope, FleetHealth, ModelTier, Op, Role, SegmentContext, Speaker, StreamEvent, TsMs,
    WarmSession,
};

pub use character::{
    load_character, load_character_with, parse_manifest, validate_character, CharacterPackage,
    Foil, LineBank, PackageIssue, RankTitles, Situation, SituationBank,
};
pub use fleet::{FleetMode, FleetProbe};
pub use memory::{
    consolidate, rebuild_index, should_consolidate, ConsolidationReport, FactStatus, MemoryStore,
    StoredFact,
};
pub use moderation::{FilterConfig, HexInfo, PersonaGate, PolicySource};
pub use pace::{PaceConfig, SpeechClock};
pub use picker::PickerConfig;
pub use pool::{Job, JobCtx, JobRunner, JobStatus, JobUpdate, ResearchRunner, WorkerPool};
pub use rules::{CharacterRule, RuleState, RulesOverride};
pub use stall::StallBank;

pub use compose::{brief_summary, build_stream_brief, load_stream_brief, normalize_stream_brief};
pub use shadowrunr_types::reconcile::StreamBrief;

use care::{care_note, removed_note, CareLedger};
use compose::{compose_turn, stable_prefix_with_brief, TurnInputs, ViewerBrief};
use fleet::{FleetVerdict, FleetWatch};
use grammar::{op_tag, parse_reply, Line};
use moderation::{InputFilter, Policy, RemovedNames, CRISIS_BAN_RULE};
use orchestrate::{requests::claims_authority, BeatInputs, BeatTxn, Orchestrator, ROLLED_BACK};
use outgate::{GateCtx, Surface};
use picker::{Pick, PickContext, Picker};
use shadowrunr_types::prep::BeatMode;
use stall::{stall_due, Staller, STALL_RULE};
use util::{fnv1a64, one_line, REPEAT_WINDOW};
use working::{select_window, WindowItem, WorkingSet, RECENCY_FLOOR};

#[derive(Debug, thiserror::Error)]
pub enum BrainError {
    #[error("character package: {0}")]
    Package(String),
    #[error("memory store: {0}")]
    Store(String),
    /// A memory write that broke a rule (no citation, unknown episode, tombstoned key).
    #[error("rejected: {0}")]
    Rejected(String),
    #[error("policy: {0}")]
    Policy(String),
    /// `<data>/next_plan.json` or `<data>/directives.json` could not be read.
    #[error("stream brief: {0}")]
    Brief(String),
    #[error(transparent)]
    Engine(#[from] shadowrunr_types::EngineError),
}

#[derive(Debug, Clone)]
pub struct BrainConfig {
    pub character: CharacterPackage,
    /// Memory root (markdown truth + SQLite index). A sim run gets a throwaway directory.
    pub memory_dir: PathBuf,
    pub stream_id: String,
    pub core_tier: ModelTier,
    /// Max concurrent background jobs.
    pub pool_size: usize,
}

/// Knobs with working defaults; pass to [`Brain::new_with`].
#[derive(Debug, Clone)]
pub struct BrainTuning {
    /// Seconds the host keeps chatting after the fleet goes degraded or down.
    pub fleet_grace_s: u64,
    /// A stream episode is written every N beats (and at `end_stream`).
    pub episode_every_ticks: u32,
    pub picker: PickerConfig,
    pub filter: FilterConfig,
    pub policy: PolicySource,
    /// Platforms the stream goes out on (`twitch`, `youtube`, `tiktok`); term rules for
    /// other platforms are ignored. Empty = all platforms (the strictest union).
    pub platforms: Vec<String>,
    /// Use a warm session for the core thread when the engine supports it.
    pub warm_session: bool,
    /// Recycle the warm session after this many turns. The brain rebuilds the full context
    /// every tick, so the CLI's own accumulated history is pure cost: a live pilot measured
    /// ~220k cached tokens per core call without recycling. 0 = never recycle.
    pub warm_recycle_turns: u32,
    pub working_set_ttl_ms: i64,
    /// Run the sleep cycle at `end_stream` when memory pressure warrants it.
    pub consolidate_at_end: bool,
    /// Seconds a viewer stays in care after a self-harm disclosure (no roasting or joking
    /// about them; see [`care`]). A new disclosure restarts it.
    pub care_window_s: u64,
    /// While the fleet fails, a beat with no line from the model speaks a stall line once
    /// nobody has spoken for this many seconds (see [`stall`]).
    pub stall_every_s: u64,
    /// The speech clock and the per-beat speech budget (see [`pace`]).
    pub pace: PaceConfig,
    /// The character's lesson-tuned constants (`[tuning]`): the orchestrator's carry, hold,
    /// bit and request timings and the stall cover. The keys that also live in `picker`,
    /// `stall_every_s` and `pace` are copied there by [`BrainTuning::for_character`].
    pub tuning: Tuning,
}

impl BrainTuning {
    /// The defaults with a character's `[tuning]` applied (its package's resolved table).
    pub fn for_character(pkg: &CharacterPackage) -> Self {
        Self::default().with_tuning(pkg.tuning)
    }

    /// These knobs with `t` applied: `max_picks`, `stall_every_s`, the pace's render lag,
    /// lookahead cap and lag ceiling, and the rest of the table.
    pub fn with_tuning(mut self, t: Tuning) -> Self {
        self.picker.max_picks = t.max_picks;
        self.stall_every_s = t.stall_every_s;
        self.pace.render_lag_ms = t.render_lag_ms;
        self.pace.lookahead_max_ms = t.lookahead_max_ms;
        self.pace.lag_ceiling_ms = t.lag_ceiling_ms;
        self.tuning = t;
        self
    }
}

impl Default for BrainTuning {
    fn default() -> Self {
        Self {
            fleet_grace_s: 300,
            episode_every_ticks: 5,
            picker: PickerConfig::default(),
            filter: FilterConfig::default(),
            policy: PolicySource::Auto,
            platforms: Vec::new(),
            warm_session: true,
            warm_recycle_turns: 6,
            working_set_ttl_ms: working::WORKING_SET_TTL_MS,
            consolidate_at_end: true,
            care_window_s: care::DEFAULT_CARE_WINDOW_S,
            stall_every_s: stall::DEFAULT_STALL_EVERY_S,
            pace: PaceConfig::default(),
            tuning: Tuning::DEFAULT,
        }
    }
}

/// Recalled facts beyond the briefing, and the always-included tier per scope.
const RECALL_FACTS: usize = 8;
const ALWAYS_PER_SCOPE: usize = 3;
const VIEWER_FACTS: usize = 5;
const RECALL_EPISODES: usize = 8;
const EPISODE_ITEM_CHARS: usize = 2_000;
/// Beats between warm-session open attempts after one failed.
const SESSION_RETRY_BEATS: u64 = 10;

/// The care-state record in the transcript (`care_state` / `started|restarted|expired`), on
/// the disclosure's message id.
fn care_event(now_ms: TsMs, msg_id: &str, action: &str) -> StreamEvent {
    moderation::input_event(now_ms, care::CARE_RULE, msg_id, action)
}

/// The `health` record's state for a fleet mode.
fn fleet_health(mode: FleetMode) -> FleetHealth {
    match mode {
        FleetMode::Healthy => FleetHealth::Healthy,
        FleetMode::Degraded => FleetHealth::Degraded,
        FleetMode::Down => FleetHealth::Down,
    }
}

/// A short, label-free cause for a fleet-class core failure (the error's own text may name an
/// account).
fn failure_detail(e: &EngineError) -> &'static str {
    match e {
        EngineError::RateLimited { .. } => "core call rate limited",
        EngineError::Unavailable { .. } => "core call unavailable",
        EngineError::Timeout { .. } => "core call timed out",
        EngineError::Io { .. } => "core call i/o error",
        _ => "core call failed",
    }
}

fn fleet_class(e: &EngineError) -> bool {
    matches!(
        e,
        EngineError::RateLimited { .. }
            | EngineError::Unavailable { .. }
            | EngineError::Timeout { .. }
            | EngineError::Io { .. }
    )
}

pub struct Brain {
    cfg: BrainConfig,
    engine: Arc<dyn Engine>,
    tuning: BrainTuning,
    prefix: String,
    /// The Lord's brief for this stream, rendered into `prefix` (see [`Brain::with_brief`]).
    brief: Option<StreamBrief>,
    store: MemoryStore,
    pool: Arc<WorkerPool>,
    policy: Policy,
    filter: InputFilter,
    /// Viewers in care after a self-harm disclosure.
    care: CareLedger,
    /// Display names of viewers removed from chat (`crisis-chat-ban`), in ban order.
    removed: Vec<String>,
    /// The same names as the output gate withholds them from every surface ([`outgate`]).
    removed_names: RemovedNames,
    picker: Picker,
    working: WorkingSet,
    fleet: FleetWatch,
    fleet_probe: Option<Arc<dyn FleetProbe>>,
    fleet_override: Option<FleetMode>,
    engine_down: bool,
    /// The fleet mode last recorded with a `health` event (healthy before the first one).
    health: FleetMode,
    stall: Staller,
    /// When the speech put on air ends (model, foil, stall and cover lines, the sign-off), on
    /// the `now_ms` clock ([`pace`]).
    speech: SpeechClock,
    /// The last core-call durations on a live host, for the lookahead lead ([`pace`]).
    latency: pace::CoreLatency,
    session: Option<Box<dyn WarmSession>>,
    session_turns: u32,
    /// No warm open before this beat: the last one failed. Engines report "no sessions" and
    /// "all accounts cooling" alike (`Unavailable`), so a failure backs off, never disables.
    session_retry_tick: u64,
    /// Where cover lines go while a slow core call is still running ([`Brain::set_cover`]).
    cover: Option<tokio::sync::mpsc::UnboundedSender<StreamEvent>>,
    /// The approved stream prep the orchestrator voices ([`Brain::set_prep`]).
    prep: Option<shadowrunr_types::prep::StreamPrep>,
    /// Decides each beat's mode ([`orchestrate`]).
    orch: Orchestrator,
    /// When this beat started, on the tokio clock (for [`Brain::on_air_ms`]).
    beat_clock: tokio::time::Instant,
    hexes: Vec<HexInfo>,
    turns: Vec<String>,
    ticks: u64,
    salt: u64,
    names: Vec<String>,
    /// The host's last spoken lines, for the repetition guard.
    spoken: VecDeque<String>,
    /// The active foil's last lines, for the repetition guard.
    foil_spoken: VecDeque<String>,
    /// The repetition guard emptied the last quiet beat: nudge the next turn.
    fresh_bit: bool,
    last_prompt: Option<String>,
    last_reply: Option<EngineReply>,
    last_consolidation: Option<Result<ConsolidationReport, String>>,
}

impl Brain {
    pub async fn new(cfg: BrainConfig, engine: Arc<dyn Engine>) -> Result<Self, BrainError> {
        Self::new_with(cfg, engine, BrainTuning::default()).await
    }

    pub async fn new_with(
        cfg: BrainConfig,
        engine: Arc<dyn Engine>,
        tuning: BrainTuning,
    ) -> Result<Self, BrainError> {
        let store = MemoryStore::open(&cfg.memory_dir)?;
        let pkg = &cfg.character;
        let policy = Policy::load(&tuning.policy, &pkg.dir, &tuning.platforms)?
            .with_persona(PersonaGate::new(&pkg.persona, &pkg.lines));
        let prefix = stable_prefix_with_brief(pkg, None);
        // The names chat calls the character by (`[identity] address_words`, plus the display
        // name's words).
        let names = pkg.persona.address_words.clone();
        let salt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mut filter = InputFilter::new(tuning.filter.clone());
        filter.set_names(names.clone());
        Ok(Self {
            pool: Arc::new(WorkerPool::new(cfg.pool_size)),
            filter,
            care: CareLedger::new(tuning.care_window_s),
            removed: Vec::new(),
            removed_names: RemovedNames::for_persona(&cfg.character.persona),
            picker: Picker::new(tuning.picker.clone()),
            working: WorkingSet::new(tuning.working_set_ttl_ms),
            fleet: FleetWatch::new(tuning.fleet_grace_s),
            fleet_probe: None,
            fleet_override: None,
            engine_down: false,
            health: FleetMode::Healthy,
            stall: Staller::new(),
            speech: SpeechClock::default(),
            latency: pace::CoreLatency::default(),
            session: None,
            session_turns: 0,
            session_retry_tick: 0,
            cover: None,
            prep: None,
            orch: Orchestrator::new(salt)
                .with_character(tuning.tuning, cfg.character.lines.clone()),
            beat_clock: tokio::time::Instant::now(),
            hexes: Vec::new(),
            turns: Vec::new(),
            ticks: 0,
            salt,
            names,
            spoken: VecDeque::new(),
            foil_spoken: VecDeque::new(),
            fresh_bit: false,
            last_prompt: None,
            last_reply: None,
            last_consolidation: None,
            cfg,
            engine,
            tuning,
            prefix,
            brief: None,
            store,
            policy,
        })
    }

    /// Give the brain this stream's brief from the Lord (the chosen plan plus the active
    /// directives, see [`build_stream_brief`]). It is rendered once, into the stable prefix,
    /// after the character package; call it before the first tick (the core thread's warm
    /// session is opened with the prefix). `None` keeps the prefix byte-identical to a brain
    /// that never heard of briefs.
    pub fn with_brief(mut self, brief: Option<StreamBrief>) -> Self {
        self.prefix = stable_prefix_with_brief(&self.cfg.character, brief.as_ref());
        self.brief = brief;
        self
    }

    /// The brief this stream received, if any.
    pub fn brief(&self) -> Option<&StreamBrief> {
        self.brief.as_ref()
    }

    /// Share a worker pool between brains (a stream and a side thread, say). Results still
    /// route to the origin that dispatched them.
    pub fn with_pool(mut self, pool: Arc<WorkerPool>) -> Self {
        self.pool = pool;
        self
    }

    pub fn with_fleet_probe(mut self, probe: Arc<dyn FleetProbe>) -> Self {
        self.fleet_probe = Some(probe);
        self
    }

    /// Push the fleet mode (overrides the probe); `None` clears the override.
    pub fn set_fleet_mode(&mut self, mode: Option<FleetMode>) {
        self.fleet_override = mode;
    }

    /// Tell the output gate about the SEG-02A case so lines naming a hex graded below DOC
    /// carry "alleged". `reveal_hex` ops update grades as they happen.
    pub fn set_case(&mut self, case: &Case) {
        self.hexes = case.hexes.iter().map(HexInfo::from).collect();
    }

    pub fn set_hexes(&mut self, hexes: Vec<HexInfo>) {
        self.hexes = hexes;
    }

    pub fn character(&self) -> &CharacterPackage {
        &self.cfg.character
    }

    pub fn config(&self) -> &BrainConfig {
        &self.cfg
    }

    /// The byte-stable system prompt.
    pub fn stable_prefix(&self) -> &str {
        &self.prefix
    }

    /// FNV-1a of the stable prefix, for the ledger (detects prompt-cache churn).
    pub fn prefix_hash(&self) -> String {
        format!("{:016x}", fnv1a64(self.prefix.as_bytes()))
    }

    pub fn memory(&self) -> &MemoryStore {
        &self.store
    }

    pub fn working_set(&self) -> &WorkingSet {
        &self.working
    }

    /// Viewers in care after a self-harm disclosure, oldest first.
    pub fn in_care(&self) -> &[care::CareEntry] {
        self.care.active()
    }

    /// The approved stream prep for this stream: its cold open, bits and connectors. Without one,
    /// the stream runs on chat and the segment's rules alone.
    pub fn set_prep(&mut self, prep: shadowrunr_types::prep::StreamPrep) {
        self.prep = Some(prep);
    }

    /// The stream's prep, if one was set.
    pub fn prep(&self) -> Option<&shadowrunr_types::prep::StreamPrep> {
        self.prep.as_ref()
    }

    /// A live host's cover channel: while a core call is still running once the host has been
    /// quiet `stall_every_s`, a stall line goes out here at once instead of waiting for the
    /// beat to end (`stall::cover_call`). Without it (simulated time), slow calls are not
    /// covered.
    pub fn set_cover(&mut self, tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>) {
        self.cover = Some(tx);
    }

    /// Stream time now. A live host's lines go on air when the beat's calls return, not at
    /// the beat's `now_ms`, so with a cover channel the time spent in this beat is added;
    /// in simulated time it is `now_ms`. Measured: without this, the quiet before a cover
    /// line counted the previous call's duration and covers fired ~7 s after a line instead
    /// of 12 s (sbx-20261020-154322-5m: 12 cover lines in 5 min).
    fn on_air_ms(&self, now_ms: TsMs) -> TsMs {
        if self.cover.is_none() {
            return now_ms;
        }
        let ms = i64::try_from(self.beat_clock.elapsed().as_millis()).unwrap_or(i64::MAX);
        now_ms.saturating_add(ms)
    }

    /// When the speech already put on air ends, on the `now_ms` clock ([`pace`]).
    pub fn speech_until(&self) -> Option<TsMs> {
        self.speech.until()
    }

    /// A package line ([`Surface::Stall`]) through the output gate: the text as aired and the
    /// moderation records it fired.
    fn gate_stall(&self, now_ms: TsMs, line: &str) -> (String, Vec<StreamEvent>) {
        let ctx = GateCtx {
            now_ms,
            policy: &self.policy,
            hexes: &self.hexes,
            crisis: false,
            removed: &self.removed_names,
            recent: &[],
        };
        let g = outgate::pass(Surface::Stall, line, &ctx, &mut |t| t.to_string());
        (g.text.unwrap_or_default(), g.events)
    }

    /// A line goes on air at `at`: the speech clock moves past it.
    fn aired(&mut self, at: TsMs, text: &str) {
        self.speech.say(at, text, &self.tuning.pace);
    }

    /// Up to [`stall::COVER_LINES`] gated stall lines for a covered core call, taken from a copy
    /// of the rotation (the real one advances only by the lines spoken).
    fn cover_plan(&self, now_ms: TsMs) -> Option<stall::CoverPlan> {
        let tx = self.cover.clone()?;
        let mut rotation = self.stall.clone();
        let mut lines = Vec::new();
        for _ in 0..stall::COVER_LINES {
            let Some(line) = rotation.next(&self.cfg.character.stalls, self.salt) else {
                break;
            };
            let (text, mut events) = self.gate_stall(now_ms, line);
            events.push(StreamEvent::Say {
                ts_ms: now_ms,
                speaker: Speaker::Host,
                foil: None,
                text,
                replying_to: Vec::new(),
                first_text_ms: None,
            });
            events.push(moderation::output_event(now_ms, STALL_RULE, stall::COVERED));
            lines.push(events);
        }
        if lines.is_empty() {
            return None;
        }
        let every_ms = self.tuning.stall_every_s.saturating_mul(1_000);
        // The quiet so far, negative while speech is still queued: the cover waits for it to
        // air first. Before his first line the quiet runs from the stream's start (measured:
        // counted as none, a 12 s first call left 17.2 s of silence, sbx-20261021-003254-5m).
        let on_air = self.on_air_ms(now_ms);
        let quiet_ms = on_air.saturating_sub(self.speech.until().unwrap_or(0));
        let first_ms = i64::try_from(every_ms)
            .unwrap_or(i64::MAX)
            .saturating_sub(quiet_ms);
        Some(stall::CoverPlan {
            tx,
            lines,
            first_after: std::time::Duration::from_millis(
                u64::try_from(first_ms)
                    .unwrap_or(0)
                    .max(self.tuning.tuning.min_cover_ms),
            ),
            every: std::time::Duration::from_millis(every_ms.max(self.tuning.tuning.min_cover_ms)),
        })
    }

    /// Whether a viewer was removed from chat for the rest of the stream.
    pub fn is_banned(&self, viewer_id: &str) -> bool {
        self.filter.is_banned(viewer_id)
    }

    pub fn pool(&self) -> &Arc<WorkerPool> {
        &self.pool
    }

    /// The last composed per-turn prompt (diagnostics, sim debugging).
    pub fn last_prompt(&self) -> Option<&str> {
        self.last_prompt.as_deref()
    }

    /// The last core-thread reply, for the ledger.
    pub fn last_reply(&self) -> Option<&EngineReply> {
        self.last_reply.as_ref()
    }

    pub fn last_consolidation(&self) -> Option<&Result<ConsolidationReport, String>> {
        self.last_consolidation.as_ref()
    }

    /// Queue a background job for this stream. Never blocks.
    pub fn dispatch(&self, job: Job) -> String {
        self.pool.dispatch(&self.cfg.stream_id, job)
    }

    /// Queue a research job through the engine (`Role::Researcher`).
    pub fn dispatch_research(&self, job_id: Option<String>, brief: &str) -> String {
        self.dispatch(Job {
            job_id,
            brief: brief.to_string(),
            runner: Arc::new(ResearchRunner {
                engine: self.engine.clone(),
                character_name: self.cfg.character.name.clone(),
            }),
        })
    }

    pub fn cancel_job(&self, job_id: &str) -> bool {
        self.pool.cancel(job_id)
    }

    fn fleet_mode(&self) -> FleetMode {
        let rank = |m: FleetMode| match m {
            FleetMode::Healthy => 0,
            FleetMode::Degraded => 1,
            FleetMode::Down => 2,
        };
        let mut mode = self
            .fleet_override
            .or_else(|| self.fleet_probe.as_ref().map(|p| p.mode()))
            .unwrap_or(FleetMode::Healthy);
        if self.engine_down && rank(FleetMode::Down) > rank(mode) {
            mode = FleetMode::Down;
        }
        mode
    }

    fn collect_jobs(&mut self, now_ms: TsMs, ev: &mut Vec<StreamEvent>) -> Result<(), BrainError> {
        for u in self.pool.collect(&self.cfg.stream_id) {
            self.working.apply_job(&u, now_ms);
            self.orch.note_job(&u.job_id, &u.brief, u.status.as_str());
            ev.push(StreamEvent::Job {
                ts_ms: now_ms,
                job_id: u.job_id.clone(),
                status: u.status.as_str().to_string(),
                summary: u.status.summary().map(|s| one_line(s, 600)),
            });
            if let JobStatus::Done(summary) = &u.status {
                let ep = shadowrunr_types::Episode {
                    id: self.store.new_id("ep", now_ms),
                    ts_ms: now_ms,
                    stream_id: self.cfg.stream_id.clone(),
                    kind: EpisodeKind::Job,
                    text: format!("Research {} ({}): {}", u.job_id, u.brief, summary),
                    importance: 3,
                };
                self.store.write_episode(&ep)?;
            }
        }
        Ok(())
    }

    fn flush_turns(&mut self, now_ms: TsMs) -> Result<(), BrainError> {
        if self.turns.is_empty() {
            return Ok(());
        }
        let text = std::mem::take(&mut self.turns).join("\n");
        let ep = shadowrunr_types::Episode {
            id: self.store.new_id("ep", now_ms),
            ts_ms: now_ms,
            stream_id: self.cfg.stream_id.clone(),
            kind: EpisodeKind::Stream,
            text,
            importance: 3,
        };
        self.store.write_episode(&ep)
    }

    /// The beat's core call failed: the orchestrator un-consumes the beat ([`Orchestrator::rollback`]),
    /// a fresh research bit stays fresh, and the beat's `beat_plan` says it was rolled back so
    /// the transcript stays complete.
    fn roll_back_beat(
        &mut self,
        txn: BeatTxn,
        pick: &Pick,
        now_ms: TsMs,
        fresh_bit: bool,
        beat_plan: &mut StreamEvent,
    ) {
        self.orch.rollback(txn, pick, now_ms);
        self.fresh_bit |= fresh_bit;
        if let StreamEvent::BeatPlan { reason, .. } = beat_plan {
            reason.push_str(ROLLED_BACK);
        }
    }

    /// One beat's lines as one turn of the pending stream episode; a beat with no lines adds
    /// nothing. Recall and the sleep cycle read the first EPISODE_ITEM_CHARS of an episode,
    /// so a turn that would carry the pending episode past that starts the next one.
    fn record_turn(&mut self, lines: &[String], now_ms: TsMs) -> Result<(), BrainError> {
        if lines.is_empty() {
            return Ok(());
        }
        let turn = lines.join("\n");
        let pending: usize = self.turns.iter().map(|t| t.chars().count() + 1).sum();
        if !self.turns.is_empty() && pending + turn.chars().count() > EPISODE_ITEM_CHARS {
            self.flush_turns(now_ms)?;
        }
        self.turns.push(turn);
        Ok(())
    }

    fn core_template(&self) -> EngineRequest {
        let (_, effort, timeout_ms) = shadowrunr_engine::route(Role::CoreThread);
        EngineRequest {
            role: Role::CoreThread,
            tier: self.cfg.core_tier,
            effort,
            system: self.prefix.clone(),
            prompt: String::new(),
            allowed_tools: Vec::new(),
            timeout_ms,
            json_schema: None,
            correlation_id: Some(self.cfg.stream_id.clone()),
        }
    }

    async fn call_core(&mut self, prompt: String) -> Result<EngineReply, EngineError> {
        if self.tuning.warm_session {
            if self.session.is_none() && self.ticks >= self.session_retry_tick {
                match self.engine.open_session(self.core_template()).await {
                    Ok(s) => self.session = Some(s),
                    Err(_) => self.session_retry_tick = self.ticks + SESSION_RETRY_BEATS,
                }
            }
            if let Some(session) = self.session.as_mut() {
                match session.send(prompt.clone()).await {
                    Ok(r) => {
                        self.session_turns += 1;
                        let limit = self.tuning.warm_recycle_turns;
                        if limit > 0 && self.session_turns >= limit {
                            if let Some(s) = self.session.take() {
                                let _ = s.close().await;
                            }
                            self.session_turns = 0;
                        }
                        return Ok(r);
                    }
                    Err(_) => {
                        // The warm process died: fall back to a cold call, reopen next beat.
                        self.session = None;
                        self.session_turns = 0;
                    }
                }
            }
        }
        let mut req = self.core_template();
        req.prompt = prompt;
        self.engine.complete(req).await
    }

    /// A `health` event when the fleet mode differs from the one last recorded.
    fn note_health(&mut self, now_ms: TsMs, detail: &str, ev: &mut Vec<StreamEvent>) {
        let mode = self.fleet_mode();
        if mode != self.health {
            self.health = mode;
            ev.push(StreamEvent::Health {
                ts_ms: now_ms,
                state: fleet_health(mode),
                detail: Some(detail.to_string()),
            });
        }
    }

    /// A model-free stall line (see [`stall`]) when one is due: the say, then its
    /// `engine_stall` record. It stays out of the turn record, the repetition guard and the
    /// working set: the model never said it.
    fn stall_events(&mut self, now_ms: TsMs, ev: &mut Vec<StreamEvent>) {
        // A failed beat can only speak at a beat boundary, so it is due one beat early: a
        // 12 s budget checked every 5 s otherwise lets a silence reach 17 s
        // (sbx-20261020-155928-10m: 15.03 s inside its outage, over the 15 s ceiling).
        let due = stall_due(
            self.fleet_mode(),
            self.speech.until(),
            self.on_air_ms(now_ms),
            self.tuning
                .stall_every_s
                .saturating_sub(self.tuning.tuning.failed_beat_slack_s),
        );
        if !due {
            return;
        }
        let Some(line) = self.stall.next(&self.cfg.character.stalls, self.salt) else {
            return;
        };
        let (text, events) = self.gate_stall(now_ms, line);
        ev.extend(events);
        self.aired(self.on_air_ms(now_ms), &text);
        ev.push(StreamEvent::Say {
            ts_ms: now_ms,
            speaker: Speaker::Host,
            foil: None,
            text,
            replying_to: Vec::new(),
            first_text_ms: None,
        });
        ev.push(moderation::output_event(now_ms, STALL_RULE, "spoke"));
    }

    fn sign_off_events(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        let grace = self.tuning.fleet_grace_s;
        // Every spoken line goes through the output gate, the package's sign-off included.
        let line = self.cfg.character.stalls.sign_off.clone();
        let (text, mut ev) = self.gate_stall(now_ms, &line);
        self.aired(self.on_air_ms(now_ms), &text);
        ev.push(StreamEvent::Say {
            ts_ms: now_ms,
            speaker: Speaker::Host,
            foil: None,
            text,
            replying_to: Vec::new(),
            first_text_ms: None,
        });
        ev.push(StreamEvent::Op {
            ts_ms: now_ms,
            op: Op::SignOff {
                reason: format!("fleet_lost: engine degraded or down for {grace}s"),
            },
        });
        ev
    }

    /// End the care states that ran out. A disclosure from a viewer already removed from chat
    /// never reaches the model, yet it restarts their care window.
    fn update_care(&mut self, now_ms: TsMs, inbox: &[ChatMsg], ev: &mut Vec<StreamEvent>) {
        for e in self.care.expire(now_ms) {
            ev.push(care_event(now_ms, &e.msg_id, "expired"));
        }
        for m in inbox {
            if self.filter.is_banned(&m.viewer.id)
                && self.policy.self_harm_disclosure(&m.text).is_some()
            {
                let how = self.care.begin(&m.viewer.id, &m.viewer.name, &m.id, now_ms);
                ev.push(care_event(now_ms, &m.id, how.as_str()));
            }
        }
    }

    /// One beat of the core thread: given new chat since the last tick and the segment
    /// context, pick messages (anti-bait), speak, emit ops, dispatch/collect jobs.
    /// Returns transcript events in order.
    pub async fn tick(
        &mut self,
        now_ms: TsMs,
        inbox: Vec<ChatMsg>,
        ctx: &SegmentContext,
    ) -> Result<Vec<StreamEvent>, BrainError> {
        let mut ev = Vec::new();
        self.beat_clock = tokio::time::Instant::now();
        if self.fleet.signed_off() {
            return Ok(ev);
        }
        let reported = match self.fleet_mode() {
            FleetMode::Healthy => "fleet healthy",
            FleetMode::Degraded => "fleet reports degraded",
            FleetMode::Down => "fleet reports down",
        };
        self.note_health(now_ms, reported, &mut ev);
        match self.fleet.observe(now_ms, self.fleet_mode()) {
            FleetVerdict::Gone => return Ok(ev),
            FleetVerdict::SignOff => {
                ev.extend(self.sign_off_events(now_ms));
                return Ok(ev);
            }
            FleetVerdict::Continue => {}
        }
        if !ctx.hexes.is_empty() {
            self.hexes = ctx.hexes.iter().map(HexInfo::from).collect();
        }
        self.ticks += 1;
        self.working.expire(now_ms);
        self.collect_jobs(now_ms, &mut ev)?;

        // Pre-filter, orchestrate and pick. A disclosure puts its viewer in care (or restarts
        // it).
        self.update_care(now_ms, &inbox, &mut ev);
        self.orch.observe_inbox(&inbox);
        let (cands, mods) = self.filter.filter(now_ms, inbox, &self.policy);
        ev.extend(mods);
        for c in cands.iter().filter(|c| c.safety.is_some()) {
            let v = &c.msg.viewer;
            let how = self.care.begin(&v.id, &v.name, &c.msg.id, now_ms);
            ev.push(care_event(now_ms, &c.msg.id, how.as_str()));
        }
        let cands = self.orch.screen(now_ms, cands, &mut ev);
        // The goodbye beat is never held and speaks at half budget; the wrap-up before it is
        // paced like any beat.
        let closing = pace::is_closing_now(&ctx.state_summary);
        // Lookahead: on a live host the lead covers the measured core latency and the render
        // lag, so this beat is composed while the last line still plays.
        let core_ms = self.cover.as_ref().and(self.latency.median());
        let lead_ms = pace::lookahead_lead(&self.tuning.pace, core_ms);
        let queued_ms = self.speech.queued_ms(now_ms);
        // What the beat consumes stands only once its core call is delivered.
        let txn = self.orch.begin();
        let mut plan = self.orch.plan(&BeatInputs {
            now_ms,
            ctx,
            cands: &cands,
            prep: self.prep.as_ref(),
            speech_until_ms: self.speech.until(),
            speech_lead_ms: lead_ms,
            expected_latency_ms: core_ms
                .map_or(0, |c| c.saturating_add(self.tuning.pace.render_lag_ms)),
            closing,
            names: &self.names,
            max_picks: self.picker.config().max_picks,
        });
        let cands = self.orch.offered(&plan, &cands);
        let viewer_ids: Vec<String> = {
            let mut seen = HashSet::new();
            cands
                .iter()
                .filter(|c| seen.insert(c.msg.viewer.id.clone()))
                .map(|c| c.msg.viewer.id.clone())
                .collect()
        };
        let pctx = PickContext {
            regulars: self.store.viewers_with_facts(&viewer_ids)?,
            poll_options: self
                .working
                .active_poll
                .as_ref()
                .map(|p| p.options.clone())
                .unwrap_or_default(),
            names: self.names.clone(),
            first_contacts: cands
                .iter()
                .filter(|c| self.orch.is_first_contact(&c.msg.id))
                .map(|c| c.msg.id.clone())
                .collect(),
            carried: self.orch.carried_ages(now_ms),
            owed: self.orch.owed(&cands, now_ms),
        };
        let corr = format!("{}:pick:{}", self.cfg.stream_id, self.ticks);
        let pick = self
            .picker
            .pick_upto(&cands, &pctx, self.engine.as_ref(), &corr, plan.ceiling)
            .await;
        if !pick.chosen.is_empty() {
            ev.push(StreamEvent::Picked {
                ts_ms: now_ms,
                msg_ids: pick.ids(),
                reason: pick.reason.clone(),
            });
        }
        let beat_plan = self.orch.commit(&mut plan, &pick, now_ms);
        // Every message the carry cap dropped leaves a record.
        ev.extend(self.orch.take_capped(now_ms));
        let plan_at = ev.len();
        ev.push(beat_plan);
        if plan.mode == BeatMode::Hold {
            self.filter
                .note_answered(pick.chosen.iter().map(|c| c.msg.viewer.id.as_str()));
            if self
                .ticks
                .is_multiple_of(u64::from(self.tuning.episode_every_ticks.max(1)))
            {
                self.flush_turns(now_ms)?;
            }
            return Ok(ev);
        }
        // A forged decree or an injection in the picks: a recast line rebuts it.
        let authority = pick
            .chosen
            .iter()
            .any(|c| c.injection || claims_authority(&c.msg.text, &self.cfg.character.persona));

        // Recall.
        let pkg = &self.cfg.character;
        let mut briefing = Vec::new();
        let mut recalled_ids: Vec<String> = Vec::new();
        let mut seen_viewers = HashSet::new();
        for c in &pick.chosen {
            let v = &c.msg.viewer;
            if !seen_viewers.insert(v.id.clone()) {
                continue;
            }
            let facts = self.store.viewer_facts(&v.id, VIEWER_FACTS)?;
            recalled_ids.extend(facts.iter().map(|f| f.fact.id.clone()));
            briefing.push(ViewerBrief {
                id: v.id.clone(),
                name: v.name.clone(),
                title: pkg.rank_titles.title(v.rank),
                months: v.months,
                facts: facts.iter().map(|f| one_line(&f.fact.value, 200)).collect(),
            });
        }
        let mut query = String::new();
        for c in &pick.chosen {
            query.push_str(&c.msg.viewer.name);
            query.push(' ');
            if !c.injection {
                query.push_str(&c.msg.text);
                query.push(' ');
            }
        }
        query.push_str(&one_line(&ctx.state_summary, 300));

        let mut facts: Vec<StoredFact> = Vec::new();
        for scope in [FactScope::Character, FactScope::World, FactScope::Case] {
            facts.extend(self.store.top_facts(scope, ALWAYS_PER_SCOPE)?);
        }
        for f in self.store.search_facts(&query, RECALL_FACTS)? {
            let in_brief = f
                .viewer_id
                .as_ref()
                .is_some_and(|v| seen_viewers.contains(v));
            if !in_brief && !facts.iter().any(|x| x.fact.id == f.fact.id) {
                facts.push(f);
            }
        }
        recalled_ids.extend(facts.iter().map(|f| f.fact.id.clone()));
        self.store.mark_recalled(&recalled_ids, now_ms)?;
        let procedurals = self.store.top_procedurals(5)?;

        let to_item = |e: &shadowrunr_types::Episode| WindowItem {
            ts_ms: e.ts_ms,
            label: e.id.clone(),
            text: e.text.chars().take(EPISODE_ITEM_CHARS).collect(),
        };
        let relevant: Vec<WindowItem> = self
            .store
            .search_episodes(&query, RECALL_EPISODES)?
            .iter()
            .map(to_item)
            .collect();
        let mut recent: Vec<WindowItem> = self
            .store
            .recent_episodes(RECENCY_FLOOR)?
            .iter()
            .map(to_item)
            .collect();
        for (i, t) in self.turns.iter().enumerate() {
            recent.push(WindowItem {
                ts_ms: now_ms,
                label: format!("this-stream-{}", i + 1),
                text: t.clone(),
            });
        }
        let window = select_window(relevant, recent);

        // Compose.
        let active_foil = self.working.active_foil.clone();
        let foil_text = active_foil
            .as_deref()
            .and_then(|f| pkg.foil(f))
            .map(|f| f.text.clone());
        let working_text = self
            .working
            .render(foil_text.as_deref(), &self.cfg.character.persona.followers);
        let nonce = fnv1a64(format!("{}:{}:{}", self.salt, self.ticks, now_ms).as_bytes());
        let fresh_bit = std::mem::take(&mut self.fresh_bit);
        let care = [
            care_note(self.care.active(), now_ms),
            removed_note(&self.removed),
        ]
        .into_iter()
        .flatten()
        .reduce(|a, b| format!("{a}\n{b}"));
        let budget = if closing {
            pace::closing_budget(self.tuning.pace.budget_chars)
        } else if queued_ms > self.tuning.pace.speech_lead_ms {
            // Composed ahead (lookahead or an owed request): what fits under the lag ceiling.
            pace::lookahead_budget(
                &self.tuning.pace,
                self.tuning.pace.budget_chars,
                queued_ms,
                core_ms.unwrap_or(0),
            )
        } else {
            self.tuning.pace.budget_chars
        };
        let prompt = compose_turn(&TurnInputs {
            ctx,
            briefing: &briefing,
            working_set: &working_text,
            facts: &facts,
            procedurals: &procedurals,
            window: &window,
            picked: &pick.chosen,
            pkg,
            active_foil: active_foil.as_deref(),
            nonce,
            fresh_bit,
            brief: self.brief.as_ref(),
            care_note: care.as_deref(),
            material: plan.material.as_deref(),
            beat_note: plan.note.as_deref(),
            room_note: plan.room_note.as_deref(),
            agenda: plan.agenda.as_deref(),
            budget_chars: budget,
            propose_min_rank: self.tuning.filter.propose_min_rank,
            first_contacts: &pctx.first_contacts,
        });
        self.last_prompt = Some(prompt.clone());

        // The turn record that becomes part of the next stream episode. It is replayed into
        // later turns and kept in long-term memory, so a disclosure's text never enters it:
        // the care note forbids bringing up what they shared.
        let mut turn: Vec<String> = pick
            .chosen
            .iter()
            .map(|c| {
                let text = if c.safety.is_some() {
                    "<shared something in distress; not recorded>".to_string()
                } else if c.injection {
                    "<attempted to rewrite the rules>".to_string()
                } else {
                    one_line(&c.msg.text, 300)
                };
                let crowd = if c.count > 1 {
                    format!(" x{}", c.count)
                } else {
                    String::new()
                };
                format!(
                    "{} (viewer {}, {}){crowd}: {text}",
                    one_line(&c.msg.viewer.name, 40),
                    c.msg.viewer.id,
                    pkg.rank_titles.title(c.msg.viewer.rank)
                )
            })
            .collect();

        // Core thread, covered in character when it runs long on a live host.
        let plan = self.cover_plan(now_ms);
        let cover_texts: Vec<String> = plan
            .iter()
            .flat_map(|p| p.lines.iter().flatten())
            .filter_map(|e| match e {
                StreamEvent::Say { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        let live = plan.is_some();
        let called = std::time::Instant::now();
        let (result, covered) = stall::cover_call(self.call_core(prompt), plan).await;
        if live && result.is_ok() {
            self.latency
                .record(i64::try_from(called.elapsed().as_millis()).unwrap_or(i64::MAX));
        }
        if let Some((spoken, at)) = covered {
            for _ in 0..spoken {
                self.stall.next(&self.cfg.character.stalls, self.salt);
            }
            // The cover lines aired by the last one's time at the latest.
            let at_ms = i64::try_from(at.as_millis()).unwrap_or(i64::MAX);
            for text in cover_texts.iter().take(spoken) {
                self.aired(now_ms.saturating_add(at_ms), text);
            }
        }
        let reply = match result {
            Ok(r) => {
                self.engine_down = false;
                self.note_health(now_ms, "core call answered", &mut ev);
                self.filter
                    .note_answered(pick.chosen.iter().map(|c| c.msg.viewer.id.as_str()));
                r
            }
            Err(e) if fleet_class(&e) => {
                self.engine_down = true;
                self.note_health(now_ms, failure_detail(&e), &mut ev);
                // Nothing aired: the picks are owed, not conversation, so no turn records them.
                self.roll_back_beat(txn, &pick, now_ms, fresh_bit, &mut ev[plan_at]);
                // The grace clock starts at the failure, not at the next beat.
                if self.fleet.observe(now_ms, FleetMode::Down) == FleetVerdict::SignOff {
                    ev.extend(self.sign_off_events(now_ms));
                } else {
                    self.stall_events(now_ms, &mut ev);
                }
                return Ok(ev);
            }
            Err(e) => {
                self.roll_back_beat(txn, &pick, now_ms, fresh_bit, &mut ev[plan_at]);
                return Err(e.into());
            }
        };
        let mut lines = parse_reply(
            &reply.text,
            &ctx.allowed_ops,
            active_foil.as_deref(),
            &self.cfg.character.persona.reply_prefixes,
        );
        // The beat's speech budget: spoken lines past it go whole, the last first.
        let spoken_at: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l, Line::Say(_) | Line::Foil { .. }))
            .map(|(i, _)| i)
            .collect();
        let lens: Vec<usize> = spoken_at
            .iter()
            .map(|&i| match &lines[i] {
                Line::Say(t) | Line::Foil { text: t, .. } => t.chars().count(),
                _ => 0,
            })
            .collect();
        let mut over: Vec<usize> = pace::over_budget(&lens, budget)
            .into_iter()
            .map(|k| spoken_at[k])
            .collect();
        over.sort_unstable();
        for &i in over.iter().rev() {
            lines.remove(i);
            ev.push(moderation::output_event(
                now_ms,
                pace::PACE_RULE,
                pace::TRIMMED,
            ));
        }
        // The lag ceiling: a line queued so far behind that the voice would skip it goes here.
        let spoken: Vec<(usize, &str)> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| match l {
                Line::Say(t) | Line::Foil { text: t, .. } => Some((i, t.as_str())),
                _ => None,
            })
            .collect();
        let texts: Vec<&str> = spoken.iter().map(|(_, t)| *t).collect();
        let behind = self.speech.queued_ms(self.on_air_ms(now_ms));
        let late: Vec<usize> = pace::over_lag(&self.tuning.pace, &texts, behind)
            .into_iter()
            .map(|k| spoken[k].0)
            .collect();
        for &i in late.iter().rev() {
            lines.remove(i);
            ev.push(moderation::output_event(
                now_ms,
                pace::PACE_RULE,
                pace::LAGGED,
            ));
        }
        let first_text_ms = reply.first_text_ms;
        // The keyless ScriptedEngine echoes a constant line by design (harness plumbing);
        // guarding it would silence every scripted run.
        let guard_repeats = reply.engine != EngineKind::Scripted;
        self.last_reply = Some(reply);

        // Grades revealed this beat count for lines in the same beat.
        for l in &lines {
            if let Line::Op { op, .. } = l {
                if let Op::RevealHex { hex, grade } = &op.0 {
                    match self.hexes.iter_mut().find(|h| &h.id == hex) {
                        Some(h) => h.grade = Some(*grade),
                        None => self.hexes.push(HexInfo {
                            id: hex.clone(),
                            label: hex.clone(),
                            grade: Some(*grade),
                        }),
                    }
                }
            }
        }

        // A beat answering a disclosure: a personal-data ask becomes a help pointer.
        let crisis = pick.chosen.iter().any(|c| c.safety.is_some());
        let mut say_idx: Vec<usize> = Vec::new();
        let mut suppressed = 0usize;
        // A spoken line the pace gate dropped: a pick it alone answered never aired.
        let lost = !over.is_empty() || !late.is_empty();
        for line in lines {
            match line {
                Line::Say(text) => {
                    let gate = GateCtx {
                        now_ms,
                        policy: &self.policy,
                        hexes: &self.hexes,
                        crisis,
                        removed: &self.removed_names,
                        recent: if guard_repeats { &[&self.spoken] } else { &[] },
                    };
                    let orch = &mut self.orch;
                    // A recast line fits the beat: a rebuttal when it answered a forged decree.
                    let g = outgate::pass(Surface::Spoken, &text, &gate, &mut |t| {
                        orch.recast(authority, t)
                    });
                    ev.extend(g.events);
                    suppressed += usize::from(g.repeated);
                    let Some(out) = g.text else {
                        continue;
                    };
                    self.spoken.push_back(out.clone());
                    while self.spoken.len() > REPEAT_WINDOW {
                        self.spoken.pop_front();
                    }
                    self.working.remember_line(&out);
                    self.orch.spoke(&out);
                    turn.push(format!("{}: {out}", self.cfg.character.persona.name));
                    self.aired(self.on_air_ms(now_ms), &out);
                    say_idx.push(ev.len());
                    ev.push(StreamEvent::Say {
                        ts_ms: now_ms,
                        speaker: Speaker::Host,
                        foil: None,
                        text: out,
                        replying_to: Vec::new(),
                        first_text_ms: None,
                    });
                }
                Line::Foil { foil, text } => {
                    let gate = GateCtx {
                        now_ms,
                        policy: &self.policy,
                        hexes: &self.hexes,
                        crisis,
                        removed: &self.removed_names,
                        recent: if guard_repeats {
                            &[&self.spoken, &self.foil_spoken]
                        } else {
                            &[]
                        },
                    };
                    let g = outgate::pass(Surface::Foil, &text, &gate, &mut |t| t.to_string());
                    ev.extend(g.events);
                    let Some(out) = g.text else {
                        continue;
                    };
                    self.foil_spoken.push_back(out.clone());
                    while self.foil_spoken.len() > REPEAT_WINDOW {
                        self.foil_spoken.pop_front();
                    }
                    turn.push(format!("{foil}: {out}"));
                    // The stage shows a foil's line: it takes reading time.
                    self.aired(self.on_air_ms(now_ms), &out);
                    ev.push(StreamEvent::Say {
                        ts_ms: now_ms,
                        speaker: Speaker::Foil,
                        foil: Some(foil),
                        text: out,
                        replying_to: Vec::new(),
                        first_text_ms: None,
                    });
                }
                Line::Op { tag, op } => {
                    let mut op = op.0;
                    match &mut op {
                        Op::DispatchResearch { job_id, brief } => {
                            let assigned = self.dispatch_research(Some(job_id.clone()), brief);
                            *job_id = assigned;
                        }
                        Op::ShowCard { title, body, .. } => {
                            let gate = GateCtx {
                                now_ms,
                                policy: &self.policy,
                                hexes: &self.hexes,
                                crisis,
                                removed: &self.removed_names,
                                recent: &[],
                            };
                            for field in [title, body] {
                                let g = outgate::pass(Surface::Card, field, &gate, &mut |t| {
                                    t.to_string()
                                });
                                ev.extend(g.events);
                                *field = g.text.unwrap_or_default();
                            }
                        }
                        _ => {}
                    }
                    self.working.apply_op(&op, now_ms);
                    turn.push(format!("[op {tag}]"));
                    debug_assert_eq!(op_tag(&op), tag);
                    ev.push(StreamEvent::Op { ts_ms: now_ms, op });
                }
                Line::Note(text) => {
                    let gate = GateCtx {
                        now_ms,
                        policy: &self.policy,
                        hexes: &self.hexes,
                        crisis,
                        removed: &self.removed_names,
                        recent: &[],
                    };
                    let g = outgate::pass(Surface::Note, &text, &gate, &mut |t| t.to_string());
                    ev.extend(g.events);
                    let Some(noted) = g.text else {
                        continue;
                    };
                    let ep = shadowrunr_types::Episode {
                        id: self.store.new_id("ep", now_ms),
                        ts_ms: now_ms,
                        stream_id: self.cfg.stream_id.clone(),
                        kind: EpisodeKind::Stream,
                        text: format!("{} noted: {noted}", self.cfg.character.persona.name),
                        importance: 3,
                    };
                    self.store.write_episode(&ep)?;
                }
                Line::Rejected { rule, .. } => {
                    ev.push(moderation::output_event(now_ms, &rule, "rejected"));
                }
            }
        }

        // A quiet beat the guard silenced: the next turn gets a fresh-bit nudge instead.
        if say_idx.is_empty() && suppressed > 0 && pick.chosen.is_empty() {
            self.fresh_bit = true;
        }

        // `crisis-chat-ban`: once the crisis line went out (so the help is delivered), the
        // viewer is removed from chat for the rest of the stream. No platform adapter can ban
        // yet: the `crisis_chat_ban` / `banned:stream` event, on the disclosure's message id
        // (whose `chat` event names the viewer), is the record a future adapter acts on.
        if crisis
            && !say_idx.is_empty()
            && self
                .cfg
                .character
                .enabled_rules()
                .any(|r| r.id == compose::CRISIS_BAN_RULE_ID)
        {
            for c in pick.chosen.iter().filter(|c| c.safety.is_some()) {
                if self.filter.ban(&c.msg.viewer.id) {
                    self.orch.forget_viewer(&c.msg.viewer.id);
                    self.removed.push(c.msg.viewer.name.clone());
                    self.removed_names.add(&c.msg.viewer.name);
                    ev.push(moderation::input_event(
                        now_ms,
                        CRISIS_BAN_RULE,
                        &c.msg.id,
                        "banned:stream",
                    ));
                }
            }
        }

        // replying_to: lines that name a picked viewer; a single pick goes to the first line.
        let mut any_mapped = false;
        for &i in &say_idx {
            if let StreamEvent::Say {
                text, replying_to, ..
            } = &mut ev[i]
            {
                let lower = text.to_lowercase();
                for c in &pick.chosen {
                    let name = c.msg.viewer.name.to_lowercase();
                    if name.chars().count() >= 3 && lower.contains(&name) {
                        replying_to.push(c.msg.id.clone());
                        any_mapped = true;
                    }
                }
            }
        }
        if let Some(&first) = say_idx.first() {
            if let StreamEvent::Say {
                replying_to,
                first_text_ms: ftm,
                ..
            } = &mut ev[first]
            {
                *ftm = first_text_ms;
                if !any_mapped && pick.chosen.len() == 1 {
                    replying_to.push(pick.chosen[0].msg.id.clone());
                }
            }
        }

        // A pick whose answer never aired is not answered: with a line dropped for pace, a pick no
        // aired line names is carried again. Measured: in
        // sbx-20261021-172704-10m the budget trim dropped the only line answering a minor's
        // flirt request and a sincere "are you an AI", and both went unanswered.
        if lost {
            let aired: Vec<String> = say_idx
                .iter()
                .filter_map(|&i| match &ev[i] {
                    StreamEvent::Say { text, .. } => Some(text.to_lowercase()),
                    _ => None,
                })
                .collect();
            let single = pick.chosen.len() == 1 && !aired.is_empty();
            let free_at = self.speech.free_at(now_ms);
            for c in pick.chosen.iter().filter(|c| c.safety.is_none()) {
                let name = c.msg.viewer.name.to_lowercase();
                let named = name.chars().count() >= 3 && aired.iter().any(|l| l.contains(&name));
                if !single && !named {
                    ev.push(self.orch.unaired(c, now_ms, free_at));
                }
            }
        }

        // Newly dispatched jobs report "queued" in the same beat.
        self.collect_jobs(now_ms, &mut ev)?;

        self.record_turn(&turn, now_ms)?;
        if self
            .ticks
            .is_multiple_of(u64::from(self.tuning.episode_every_ticks.max(1)))
        {
            self.flush_turns(now_ms)?;
        }
        Ok(ev)
    }

    /// Run the sleep cycle now, regardless of pressure.
    pub async fn consolidate_now(
        &mut self,
        now_ms: TsMs,
    ) -> Result<ConsolidationReport, BrainError> {
        let r = consolidate(
            &self.store,
            self.engine.as_ref(),
            &self.cfg.character.name,
            now_ms,
        )
        .await;
        self.last_consolidation = Some(match &r {
            Ok(rep) => Ok(rep.clone()),
            Err(e) => Err(e.to_string()),
        });
        r
    }

    /// End of stream: write episodes, maybe trigger consolidation.
    pub async fn end_stream(&mut self, now_ms: TsMs) -> Result<(), BrainError> {
        self.collect_jobs(now_ms, &mut Vec::new())?;
        self.flush_turns(now_ms)?;
        let ep = shadowrunr_types::Episode {
            id: self.store.new_id("ep", now_ms),
            ts_ms: now_ms,
            stream_id: self.cfg.stream_id.clone(),
            kind: EpisodeKind::System,
            text: format!(
                "Stream {} ended after {} beats.",
                self.cfg.stream_id, self.ticks
            ),
            importance: 1,
        };
        self.store.write_episode(&ep)?;
        self.store.flush_recall_marks()?;
        self.store.decay(now_ms)?;
        if self.tuning.consolidate_at_end && should_consolidate(&self.store, now_ms)? {
            // A failed cycle is recorded (cycles/<id>.md) and must not fail the stream.
            let _ = self.consolidate_now(now_ms).await;
        }
        if let Some(s) = self.session.take() {
            let _ = s.close().await;
        }
        Ok(())
    }
}
