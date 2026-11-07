//! One bounded, persistent, audience-authored arc ([`ArcDef`] on an approved stream prep).
//!
//! The arc is a small state machine the owner writes and approves: states with what is true in
//! them, a consequence (payoff) owed the night after a state is entered, a vote that moves it on,
//! and the permitted transitions keyed to that vote's options. The model performs it; it never
//! moves it. A transition happens only when:
//! 1. the runtime confirms the outcome of a poll the brain itself put up for the current
//!    state's vote, with a single winner that a permitted transition names
//!    ([`ArcRun::confirm_vote`], a typed vote result, never state text or chat), and
//! 2. a beat that announces it airs ([`ArcRun::delivered`]).
//!
//! Both steps are written to markdown under `<memory>/arcs/<arc id>/` with an idempotent event
//! id (`<stream id>:<poll id>`), so a restart neither loses a confirmed result (it is announced
//! by the next brain) nor applies one twice (a replayed result finds its file). A delivered
//! transition cites the episode of the beat that announced it. A payoff is delivered once, the
//! night after its state was entered, and a crisis (or any beat that cannot carry it) leaves it
//! pending. A newcomer gets the arc's recap on their first pick while the arc is in progress.
//!
//! A draft arc is never run: only an arc whose `status` is `approved`, on a prep the owner
//! approved, is loaded ([`ArcRun::load`]).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use shadowrunr_types::prep::{ArcDef, ArcStateDef, ArcStatus, ArcTransition};
use shadowrunr_types::{Op, Rank, TsMs, VoteTally};

use crate::memory::markdown::{read_doc, safe_segment, walk_md, write_doc, Doc};
use crate::util::{normalize, one_line};
use crate::BrainError;

/// The arcs' directory under the memory root.
pub const ARC_DIR: &str = "arcs";
/// At least this long between two arc beats, so the arc is a thread, not the show.
pub const ARC_SPACING_MS: TsMs = 45_000;
/// A state's vote is asked for at most this many times a stream when no poll comes of it.
pub const VOTE_ATTEMPTS: u32 = 3;
/// How long the arc's poll stays open.
pub const ARC_POLL_SECS: u32 = 60;

/// What one arc beat voices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArcCue {
    /// Announce a confirmed transition (its event id).
    Announce(String),
    /// Land the current state's consequence (its state id).
    Payoff(String),
    /// Put the current state's vote (its state id).
    Vote(String),
}

#[derive(Debug, Clone)]
struct Pending {
    event_id: String,
    transition: ArcTransition,
}

/// An approved arc as this stream runs it.
#[derive(Debug, Clone)]
pub struct ArcRun {
    def: ArcDef,
    dir: PathBuf,
    stream_id: String,
    state: String,
    /// The stream the current state was entered in (`None`: the start state).
    entered_in: Option<String>,
    /// States whose payoff was delivered.
    paid: HashSet<String>,
    /// Confirmed results not yet announced, oldest first.
    pending: Vec<Pending>,
    /// Polls this stream put up for the current state's vote.
    polls: HashSet<String>,
    vote_cues: u32,
    last_cue_ms: Option<TsMs>,
    /// Viewers given the recap this stream.
    recapped: HashSet<String>,
}

