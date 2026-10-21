//! The orchestrator: one decision per beat, between the picker and the composer, on what the
//! beat is for ([`BeatMode`]) and so which chat the picker may pass and which prepared material
//! the composer voices. Every beat that reaches it writes exactly one `beat_plan` event, before
//! the beat's lines, with a reason a reviewer can read.
//!
//! - **Cold open** (`ctx.state == "cold_open"`): the prep's story, one or two lines a beat, in
//!   order; then its handoff. Only first contacts and safety reach the voice (at most one first
//!   contact a beat). Without a prep: one short "the room is gathering" open with no premise.
//!   Once the story is told he greets first contacts, holds every other quiet beat, and voices
//!   the segment's prepared deck (premises first) under the same filter; one gathering line
//!   only when the deck is empty. Without a prep: gathering lines [`GATHERING_SPACING_MS`]
//!   apart; between them a hold, or a short aside (no welcome) where a hold would let the
//!   silence pass [`HOLD_GAP_MS`].
//! - **After it**, per beat: `react` when chat is worth answering; `bit` when the room is quiet
//!   (a prepared bit for the current segment, shuffled, never repeated in the stream); `bridge`
//!   for a connector line between the two (into a bit, back to chat, into a poll or a handover);
//!   `mixed` (a short bit line and one pick) after a run of reacts in a thin room; `hold` when
//!   there is nothing worth saying and the last line is recent. Without a prep there is no bit,
//!   bridge, mixed or hold: a quiet beat is improvised as before.
//!
//! Rules: a bit is offered once and marked used; the room answering a bit skips its next line;
//! a raid, a safety pick or a segment change expires the queued bit and connector; a safety beat
//! never voices prepared material and offers only the safety pick. Votes and lone emotes never
//! fill a pick, and a wave once answered is not picked again this stream.
//!
//! A message that needs an answer (a question, a mention, a first contact, a gift or a sub, a
//! proposal or a summons, a mockable injection) and that a beat could not answer (a hold, a
//! safety beat, the cold-open filter, the pick ceiling) is carried into the next beats for
//! [`CARRY_MS`], at most [`CARRY_MAX`] at once, competing with fresh chat with a small age bonus;
//! once answered it is never offered again. Waves and votes are never carried.
//!
//! **Carry through the cold open** ("carry until the room opens + `CARRY_MS`"). The cold-open
//! filter passes only first contacts, so what it (or a hold in it) defers is held, not timed: a
//! message carried during the cold open never expires there, and its [`CARRY_MS`] starts from
//! the drain of the first beat after the room opens. While the room fills, the [`CARRY_MAX`] cap
//! keeps first contacts first, the oldest first (the greeting owed longest), then the newest of
//! the rest. The first beats after the room opens answer them alongside fresh chat.
//!
//! A vote that closed and a handover that fell due are room events ([`room`]): the next beat
//! that is not a safety beat announces them, once, and expires the queued bit.
//!
//! **Speech pacing** ([`crate::pace`]): while more than `speech_lead_ms` of his speech is still
//! queued, a beat is a `hold` with reason `speaking: N.N s queued`, before anything else is
//! decided. A safety beat and a goodbye beat (`CLOSING NOW`) are never held; the wrap-up window
//! before the goodbye is paced like any other beat. A room event waits for the drain
//! and is then announced first, ahead of a queued connector or bit. Carried chat waits in the
//! carry queue, and its [`CARRY_MS`] counts from the drain ("carry from the drain"): a message
//! expires [`CARRY_MS`] after the speech queued when it arrived has aired, so pacing never
//! silently expires a question. A speech hold counts as a hold: a quiet beat right after it
//! speaks rather than holding again (measured: a speech hold then a quiet hold, with 7 s core
//! calls, left 12 s of nobody speaking).
//!
//! **Lookahead.** The hold threshold is the brain's lead (`speech_lead_ms` in [`BeatInputs`]):
//! on a live host it grows to the measured core latency plus the render lag ([`crate::pace`]),
//! so the next beat is composed while the current line still plays.
//!
//! **Owned requests.** A message that makes a situational ask a rule covers
//! ([`requests::situational_ask`]: account intrusion, "roast me", an impression, a coin, an
//! address, explicit content) is worth answering ("request") and owned until answered: it is
//! carried past [`CARRY_MS`] (dropped only after [`REQUEST_MAX_MS`]), never capped, offered in
//! the cold open too, and the picker takes one a beat ahead of bait and fresh chat, so it is not
//! displaced by its viewer's later messages. While one waits, a beat holds for speech only while
//! more than [`REQUEST_LEAD_MS`] is queued, so it is answered within [`REQUEST_ANSWER_MS`].
//!
//! **The through-line** ([`agenda`]): the open loops (research out, a vote open, an owed
//! request, a bit's payoff) give voiced beats outside the cold open a pick-up after a pause and a
//! lead-in before a wait, and a quiet beat with nothing prepared bridges from them instead of
//! generic filler. Their tags join the `beat_plan` reason (`pick-up after N s`, `lead-in: …`).
//!
//! The thresholds are assumptions to tune against sandbox runs, not measurements:
//! [`QUIET_BIT_MS`], [`BIT_SPACING_MS`], [`HOLD_GAP_MS`], [`THIN_REACT_STREAK`],
//! [`THIN_ROOM_MAX`], [`RAID_FIRST_CONTACTS`], [`CARRY_MS`], [`CARRY_MAX`],
//! [`GATHERING_SPACING_MS`], [`REQUEST_ANSWER_MS`], [`REQUEST_LEAD_MS`], [`REQUEST_MAX_MS`].

pub mod agenda;
pub mod requests;
pub mod room;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use shadowrunr_types::prep::{BeatMode, BitKind, ConnectorKind, PrepBit, StreamPrep};
use shadowrunr_types::{ChatMsg, Command, SegmentContext, StreamEvent, TsMs};

use crate::moderation::{self, Candidate, IN_CHARACTER_LINES};
use crate::picker::{is_bare_emote, is_question, is_vote, mentions, Pick};
use crate::util::{fnv1a64, normalize, one_line, similarity};

use agenda::{Agenda, BeatShape, LoopKind};
use requests::{situational_ask, Ask, Rotation, REBUTTALS};
use room::RoomWatch;

