//! `SegmentRuntime`: drives the active segment's table from chat, the brain's ops and the
//! clock. Deterministic given its inputs: time only ever comes from `now_ms` arguments, and
//! every collection that affects output order is ordered.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use serde::Serialize;
use shadowrunr_types::{
    Case, ChatMsg, Command, EvidenceGrade, ModStage, Op, Rank, SegmentContext, SegmentId,
    StreamEvent, TsMs, VoteTally,
};

use crate::case::{demo_case, validate_case};
use crate::command::{normalize_hex, parse_command};
use crate::evidence::SourceAllowlist;
use crate::ranks::RankTable;
use crate::table::{
    op_tag, segment_name, segment_table, GuardFacts, HandoverRule, SegmentTable, Trigger,
};
use crate::SegmentError;

/// How many chat message ids the runtime remembers for `highlight` validation.
const SEEN_MSG_CAP: usize = 10_000;

/// Runtime configuration. Every field has a working default.
#[derive(Debug, Clone, Default)]
pub struct SegmentConfig {
    /// Foil ids from the character package (`character.toml` `foils`), for SEG-01.
    pub foils: Vec<String>,
    /// The SEG-02A case. `None` loads the fictional demo case (`fictional: true`).
    pub case: Option<Case>,
    pub ranks: RankTable,
    pub allowlist: SourceAllowlist,
    /// Overrides the table's handover thresholds (assumptions) for segments that have one.
    pub handover: Option<HandoverRule>,
    /// Segment start time. `None`: the first `now_ms` the runtime sees.
    pub start_ms: Option<TsMs>,
}

/// A queued `!hint` lead. The text is untrusted chat; it is never put into the prompt
/// context by the runtime (only counts are).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lead {
    pub viewer_id: String,
    pub text: String,
    pub ts_ms: TsMs,
}

/// An accepted `!propose`. Untrusted chat text, like [`Lead`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Proposal {
    pub msg_id: String,
    pub viewer_id: String,
    pub rank: Rank,
    pub text: String,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PollKind {
    Crowd,
    Foil,
}

#[derive(Debug, Clone)]
struct Poll {
    id: String,
    question: String,
    options: Vec<String>,
    min_rank: Rank,
    closes_ms: TsMs,
    kind: PollKind,
    /// viewer id -> (option index, weight). One vote per viewer; the last one counts.
    votes: BTreeMap<String, (usize, f64)>,
}

impl Poll {
    fn option_index(&self, raw: &str) -> Option<usize> {
        let raw = raw.trim();
        if let Ok(n) = raw.parse::<usize>() {
            if (1..=self.options.len()).contains(&n) {
                return Some(n - 1);
            }
        }
        self.options
            .iter()
            .position(|o| o.trim().eq_ignore_ascii_case(raw))
    }

    fn tallies(&self) -> Vec<VoteTally> {
        let mut out: Vec<VoteTally> = self
            .options
            .iter()
            .map(|o| VoteTally {
                option: o.clone(),
                voters: 0,
                weight: 0.0,
            })
            .collect();
        for (idx, w) in self.votes.values() {
            out[*idx].voters += 1;
            out[*idx].weight += w;
        }
        out
    }

    fn summary(&self, now_ms: TsMs) -> String {
        let tallies = self
            .tallies()
            .iter()
            .map(|t| format!("{} = {:.1} ({} votes)", t.option, t.weight, t.voters))
            .collect::<Vec<_>>()
            .join(", ");
        let left = ((self.closes_ms - now_ms).max(0) + 999) / 1000;
        let kind = match self.kind {
            PollKind::Crowd => "crowd poll",
            PollKind::Foil => "foil vote",
        };
        format!(
            "open {kind} {}: \"{}\" | {tallies} | closes in {left} s | min rank {:?}",
            self.id, self.question, self.min_rank
        )
    }
}

