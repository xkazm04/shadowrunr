//! The character's mind: package loader, prompt composer, long-term memory, operational
//! memory, worker pool, chat picker, moderation gates and the fleet-loss policy.
//!
//! One `Brain::tick` is one beat of the core thread:
//! 1. fleet check (sign off after the grace period);
//! 2. collect finished background jobs (never awaits one);
//! 3. pre-filter chat (term rules, rate caps, injection tagging, flood/copypasta clusters);
//! 4. pick messages (deterministic, or `Role::ChatPicker` above a volume threshold);
//! 5. recall and compose (stable prefix + per-turn block);
//! 6. call the core thread (warm session when available, else `complete`);
//! 7. parse spoken lines, `OP:` and `NOTE:`; gate output; dispatch research jobs.

pub mod character;
pub mod compose;
pub mod fleet;
pub mod grammar;
pub mod memory;
pub mod moderation;
pub mod picker;
pub mod pool;
pub mod util;
pub mod working;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use shadowrunr_types::{
    Case, ChatMsg, Engine, EngineError, EngineReply, EngineRequest, EpisodeKind, FactScope,
    ModelTier, Op, Role, SegmentContext, Speaker, StreamEvent, TsMs, WarmSession,
};

pub use character::{load_character, CharacterPackage, Foil, RankTitles, Situation, SituationBank};
pub use fleet::{FleetMode, FleetProbe};
pub use memory::{
    consolidate, rebuild_index, should_consolidate, ConsolidationReport, FactStatus, MemoryStore,
    StoredFact,
};
pub use moderation::{FilterConfig, HexInfo, PolicySource};
pub use picker::PickerConfig;
pub use pool::{Job, JobCtx, JobRunner, JobStatus, JobUpdate, ResearchRunner, WorkerPool};

use compose::{compose_turn, stable_prefix, TurnInputs, ViewerBrief};
use fleet::{FleetVerdict, FleetWatch};
use grammar::{op_tag, parse_reply, Line};
use moderation::{gate_text, output_events, InputFilter, Policy, TermAction};
use picker::{PickContext, Picker};
use util::{fnv1a64, one_line};
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
    /// Seconds Malakor keeps chatting after the fleet goes degraded or down.
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
    pub working_set_ttl_ms: i64,
    /// Run the sleep cycle at `end_stream` when memory pressure warrants it.
    pub consolidate_at_end: bool,
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
            working_set_ttl_ms: working::WORKING_SET_TTL_MS,
            consolidate_at_end: true,
        }
    }
}

const SIGN_OFF_LINE: &str = "The Lord of the Darkness recalls me to the crypt... Guard the \
    throne room, minions. I shall return.";