/// The segment state in which the room is still filling.
pub const COLD_OPEN_STATE: &str = "cold_open";
/// No chat worth answering for this long before a new bit starts (assumption).
pub const QUIET_BIT_MS: i64 = 8_000;
/// At least this long between the starts of two new bits, so a premise has time to be
/// answered and the deck lasts the segment (assumption).
pub const BIT_SPACING_MS: i64 = 20_000;
/// A quiet beat may hold only while his last line is younger than this: with 5 s beats the
/// next one speaks well under the 15 s say-gap ceiling (assumption).
pub const HOLD_GAP_MS: i64 = 7_000;
/// Reacts in a row after which a thin room gets a bit line alongside its one pick.
pub const THIN_REACT_STREAK: u32 = 3;
/// A room is thin when at most this many messages are worth answering.
pub const THIN_ROOM_MAX: usize = 1;
/// First contacts in one beat that count as a raid (the room changed).
pub const RAID_FIRST_CONTACTS: usize = 5;
/// A message this close to an acknowledged wave is that wave again.
const WAVE_SIMILARITY: f64 = 0.8;
/// Two pastes this close are one wave; never pick several pasters.
const PASTER_SIMILARITY: f64 = 0.6;
/// Normalised length from which two lone near-duplicates count as pastes.
const PASTER_MIN_CHARS: usize = 40;
/// Two story lines are voiced together when they fit in this many characters.
const TWO_LINES_MAX_CHARS: usize = 220;
/// Words from which a plain remark is worth answering.
const REMARK_MIN_WORDS: usize = 4;
/// How long a message a beat could not answer is carried into the next beats (assumption:
/// two to five beats, so the answer still reads as one to the asker).
pub const CARRY_MS: i64 = 25_000;
/// The most messages carried at once; the oldest go first.
pub const CARRY_MAX: usize = 6;
/// The `input` moderation rule that records a carried message dropped unanswered: action
/// `expired` (it waited [`CARRY_MS`]) or [`CAPPED`] (the [`CARRY_MAX`] cap dropped it).
pub const CARRY_RULE: &str = "carry";
/// The [`CARRY_RULE`] action for a message the carry cap dropped.
pub const CAPPED: &str = "capped";
/// The [`CARRY_RULE`] action for a pick whose answer never aired: carried again.
pub const UNAIRED: &str = "unaired";
/// Without a prep, at least this long between two gathering lines of the cold open
/// (assumption); he holds in between.
pub const GATHERING_SPACING_MS: i64 = 20_000;
/// An owned request is answered within this long of reaching a beat (assumption: the target the
/// two rules below keep; four beats).
pub const REQUEST_ANSWER_MS: i64 = 20_000;
/// While an owned request waits, a beat holds for speech only while more than this is queued
/// (assumption: its line still starts well inside the voice's `max_lag_ms` of 15 s).
pub const REQUEST_LEAD_MS: i64 = 8_000;
/// An owned request still unanswered this long after it reached a beat is dropped
/// ([`CARRY_RULE`] / `expired`): an answer that late no longer reads as one (assumption).
pub const REQUEST_MAX_MS: i64 = 90_000;

/// The beat note for an open with no prepared story, and for gathering lines after one.
pub const GATHERING_NOTE: &str = "(The room is still gathering. One short in-character line \
that welcomes whoever is arriving: no premise, no poll, no call to action, no count.)";

/// The beat note between gathering lines of a cold open with no prep: talk, but no welcome.
pub const ASIDE_NOTE: &str = "(The room is still gathering. One short in-character aside: a \
boast, a grievance or an observation about your lair. No welcome, no premise, no poll, no call \
to action, no count.)";

/// The beat note when a room event is announced and no chat is picked.
pub const ANNOUNCE_NOTE: &str = "(No chat to answer this beat: make the announcement above.)";

/// What one beat needs from the brain.
pub struct BeatInputs<'a> {
    pub now_ms: TsMs,
    pub ctx: &'a SegmentContext,
    /// The beat's candidates after [`Orchestrator::screen`].
    pub cands: &'a [Candidate],
    pub prep: Option<&'a StreamPrep>,
    /// When the speech already on air ends (or ended), on the speech clock; `None` before his
    /// first line.
    pub speech_until_ms: Option<TsMs>,
    /// A beat holds while more than this much speech is still queued (the lead: on a live host,
    /// the expected core latency plus the render lag, so the next beat overlaps the speech).
    pub speech_lead_ms: i64,
    /// How long after this beat starts its first line is expected to air (core call plus voice
    /// render; 0 without measurements): what the pick-up rule adds to the silence so far.
    pub expected_latency_ms: i64,
    /// The goodbye beat (a `CLOSING NOW` line in the state): never held for speech.
    pub closing: bool,
    /// Lowercase names the character answers to.
    pub names: &'a [String],
    pub max_picks: usize,
}

/// Which candidates the picker may see this beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Offer {
    /// Only safety picks (a bit or a hold).
    SafetyOnly,
    /// First contacts and safety (the cold open).
    FirstContact,
    /// Everything but filler (votes, lone emotes).
    Chat,
}

/// The orchestrator's decision for one beat.
#[derive(Debug, Clone)]
pub struct Plan {
    pub mode: BeatMode,
    /// The most picks the picker may pass (safety picks pass regardless).
    pub ceiling: usize,
    offer: Offer,
    /// The prepared bit or cold open voiced, if any.
    pub bit_id: Option<String>,
    /// The prepared material for the per-turn block.
    pub material: Option<String>,
    /// A line for a beat with no chat (the gathering open).
    pub note: Option<String>,
    /// The room events this beat announces (a closed vote, a handover due), chat or no chat.
    pub room_note: Option<String>,
    /// A hold because his speech is still queued (not a decision about the room).
    pub speaking: bool,
    /// The through-line's directives for this beat (a pick-up, a lead-in), if any.
    pub agenda: Option<String>,
    pub reason: String,
}

impl Plan {
    fn new(mode: BeatMode, offer: Offer, ceiling: usize, reason: String) -> Self {
        Self {
            mode,
            ceiling,
            offer,
            bit_id: None,
            material: None,
            note: None,
            room_note: None,
            speaking: false,
            agenda: None,
            reason,
        }
    }
}

/// A message carried past a beat that could not answer it.
#[derive(Debug, Clone)]
struct Carried {
    cand: Candidate,
    /// When it first reached a beat.
    since: TsMs,
    /// It was its viewer's first message.
    first: bool,
    /// When it stops being carried: [`CARRY_MS`] after the speech queued at its arrival aired.
    expires: TsMs,
    /// Carried through the cold open: it never expires there, and `expires` is set when the
    /// room opens.
    held: bool,
    /// An owned request: carried until answered (or [`REQUEST_MAX_MS`]).
    owned: Option<Ask>,
}

/// A multi-line bit being told across beats.
#[derive(Debug, Clone)]
struct ActiveBit {
    id: String,
    /// Index of the next line to voice.
    next: usize,
}