/// Highest weight wins; ties go to more voters, then to the earlier option. No votes, no
/// winner.
fn winner(tallies: &[VoteTally]) -> Option<String> {
    let mut best: Option<&VoteTally> = None;
    for t in tallies.iter().filter(|t| t.voters > 0) {
        let better = best.is_none_or(|b| {
            t.weight > b.weight + 1e-9
                || ((t.weight - b.weight).abs() <= 1e-9 && t.voters > b.voters)
        });
        if better {
            best = Some(t);
        }
    }
    best.map(|t| t.option.clone())
}

fn moderation(
    ts_ms: TsMs,
    stage: ModStage,
    rule: &str,
    msg_id: Option<&str>,
    action: String,
) -> StreamEvent {
    StreamEvent::Moderation {
        ts_ms,
        stage,
        rule: rule.to_string(),
        msg_id: msg_id.map(str::to_string),
        action,
    }
}

fn command_name(c: &Command) -> &'static str {
    match c {
        Command::Summon { .. } => "summon",
        Command::Hint { .. } => "hint",
        Command::Grade { .. } => "grade",
        Command::Propose { .. } => "propose",
        Command::Vote { .. } => "vote",
    }
}

const GRADES: [EvidenceGrade; 4] = [
    EvidenceGrade::Doc,
    EvidenceGrade::Rep,
    EvidenceGrade::Clm,
    EvidenceGrade::Spc,
];

fn grade_name(g: EvidenceGrade) -> &'static str {
    match g {
        EvidenceGrade::Doc => "DOC",
        EvidenceGrade::Rep => "REP",
        EvidenceGrade::Clm => "CLM",
        EvidenceGrade::Spc => "SPC",
    }
}

pub struct SegmentRuntime {
    config: SegmentConfig,
    table: &'static SegmentTable,
    state: String,
    case: Case,
    started_ms: Option<TsMs>,
    last_now_ms: TsMs,
    poll: Option<Poll>,
    used_poll_ids: BTreeSet<String>,
    foil_vote_seq: u32,
    active_foil: Option<String>,
    /// A foil carried over from a previous segment as a "skeptical observer".
    observer_foil: Option<String>,
    ccv_since: Option<TsMs>,
    handover_pending_since: Option<TsMs>,
    /// hex id -> viewer id -> (grade, weight). Last grade per viewer per hex counts.
    grades: BTreeMap<String, BTreeMap<String, (EvidenceGrade, f64)>>,
    /// hex id -> viewer id -> lead. Last lead per viewer per hex counts.
    leads: BTreeMap<String, BTreeMap<String, Lead>>,
    proposals: Vec<Proposal>,
    jobs: BTreeSet<String>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    signed_off: bool,
}

impl SegmentRuntime {
    /// A runtime with the default config (no foils, the fictional demo case).
    ///
    /// # Panics
    ///
    /// Panics for a descoped segment (SEG-02B-I, SEG-02B-II). Use [`Self::with_config`] to
    /// get the refusal as an error instead.
    pub fn new(segment: SegmentId) -> Self {
        Self::with_config(segment, SegmentConfig::default()).unwrap_or_else(|e| panic!("{e}"))
    }