fn same(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

fn field(doc: &Doc, key: &str) -> String {
    doc.get(key).unwrap_or("").to_string()
}

impl ArcRun {
    /// The arc's state from the memory: the start state, then every delivered transition in
    /// order (one that does not follow from the state reached is skipped), then the confirmed
    /// ones still to announce. `Ok(None)` for a draft or an arc that does not validate.
    pub fn load(
        def: &ArcDef,
        memory_dir: &Path,
        stream_id: &str,
    ) -> Result<Option<Self>, BrainError> {
        if def.status != ArcStatus::Approved || def.validate().is_err() {
            return Ok(None);
        }
        let dir = memory_dir.join(ARC_DIR).join(safe_segment(&def.id));
        let mut run = ArcRun {
            def: def.clone(),
            dir,
            stream_id: stream_id.to_string(),
            state: def.start.clone(),
            entered_in: None,
            paid: HashSet::new(),
            pending: Vec::new(),
            polls: HashSet::new(),
            vote_cues: 0,
            last_cue_ms: None,
            recapped: HashSet::new(),
        };
        let mut delivered: Vec<(TsMs, Doc)> = Vec::new();
        let mut confirmed: Vec<(TsMs, Doc)> = Vec::new();
        for path in walk_md(&run.dir)? {
            let (doc, _) = read_doc(&path)?;
            match doc.get("kind") {
                Some("payoff") => {
                    run.paid.insert(field(&doc, "state"));
                }
                Some("transition") => {
                    let at = doc.num("confirmed_ms", &path)?;
                    match doc.get("status") {
                        Some("delivered") => delivered.push((at, doc)),
                        Some("confirmed") => confirmed.push((at, doc)),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        delivered.sort_by_key(|(at, doc)| (*at, field(doc, "event_id")));
        for (_, doc) in delivered {
            if let Some(t) = run.permitted(&field(&doc, "transition")) {
                if t.from == run.state {
                    run.state = t.to.clone();
                    run.entered_in = Some(field(&doc, "stream_id"));
                }
            }
        }
        confirmed.sort_by_key(|(at, doc)| (*at, field(doc, "event_id")));
        for (_, doc) in confirmed {
            if let Some(t) = run.permitted(&field(&doc, "transition")) {
                if t.from == run.state && run.pending.is_empty() {
                    run.pending.push(Pending {
                        event_id: field(&doc, "event_id"),
                        transition: t.clone(),
                    });
                }
            }
        }
        Ok(Some(run))
    }

    fn permitted(&self, id: &str) -> Option<&ArcTransition> {
        self.def.transitions.iter().find(|t| t.id == id)
    }

    pub fn def(&self) -> &ArcDef {
        &self.def
    }

    /// The current state's id.
    pub fn state(&self) -> &str {
        &self.state
    }

    fn state_def(&self) -> &ArcStateDef {
        self.def
            .states
            .iter()
            .find(|s| s.id == self.state)
            .expect("a validated arc names its states")
    }

    /// Whether the arc has moved on from where it starts: a newcomer needs the recap.
    pub fn in_progress(&self) -> bool {
        self.state != self.def.start || !self.pending.is_empty()
    }

    /// Whether the state was entered on an earlier night: its consequence and its vote are due.
    fn entered_before(&self) -> bool {
        self.entered_in.as_deref() != Some(self.stream_id.as_str())
    }

    fn payoff_due(&self) -> Option<&str> {
        let st = self.state_def();
        st.payoff
            .as_deref()
            .filter(|_| self.entered_in.is_some() && self.entered_before())
            .filter(|_| !self.paid.contains(&self.state))
    }

    /// The open loop the through-line keeps while a consequence or an announcement is owed:
    /// `(key, label)`. It stands across beats, and across nights, until delivered.
    pub fn open_loop(&self) -> Option<(String, String)> {
        let key = format!("arc:{}", self.def.id);
        if let Some(p) = self.pending.first() {
            return Some((
                key,
                format!(
                    "the result of the vote in {} ({})",
                    one_line(&self.def.title, 60),
                    one_line(&p.transition.option, 40)
                ),
            ));
        }
        self.payoff_due().map(|p| {
            (
                key,
                format!(
                    "the consequence of {}: {}",
                    one_line(&self.def.title, 60),
                    one_line(p, 80)
                ),
            )
        })
    }

    /// The arc's next beat, if one is due now: an announcement first, then the consequence,
    /// then the vote. `can_poll`: the segment allows `start_poll`.
    pub fn cue(&self, now_ms: TsMs, can_poll: bool) -> Option<ArcCue> {
        if self
            .last_cue_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < ARC_SPACING_MS)
        {
            return None;
        }
        if let Some(p) = self.pending.first() {
            return Some(ArcCue::Announce(p.event_id.clone()));
        }
        if self.payoff_due().is_some() {
            return Some(ArcCue::Payoff(self.state.clone()));
        }
        let st = self.state_def();
        let vote_due = st.vote.is_some()
            && (self.entered_in.is_none() || self.entered_before())
            && self.polls.is_empty()
            && self.vote_cues < VOTE_ATTEMPTS
            && can_poll;
        vote_due.then(|| ArcCue::Vote(self.state.clone()))
    }

    /// The `start_poll` op this state's vote is put with.
    fn vote_op(&self) -> Option<String> {
        let v = self.state_def().vote.as_ref()?;
        let op = Op::StartPoll {
            poll_id: format!("arc-{}-{}", self.state, self.vote_cues + 1),
            question: v.question.clone(),
            options: v.options.clone(),
            min_rank: Rank::Thrall,
            secs: ARC_POLL_SECS,
        };
        serde_json::to_string(&op).ok()
    }

    /// The material for a cue (owner-approved prep: the turn fences it as such).
    pub fn material(&self, cue: &ArcCue) -> String {
        let title = one_line(&self.def.title, 80);
        match cue {
            ArcCue::Announce(id) => {
                let t = &self
                    .pending
                    .iter()
                    .find(|p| &p.event_id == id)
                    .expect("a cue names a pending result")
                    .transition;
                format!(
                    "Arc \"{title}\": the room's vote chose \"{}\". Announce what it changes, \
                     in your words, and that it happened because they voted: {}\n",
                    one_line(&t.option, 60),
                    one_line(&t.announce, 300)
                )
            }
            ArcCue::Payoff(_) => {
                let st = self.state_def();
                format!(
                    "Arc \"{title}\", from an earlier night: {}. Its consequence lands \
                     tonight: {}. Give newcomers the one-line recap first: {}\n",
                    one_line(&st.summary, 200),
                    one_line(st.payoff.as_deref().unwrap_or(""), 300),
                    one_line(&self.def.recap, 240)
                )
            }
            ArcCue::Vote(_) => {
                let st = self.state_def();
                let q = st
                    .vote
                    .as_ref()
                    .map_or(String::new(), |v| one_line(&v.question, 160));
                format!(
                    "Arc \"{title}\": {}. Recap it in one line for anyone new ({}), then put \
                     it to the room: \"{q}\", with exactly this action:\nOP: {}\nThe vote \
                     decides it; you obey the result.\n",
                    one_line(&st.summary, 200),
                    one_line(&self.def.recap, 240),
                    self.vote_op().unwrap_or_default()
                )
            }
        }
    }

    /// The recap for newcomers on their first pick, while the arc is in progress: `None` when
    /// none of `newcomers` (viewer id, name) is owed it.
    pub fn recap_for(&self, newcomers: &[(String, String)]) -> Option<String> {
        if !self.in_progress() {
            return None;
        }
        let owed: Vec<&str> = newcomers
            .iter()
            .filter(|(id, _)| !self.recapped.contains(id))
            .map(|(_, name)| name.as_str())
            .collect();
        (!owed.is_empty()).then(|| {
            format!(
                "Arc \"{}\" so far, for newcomers ({}): {} Give them this in one line before \
                 they are asked to take part.\n",
                one_line(&self.def.title, 80),
                one_line(&owed.join(", "), 120),
                one_line(&self.def.recap, 240)
            )
        })
    }

    /// The recap aired for these viewers.
    pub fn recapped(&mut self, ids: impl IntoIterator<Item = String>) {
        self.recapped.extend(ids);
    }

    /// A `start_poll` the brain aired: it is the arc's when its options are the current
    /// state's vote options. Returns whether it was taken as the arc's poll.
    pub fn note_poll(&mut self, poll_id: &str, options: &[String]) -> bool {
        let Some(v) = &self.state_def().vote else {
            return false;
        };
        let matches = options.len() == v.options.len()
            && v.options.iter().all(|o| options.iter().any(|x| same(o, x)));
        if matches && self.pending.is_empty() {
            self.polls.insert(poll_id.to_string());
        }
        matches
    }

    fn transition_path(&self, event_id: &str) -> PathBuf {
        self.dir.join(format!("t-{}.md", safe_segment(event_id)))
    }

    /// The runtime confirmed a closed poll. It moves the arc only when it is a poll the brain
    /// put up for the current state's vote, it has a single winner, and a permitted transition
    /// names that winner; the result is written at once, `confirmed`, and announced by the next
    /// beat that can. Returns the event id when it was taken; a result already on file (a
    /// replay after a restart) is not taken again.
    pub fn confirm_vote(
        &mut self,
        poll_id: &str,
        tallies: &[VoteTally],
        now_ms: TsMs,
    ) -> Result<Option<String>, BrainError> {
        if !self.polls.contains(poll_id) || !self.pending.is_empty() {
            return Ok(None);
        }
        let Some(top) = tallies
            .iter()
            .filter(|t| t.voters > 0)
            .max_by(|a, b| a.weight.total_cmp(&b.weight))
        else {
            return Ok(None);
        };
        let tie = tallies
            .iter()
            .filter(|t| t.voters > 0 && t.weight == top.weight)
            .count()
            > 1;
        if tie {
            return Ok(None);
        }
        let Some(t) = self
            .def
            .transitions
            .iter()
            .find(|t| t.from == self.state && same(&t.option, &top.option))
            .cloned()
        else {
            return Ok(None);
        };
        let event_id = format!("{}:{poll_id}", self.stream_id);
        let path = self.transition_path(&event_id);
        self.polls.remove(poll_id);
        if path.is_file() {
            return Ok(None);
        }
        let mut doc = Doc::new();
        doc.set("kind", "transition")
            .set("arc", &self.def.id)
            .set("event_id", &event_id)
            .set("transition", &t.id)
            .set("from", &t.from)
            .set("to", &t.to)
            .set("option", &t.option)
            .set("poll_id", poll_id)
            .set("stream_id", &self.stream_id)
            .set("status", "confirmed")
            .set("confirmed_ms", now_ms);
        doc.body = t.announce.clone();
        write_doc(&path, &doc)?;
        self.pending.push(Pending {
            event_id: event_id.clone(),
            transition: t,
        });
        Ok(Some(event_id))
    }

    /// A beat carrying `cue` aired: an announcement moves the arc (written `delivered`, citing
    /// `episode`, the beat's own episode), a consequence is paid, a vote ask is counted.
    pub fn delivered(
        &mut self,
        cue: &ArcCue,
        episode: &str,
        now_ms: TsMs,
    ) -> Result<(), BrainError> {
        self.last_cue_ms = Some(now_ms);
        match cue {
            ArcCue::Announce(id) => {
                let Some(i) = self.pending.iter().position(|p| &p.event_id == id) else {
                    return Ok(());
                };
                let p = self.pending.remove(i);
                let path = self.transition_path(&p.event_id);
                let (mut doc, _) = read_doc(&path)?;
                doc.set("status", "delivered")
                    .set("delivered_ms", now_ms)
                    .set("episode", episode);
                write_doc(&path, &doc)?;
                self.state = p.transition.to.clone();
                self.entered_in = Some(self.stream_id.clone());
                self.polls.clear();
                self.vote_cues = 0;
            }
            ArcCue::Payoff(state) => {
                let mut doc = Doc::new();
                doc.set("kind", "payoff")
                    .set("arc", &self.def.id)
                    .set("state", state)
                    .set("stream_id", &self.stream_id)
                    .set("delivered_ms", now_ms)
                    .set("episode", episode);
                write_doc(
                    &self.dir.join(format!("p-{}.md", safe_segment(state))),
                    &doc,
                )?;
                self.paid.insert(state.clone());
            }
            ArcCue::Vote(_) => self.vote_cues += 1,
        }
        Ok(())
    }
}