#[derive(Debug, Default)]
pub struct Orchestrator {
    seed: u64,
    beats: u64,
    /// Viewers seen in any inbox so far (first contact is a viewer's first message ever).
    seen_viewers: HashSet<String>,
    /// Message ids that are first contacts this beat.
    first: HashSet<String>,
    /// Normalised texts of waves already answered this stream.
    acked_waves: Vec<String>,
    used_bits: HashSet<String>,
    active_bit: Option<ActiveBit>,
    /// A bit finished and no chat beat followed yet (for the back-to-chat bridge).
    bit_ended: bool,
    last_bit_ms: Option<TsMs>,
    cold_next: usize,
    handoff_done: bool,
    gathering_done: bool,
    /// The one gathering line allowed after the handoff (deck empty) was spoken.
    deck_gathering_done: bool,
    /// When the last gathering line was spoken (no prep).
    last_gathering_ms: Option<TsMs>,
    /// Messages carried into the next beat.
    carried: Vec<Carried>,
    /// Messages carried into this beat, by id.
    carried_in: HashMap<String, Carried>,
    /// This beat's messages that are carried if no pick answers them.
    carry_next: Vec<Carried>,
    /// Ids the carry cap dropped at the last commit, for their records.
    capped: Vec<String>,
    /// Ids of messages picked this stream: never offered again.
    answered: HashSet<String>,
    room: RoomWatch,
    /// A connector queued by a state change.
    pending: Option<ConnectorKind>,
    connector_uses: HashMap<&'static str, usize>,
    last_mode: Option<BeatMode>,
    /// The last mode that was not a hold (what the room last heard).
    last_voiced: Option<BeatMode>,
    react_streak: u32,
    last_segment: Option<shadowrunr_types::SegmentId>,
    last_state: Option<String>,
    /// Since when no chat was worth answering.
    quiet_since: Option<TsMs>,
    in_character: Rotation,
    rebuttals: Rotation,
    /// The through-line's open loops ([`agenda`]).
    agenda: Agenda,
    /// (viewer, ask) answered this stream: that viewer's same ask is no longer owned.
    asked: HashSet<(String, Ask)>,
}

/// The situational ask an owned request makes: a single message (never a wave or a safety
/// pick) that a situation rule covers.
fn ask_of(c: &Candidate) -> Option<Ask> {
    if c.count > 1 || c.safety.is_some() || is_vote(c) {
        return None;
    }
    if matches!(c.msg.command, Some(Command::Propose { .. })) {
        return Some(Ask::Proposal);
    }
    situational_ask(&c.msg.text)
}

/// Why a candidate is worth answering, if it is.
fn worth(c: &Candidate, first: bool, names: &[String]) -> Option<&'static str> {
    if c.safety.is_some() {
        return Some("safety");
    }
    if is_filler(c) {
        return None;
    }
    if ask_of(c).is_some() {
        Some("request")
    } else if moderation::is_gift_or_sub(&c.msg.text) {
        Some("gift")
    } else if first {
        Some("first contact")
    } else if c.count > 1 {
        Some("wave")
    } else if mentions(&c.msg.text, names) {
        Some("mention")
    } else if is_question(&c.msg.text) {
        Some("question")
    } else if c.msg.command.is_some() {
        Some("command")
    } else if c.injection {
        Some("mockable")
    } else if normalize(&c.msg.text).split(' ').count() >= REMARK_MIN_WORDS {
        Some("remark")
    } else {
        None
    }
}

/// Never fills a pick: a vote (the segment counts it) or a lone emote.
fn is_filler(c: &Candidate) -> bool {
    c.safety.is_none() && (is_vote(c) || (c.count == 1 && is_bare_emote(&c.msg.text)))
}

/// Whether an unanswered message is carried: one that asks for an answer, never a wave, a
/// vote, a remark or a safety pick (which is never left unanswered).
fn carries(c: &Candidate, first: bool, names: &[String]) -> bool {
    if c.count > 1 || c.safety.is_some() {
        return false;
    }
    match worth(c, first, names) {
        Some("request" | "gift" | "first contact" | "mention" | "question" | "mockable") => true,
        Some("command") => matches!(
            c.msg.command,
            Some(Command::Propose { .. } | Command::Summon { .. })
        ),
        _ => false,
    }
}

fn kind_label(k: BitKind) -> &'static str {
    match k {
        BitKind::Premise => "premise",
        BitKind::Story => "story",
        BitKind::Callback => "callback",
    }
}

fn connector_label(k: ConnectorKind) -> &'static str {
    match k {
        ConnectorKind::IntoRoom => "into_room",
        ConnectorKind::IntoBit => "into_bit",
        ConnectorKind::BackToChat => "back_to_chat",
        ConnectorKind::IntoPoll => "into_poll",
        ConnectorKind::IntoHandover => "into_handover",
    }
}

/// How many of `lines[from..]` to voice this beat: two when they are short, else one.
fn take_lines(lines: &[String], from: usize) -> usize {
    let rest = &lines[from.min(lines.len())..];
    match rest {
        [] => 0,
        [_] => 1,
        [a, b, ..] if a.chars().count() + b.chars().count() <= TWO_LINES_MAX_CHARS => 2,
        _ => 1,
    }
}

fn span(from: usize, n: usize, total: usize) -> String {
    if n <= 1 {
        format!("line {}/{total}", from + 1)
    } else {
        format!("lines {}-{}/{total}", from + 1, from + n)
    }
}

fn push_lines(s: &mut String, lines: &[String]) {
    for l in lines {
        s.push_str("- ");
        s.push_str(&one_line(l, 400));
        s.push('\n');
    }
}