/// Recalled facts beyond the briefing, and the always-included tier per scope.
const RECALL_FACTS: usize = 8;
const ALWAYS_PER_SCOPE: usize = 3;
const VIEWER_FACTS: usize = 5;
const RECALL_EPISODES: usize = 8;
const EPISODE_ITEM_CHARS: usize = 2_000;

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
    store: MemoryStore,
    pool: Arc<WorkerPool>,
    policy: Policy,
    filter: InputFilter,
    picker: Picker,
    working: WorkingSet,
    fleet: FleetWatch,
    fleet_probe: Option<Arc<dyn FleetProbe>>,
    fleet_override: Option<FleetMode>,
    engine_down: bool,
    session: Option<Box<dyn WarmSession>>,
    session_unsupported: bool,
    hexes: Vec<HexInfo>,
    turns: Vec<String>,
    ticks: u64,
    salt: u64,
    names: Vec<String>,
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
        let policy = Policy::load(&tuning.policy, &cfg.character.dir, &tuning.platforms)?;
        let prefix = stable_prefix(&cfg.character);
        let mut names: Vec<String> = ["malakor", "lich", "overlord", "my lord"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        for w in cfg.character.name.to_lowercase().split_whitespace() {
            if w.chars().count() >= 4 && !names.iter().any(|n| n == w) {
                names.push(w.to_string());
            }
        }
        let salt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Ok(Self {
            pool: Arc::new(WorkerPool::new(cfg.pool_size)),
            filter: InputFilter::new(tuning.filter.clone()),
            picker: Picker::new(tuning.picker.clone()),
            working: WorkingSet::new(tuning.working_set_ttl_ms),
            fleet: FleetWatch::new(tuning.fleet_grace_s),
            fleet_probe: None,
            fleet_override: None,
            engine_down: false,
            session: None,
            session_unsupported: false,
            hexes: Vec::new(),
            turns: Vec::new(),
            ticks: 0,
            salt,
            names,
            last_prompt: None,
            last_reply: None,
            last_consolidation: None,
            cfg,
            engine,
            tuning,
            prefix,
            store,
            policy,
        })
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
        if self.tuning.warm_session && !self.session_unsupported {
            if self.session.is_none() {
                match self.engine.open_session(self.core_template()).await {
                    Ok(s) => self.session = Some(s),
                    Err(_) => self.session_unsupported = true,
                }
            }
            if let Some(session) = self.session.as_mut() {
                match session.send(prompt.clone()).await {
                    Ok(r) => return Ok(r),
                    Err(_) => {
                        // The warm process died: fall back to a cold call, reopen next beat.
                        self.session = None;
                    }
                }
            }
        }
        let mut req = self.core_template();
        req.prompt = prompt;
        self.engine.complete(req).await
    }

    fn sign_off_events(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        let grace = self.tuning.fleet_grace_s;
        vec![
            StreamEvent::Say {
                ts_ms: now_ms,
                speaker: Speaker::Malakor,
                foil: None,
                text: SIGN_OFF_LINE.to_string(),
                replying_to: Vec::new(),
                first_text_ms: None,
            },
            StreamEvent::Op {
                ts_ms: now_ms,
                op: Op::SignOff {
                    reason: format!("fleet_lost: engine degraded or down for {grace}s"),
                },
            },
        ]
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

        // Pre-filter and pick.
        let (cands, mods) = self.filter.filter(now_ms, inbox, &self.policy);
        ev.extend(mods);
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
        };
        let corr = format!("{}:pick:{}", self.cfg.stream_id, self.ticks);
        let pick = self
            .picker
            .pick(&cands, &pctx, self.engine.as_ref(), &corr)
            .await;
        if !pick.chosen.is_empty() {
            ev.push(StreamEvent::Picked {
                ts_ms: now_ms,
                msg_ids: pick.ids(),
                reason: pick.reason.clone(),
            });
        }

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
        let working_text = self.working.render(foil_text.as_deref());
        let nonce = fnv1a64(format!("{}:{}:{}", self.salt, self.ticks, now_ms).as_bytes());
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
        });
        self.last_prompt = Some(prompt.clone());

        // The turn record that becomes part of the next stream episode.
        let mut turn: Vec<String> = pick
            .chosen
            .iter()
            .map(|c| {
                let text = if c.injection {
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

        // Core thread.
        let reply = match self.call_core(prompt).await {
            Ok(r) => {
                self.engine_down = false;
                r
            }
            Err(e) if fleet_class(&e) => {
                self.engine_down = true;
                self.turns.extend(turn);
                // The grace clock starts at the failure, not at the next beat.
                if self.fleet.observe(now_ms, FleetMode::Down) == FleetVerdict::SignOff {
                    ev.extend(self.sign_off_events(now_ms));
                }
                return Ok(ev);
            }
            Err(e) => return Err(e.into()),
        };
        let lines = parse_reply(&reply.text, &ctx.allowed_ops, active_foil.as_deref());
        let first_text_ms = reply.first_text_ms;
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

        let mut say_idx: Vec<usize> = Vec::new();
        for line in lines {
            match line {
                Line::Say(text) => {
                    let (out, fired) = gate_text(&text, &self.policy, &self.hexes);
                    ev.extend(output_events(now_ms, &fired));
                    self.working.remember_line(&out);
                    turn.push(format!("Malakor: {out}"));
                    say_idx.push(ev.len());
                    ev.push(StreamEvent::Say {
                        ts_ms: now_ms,
                        speaker: Speaker::Malakor,
                        foil: None,
                        text: out,
                        replying_to: Vec::new(),
                        first_text_ms: None,
                    });
                }
                Line::Foil { foil, text } => {
                    let (out, fired) = gate_text(&text, &self.policy, &self.hexes);
                    ev.extend(output_events(now_ms, &fired));
                    turn.push(format!("{foil}: {out}"));
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
                            let (t, f1) = gate_text(title, &self.policy, &self.hexes);
                            let (b, f2) = gate_text(body, &self.policy, &self.hexes);
                            ev.extend(output_events(now_ms, &f1));
                            ev.extend(output_events(now_ms, &f2));
                            *title = t;
                            *body = b;
                        }
                        _ => {}
                    }
                    self.working.apply_op(&op, now_ms);
                    turn.push(format!("[op {tag}]"));
                    debug_assert_eq!(op_tag(&op), tag);
                    ev.push(StreamEvent::Op { ts_ms: now_ms, op });
                }
                Line::Note(text) => {
                    if self
                        .policy
                        .strictest(&text)
                        .is_some_and(|r| r.action == TermAction::Block)
                    {
                        ev.push(moderation::output_event(now_ms, "note_blocked", "dropped"));
                        continue;
                    }
                    let ep = shadowrunr_types::Episode {
                        id: self.store.new_id("ep", now_ms),
                        ts_ms: now_ms,
                        stream_id: self.cfg.stream_id.clone(),
                        kind: EpisodeKind::Stream,
                        text: format!("Malakor noted: {}", self.policy.bleep(&text)),
                        importance: 3,
                    };
                    self.store.write_episode(&ep)?;
                }
                Line::Rejected { rule, .. } => {
                    ev.push(moderation::output_event(now_ms, &rule, "rejected"));
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

        // Newly dispatched jobs report "queued" in the same beat.
        self.collect_jobs(now_ms, &mut ev)?;

        self.turns.push(turn.join("\n"));
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