    /// A runtime for `segment`. Refuses descoped segments with [`SegmentError::Descoped`] and
    /// rejects an invalid config (bad foil ids, a broken case, zero thresholds).
    pub fn with_config(segment: SegmentId, config: SegmentConfig) -> Result<Self, SegmentError> {
        let table = Self::startable_table(segment)?;
        let mut foils = BTreeSet::new();
        for f in &config.foils {
            let ok = !f.is_empty()
                && f.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
                });
            if !ok || !foils.insert(f.as_str()) {
                return Err(SegmentError::Config {
                    detail: format!("foil id {f:?} is not a unique lowercase slug"),
                });
            }
        }
        if let Some(h) = &config.handover {
            if h.ccv_min == 0 || h.max_elapsed_s == 0 {
                return Err(SegmentError::Config {
                    detail: "handover thresholds must be positive".into(),
                });
            }
        }
        let case = config.case.clone().unwrap_or_else(demo_case);
        validate_case(&case)?;
        Ok(Self {
            started_ms: config.start_ms,
            config,
            table,
            state: table.initial.clone(),
            case,
            last_now_ms: 0,
            poll: None,
            used_poll_ids: BTreeSet::new(),
            foil_vote_seq: 0,
            active_foil: None,
            observer_foil: None,
            ccv_since: None,
            handover_pending_since: None,
            grades: BTreeMap::new(),
            leads: BTreeMap::new(),
            proposals: Vec::new(),
            jobs: BTreeSet::new(),
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            signed_off: false,
        })
    }

    fn startable_table(segment: SegmentId) -> Result<&'static SegmentTable, SegmentError> {
        let table = segment_table(segment)?;
        if table.is_descoped() {
            return Err(SegmentError::Descoped {
                segment,
                title: table.title.clone(),
            });
        }
        Ok(table)
    }

    /// Mark the segment as started at `now_ms` and return its `segment_start` event.
    /// Optional: without it the start time is the first `now_ms` seen.
    pub fn start(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        self.started_ms = Some(now_ms);
        self.last_now_ms = now_ms;
        vec![StreamEvent::SegmentStart {
            ts_ms: now_ms,
            segment: self.table.id,
        }]
    }

    // ----- accessors -------------------------------------------------------------------

    pub fn segment(&self) -> SegmentId {
        self.table.id
    }

    pub fn table(&self) -> &'static SegmentTable {
        self.table
    }

    /// Current state name in the active segment's table.
    pub fn state(&self) -> &str {
        &self.state
    }

    pub fn active_foil(&self) -> Option<&str> {
        self.active_foil.as_deref()
    }

    pub fn open_poll_id(&self) -> Option<&str> {
        self.poll.as_ref().map(|p| p.id.as_str())
    }

    /// Running tallies of the open poll, in option order.
    pub fn open_poll_tallies(&self) -> Option<Vec<VoteTally>> {
        self.poll.as_ref().map(Poll::tallies)
    }

    /// The case map with live fog and grades.
    pub fn case(&self) -> &Case {
        &self.case
    }

    pub fn proposals(&self) -> &[Proposal] {
        &self.proposals
    }

    /// Queued leads for a hex, ordered by viewer id.
    pub fn leads(&self, hex: &str) -> Vec<&Lead> {
        self.leads
            .get(hex)
            .map(|m| m.values().collect())
            .unwrap_or_default()
    }

    /// Rank-weighted community grades for a hex (DOC, REP, CLM, SPC order).
    pub fn jury_tally(&self, hex: &str) -> Vec<VoteTally> {
        let mut out: Vec<VoteTally> = GRADES
            .iter()
            .map(|g| VoteTally {
                option: grade_name(*g).to_string(),
                voters: 0,
                weight: 0.0,
            })
            .collect();
        if let Some(votes) = self.grades.get(hex) {
            for (g, w) in votes.values() {
                let i = GRADES.iter().position(|x| x == g).unwrap_or(0);
                out[i].voters += 1;
                out[i].weight += w;
            }
        }
        out
    }

    /// The jury's leading grade for a hex: advice for the brain, never proof.
    pub fn jury_grade(&self, hex: &str) -> Option<EvidenceGrade> {
        let w = winner(&self.jury_tally(hex))?;
        GRADES.iter().copied().find(|g| grade_name(*g) == w)
    }

    pub fn is_signed_off(&self) -> bool {
        self.signed_off
    }

    pub fn is_handover_pending(&self) -> bool {
        self.handover_pending_since.is_some()
    }

    // ----- internals -------------------------------------------------------------------

    fn observe(&mut self, now_ms: TsMs) {
        self.started_ms.get_or_insert(now_ms);
        self.last_now_ms = self.last_now_ms.max(now_ms);
    }

    fn remember_msg(&mut self, id: &str) {
        if self.seen.insert(id.to_string()) {
            self.seen_order.push_back(id.to_string());
            if self.seen_order.len() > SEEN_MSG_CAP {
                if let Some(old) = self.seen_order.pop_front() {
                    self.seen.remove(&old);
                }
            }
        }
    }

    fn facts(&self) -> GuardFacts {
        GuardFacts {
            foil_active: self.active_foil.is_some(),
            any_revealed: self.case.hexes.iter().any(|h| !h.fog),
        }
    }

    /// Run the table interpreter for one trigger.
    fn fire(&mut self, on: Trigger) {
        if let Some(to) = self.table.next_state(&self.state, on, self.facts()) {
            self.state = to.to_string();
        }
    }

    fn handover_rule(&self) -> Option<HandoverRule> {
        self.table
            .clock
            .handover
            .map(|table_rule| self.config.handover.unwrap_or(table_rule))
    }

    fn find_hex(&self, raw: &str) -> Option<usize> {
        let canon = normalize_hex(raw);
        self.case
            .hexes
            .iter()
            .position(|h| h.id == raw || canon.as_deref() == Some(h.id.as_str()))
    }

    fn open_poll(&mut self, poll: Poll) {
        self.used_poll_ids.insert(poll.id.clone());
        self.poll = Some(poll);
    }

    fn vote_event(&self, now_ms: TsMs, poll: &Poll, closed: bool) -> StreamEvent {
        StreamEvent::Vote {
            ts_ms: now_ms,
            poll_id: poll.id.clone(),
            tallies: poll.tallies(),
            closed,
        }
    }

    /// Close the open poll. A foil vote that closes with `resolve` summons its winner.
    fn close_poll(&mut self, now_ms: TsMs, resolve: bool) -> Vec<StreamEvent> {
        let Some(poll) = self.poll.take() else {
            return Vec::new();
        };
        let mut events = vec![self.vote_event(now_ms, &poll, true)];
        match poll.kind {
            PollKind::Crowd => self.fire(Trigger::PollClosed),
            PollKind::Foil => {
                if resolve {
                    if let Some(foil) = winner(&poll.tallies()) {
                        self.active_foil = Some(foil.clone());
                        events.push(StreamEvent::Op {
                            ts_ms: now_ms,
                            op: Op::SummonFoil { foil },
                        });
                    }
                }
                self.fire(Trigger::FoilVoteClosed);
            }
        }
        events
    }

    fn switch_to(&mut self, now_ms: TsMs, to: SegmentId) -> Result<Vec<StreamEvent>, SegmentError> {
        let table = Self::startable_table(to)?;
        let mut events = self.close_poll(now_ms, false);
        events.push(StreamEvent::SegmentEnd {
            ts_ms: now_ms,
            segment: self.table.id,
            ses: None,
        });
        events.push(StreamEvent::SegmentStart {
            ts_ms: now_ms,
            segment: to,
        });
        self.table = table;
        self.state = table.initial.clone();
        self.started_ms = Some(now_ms);
        self.ccv_since = None;
        self.handover_pending_since = None;
        if let Some(foil) = self.active_foil.take() {
            self.observer_foil = Some(foil);
        }
        Ok(events)
    }

    // ----- chat ------------------------------------------------------------------------

    /// Commands and votes from new chat. Returns `vote` events (one running tally per poll
    /// or graded hex touched in this batch) and `moderation` events (stage `input`) for
    /// under-ranked or invalid commands. Commands the segment does not take are ignored.
    pub fn ingest_chat(&mut self, now_ms: TsMs, msgs: &[ChatMsg]) -> Vec<StreamEvent> {
        self.observe(now_ms);
        let mut events = Vec::new();
        let mut poll_dirty = false;
        let mut graded: BTreeSet<String> = BTreeSet::new();
        for msg in msgs {
            self.remember_msg(&msg.id);
            if self.signed_off {
                continue;
            }
            let Some(cmd) = msg.command.clone().or_else(|| parse_command(&msg.text)) else {
                continue;
            };
            if !self.table.allows_command(command_name(&cmd)) {
                continue;
            }
            let rank = msg.viewer.rank;
            let weight = self.config.ranks.vote_weight(rank);
            let viewer = msg.viewer.id.clone();
            let ignored = |rule: &str| {
                moderation(
                    now_ms,
                    ModStage::Input,
                    rule,
                    Some(&msg.id),
                    "ignored".into(),
                )
            };
            match cmd {
                Command::Summon { foil } => {
                    if !self.config.foils.contains(&foil) {
                        events.push(ignored("unknown_foil"));
                        continue;
                    }
                    if self.poll.is_none()
                        && self.table.accepts(&self.state, Trigger::FoilVoteOpened)
                    {
                        let ev = self.open_foil_vote(now_ms);
                        events.push(ev);
                    }
                    if let Some(p) = self.poll.as_mut().filter(|p| p.kind == PollKind::Foil) {
                        if let Some(i) = p.option_index(&foil) {
                            p.votes.insert(viewer, (i, weight));
                            poll_dirty = true;
                        }
                    }
                }
                Command::Vote { option } => {
                    let Some(p) = self.poll.as_mut() else {
                        continue;
                    };
                    let Some(i) = p.option_index(&option) else {
                        continue;
                    };
                    if rank < p.min_rank {
                        events.push(ignored("poll_min_rank"));
                        continue;
                    }
                    p.votes.insert(viewer, (i, weight));
                    poll_dirty = true;
                }
                Command::Propose { text } => {
                    if !self.config.ranks.may_propose(rank) {
                        events.push(ignored("propose_min_rank"));
                        continue;
                    }
                    self.proposals.push(Proposal {
                        msg_id: msg.id.clone(),
                        viewer_id: viewer,
                        rank,
                        text,
                        ts_ms: msg.ts_ms,
                    });
                }
                Command::Hint { hex, text } => {
                    let Some(i) = self.find_hex(&hex) else {
                        events.push(ignored("unknown_hex"));
                        continue;
                    };
                    let id = self.case.hexes[i].id.clone();
                    self.leads.entry(id).or_default().insert(
                        viewer.clone(),
                        Lead {
                            viewer_id: viewer,
                            text,
                            ts_ms: msg.ts_ms,
                        },
                    );
                }
                Command::Grade { hex, grade } => {
                    let Some(i) = self.find_hex(&hex) else {
                        events.push(ignored("unknown_hex"));
                        continue;
                    };
                    let id = self.case.hexes[i].id.clone();
                    self.grades
                        .entry(id.clone())
                        .or_default()
                        .insert(viewer, (grade, weight));
                    graded.insert(id);
                }
            }
        }
        if poll_dirty {
            if let Some(p) = &self.poll {
                events.push(self.vote_event(now_ms, p, false));
            }
        }
        for hex in graded {
            events.push(StreamEvent::Vote {
                ts_ms: now_ms,
                tallies: self.jury_tally(&hex),
                poll_id: format!("grade:{hex}"),
                closed: false,
            });
        }
        events
    }

    fn open_foil_vote(&mut self, now_ms: TsMs) -> StreamEvent {
        let id = loop {
            self.foil_vote_seq += 1;
            let id = format!("foil-vote-{}", self.foil_vote_seq);
            if !self.used_poll_ids.contains(&id) {
                break id;
            }
        };
        let secs = self.table.clock.foil_vote_secs;
        let question = "Which foil shall the Arch-Lich summon?".to_string();
        let options = self.config.foils.clone();
        self.open_poll(Poll {
            id: id.clone(),
            question: question.clone(),
            options: options.clone(),
            min_rank: Rank::Thrall,
            closes_ms: now_ms + i64::from(secs) * 1000,
            kind: PollKind::Foil,
            votes: BTreeMap::new(),
        });
        self.fire(Trigger::FoilVoteOpened);
        StreamEvent::Op {
            ts_ms: now_ms,
            op: Op::StartPoll {
                poll_id: id,
                question,
                options,
                min_rank: Rank::Thrall,
                secs,
            },
        }
    }

    // ----- ops -------------------------------------------------------------------------

    /// Validate and apply the brain's ops. An accepted op yields its `op` event followed by
    /// any consequence (a closed `vote`, `segment_end`/`segment_start`...). A rejected op
    /// yields one `moderation` event (stage `output`, action `rejected:<op>`) whose `rule`
    /// names the failed check.
    pub fn apply_ops(&mut self, now_ms: TsMs, ops: &[Op]) -> Vec<StreamEvent> {
        self.observe(now_ms);
        let mut events = Vec::new();
        for op in ops {
            match self.apply_op(now_ms, op) {
                Ok(consequences) => {
                    events.push(StreamEvent::Op {
                        ts_ms: now_ms,
                        op: op.clone(),
                    });
                    events.extend(consequences);
                }
                Err(rule) => events.push(moderation(
                    now_ms,
                    ModStage::Output,
                    rule,
                    None,
                    format!("rejected:{}", op_tag(op)),
                )),
            }
        }
        events
    }

    fn require_state(&self, on: Trigger) -> Result<(), &'static str> {
        if self.table.accepts(&self.state, on) {
            Ok(())
        } else {
            Err("not_in_state")
        }
    }

    fn apply_op(&mut self, now_ms: TsMs, op: &Op) -> Result<Vec<StreamEvent>, &'static str> {
        if self.signed_off {
            return Err("signed_off");
        }
        if !self.table.allows_op(op_tag(op)) {
            return Err("op_not_allowed");
        }
        match op {
            Op::Highlight { msg_id } => {
                if !self.seen.contains(msg_id) {
                    return Err("unknown_msg");
                }
            }
            Op::StartPoll {
                poll_id,
                question,
                options,
                min_rank,
                secs,
            } => {
                if self.poll.is_some() {
                    return Err("poll_already_open");
                }
                self.require_state(Trigger::PollOpened)?;
                if poll_id.trim().is_empty() || self.used_poll_ids.contains(poll_id) {
                    return Err("duplicate_poll_id");
                }
                if question.trim().is_empty() {
                    return Err("invalid_poll_question");
                }
                let clock = &self.table.clock;
                let distinct: BTreeSet<String> =
                    options.iter().map(|o| o.trim().to_lowercase()).collect();
                if options.len() < 2
                    || options.len() > clock.max_poll_options
                    || distinct.len() != options.len()
                    || distinct.contains("")
                {
                    return Err("invalid_poll_options");
                }
                if !(clock.poll_min_secs..=clock.poll_max_secs).contains(secs) {
                    return Err("invalid_poll_secs");
                }
                self.open_poll(Poll {
                    id: poll_id.clone(),
                    question: question.clone(),
                    options: options.clone(),
                    min_rank: *min_rank,
                    closes_ms: now_ms + i64::from(*secs) * 1000,
                    kind: PollKind::Crowd,
                    votes: BTreeMap::new(),
                });
                self.fire(Trigger::PollOpened);
            }
            Op::ClosePoll { poll_id } => {
                if self.poll.as_ref().is_none_or(|p| &p.id != poll_id) {
                    return Err("no_such_poll");
                }
                return Ok(self.close_poll(now_ms, true));
            }
            Op::SummonFoil { foil } => {
                if !self.config.foils.contains(foil) {
                    return Err("unknown_foil");
                }
                self.require_state(Trigger::FoilSummoned)?;
                if self.active_foil.as_deref() == Some(foil) {
                    return Err("foil_already_active");
                }
                self.active_foil = Some(foil.clone());
                self.fire(Trigger::FoilSummoned);
            }
            Op::DismissFoil => {
                if self.active_foil.is_none() {
                    return Err("no_active_foil");
                }
                self.require_state(Trigger::FoilDismissed)?;
                self.active_foil = None;
                self.fire(Trigger::FoilDismissed);
            }
            Op::RevealHex { hex, grade } => {
                let Some(i) = self.find_hex(hex) else {
                    return Err("unknown_hex");
                };
                self.require_state(Trigger::HexRevealed)?;
                let h = &self.case.hexes[i];
                if !h.fog && h.grade == Some(*grade) {
                    return Err("already_revealed");
                }
                if *grade == EvidenceGrade::Doc
                    && !self
                        .config
                        .allowlist
                        .hex_has_primary_source(h, self.case.fictional)
                {
                    return Err("doc_requires_primary_source");
                }
                let h = &mut self.case.hexes[i];
                h.fog = false;
                h.grade = Some(*grade);
                self.fire(Trigger::HexRevealed);
            }
            Op::DispatchResearch { job_id, brief } => {
                if job_id.trim().is_empty() || brief.trim().is_empty() {
                    return Err("invalid_job");
                }
                if !self.jobs.insert(job_id.clone()) {
                    return Err("duplicate_job_id");
                }
            }
            Op::ShowCard { card_id, title, .. } => {
                if card_id.trim().is_empty() || title.trim().is_empty() {
                    return Err("invalid_card");
                }
            }
            Op::MarkMoment { label } => {
                if label.trim().is_empty() {
                    return Err("invalid_moment");
                }
            }
            Op::SwitchSegment { to } => {
                if *to == self.table.id {
                    return Err("already_in_segment");
                }
                if !self.table.switch_to.contains(to) {
                    return Err("switch_not_allowed");
                }
                if Self::startable_table(*to).is_err() {
                    return Err("segment_descoped");
                }
                let mut events = Vec::new();
                if self.handover_pending_since.is_none() {
                    events.push(StreamEvent::Handover {
                        ts_ms: now_ms,
                        from: self.table.id,
                        to: *to,
                        reason: "switch_segment op".into(),
                    });
                }
                events.extend(
                    self.switch_to(now_ms, *to)
                        .map_err(|_| "segment_descoped")?,
                );
                return Ok(events);
            }
            Op::SignOff { .. } => {
                let events = self.close_poll(now_ms, false);
                self.signed_off = true;
                return Ok(events);
            }
        }
        Ok(Vec::new())
    }

    // ----- clock -----------------------------------------------------------------------

    /// Clock: poll timeouts (a closed `vote`; a foil vote also summons its winner) and the
    /// handover rule. When the rule fires it emits `handover` and enters the
    /// `handover_pending` state; the brain is expected to emit `switch_segment`, and after
    /// the rule's grace period the clock switches by itself (`segment_end`, `segment_start`).
    pub fn tick(&mut self, now_ms: TsMs, ccv: u32) -> Vec<StreamEvent> {
        self.observe(now_ms);
        let mut events = Vec::new();
        if self.signed_off {
            return events;
        }
        if self.poll.as_ref().is_some_and(|p| now_ms >= p.closes_ms) {
            events.extend(self.close_poll(now_ms, true));
        }
        let Some(rule) = self.handover_rule() else {
            return events;
        };
        if let Some(since) = self.handover_pending_since {
            if now_ms - since >= i64::from(rule.grace_s) * 1000 {
                if let Ok(evs) = self.switch_to(now_ms, rule.to) {
                    events.extend(evs);
                }
            }
            return events;
        }
        if ccv >= rule.ccv_min {
            self.ccv_since.get_or_insert(now_ms);
        } else {
            self.ccv_since = None;
        }
        let started = self.started_ms.unwrap_or(now_ms);
        let sustained = self
            .ccv_since
            .is_some_and(|s| now_ms - s >= i64::from(rule.sustain_s) * 1000);
        let timed_out = now_ms - started >= i64::from(rule.max_elapsed_s) * 1000;
        if sustained || timed_out {
            let reason = if sustained {
                format!("ccv >= {} sustained {} s", rule.ccv_min, rule.sustain_s)
            } else {
                format!("elapsed >= {} s", rule.max_elapsed_s)
            };
            events.push(StreamEvent::Handover {
                ts_ms: now_ms,
                from: self.table.id,
                to: rule.to,
                reason,
            });
            self.fire(Trigger::HandoverDue);
            self.handover_pending_since = Some(now_ms);
            if rule.grace_s == 0 {
                if let Ok(evs) = self.switch_to(now_ms, rule.to) {
                    events.extend(evs);
                }
            }
        }
        events
    }

    // ----- context ---------------------------------------------------------------------

    /// What the brain sees this tick. `rules_prompt` is stable within a segment (table text
    /// plus config facts); `state_summary` carries the live state. Untrusted chat text
    /// (leads, proposals) is summarised as counts only.
    pub fn context(&self, ccv: u32, elapsed_s: u32) -> SegmentContext {
        SegmentContext {
            segment: self.table.id,
            rules_prompt: self.rules_prompt(),
            state_summary: self.state_summary(),
            allowed_ops: if self.signed_off {
                Vec::new()
            } else {
                self.table.allowed_ops.clone()
            },
            ccv,
            elapsed_s,
        }
    }

    fn rules_prompt(&self) -> String {
        let t = self.table;
        let mut p = t.rules_prompt.trim_end().to_string();
        if t.allows_op("summon_foil") || t.allows_command("summon") {
            if self.config.foils.is_empty() {
                p.push_str("\nNo foils are available tonight.");
            } else {
                p.push_str(&format!(
                    "\nFoils you may summon: {}.",
                    self.config.foils.join(", ")
                ));
            }
        }
        if t.allows_op("reveal_hex") {
            p.push_str(&format!(
                "\nCase: {} ({}), {} hexes.",
                self.case.title,
                self.case.id,
                self.case.hexes.len()
            ));
            if self.case.fictional {
                p.push_str(
                    " THIS CASE IS FICTIONAL DEMO DATA: say so when asked and never present \
                     it as a real investigation.",
                );
            }
        }
        p
    }

    fn state_summary(&self) -> String {
        let mut lines = vec![format!(
            "segment {} ({}), state {}",
            segment_name(self.table.id),
            self.table.title,
            self.state
        )];
        if self.signed_off {
            lines.push("signed off: the show is over".into());
        }
        if let Some(f) = &self.active_foil {
            lines.push(format!("active foil: {f}"));
        }
        if let Some(f) = &self.observer_foil {
            lines.push(format!("skeptical observer: {f}"));
        }
        match &self.poll {
            Some(p) => lines.push(p.summary(self.last_now_ms)),
            None => lines.push("no poll open".into()),
        }
        if self.handover_pending_since.is_some() {
            if let Some(rule) = self.handover_rule() {
                lines.push(format!(
                    "HANDOVER DUE: let the foil dare you, then emit switch_segment to {}",
                    segment_name(rule.to)
                ));
            }
        }
        if self.table.allows_op("reveal_hex") {
            let revealed: Vec<String> = self
                .case
                .hexes
                .iter()
                .filter(|h| !h.fog)
                .map(|h| {
                    let grade = h.grade.map_or("?", grade_name);
                    let alleged = if h.grade.is_some_and(crate::requires_alleged) {
                        " ALLEGED"
                    } else {
                        ""
                    };
                    let jury = self
                        .jury_grade(&h.id)
                        .map(|g| format!(", jury {}", grade_name(g)))
                        .unwrap_or_default();
                    format!("{} {grade}{alleged}{jury}: {}", h.id, h.label)
                })
                .collect();
            let fogged: Vec<&str> = self
                .case
                .hexes
                .iter()
                .filter(|h| h.fog)
                .map(|h| h.id.as_str())
                .collect();
            lines.push(format!(
                "revealed {}/{}: {}",
                revealed.len(),
                self.case.hexes.len(),
                if revealed.is_empty() {
                    "none".to_string()
                } else {
                    revealed.join("; ")
                }
            ));
            lines.push(format!("fogged: {}", fogged.join(", ")));
            let mut leads: Vec<(usize, &String)> =
                self.leads.iter().map(|(h, m)| (m.len(), h)).collect();
            leads.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
            if !leads.is_empty() {
                let top: Vec<String> = leads
                    .iter()
                    .take(5)
                    .map(|(n, h)| format!("{h} x{n}"))
                    .collect();
                lines.push(format!("minion leads: {}", top.join(", ")));
            }
            if !self.jobs.is_empty() {
                lines.push(format!("research jobs dispatched: {}", self.jobs.len()));
            }
        }
        if !self.proposals.is_empty() {
            lines.push(format!("proposals waiting: {}", self.proposals.len()));
        }
        lines.join("\n")
    }
}