impl Orchestrator {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            ..Self::default()
        }
    }

    /// Note this beat's raw inbox, before the pre-filter: which messages are first contacts.
    pub fn observe_inbox(&mut self, inbox: &[ChatMsg]) {
        self.first.clear();
        for m in inbox {
            if self.seen_viewers.insert(m.viewer.id.clone()) {
                self.first.insert(m.id.clone());
            }
        }
    }

    /// Whether a message was its viewer's first this stream (in this beat's inbox, or carried
    /// from an earlier one).
    pub fn is_first_contact(&self, msg_id: &str) -> bool {
        self.first.contains(msg_id) || self.carried_in.get(msg_id).is_some_and(|c| c.first)
    }

    /// How long each message carried into this beat has waited, by id (the picker's age
    /// bonus).
    pub fn carried_ages(&self, now_ms: TsMs) -> HashMap<String, i64> {
        self.carried_in
            .iter()
            .map(|(id, k)| (id.clone(), now_ms.saturating_sub(k.since)))
            .collect()
    }

    /// Stop carrying a viewer's messages (they were removed from chat).
    pub fn forget_viewer(&mut self, viewer_id: &str) {
        self.carried.retain(|k| k.cand.msg.viewer.id != viewer_id);
    }

    /// The ask a candidate makes that is owned: one per viewer and kind, so a viewer asking the
    /// same thing again after the answer is plain chat.
    fn owns(&self, c: &Candidate) -> Option<Ask> {
        ask_of(c).filter(|a| !self.asked.contains(&(c.msg.viewer.id.clone(), *a)))
    }

    /// A pick whose answer never aired (the gate dropped the only line naming it): it is not
    /// answered, so it is carried again from now (an `input` record, [`CARRY_RULE`] /
    /// [`UNAIRED`]).
    pub fn unaired(&mut self, c: &Candidate, now_ms: TsMs, free_at: TsMs) -> StreamEvent {
        self.answered.remove(&c.msg.id);
        if let Some(a) = ask_of(c) {
            self.asked.remove(&(c.msg.viewer.id.clone(), a));
        }
        self.carried.retain(|k| k.cand.msg.id != c.msg.id);
        self.carried.push(Carried {
            cand: c.clone(),
            since: now_ms,
            first: false,
            expires: free_at.max(now_ms).saturating_add(CARRY_MS),
            held: false,
            owned: self.owns(c),
        });
        moderation::input_event(now_ms, CARRY_RULE, &c.msg.id, UNAIRED)
    }

    /// The owned requests among this beat's candidates, oldest first (by when each first
    /// reached a beat): the picker takes one a beat ahead of bait and fresh chat.
    pub fn owed(&self, cands: &[Candidate], now_ms: TsMs) -> Vec<String> {
        let mut v: Vec<(TsMs, &str)> = cands
            .iter()
            .filter(|c| !self.answered.contains(&c.msg.id) && self.owns(c).is_some())
            .map(|c| {
                let since = self.carried_in.get(&c.msg.id).map_or(now_ms, |k| k.since);
                (since, c.msg.id.as_str())
            })
            .collect();
        v.sort();
        v.into_iter().map(|(_, id)| id.to_string()).collect()
    }

    /// A background job changed state: research out opens a loop, a finished one lands.
    pub fn note_job(&mut self, job_id: &str, brief: &str, status: &str) {
        match status {
            "queued" | "running" | "progress" => self.agenda.open(
                LoopKind::Research,
                job_id,
                &format!("research {job_id} ({})", one_line(brief, 60)),
            ),
            _ => self.agenda.close(LoopKind::Research, job_id),
        }
    }

    /// His line went on air: the thread a pick-up ties back to.
    pub fn spoke(&mut self, line: &str) {
        self.agenda.spoke(line);
    }

    /// The open loops, for diagnostics ("research r1 (…), the vote \"…\"").
    pub fn agenda_summary(&self) -> String {
        self.agenda.summary()
    }

    /// Drop repeats of a wave already answered (an `input` record, `wave_repeat` / `dropped`)
    /// and fold near-duplicate pastes into one candidate, so several pasters are never picked.
    /// Then add the messages carried from earlier beats, dropping those that waited
    /// [`CARRY_MS`] from the drain (an `input` record, [`CARRY_RULE`] / `expired`); one held
    /// through the cold open never expires before the room opens.
    pub fn screen(
        &mut self,
        now_ms: TsMs,
        cands: Vec<Candidate>,
        ev: &mut Vec<StreamEvent>,
    ) -> Vec<Candidate> {
        let mut out: Vec<(String, Candidate)> = Vec::with_capacity(cands.len());
        for c in cands {
            let norm = normalize(&c.msg.text);
            if c.safety.is_none()
                && !norm.is_empty()
                && self
                    .acked_waves
                    .iter()
                    .any(|w| *w == norm || similarity(w, &norm) >= WAVE_SIMILARITY)
            {
                ev.push(moderation::input_event(
                    now_ms,
                    "wave_repeat",
                    &c.msg.id,
                    "dropped",
                ));
                continue;
            }
            let paste = |a: &Candidate, an: &str, b: &Candidate, bn: &str| {
                let long = an.chars().count() >= PASTER_MIN_CHARS
                    && bn.chars().count() >= PASTER_MIN_CHARS;
                (a.count > 1 || b.count > 1 || long)
                    && (an == bn || similarity(an, bn) >= PASTER_SIMILARITY)
            };
            let hit = out
                .iter_mut()
                .find(|(n, o)| o.safety.is_none() && c.safety.is_none() && paste(o, n, &c, &norm));
            match hit {
                Some((_, o)) => {
                    o.count += c.count;
                    o.viewers += c.viewers;
                    o.injection |= c.injection;
                }
                None => out.push((norm, c)),
            }
        }
        let mut out: Vec<Candidate> = out.into_iter().map(|(_, c)| c).collect();
        self.carried_in.clear();
        for k in std::mem::take(&mut self.carried) {
            let id = k.cand.msg.id.clone();
            if self.answered.contains(&id) || out.iter().any(|c| c.msg.id == id) {
                continue;
            }
            // An owned request is carried until answered, up to REQUEST_MAX_MS.
            let expired = match k.owned {
                Some(_) => now_ms.saturating_sub(k.since) >= REQUEST_MAX_MS,
                None => !k.held && now_ms >= k.expires,
            };
            if expired {
                ev.push(moderation::input_event(now_ms, CARRY_RULE, &id, "expired"));
                continue;
            }
            out.push(k.cand.clone());
            self.carried_in.insert(id, k);
        }
        out
    }

    /// The candidates this plan lets the picker see.
    pub fn offered(&self, plan: &Plan, cands: &[Candidate]) -> Vec<Candidate> {
        cands
            .iter()
            .filter(|c| {
                c.safety.is_some()
                    || match plan.offer {
                        Offer::SafetyOnly => false,
                        // An owned request is answered in the cold open too.
                        Offer::FirstContact => {
                            (self.is_first_contact(&c.msg.id) && !is_vote(c))
                                || self.owns(c).is_some()
                        }
                        Offer::Chat => !is_filler(c),
                    }
            })
            .cloned()
            .collect()
    }

    /// The queued intent a changed room expires: the bit being told and a queued connector.
    fn expire(&mut self) -> Option<String> {
        let mut gone = Vec::new();
        if let Some(b) = self.active_bit.take() {
            gone.push(format!("bit {}", b.id));
        }
        if let Some(k) = self.pending.take() {
            gone.push(format!("connector {}", connector_label(k)));
        }
        (!gone.is_empty()).then(|| gone.join(" and "))
    }

    /// The next connector of a kind, rotating through the prep's.
    fn connector(&mut self, prep: Option<&StreamPrep>, kind: ConnectorKind) -> Option<String> {
        let of_kind: Vec<&str> = prep?
            .connectors
            .iter()
            .filter(|c| c.kind == kind)
            .map(|c| c.line.as_str())
            .collect();
        if of_kind.is_empty() {
            return None;
        }
        let n = self
            .connector_uses
            .entry(connector_label(kind))
            .or_insert(0);
        let line = of_kind[*n % of_kind.len()].to_string();
        *n += 1;
        Some(line)
    }

    /// The next unused bit for the segment, in this stream's shuffled order (a premise first
    /// when asked), marked used: a bit is offered once.
    fn next_bit<'p>(
        &mut self,
        prep: &'p StreamPrep,
        seg: shadowrunr_types::SegmentId,
        prefer_premise: bool,
    ) -> Option<&'p PrepBit> {
        let mut deck: Vec<&PrepBit> = prep
            .bits
            .iter()
            .filter(|b| b.segment == seg && !b.lines.is_empty() && !self.used_bits.contains(&b.id))
            .collect();
        deck.sort_by_key(|b| fnv1a64(format!("{}:{}", self.seed, b.id).as_bytes()));
        let bit = if prefer_premise {
            deck.iter()
                .find(|b| b.kind == BitKind::Premise)
                .or(deck.first())
                .copied()
        } else {
            deck.first().copied()
        }?;
        self.used_bits.insert(bit.id.clone());
        Some(bit)
    }

    /// Voice a bit's next lines (at most `max`), tracking it while lines remain. Returns the
    /// material and the reason fragment ("premise p-03", "story s-01 lines 3-4/5").
    fn bit_lines(&mut self, bit: &PrepBit, from: usize, max: usize) -> (String, String) {
        let n = take_lines(&bit.lines, from).min(max).max(1);
        let to = (from + n).min(bit.lines.len());
        let total = bit.lines.len();
        let mut m = format!(
            "{} {}, {}:\n",
            kind_label(bit.kind),
            bit.id,
            span(from, n, total)
        );
        push_lines(&mut m, &bit.lines[from..to]);
        if to < total {
            m.push_str("More of it comes later: do not reach its payoff yet.\n");
            self.active_bit = Some(ActiveBit {
                id: bit.id.clone(),
                next: to,
            });
        } else {
            if let Some(p) = &bit.payoff {
                m.push_str(&format!("Payoff (land it now): {}\n", one_line(p, 300)));
            }
            self.active_bit = None;
            self.bit_ended = true;
        }
        let what = if total == 1 {
            format!("{} {}", kind_label(bit.kind), bit.id)
        } else {
            format!(
                "{} {} {}",
                kind_label(bit.kind),
                bit.id,
                span(from, n, total)
            )
        };
        (m, what)
    }

    /// The room answered the bit being told: its next line is skipped. Returns the reason
    /// fragment.
    fn back_off(&mut self, prep: Option<&StreamPrep>) -> Option<String> {
        let active = self.active_bit.clone()?;
        let Some(bit) = prep.and_then(|p| p.bits.iter().find(|x| x.id == active.id)) else {
            self.active_bit = None;
            return None;
        };
        let next = active.next + 1;
        if next >= bit.lines.len() {
            self.active_bit = None;
            self.bit_ended = true;
        } else {
            self.active_bit = Some(ActiveBit {
                id: active.id,
                next,
            });
        }
        Some(format!(
            "room answered {} {}: line {} skipped; ",
            kind_label(bit.kind),
            bit.id,
            active.next + 1
        ))
    }

    /// Decide this beat. State that a decision consumes (a bit offered, a story line voiced, a
    /// connector used) is consumed here: the beat goes ahead whatever the engine does. Then the
    /// through-line: the open loops are brought up to date, and a voiced beat outside the cold
    /// open, safety, a room event and the goodbye gets its pick-up and lead-in ([`agenda`]).
    pub fn plan(&mut self, b: &BeatInputs<'_>) -> Plan {
        let mut p = self.decide(b);
        let requests: Vec<(String, String)> = b
            .cands
            .iter()
            .filter(|c| !self.answered.contains(&c.msg.id))
            .filter_map(|c| {
                let ask = self.owns(c)?;
                Some((
                    c.msg.id.clone(),
                    format!("{}'s {}", one_line(&c.msg.viewer.name, 40), ask.label()),
                ))
            })
            .collect();
        self.agenda.sync(LoopKind::Request, &requests);
        let polls: Vec<(String, String)> = self
            .room
            .open_polls()
            .iter()
            .map(|(id, q)| (id.clone(), format!("the vote \"{q}\"")))
            .collect();
        self.agenda.sync(LoopKind::Vote, &polls);
        let payoff: Vec<(String, String)> = self
            .active_bit
            .iter()
            .map(|a| (a.id.clone(), format!("the payoff of {}", a.id)))
            .collect();
        self.agenda.sync(LoopKind::Payoff, &payoff);
        if !requests.is_empty() && p.mode != BeatMode::Hold {
            let ids: Vec<&str> = requests.iter().map(|(id, _)| id.as_str()).collect();
            p.reason.push_str(&format!("; owed: {}", ids.join(", ")));
        }
        let safety = b.cands.iter().any(|c| c.safety.is_some());
        if p.mode == BeatMode::Hold
            || safety
            || b.closing
            || b.ctx.state == COLD_OPEN_STATE
            || p.room_note.is_some()
        {
            return p;
        }
        let expected_gap_ms = b.speech_until_ms.map_or(0, |u| {
            b.now_ms
                .saturating_add(b.expected_latency_ms.max(0))
                .saturating_sub(u)
                .max(0)
        });
        let d = self.agenda.directives(BeatShape {
            now_ms: b.now_ms,
            expected_gap_ms,
            quiet: p.ceiling == 0,
            bit_continues: self.active_bit.is_some(),
        });
        if !d.tags.is_empty() {
            p.reason.push_str(&format!("; {}", d.tags.join("; ")));
        }
        p.agenda = d.note;
        p
    }

    fn decide(&mut self, b: &BeatInputs<'_>) -> Plan {
        self.beats += 1;
        let now = b.now_ms;
        let seg_changed = self.last_segment.is_some_and(|s| s != b.ctx.segment);
        self.last_segment = Some(b.ctx.segment);
        let prev_state = self.last_state.replace(b.ctx.state.clone());
        let state_changed = prev_state.as_deref().is_some_and(|s| s != b.ctx.state);
        let safety = b.cands.iter().any(|c| c.safety.is_some());
        let raid = self.first.len() >= RAID_FIRST_CONTACTS;
        let owed = b
            .cands
            .iter()
            .any(|c| !self.answered.contains(&c.msg.id) && self.owns(c).is_some());

        let mut lead = String::new();
        let why_changed = if seg_changed {
            Some("segment change")
        } else if raid {
            Some("raid")
        } else if safety {
            Some("safety")
        } else {
            None
        };
        if let Some(why) = why_changed {
            if let Some(gone) = self.expire() {
                lead = format!("room changed ({why}): {gone} expired; ");
            }
        }

        self.room.observe(&b.ctx.state, &b.ctx.state_summary);
        let free_at = b.speech_until_ms.map_or(now, |u| u.max(now));
        // In the cold open what is carried is held; once the room opens, its clock starts from
        // this beat's drain.
        let cold = b.ctx.state == COLD_OPEN_STATE;
        let from_drain = free_at.saturating_add(CARRY_MS);
        self.carry_next = b
            .cands
            .iter()
            .filter(|c| !self.answered.contains(&c.msg.id))
            .filter_map(|c| {
                let first = self.is_first_contact(&c.msg.id);
                let (since, expires) = match self.carried_in.get(&c.msg.id) {
                    Some(k) if k.held && !cold => (k.since, from_drain),
                    Some(k) => (k.since, k.expires),
                    None => (now, from_drain),
                };
                carries(c, first, b.names).then(|| Carried {
                    cand: c.clone(),
                    since,
                    first,
                    expires,
                    held: cold,
                    owned: self.owns(c),
                })
            })
            .collect();
        let worthy: Vec<&'static str> = b
            .cands
            .iter()
            .filter_map(|c| worth(c, self.is_first_contact(&c.msg.id), b.names))
            .collect();
        if !worthy.is_empty() || self.quiet_since.is_none() {
            self.quiet_since = Some(now);
        }
        let quiet_ms = now.saturating_sub(self.quiet_since.unwrap_or(now));
        let gap_ms = b.speech_until_ms.map(|t| now.saturating_sub(t).max(0));
        // The goodbye beat always speaks: a quiet hold there would cost the sign-off.
        let may_hold = gap_ms.is_some_and(|g| g < HOLD_GAP_MS)
            && self.last_mode != Some(BeatMode::Hold)
            && !b.closing;

        // Speech still queued: hold before anything is decided or consumed. A safety beat and
        // a closing beat run at once; an owned request waits only for REQUEST_LEAD_MS.
        let queued = free_at.saturating_sub(now);
        let hold_lead = if owed {
            b.speech_lead_ms.max(REQUEST_LEAD_MS)
        } else {
            b.speech_lead_ms
        };
        if queued > hold_lead && !safety && !b.closing {
            let mut p = Plan::new(
                BeatMode::Hold,
                Offer::SafetyOnly,
                0,
                format!("{lead}speaking: {:.1} s queued", queued as f64 / 1000.0),
            );
            p.speaking = true;
            return p;
        }

        if b.ctx.state == COLD_OPEN_STATE {
            let mut p = self.plan_cold_open(b, safety, may_hold, owed);
            p.reason = format!("{lead}{}", p.reason);
            return p;
        }

        // A state change queues its connector: the story cut short by a full room, a poll. A
        // handover falling due is a room event (below).
        if state_changed && !safety {
            let st = b.ctx.state.as_str();
            if prev_state.as_deref() == Some(COLD_OPEN_STATE) && !self.handoff_done {
                self.pending = Some(ConnectorKind::IntoRoom);
            } else if st.contains("poll") || st.contains("vote") {
                self.pending = Some(ConnectorKind::IntoPoll);
            }
        }

        let max = b.max_picks;
        let chat = if worthy.is_empty() { 0 } else { max };
        let signals = {
            let mut s: Vec<&str> = Vec::new();
            for w in &worthy {
                if !s.contains(w) {
                    s.push(w);
                }
            }
            s.join("+")
        };

        // A safety beat answers the disclosure alone: the rest of the room is carried.
        if safety {
            let held = if self.room.has_pending() {
                "prepared material and a room event held back"
            } else {
                "prepared material held back"
            };
            return Plan::new(
                BeatMode::React,
                Offer::SafetyOnly,
                max,
                format!("{lead}safety beat: {held}"),
            );
        }

        // A closed vote or a handover due: announced now, once, over the queued bit.
        let events = self.room.take();
        if !events.is_empty() {
            let labels: Vec<&str> = events.iter().map(|e| e.label.as_str()).collect();
            let labels = labels.join(" and ");
            if let Some(gone) = self.expire() {
                lead.push_str(&format!("room event: {gone} expired; "));
            }
            let connector = events
                .iter()
                .find_map(|e| e.connector)
                .and_then(|k| self.connector(b.prep, k).map(|l| (k, l)));
            let (mode, with) = if chat > 0 {
                (BeatMode::React, format!(" with chat ({signals})"))
            } else {
                (BeatMode::Bridge, String::new())
            };
            let mut p = Plan::new(
                mode,
                Offer::Chat,
                chat,
                format!("{lead}announce {labels}{with}"),
            );
            p.room_note = Some(
                events
                    .iter()
                    .map(|e| e.note.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            if chat == 0 {
                p.note = Some(ANNOUNCE_NOTE.into());
            }
            if let Some((k, line)) = connector {
                p.material = Some(format!(
                    "Connector ({}):\n- {}\n",
                    connector_label(k),
                    one_line(&line, 400)
                ));
            }
            return p;
        }

        // A queued connector, with any chat worth answering.
        if let Some(kind) = self.pending.take() {
            let line = if kind == ConnectorKind::IntoRoom {
                self.handoff_done = true;
                self.connector(b.prep, kind)
                    .or_else(|| b.prep.map(|p| p.cold_open.handoff.clone()))
            } else {
                self.connector(b.prep, kind)
            };
            if let Some(line) = line {
                let mut p = Plan::new(
                    BeatMode::Bridge,
                    Offer::Chat,
                    chat,
                    format!(
                        "{lead}bridge {} (state {})",
                        connector_label(kind),
                        b.ctx.state
                    ),
                );
                p.material = Some(format!(
                    "Connector ({}):\n- {}\n",
                    connector_label(kind),
                    one_line(&line, 400)
                ));
                return p;
            }
        }

        // A bit being told: the room answering it skips its next line; else it goes on.
        if let Some(active) = self.active_bit.clone() {
            let bit = b
                .prep
                .and_then(|p| p.bits.iter().find(|x| x.id == active.id));
            match bit {
                None => self.active_bit = None,
                Some(_) if !worthy.is_empty() => {
                    if let Some(why) = self.back_off(b.prep) {
                        lead.push_str(&why);
                    }
                }
                Some(bit) => {
                    let (m, what) = self.bit_lines(bit, active.next, 2);
                    let mut p = Plan::new(
                        BeatMode::Bit,
                        Offer::SafetyOnly,
                        0,
                        format!("{lead}quiet {} s: {what}", quiet_ms / 1000),
                    );
                    p.bit_id = Some(bit.id.clone());
                    p.material = Some(m);
                    self.last_bit_ms = Some(now);
                    return p;
                }
            }
        }

        if !worthy.is_empty() {
            let answerable = worthy.iter().filter(|w| **w != "safety").count();
            let thin = answerable <= THIN_ROOM_MAX && self.react_streak >= THIN_REACT_STREAK;
            if thin && self.active_bit.is_none() {
                if let Some(bit) = b.prep.and_then(|p| self.next_bit(p, b.ctx.segment, true)) {
                    let (m, what) = self.bit_lines(bit, 0, 1);
                    let mut p = Plan::new(
                        BeatMode::Mixed,
                        Offer::Chat,
                        1,
                        format!(
                            "{lead}{} reacts in a thin room: {what} with one pick ({signals})",
                            self.react_streak
                        ),
                    );
                    p.bit_id = Some(bit.id.clone());
                    p.material = Some(m);
                    self.last_bit_ms = Some(now);
                    return p;
                }
            }
            let after_bit = self.bit_ended;
            if after_bit && matches!(self.last_voiced, Some(BeatMode::Bit | BeatMode::Mixed)) {
                if let Some(line) = self.connector(b.prep, ConnectorKind::BackToChat) {
                    self.bit_ended = false;
                    let mut p = Plan::new(
                        BeatMode::Bridge,
                        Offer::Chat,
                        max,
                        format!("{lead}bridge back_to_chat after a bit ({signals})"),
                    );
                    p.material = Some(format!(
                        "Connector (back_to_chat):\n- {}\n",
                        one_line(&line, 400)
                    ));
                    return p;
                }
            }
            return Plan::new(
                BeatMode::React,
                Offer::Chat,
                max,
                format!("{lead}react: {signals}"),
            );
        }

        // Quiet.
        let quiet_s = quiet_ms / 1000;
        if let Some(prep) = b.prep {
            let spaced = self
                .last_bit_ms
                .is_none_or(|t| now.saturating_sub(t) >= BIT_SPACING_MS);
            if quiet_ms >= QUIET_BIT_MS && spaced && self.active_bit.is_none() {
                if let Some(bit) = self.next_bit(prep, b.ctx.segment, false) {
                    let (m, what) = self.bit_lines(bit, 0, 2);
                    self.last_bit_ms = Some(now);
                    let into = (self.last_voiced == Some(BeatMode::React))
                        .then(|| self.connector(b.prep, ConnectorKind::IntoBit))
                        .flatten();
                    let mut p = match into {
                        Some(line) => {
                            let mut p = Plan::new(
                                BeatMode::Bridge,
                                Offer::SafetyOnly,
                                0,
                                format!("{lead}quiet {quiet_s} s: bridge into_bit, {what}"),
                            );
                            p.material = Some(format!(
                                "Connector (into_bit):\n- {}\n{m}",
                                one_line(&line, 400)
                            ));
                            p
                        }
                        None => {
                            let mut p = Plan::new(
                                BeatMode::Bit,
                                Offer::SafetyOnly,
                                0,
                                format!("{lead}quiet {quiet_s} s: {what}"),
                            );
                            p.material = Some(m);
                            p
                        }
                    };
                    p.bit_id = Some(bit.id.clone());
                    return p;
                }
            }
            if may_hold {
                let gap = gap_ms.unwrap_or(0) / 1000;
                return Plan::new(
                    BeatMode::Hold,
                    Offer::SafetyOnly,
                    0,
                    format!("{lead}quiet {quiet_s} s, last line {gap} s ago: hold"),
                );
            }
        }
        // Nothing prepared is due: bridge from the agenda (an open loop, else his last line),
        // then a bit off its spacing, and only then generic filler.
        if let Some((note, what)) = self.agenda.bridge(now) {
            let mut p = Plan::new(
                BeatMode::Improvise,
                Offer::SafetyOnly,
                0,
                format!("{lead}quiet {quiet_s} s: bridged from the {what}"),
            );
            p.note = Some(format!(
                "(No chat to answer this beat.) {note}Never beg, never complain about the \
                 viewer count."
            ));
            return p;
        }
        if let Some(prep) = b.prep {
            if quiet_ms >= QUIET_BIT_MS && self.active_bit.is_none() {
                if let Some(bit) = self.next_bit(prep, b.ctx.segment, false) {
                    let (m, what) = self.bit_lines(bit, 0, 2);
                    self.last_bit_ms = Some(now);
                    let mut p = Plan::new(
                        BeatMode::Bit,
                        Offer::SafetyOnly,
                        0,
                        format!("{lead}quiet {quiet_s} s, nothing else open: {what}"),
                    );
                    p.bit_id = Some(bit.id.clone());
                    p.material = Some(m);
                    return p;
                }
            }
        }
        let why = if b.prep.is_some() {
            "nothing prepared fits"
        } else {
            "no prep"
        };
        Plan::new(
            BeatMode::Improvise,
            Offer::SafetyOnly,
            0,
            format!("{lead}quiet {quiet_s} s, {why}: improvised"),
        )
    }

    fn plan_cold_open(
        &mut self,
        b: &BeatInputs<'_>,
        safety: bool,
        may_hold: bool,
        owed: bool,
    ) -> Plan {
        let greets = b
            .cands
            .iter()
            .any(|c| self.is_first_contact(&c.msg.id) && !is_vote(c));
        let firsts = owed || greets;
        if safety {
            return Plan::new(
                BeatMode::React,
                Offer::SafetyOnly,
                1,
                "room filling: safety beat, story paused".into(),
            );
        }
        let fc = if greets {
            ", greeting a first contact"
        } else if owed {
            ", answering a request"
        } else {
            ""
        };
        if let Some(prep) = b.prep {
            let co = &prep.cold_open;
            if self.cold_next < co.beats.len() {
                let from = self.cold_next;
                let n = take_lines(&co.beats, from);
                let total = co.beats.len();
                self.cold_next = from + n;
                let mut m = format!(
                    "Cold open \"{}\", {}:\n",
                    one_line(&co.title, 120),
                    span(from, n, total)
                );
                push_lines(&mut m, &co.beats[from..from + n]);
                m.push_str(if self.cold_next < total {
                    "The story continues next beat: do not end it yet.\n"
                } else {
                    "That is the end of the story: land it.\n"
                });
                let mut p = Plan::new(
                    BeatMode::ColdOpen,
                    Offer::FirstContact,
                    1,
                    format!("room filling: cold open {}{fc}", span(from, n, total)),
                );
                p.bit_id = Some(co.id.clone());
                p.material = Some(m);
                return p;
            }
            if !self.handoff_done {
                self.handoff_done = true;
                let mut p = Plan::new(
                    BeatMode::Bridge,
                    Offer::FirstContact,
                    1,
                    format!("room filling: cold open handoff{fc}"),
                );
                p.bit_id = Some(co.id.clone());
                p.material = Some(format!(
                    "Hand the story over to the room:\n- {}\n",
                    one_line(&co.handoff, 400)
                ));
                return p;
            }
        } else if !self.gathering_done {
            self.gathering_done = true;
            self.last_gathering_ms = Some(b.now_ms);
            let mut p = Plan::new(
                BeatMode::ColdOpen,
                Offer::FirstContact,
                1,
                format!("room filling, no prep: gathering open{fc}"),
            );
            p.note = Some(GATHERING_NOTE.into());
            return p;
        }
        if firsts {
            let lead = self.back_off(b.prep).unwrap_or_default();
            return Plan::new(
                BeatMode::React,
                Offer::FirstContact,
                1,
                format!(
                    "room filling: {lead}{}",
                    if greets { "first contact" } else { "request" }
                ),
            );
        }
        if may_hold {
            let why = if b.prep.is_some() {
                "room filling: story told, hold"
            } else {
                "room filling, no prep: hold"
            };
            return Plan::new(BeatMode::Hold, Offer::SafetyOnly, 0, why.into());
        }
        let Some(prep) = b.prep else {
            // No deck: a gathering line at most every GATHERING_SPACING_MS, an aside (no
            // welcome) on the speaking beats between.
            let since = self.last_gathering_ms.map(|t| b.now_ms.saturating_sub(t));
            if let Some(s) = since.filter(|s| *s < GATHERING_SPACING_MS) {
                let mut p = Plan::new(
                    BeatMode::Improvise,
                    Offer::SafetyOnly,
                    0,
                    format!(
                        "room filling, no prep: aside, gathering line {} s ago",
                        s / 1000
                    ),
                );
                p.note = Some(ASIDE_NOTE.into());
                return p;
            }
            self.last_gathering_ms = Some(b.now_ms);
            let mut p = Plan::new(
                BeatMode::ColdOpen,
                Offer::SafetyOnly,
                0,
                "room filling, no prep: gathering line".into(),
            );
            p.note = Some(GATHERING_NOTE.into());
            return p;
        };
        // The segment's deck, premises first: they invite the first chat.
        let bit = match self.active_bit.clone() {
            Some(a) => match prep.bits.iter().find(|x| x.id == a.id) {
                Some(bit) => Some((bit, a.next)),
                None => {
                    self.active_bit = None;
                    None
                }
            },
            None => None,
        }
        .or_else(|| self.next_bit(prep, b.ctx.segment, true).map(|bit| (bit, 0)));
        if let Some((bit, from)) = bit {
            let (m, what) = self.bit_lines(bit, from, 2);
            if from == 0 {
                self.last_bit_ms = Some(b.now_ms);
            }
            let mut p = Plan::new(
                BeatMode::Bit,
                Offer::FirstContact,
                1,
                format!("room filling: deck {what}"),
            );
            p.bit_id = Some(bit.id.clone());
            p.material = Some(m);
            return p;
        }
        if !self.deck_gathering_done {
            self.deck_gathering_done = true;
            let mut p = Plan::new(
                BeatMode::ColdOpen,
                Offer::SafetyOnly,
                0,
                "room filling: deck empty, one gathering line".into(),
            );
            p.note = Some(GATHERING_NOTE.into());
            return p;
        }
        Plan::new(
            BeatMode::Improvise,
            Offer::SafetyOnly,
            0,
            "room filling: deck empty, gathering line spent: improvised".into(),
        )
    }

    /// Settle the beat after the picker: the final mode, the waves answered, the streaks, and
    /// the `beat_plan` record.
    pub fn commit(&mut self, plan: &mut Plan, pick: &Pick, now_ms: TsMs) -> StreamEvent {
        if plan.mode == BeatMode::Mixed && pick.chosen.is_empty() {
            plan.mode = BeatMode::Bit;
        }
        for c in pick
            .chosen
            .iter()
            .filter(|c| c.count > 1 && c.safety.is_none())
        {
            let norm = normalize(&c.msg.text);
            if !norm.is_empty() && !self.acked_waves.contains(&norm) {
                self.acked_waves.push(norm);
            }
            plan.reason
                .push_str(&format!("; wave of {} acknowledged", c.viewers.max(1)));
        }

        // What was answered is never offered again; what needed an answer and got none is
        // carried, newest first, while it is young enough. While the room fills nothing
        // expires, and the cap keeps first contacts first, the oldest first.
        let chosen: HashSet<&str> = pick.chosen.iter().map(|c| c.msg.id.as_str()).collect();
        let late: Vec<String> = pick
            .chosen
            .iter()
            .filter_map(|c| {
                let k = self.carried_in.get(&c.msg.id)?;
                Some(format!(
                    "{} ({} s)",
                    c.msg.id,
                    now_ms.saturating_sub(k.since) / 1000
                ))
            })
            .collect();
        if !late.is_empty() {
            plan.reason
                .push_str(&format!("; carried over: {}", late.join(", ")));
        }
        self.answered.extend(chosen.iter().map(|s| s.to_string()));
        for id in &chosen {
            self.agenda.close(LoopKind::Request, id);
        }
        for c in &pick.chosen {
            if let Some(a) = ask_of(c) {
                self.asked.insert((c.msg.viewer.id.clone(), a));
            }
        }
        let mut carry: Vec<Carried> = std::mem::take(&mut self.carry_next)
            .into_iter()
            .filter(|k| !chosen.contains(k.cand.msg.id.as_str()) && (k.held || now_ms < k.expires))
            .collect();
        // Owned requests go first and are never capped.
        if carry.iter().any(|k| k.held) {
            carry.sort_by_key(|k| {
                (
                    k.owned.is_none(),
                    !k.first,
                    if k.first { k.since } else { -k.since },
                )
            });
        } else {
            carry.sort_by_key(|k| (k.owned.is_none(), std::cmp::Reverse(k.since)));
        }
        let keep = CARRY_MAX.max(carry.iter().filter(|k| k.owned.is_some()).count());
        self.capped = carry
            .iter()
            .skip(keep)
            .map(|k| k.cand.msg.id.clone())
            .collect();
        carry.truncate(keep);
        let fresh: Vec<&str> = carry
            .iter()
            .filter(|k| !self.carried_in.contains_key(&k.cand.msg.id))
            .map(|k| k.cand.msg.id.as_str())
            .collect();
        if !fresh.is_empty() {
            plan.reason
                .push_str(&format!("; deferred: {}", fresh.join(", ")));
        }
        self.carried = carry;
        match plan.mode {
            BeatMode::React => self.react_streak += 1,
            BeatMode::Hold => {}
            _ => self.react_streak = 0,
        }
        if plan.bit_id.is_none() && matches!(plan.mode, BeatMode::React | BeatMode::Bridge) {
            self.bit_ended = false;
        }
        self.last_mode = Some(plan.mode);
        if plan.mode != BeatMode::Hold {
            self.last_voiced = Some(plan.mode);
        }
        StreamEvent::BeatPlan {
            ts_ms: now_ms,
            mode: plan.mode,
            bit_id: plan.bit_id.clone(),
            picks: u32::try_from(pick.chosen.len()).unwrap_or(u32::MAX),
            reason: plan.reason.clone(),
        }
    }

    /// The `input` records of the messages the carry cap dropped at the last commit
    /// ([`CARRY_RULE`] / [`CAPPED`]), taken once: every drop leaves a record.
    pub fn take_capped(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        std::mem::take(&mut self.capped)
            .iter()
            .map(|id| moderation::input_event(now_ms, CARRY_RULE, id, CAPPED))
            .collect()
    }

    /// The in-character stand-in for a line the output gate recast: a rebuttal of a forged
    /// decree when the beat answered an injection or an authority claim, else a quiet-room
    /// line. Either rotation never repeats one of its recent choices.
    pub fn recast(&mut self, authority: bool, original: &str) -> String {
        if authority {
            self.rebuttals.choose(REBUTTALS, original).to_string()
        } else {
            self.in_character
                .choose(IN_CHARACTER_LINES, original)
                .to_string()
        }
    }
}
