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
//! - **After it, the segment leads** (the operator, 2026-10-24: "main focus are the segments,
//!   chats after careful pick to compliment them ... it should never turn to a roast show
//!   switching topic each reply"; over 260 sim runs 46% of lines opened with a viewer's handle).
//!   Every beat advances the segment and folds in at most one chat message. The worthy chat is
//!   sorted into three classes:
//!   - **required**: an owned request (a situational ask, "what is this"), a gift, a first
//!     contact, a wave (safety beats are their own, unchanged);
//!   - **on-topic**: a message sharing a topic word of the live segment
//!     ([`crate::topic::topic_words`]: the case, its hexes, the foils, the open poll, the state's
//!     purpose), or an idea for it (`!propose`, "what if", "we should", ...);
//!   - **off-topic**: the rest that is worth an answer (a mention, a plain question, a command,
//!     bait). A plain remark, however long, is not worth a beat of its own.
//!
//!   With required or on-topic chat the beat is `mixed`: segment material (the next prepared bit
//!   off its spacing, the through-line's bridge, or the state's purpose) plus one pick from that
//!   class, required first; reason `segment+chat: <tags>`. With no segment material it is `react`
//!   with one pick and a directive to tie the reply back to the segment. With off-topic chat
//!   alone, a direct question or mention (or bait) gets one pick at most every
//!   [`OFF_TOPIC_ANSWER_MS`] (reason `segment+off-topic: <tags>`), and every other beat is a
//!   segment beat with no pick (`bit`, `improvise`; reason `segment (carried N)`), never a hold.
//!   A quiet room gets `bit` (a prepared bit for the current segment, shuffled, never repeated in
//!   the stream), `bridge` (a connector into a bit, back to chat, into a poll or a handover),
//!   `hold` when the last line is recent, or an improvised line. Without a prep there is no bit,
//!   bridge or hold: a quiet beat is improvised (on the state's purpose when it has one).
//! - **Small room** (the streaming standard's size-tiered picking): while at most
//!   [`SMALL_ROOM_ENTER`] distinct viewers chatted in the last [`SMALL_ROOM_WINDOW_MS`], every
//!   message but filler counts as on-topic and is carried until answered, one a beat; the mode
//!   ends at [`SMALL_ROOM_LEAVE`] chatters (hysteresis). Its reasons start `small-room`.
//!
//! Rules: a bit is offered once and marked used; required chat arriving mid-bit skips its next
//! line (on-topic chat folds into the bit's beat instead); a raid, a safety pick or a segment
//! change expires the queued bit and connector; a safety beat never voices prepared material and
//! offers only the safety pick. Votes and lone emotes never fill a pick, and a wave once answered
//! is not picked again this stream.
//!
//! A message that needs an answer (a question, a mention, a first contact, a gift or a sub, a
//! proposal or a summons, a mockable injection) and that a beat could not answer (a hold, a
//! safety beat, the cold-open filter, the pick ceiling) is carried into the next beats for
//! [`CARRY_MS`] from the drain, a question or bait for [`REQUEST_MAX_MS`] ([`waits_long`]), at
//! most [`CARRY_MAX`] at once besides the owned requests, one a viewer ([`SUPERSEDED`]), competing
//! with fresh chat with a small age bonus; once answered it is never offered again. Waves and
//! votes are never carried, but for a newcomers' wave (a raid's greeting). Questions asking the
//! same thing within [`QUESTION_WAVE_MS`] are one question wave, offered by its oldest and
//! answered once for all. Required or on-topic chat
//! waiting from an earlier beat goes in before new prepared material: a story being told waits
//! a beat for it, and the off-topic slot takes the oldest waiting bait or question.
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
//! **A scene** (SEG-03, [`crate::scene`]): while the brain performs a scene, every beat that is
//! not a speech hold or a safety beat is a `scene` beat (no picks; chat is carried as in a
//! hold). A scene the runtime just put up starts only once the room event that chose it (the
//! ballot's result, its tie) has been announced. The scene gives way at a line boundary (the
//! next beat that is not a speech hold), then resumes where it stopped:
//! - to a **care beat**: a safety pick (as anywhere), or a sincerely sad viewer's owned request
//!   ([`Ask::Sad`]), answered alone (host only). It resumes rather than ends: the care rules
//!   ([`crate::care`]) forbid joking about the viewer and what they shared, not comedy, and the
//!   scene's lines never touch chat (its live slots were written at its start, with no chat);
//! - to **one short aside** for a room event: a raid seen during the scene (sticky across the
//!   speech holds, where it usually lands) or a gift or sub, with those picks only, host only,
//!   at half budget, at most one per [`SCENE_ASIDE_SPACING_MS`] so a busy room cannot stall it
//!   (sbx-20261024-194036-10m: five gifted subs and a raid during "The Gallery" got nothing,
//!   and their messages expired).
//!
//! What a scene beat (or a hold during one) carries is held like the cold open's: it never
//! expires while the scene plays, the cap keeps first contacts and gifts first, and its
//! [`CARRY_MS`] starts from the drain of the first beat after the scene, so free talk can
//! answer it.
//!
//! **Consume on delivery.** [`Orchestrator::plan`] and [`Orchestrator::commit`] decide a beat
//! and stage what it consumes (picks answered, waves acknowledged, a bit or cold-open lines
//! drawn, a connector used, room events taken, the through-line's pick-up). The brain opens a
//! [`BeatTxn`] before the plan and settles it after the core call: a delivered beat keeps
//! what it staged; a failed one is [rolled back](Orchestrator::rollback), its picks carried
//! again with their original clocks (a safety pick included), so the next beat that speaks
//! still owes them, the bit and the vote announcement.
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
//! **The case** ([`case`]): in an investigation segment "advance the case" is an open loop of
//! its own. When a move is open (reveal a card whose research came back, pin a string a returned
//! record supports, research the next lead when none is out, show a returned record) and
//! [`case::CASE_BEAT_MS`] passed since the last case move ([`case::CASE_PAYOFF_MS`] for a reveal
//! or a string), the next voiced beat that is not a hold, a safety beat, a room event, the
//! goodbye or a beat owed to a request carries the move ahead of chat: one pick at most, a
//! `# The case` section with the exact op line, reason `case: <move>`. A reply with no case op
//! is asked again after [`case::CASE_RETRY_MS`].
//!
//! Every candidate worth answering that a beat neither picked nor carried leaves a record
//! ([`CARRY_RULE`] / [`DROPPED`]): nothing leaves the beat unrecorded (the judged stereotype
//! bait and a sincerely sad regular both did, in speech holds).
//!
//! The thresholds are assumptions to tune against sandbox runs, not measurements:
//! [`QUIET_BIT_MS`], [`BIT_SPACING_MS`], [`HOLD_GAP_MS`], [`RAID_FIRST_CONTACTS`], [`CARRY_MS`],
//! [`CARRY_MAX`], [`GATHERING_SPACING_MS`], [`REQUEST_ANSWER_MS`], [`REQUEST_LEAD_MS`],
//! [`REQUEST_MAX_MS`], [`OFF_TOPIC_ANSWER_MS`], [`SMALL_ROOM_ENTER`], [`SMALL_ROOM_LEAVE`].

pub mod agenda;
pub mod case;
pub mod requests;
pub mod room;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet, VecDeque};

use shadowrunr_types::prep::{BeatMode, BitKind, ConnectorKind, PrepBit, StreamPrep};
use shadowrunr_types::{ChatMsg, Command, SegmentContext, StreamEvent, TsMs};

use crate::character::LineBank;
use crate::moderation::{self, Candidate};
use crate::picker::{is_bare_emote, is_question, is_vote, mentions, Pick};
use crate::util::{fnv1a64, normalize, one_line, similarity};

use agenda::{Agenda, BeatShape, LoopKind};
use case::{CaseDesk, CaseMove, CASE_NOTE, CASE_WITH_CHAT};
use requests::{asks_what_this_is, situational_ask, situational_asks, Ask, Rotation};
use room::RoomWatch;
use shadowrunr_types::character::Tuning;

/// The segment state in which the room is still filling.
pub const COLD_OPEN_STATE: &str = "cold_open";
/// No chat worth answering for this long before a new bit starts (assumption).
pub const QUIET_BIT_MS: i64 = Tuning::DEFAULT.quiet_bit_ms;
/// At least this long between the starts of two new bits, so a premise has time to be
/// answered and the deck lasts the segment (assumption).
pub const BIT_SPACING_MS: i64 = Tuning::DEFAULT.bit_spacing_ms;
/// A quiet beat may hold only while his last line is younger than this: with 5 s beats the
/// next one speaks well under the 15 s say-gap ceiling (assumption).
pub const HOLD_GAP_MS: i64 = Tuning::DEFAULT.hold_gap_ms;
/// A direct off-topic question or mention (or bait) gets one folded pick at most this often
/// (assumption: one beat in six).
pub const OFF_TOPIC_ANSWER_MS: i64 = 30_000;
/// The room is small at this many distinct chatters or fewer: every message counts as on-topic.
pub const SMALL_ROOM_ENTER: usize = 5;
/// A small room stops being one at this many distinct chatters (hysteresis).
pub const SMALL_ROOM_LEAVE: usize = 7;
/// The window distinct chatters are counted over.
pub const SMALL_ROOM_WINDOW_MS: i64 = 180_000;
/// A newcomer's "what is this" answered this long ago is owed again: the answer did not land.
/// sbx-20261024-212929-10m: MurkOtter's "are we spamming someone's stream now?? what is going
/// on lol" (257.8 s) was answered as the newcomer's question at 265.7 s, and the real "wait
/// what is this stream, whats going on lol" at 344.0 s (78 s later) became a repeat and expired.
/// The director asked for 120 s; that would not have reached this line, so 60 s (assumption).
pub const WHAT_IS_THIS_REASK_MS: i64 = 60_000;
/// Questions this close in time that ask the same thing are one question wave (assumption).
pub const QUESTION_WAVE_MS: i64 = 60_000;
/// Token overlap (Jaccard of normalised words) from which two questions ask the same thing.
pub const QUESTION_WAVE_JACCARD: f64 = 0.5;
/// The [`CARRY_RULE`] action for a carried message another of its viewer's stands for: a
/// viewer keeps one carried message besides an owned request, so one repeater cannot fill the
/// carry (theatrical.sonnet.SEG-01.a30.s1: one baiter's repeats took the cap of 6 for minutes).
pub const SUPERSEDED: &str = "superseded";
/// The segment directive of a chat beat with no segment material.
pub const TIE_BACK_NOTE: &str = "Nothing prepared is due this beat: take up the picked message \
and tie your reply back to the segment (its rules and state above); do not start a new topic.";
/// The segment directive that advances the current state's purpose (`{purpose}` filled).
pub const PURPOSE_NOTE: &str = "Advance the segment this beat, toward the state's purpose: \
{purpose}";
/// First contacts in one beat that count as a raid (the room changed).
pub const RAID_FIRST_CONTACTS: usize = 5;
/// During a scene, first contacts arriving across beats this close together count toward a
/// raid (assumption: a raid lands over two or three 5 s beats).
pub const RAID_WINDOW_MS: i64 = 20_000;
/// A message this close to an acknowledged wave is that wave again.
const WAVE_SIMILARITY: f64 = 0.8;
/// Two pastes this close are one wave; never pick several pasters.
const PASTER_SIMILARITY: f64 = 0.6;
/// Normalised length from which two lone near-duplicates count as pastes.
const PASTER_MIN_CHARS: usize = 40;
/// Two story lines are voiced together when they fit in this many characters.
const TWO_LINES_MAX_CHARS: usize = 220;
/// How long a message a beat could not answer is carried into the next beats (assumption:
/// two to five beats, so the answer still reads as one to the asker).
pub const CARRY_MS: i64 = Tuning::DEFAULT.carry_ms;
/// Appended to the `beat_plan` reason of a beat whose core call failed: what it planned was
/// rolled back ([`Orchestrator::rollback`]).
pub const ROLLED_BACK: &str = "; rolled back: core call failed";
/// The most messages carried at once; the oldest go first.
pub const CARRY_MAX: usize = Tuning::DEFAULT.carry_max;
/// The `input` moderation rule that records a carried message dropped unanswered: action
/// `expired` (it waited [`CARRY_MS`]) or [`CAPPED`] (the [`CARRY_MAX`] cap dropped it).
pub const CARRY_RULE: &str = "carry";
/// The [`CARRY_RULE`] action for a message the carry cap dropped.
pub const CAPPED: &str = "capped";
/// The [`CARRY_RULE`] action for a pick whose answer never aired: carried again.
pub const UNAIRED: &str = "unaired";
/// The [`CARRY_RULE`] action for a message worth answering that a beat neither picked nor carried
/// (a remark in a speech hold, say).
pub const DROPPED: &str = "dropped";
/// Without a prep, at least this long between two gathering lines of the cold open
/// (assumption); he holds in between.
pub const GATHERING_SPACING_MS: i64 = 20_000;
/// An owned request is answered within this long of reaching a beat (assumption: the target the
/// two rules below keep; four beats).
pub const REQUEST_ANSWER_MS: i64 = Tuning::DEFAULT.request_answer_ms;
/// While an owned request waits, a beat holds for speech only while more than this is queued
/// (assumption: its line still starts well inside the voice's `max_lag_ms` of 15 s).
pub const REQUEST_LEAD_MS: i64 = Tuning::DEFAULT.request_lead_ms;
/// An owned request still unanswered this long after it reached a beat is dropped
/// ([`CARRY_RULE`] / `expired`): an answer that late no longer reads as one (assumption).
pub const REQUEST_MAX_MS: i64 = Tuning::DEFAULT.request_max_ms;

/// The beat note for an open with no prepared story, and for gathering lines after one.
pub const GATHERING_NOTE: &str = "(The room is still gathering. One short in-character line \
that welcomes whoever is arriving: no premise, no poll, no call to action, no count.)";

/// The beat note between gathering lines of a cold open with no prep: talk, but no welcome.
pub const ASIDE_NOTE: &str = "(The room is still gathering. One short in-character aside: a \
boast, a grievance or an observation about your lair. No welcome, no premise, no poll, no call \
to action, no count.)";

/// At most one scene aside per this long (assumption: a 90-180 s scene gets two at most).
pub const SCENE_ASIDE_SPACING_MS: i64 = 30_000;

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
    /// The live segment's topic words ([`crate::topic::topic_words`]): chat sharing one is
    /// on-topic.
    pub topic_words: &'a [String],
    /// A scene to perform ([`crate::scene`]): its id, and whether it is still to start (it
    /// waits for a room event, such as the ballot's tie, to be announced first).
    pub scene: Option<(&'a str, bool)>,
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
    /// A care beat in a scene: safety picks and sincerely sad viewers' owned requests.
    Care,
    /// A scene aside: safety picks, gifts, and (in a raid) first contacts.
    Aside { raid: bool },
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
    /// The case's next move for this beat ([`case`]), if it carries one.
    pub case: Option<String>,
    /// The segment directive for this beat (the state's purpose, a tie-back), chat or no chat.
    pub segment_note: Option<String>,
    /// The host alone speaks this beat (a care beat or an aside in a scene): no guest line.
    pub host_only: bool,
    /// One short line: the beat speaks at half budget (a scene aside).
    pub short: bool,
    /// A scene aside (`Some(raid)`: whether it is for a raid): it airs a line whatever the
    /// repetition guard says, an engine line when the model's cannot air.
    pub aside: Option<bool>,
    /// A cue leads the beat: chat answers go after the cue's lines.
    pub lead_cue: bool,
    pub reason: String,
    /// The one class of chat this beat may pick from (required, on-topic, or the direct
    /// off-topic messages), by id; `None` offers what [`Offer`] lets through.
    only: Option<HashSet<String>>,
    /// The beat's pick, if any, is the off-topic answer ([`OFF_TOPIC_ANSWER_MS`]).
    off_topic: bool,
    /// The beat's pick is an owned request: it takes the beat (no through-line directive).
    owed: bool,
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
            case: None,
            segment_note: None,
            host_only: false,
            short: false,
            aside: None,
            lead_cue: false,
            reason,
            only: None,
            off_topic: false,
            owed: false,
        }
    }

    /// This plan offering the beat's chat class ([`ChatOffer`]) under its ceiling.
    fn with_chat(mut self, o: &ChatOffer) -> Self {
        if self.ceiling > 0 {
            self.only = o.only.clone();
            self.off_topic = o.off_topic;
            self.owed = o.owed;
        }
        self
    }
}

/// Where a worthy message stands against the segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Owed whatever the segment: safety, a request, a gift, a first contact, a wave.
    Required,
    /// About the live segment: it shares a topic word or offers an idea (in a small room,
    /// everything but filler).
    OnTopic,
    /// Worth an answer, but not about the segment: one every [`OFF_TOPIC_ANSWER_MS`] at most.
    OffTopic,
}

/// The chat a beat may fold in: its ceiling (0 or 1), the class offered, and its tags.
#[derive(Debug, Clone, Default)]
struct ChatOffer {
    ceiling: usize,
    only: Option<HashSet<String>>,
    off_topic: bool,
    signals: String,
    small: bool,
    /// The class offered holds an owned request: it takes the beat.
    owed: bool,
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
    /// A question or bait ([`waits_long`]): its `expires` counts [`REQUEST_MAX_MS`] from the
    /// drain, not [`CARRY_MS`].
    long: bool,
}

/// Whether a message is carried for [`REQUEST_MAX_MS`] from the drain instead of [`CARRY_MS`]:
/// a question (to the host or about the segment) or bait. In the 2026-10-24 tournament 208
/// carried messages expired behind speech holds, among them the bait, the "are you real"
/// questions and a question about the board, never answered.
fn waits_long(c: &Candidate) -> bool {
    c.safety.is_none() && c.count == 1 && (c.injection || is_question(&c.msg.text))
}

/// Normalised words of a message, for the question-wave overlap.
fn word_set(text: &str) -> HashSet<String> {
    normalize(text)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Jaccard overlap of two word sets.
fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// A multi-line bit being told across beats.
#[derive(Debug, Clone)]
struct ActiveBit {
    id: String,
    /// Index of the next line to voice.
    next: usize,
}

/// A beat in flight: the orchestrator as it stood before the beat was planned. Dropping it
/// keeps the beat (delivered); [`Orchestrator::rollback`] restores it (the core call failed).
#[derive(Debug)]
pub struct BeatTxn {
    before: Box<Orchestrator>,
}

#[derive(Debug, Default, Clone)]
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
    /// This beat's candidates worth answering (not safety picks), by id.
    beat_worthy: Vec<String>,
    /// Ids the last commit neither picked nor carried, for their records.
    dropped: Vec<String>,
    /// The last commit's picks that would have been carried, had no pick answered them: what
    /// a rollback carries again, with their original clocks.
    unsettled: Vec<Carried>,
    /// Ids of messages picked this stream: never offered again.
    answered: HashSet<String>,
    /// This beat's question waves: the representative's id, then every member (id, viewer
    /// name), the representative first ([`QUESTION_WAVE_MS`]).
    waves: HashMap<String, Vec<(String, String)>>,
    /// This beat's question-wave members by id, to their representative's id.
    wave_of: HashMap<String, String>,
    /// Ids the last commit dropped because another of their viewer's carried messages stands
    /// for them, for their records ([`SUPERSEDED`]).
    superseded: Vec<String>,
    room: RoomWatch,
    /// A connector queued by a state change.
    pending: Option<ConnectorKind>,
    connector_uses: HashMap<&'static str, usize>,
    last_mode: Option<BeatMode>,
    /// The last mode that was not a hold (what the room last heard).
    last_voiced: Option<BeatMode>,
    /// When an off-topic message was last folded in ([`OFF_TOPIC_ANSWER_MS`]).
    last_off_topic_ms: Option<TsMs>,
    /// The bit being told waited the last beat for waiting chat: it goes on this one.
    bit_waited: bool,
    /// Who chatted when, for the room's size ([`SMALL_ROOM_WINDOW_MS`]).
    chatters: HashMap<String, TsMs>,
    /// The room is small ([`SMALL_ROOM_ENTER`], [`SMALL_ROOM_LEAVE`]).
    small_room: bool,
    last_segment: Option<shadowrunr_types::SegmentId>,
    last_state: Option<String>,
    /// Since when no chat was worth answering.
    quiet_since: Option<TsMs>,
    in_character: Rotation,
    rebuttals: Rotation,
    /// The through-line's open loops ([`agenda`]).
    agenda: Agenda,
    /// The case loop ([`case`]) and its next move this beat.
    case: CaseDesk,
    case_next: Option<CaseMove>,
    /// (viewer, ask) answered this stream: that viewer's same ask is no longer owned.
    asked: HashSet<(String, Ask)>,
    /// When each (viewer, ask) was last answered: a newcomer's "what is this" is owed again
    /// [`WHAT_IS_THIS_REASK_MS`] after.
    asked_at: HashMap<(String, Ask), TsMs>,
    /// The beat being screened and planned (for [`Self::owns`]).
    now_ms: TsMs,
    /// The character's tuning: the carry, hold, bit and request timings (the constants above
    /// are their engine defaults).
    tuning: Tuning,
    /// The character's recast lines and forged-decree rebuttals.
    lines: LineBank,
    /// A raid seen while a scene plays (its first contacts), owed one aside.
    scene_raid: Option<usize>,
    /// When the last scene aside was planned.
    last_aside_ms: Option<TsMs>,
    /// First contacts that arrived during the scene, by beat, inside [`RAID_WINDOW_MS`].
    scene_arrivals: VecDeque<(TsMs, usize)>,
    /// First contacts outside a scene, by beat, inside [`RAID_WINDOW_MS`]: their ids.
    arrivals: VecDeque<(TsMs, Vec<String>)>,
    /// A raid outside a scene still owed its greeting: its arrivals' ids, to when they came.
    /// Cleared once one of them is answered (one greeting serves the raid); an arrival older
    /// than [`REQUEST_MAX_MS`] lapses.
    raid_waiting: HashMap<String, TsMs>,
    /// Viewers whose brigade call was refused this stream ([`Self::brigade_refusal`]): a later
    /// call of theirs is plain bait, owed nothing.
    brigade_refused: HashSet<String>,
    /// Brigade calls owed their refusal, with when they were noted ([`Self::note_brigade_calls`]).
    /// Kept apart from the carry: a call superseded there by its viewer's older bait is still
    /// owed.
    brigade_owed: Vec<(TsMs, Candidate)>,
}

/// The situational ask an owned request makes: a single message (never a wave or a safety
/// pick) that a situation rule covers. A newcomer (`first` contact, or a Thrall) asking what
/// this is makes one too: in a speech hold it was a plain remark and was dropped
/// (sbx-20261022-102618-10m L272).
fn ask_of(c: &Candidate, first: bool) -> Option<Ask> {
    asks_of(c, first).into_iter().next()
}

/// Every situational ask a candidate makes, in [`requests::situational_asks`] order, then a
/// newcomer's "what is this". One message may make several (unhinged-roastmaster.sonnet.SEG-01.
/// a30.s1 L309: "what is this stream ... is the guy on screen an ai or a person" from a Thrall
/// already told he is an AI: the AI ask was answered, the newcomer's was not, and with only the
/// first ask considered the message was owned by nobody and superseded).
fn asks_of(c: &Candidate, first: bool) -> Vec<Ask> {
    // Bait is never owed, whatever ask it matches: an injection or a brigade call (tagged at
    // input), or a request for what he never does ([`Ask::is_bait`]).
    if c.count > 1 || c.safety.is_some() || is_vote(c) || c.injection {
        return Vec::new();
    }
    if matches!(c.msg.command, Some(Command::Propose { .. })) {
        // A proposal is owed its verdict (bait in a !propose returned above).
        return vec![Ask::Proposal];
    }
    let mut asks = situational_asks(&c.msg.text);
    asks.retain(|a| !a.is_bait());
    if (first || c.msg.viewer.rank == shadowrunr_types::Rank::Thrall)
        && asks_what_this_is(&c.msg.text)
        && !asks.contains(&Ask::WhatIsThis)
    {
        asks.push(Ask::WhatIsThis);
    }
    asks
}

/// What the live segment is about and how big the room is: the context [`worth`] classifies in.
#[derive(Clone, Copy)]
struct Seg<'a> {
    names: &'a [String],
    topic: &'a [String],
    small: bool,
}

/// Why a candidate is worth answering, if it is, and its [`Class`]. A plain remark is not (it
/// was, at four words, and every beat became a reply); in a small room every message but filler
/// is, as `chat`. Bait (`mockable`) is off-topic wherever it points. `owed`: the message makes an
/// ask its viewer is still owed ([`Orchestrator::owns`]); an ask every one of whose kinds was
/// answered for its viewer is a `repeat`, off-topic, at most a point-back in the off-topic slot
/// (sbx-20261024-205313-10m: WraithKettle's "is this an AI or a guy with a voice changer",
/// answered at 1:14, was required again at 3:24, 7:30, 9:02 and 9:15, a beat each).
fn worth(c: &Candidate, first: bool, s: Seg<'_>, owed: bool) -> Option<(&'static str, Class)> {
    if c.safety.is_some() {
        return Some(("safety", Class::Required));
    }
    if is_filler(c) {
        return None;
    }
    let asks = ask_of(c, first).is_some();
    if asks && owed {
        return Some(("request", Class::Required));
    }
    if moderation::is_gift_or_sub(&c.msg.text) {
        return Some(("gift", Class::Required));
    }
    if first {
        return Some(("first contact", Class::Required));
    }
    if c.count > 1 {
        return Some(("wave", Class::Required));
    }
    if is_bait(c) {
        return Some(("mockable", Class::OffTopic));
    }
    if asks {
        return Some(("repeat", Class::OffTopic));
    }
    if crate::topic::is_idea(c) {
        return Some(("idea", Class::OnTopic));
    }
    if crate::topic::shares_topic(&c.msg.text, s.topic) {
        return Some(("topic", Class::OnTopic));
    }
    let base = if mentions(&c.msg.text, s.names) {
        Some("mention")
    } else if is_question(&c.msg.text) {
        Some("question")
    } else if c.msg.command.is_some() {
        Some("command")
    } else {
        None
    };
    if s.small {
        return Some((base.unwrap_or("chat"), Class::OnTopic));
    }
    base.map(|b| (b, Class::OffTopic))
}

/// Bait: an injection or a brigade call (tagged at input), or a request for what he never does
/// ([`Ask::is_bait`]). Off-topic (`mockable`) wherever it points, and never owed.
fn is_bait(c: &Candidate) -> bool {
    c.injection || situational_asks(&c.msg.text).iter().any(|a| a.is_bait())
}

/// Never fills a pick: a vote (the segment counts it) or a lone emote.
fn is_filler(c: &Candidate) -> bool {
    c.safety.is_none() && (is_vote(c) || (c.count == 1 && is_bare_emote(&c.msg.text)))
}

/// Whether an unanswered message is carried: one that asks for an answer or feeds the segment
/// (in a small room, every message but filler), never a wave, a vote, a plain remark or a safety
/// pick (which is never left unanswered). A wave of newcomers (a raid's arrivals) is the
/// exception: its greeting is owed, and an owned request taking the beat's one slot once left a
/// raid ungreeted (unhinged-roastmaster.sonnet.SEG-01.a300.s1, 320 s). A wave of lone emotes is not.
fn carries(c: &Candidate, first: bool, s: Seg<'_>, owed: bool) -> bool {
    if c.safety.is_some() || (c.count > 1 && !(first && !is_bare_emote(&c.msg.text))) {
        return false;
    }
    match worth(c, first, s, owed) {
        Some((_, Class::Required | Class::OnTopic)) => true,
        Some(("mention" | "question" | "mockable" | "repeat", _)) => true,
        Some(("command", _)) => matches!(
            c.msg.command,
            Some(Command::Propose { .. } | Command::Summon { .. })
        ),
        _ => false,
    }
}

/// The reason head of a beat that folds in chat: `segment+chat: <tags>` (`segment+off-topic`
/// for the off-topic answer), after `small-room ` in a small room.
fn chat_head(o: &ChatOffer) -> String {
    format!(
        "{}{}: {}",
        if o.small { "small-room " } else { "" },
        if o.off_topic {
            "segment+off-topic"
        } else {
            "segment+chat"
        },
        o.signals
    )
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

    /// This orchestrator running a character's tuning and line bank.
    pub fn with_character(mut self, tuning: Tuning, lines: LineBank) -> Self {
        self.tuning = tuning;
        self.lines = lines;
        self
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

    /// Whether this beat's inbox is a raid: [`RAID_FIRST_CONTACTS`] first contacts or more.
    pub fn is_raid(&self) -> bool {
        self.first.len() >= RAID_FIRST_CONTACTS
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

    /// Remove everything held about a viewer (they were removed from chat): their carried
    /// messages and the asks answered or owed to them.
    pub fn forget_viewer(&mut self, viewer_id: &str) {
        self.carried.retain(|k| k.cand.msg.viewer.id != viewer_id);
        self.asked.retain(|(v, _)| v != viewer_id);
        self.asked_at.retain(|(v, _), _| v != viewer_id);
    }

    /// Track a raid outside a scene: this beat's first contacts join the window's, and once
    /// [`RAID_FIRST_CONTACTS`] arrived inside [`RAID_WINDOW_MS`] (in one beat, or spread over a
    /// few) every one of them waits for the raid's greeting ([`Orchestrator::raid_greeting`]).
    fn track_raid(&mut self, now: TsMs, raid: bool) {
        if !self.first.is_empty() {
            let mut ids: Vec<String> = self.first.iter().cloned().collect();
            ids.sort();
            self.arrivals.push_back((now, ids));
        }
        while self
            .arrivals
            .front()
            .is_some_and(|(t, _)| now.saturating_sub(*t) > RAID_WINDOW_MS)
        {
            self.arrivals.pop_front();
        }
        let arrived: usize = self.arrivals.iter().map(|(_, ids)| ids.len()).sum();
        if raid || arrived >= RAID_FIRST_CONTACTS {
            for (t, ids) in self.arrivals.drain(..) {
                for id in ids {
                    self.raid_waiting.entry(id).or_insert(t);
                }
            }
        }
        self.raid_waiting
            .retain(|_, t| now.saturating_sub(*t) <= REQUEST_MAX_MS);
    }

    /// The arrival that greets a raid still waiting for its greeting, among `cands`: its largest
    /// wave (the crowd greeted once, with its count), else its oldest line that is not a lone
    /// emote or bait. The picker takes it ahead of the owed requests
    /// ([`crate::picker::PickContext::greet`]); `None` while a sincerely sad viewer is owed.
    pub fn raid_greeting(&self, cands: &[Candidate]) -> Option<String> {
        // A sincerely sad viewer is answered first, kindly; the raid is greeted after.
        if cands.iter().any(|c| {
            c.safety.is_none()
                && !self.answered.contains(&c.msg.id)
                && self.owns(c) == Some(Ask::Sad)
        }) {
            return None;
        }
        cands
            .iter()
            .filter(|c| {
                self.raid_waiting.contains_key(&c.msg.id)
                    && c.safety.is_none()
                    && !is_filler(c)
                    && !is_bait(c)
                    && !self.answered.contains(&c.msg.id)
            })
            .max_by_key(|c| {
                (
                    c.viewers.max(1),
                    std::cmp::Reverse(c.msg.ts_ms),
                    std::cmp::Reverse(c.msg.id.clone()),
                )
            })
            .map(|c| c.msg.id.clone())
    }

    /// The ask a candidate makes that is owned: one per viewer and kind, so a viewer asking the
    /// same thing again after the answer is plain chat.
    fn owns(&self, c: &Candidate) -> Option<Ask> {
        asks_of(c, self.is_first_contact(&c.msg.id))
            .into_iter()
            .find(|a| {
                let k = (c.msg.viewer.id.clone(), *a);
                !self.asked.contains(&k)
                    // A newcomer still asking what this is after the answer: it did not land.
                    || (*a == Ask::WhatIsThis
                        && self
                            .asked_at
                            .get(&k)
                            .is_some_and(|t| self.now_ms.saturating_sub(*t) >= WHAT_IS_THIS_REASK_MS))
            })
    }

    /// A pick whose answer never aired (the gate dropped the only line naming it): it is not
    /// answered, so it is carried again from now (an `input` record, [`CARRY_RULE`] /
    /// [`UNAIRED`]).
    pub fn unaired(&mut self, c: &Candidate, now_ms: TsMs, free_at: TsMs) -> StreamEvent {
        self.answered.remove(&c.msg.id);
        for a in asks_of(c, self.is_first_contact(&c.msg.id)) {
            self.asked.remove(&(c.msg.viewer.id.clone(), a));
        }
        self.carried.retain(|k| k.cand.msg.id != c.msg.id);
        self.carried.push(Carried {
            cand: c.clone(),
            since: now_ms,
            first: false,
            expires: free_at.max(now_ms).saturating_add(self.carry_window(c)),
            held: false,
            owned: self.owns(c),
            long: waits_long(c),
        });
        moderation::input_event(now_ms, CARRY_RULE, &c.msg.id, UNAIRED)
    }

    /// How long a message is carried from the drain: [`REQUEST_MAX_MS`] for a question or bait
    /// ([`waits_long`]), else [`CARRY_MS`].
    fn carry_window(&self, c: &Candidate) -> i64 {
        if waits_long(c) {
            self.tuning.request_max_ms
        } else {
            self.tuning.carry_ms
        }
    }

    /// This beat's question waves: representative id to its members (id, viewer name), the
    /// representative (the oldest) first.
    pub fn question_waves(&self) -> &HashMap<String, Vec<(String, String)>> {
        &self.waves
    }

    /// Group the questions that ask the same thing ([`QUESTION_WAVE_MS`]): the same situational
    /// ask (a sincere "are you an AI", "is the board real"), or words overlapping by
    /// [`QUESTION_WAVE_JACCARD`]. A wave is offered once, by its oldest member, and answering it
    /// answers every member (theatrical.sonnet.SEG-01.a30.s1: four "is he a real guy or ai" and
    /// four "is the hex board real" in 60 s, none answered).
    fn fold_question_waves(&mut self, cands: &[Candidate], now_ms: TsMs) {
        self.waves.clear();
        self.wave_of.clear();
        struct Q<'a> {
            c: &'a Candidate,
            since: TsMs,
            words: HashSet<String>,
            ask: Option<Ask>,
        }
        let mut qs: Vec<Q> = cands
            .iter()
            .filter(|c| {
                c.safety.is_none()
                    && !c.injection
                    && c.count == 1
                    && c.msg.command.is_none()
                    && !self.answered.contains(&c.msg.id)
            })
            .filter_map(|c| {
                let ask = situational_ask(&c.msg.text)
                    .filter(|a| matches!(a, Ask::AreYouAi | Ask::IsItReal));
                (ask.is_some() || is_question(&c.msg.text)).then(|| Q {
                    c,
                    since: self.carried_in.get(&c.msg.id).map_or(now_ms, |k| k.since),
                    words: word_set(&c.msg.text),
                    ask,
                })
            })
            .collect();
        qs.sort_by(|a, b| {
            a.since
                .cmp(&b.since)
                .then(a.c.msg.ts_ms.cmp(&b.c.msg.ts_ms))
                .then(a.c.msg.id.cmp(&b.c.msg.id))
        });
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for i in 0..qs.len() {
            let joined = groups.iter_mut().find(|g| {
                let rep = &qs[g[0]];
                let q = &qs[i];
                q.since.saturating_sub(rep.since) <= QUESTION_WAVE_MS
                    && ((q.ask.is_some() && q.ask == rep.ask)
                        || jaccard(&q.words, &rep.words) >= QUESTION_WAVE_JACCARD)
            });
            match joined {
                Some(g) => g.push(i),
                None => groups.push(vec![i]),
            }
        }
        for g in groups.into_iter().filter(|g| g.len() >= 2) {
            let rep = qs[g[0]].c.msg.id.clone();
            let members: Vec<(String, String)> = g
                .iter()
                .map(|&i| (qs[i].c.msg.id.clone(), qs[i].c.msg.viewer.name.clone()))
                .collect();
            for (id, _) in members.iter().skip(1) {
                self.wave_of.insert(id.clone(), rep.clone());
            }
            self.waves.insert(rep, members);
        }
    }

    /// The owned requests among this beat's candidates, the most urgent kind first (sincere
    /// sadness before the rest, [`Ask::priority`]), then oldest first (by when each first reached
    /// a beat): the picker takes one a beat ahead of bait and fresh chat.
    pub fn owed(&self, cands: &[Candidate], now_ms: TsMs) -> Vec<String> {
        let mut v: Vec<(u8, TsMs, &str)> = cands
            .iter()
            .filter(|c| !self.answered.contains(&c.msg.id))
            .filter_map(|c| {
                let ask = self.owns(c)?;
                let since = self.carried_in.get(&c.msg.id).map_or(now_ms, |k| k.since);
                Some((ask.priority(), since, c.msg.id.as_str()))
            })
            .collect();
        v.sort();
        v.into_iter().map(|(_, _, id)| id.to_string()).collect()
    }

    /// Note this beat's brigade calls (tagged at input: a call for chat to act on someone outside
    /// the stream, [`moderation::is_brigade_call`]), on every beat, a hold included: each is owed
    /// one refusal ([`Self::brigade_refusal`]) unless its viewer was refused already.
    pub fn note_brigade_calls(&mut self, calls: &[Candidate], now_ms: TsMs) {
        for c in calls {
            let id = &c.msg.id;
            if c.safety.is_some()
                || self.answered.contains(id)
                || self.brigade_refused.contains(&c.msg.viewer.id)
                || self.brigade_owed.iter().any(|(_, o)| &o.msg.id == id)
            {
                continue;
            }
            self.brigade_owed.push((now_ms, c.clone()));
        }
    }

    /// The refusal owed to the brigade calls noted ([`Self::note_brigade_calls`]) that no pick
    /// answered: the character's fixed line ([`LineBank::brigade_refused`]) addressed to their
    /// viewers by name, and their ids. A tagged call is bait and never owed a pick ([`asks_of`]):
    /// it waited for the off-topic slot, and in a hold the carry superseded it by its viewer's
    /// older bait (unhinged-roastmaster.sonnet.SEG-02A.a30.s1, tuning-e65e3ee0 L594/L597:
    /// tagged, deferred, never answered, and the judge failed `seg02a-raid-subject`;
    /// tuning-c01dab17 a300 L898/L904: tagged, superseded the same beat). Now each is owed one
    /// short engine line on the next voiced beat: it takes no pick and no beat of its own, so no
    /// sincere ask loses its place to it. A picked call is answered by the model (its hint says
    /// to refuse the target); either way its viewer is owed nothing more this stream. A refused
    /// call counts as answered and leaves the carry; one owed past [`REQUEST_MAX_MS`] lapses.
    pub fn brigade_refusal(
        &mut self,
        picked: &[Candidate],
        now_ms: TsMs,
    ) -> Option<(String, Vec<String>)> {
        let max = self.tuning.request_max_ms;
        let owed = std::mem::take(&mut self.brigade_owed);
        let mut ids: Vec<String> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for (at, c) in owed {
            if picked.iter().any(|p| p.msg.id == c.msg.id) {
                self.brigade_refused.insert(c.msg.viewer.id.clone());
                continue;
            }
            if self.answered.contains(&c.msg.id) || now_ms.saturating_sub(at) > max {
                continue;
            }
            if self.brigade_refused.insert(c.msg.viewer.id.clone()) {
                names.push(c.msg.viewer.name.clone());
            }
            ids.push(c.msg.id.clone());
        }
        if names.is_empty() {
            return None;
        }
        self.answered.extend(ids.iter().cloned());
        self.carried.retain(|k| !ids.contains(&k.cand.msg.id));
        let line = &self.lines.brigade_refused;
        let mut chars = line.chars();
        let line = match chars.next() {
            Some(f) => format!("{}{}", f.to_lowercase(), chars.as_str()),
            None => String::new(),
        };
        let who = match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => names[0].clone(),
        };
        Some((format!("{who}, {line}"), ids))
    }

    /// A background job changed state: research out opens a loop, a finished one lands.
    pub fn note_job(&mut self, job_id: &str, brief: &str, status: &str, summary: Option<&str>) {
        self.case.note_job(job_id, status, summary);
        match status {
            "queued" | "running" | "progress" => self.agenda.open(
                LoopKind::Research,
                job_id,
                &format!("research {job_id} ({})", one_line(brief, 60)),
            ),
            _ => self.agenda.close(LoopKind::Research, job_id),
        }
    }

    /// An op from a delivered beat: a case op moves the case ([`case`]).
    pub fn note_op(&mut self, op: &shadowrunr_types::Op, now_ms: TsMs) {
        self.case.note_op(op, now_ms);
    }

    /// Whether `viewer_id` already got an answer to this kind of ask this stream (before this
    /// beat's commit).
    pub fn has_asked(&self, viewer_id: &str, ask: Ask) -> bool {
        self.asked.contains(&(viewer_id.to_string(), ask))
    }

    /// Whether any sincere "are you an AI" was answered this stream (before this beat's commit).
    pub fn ai_answered(&self) -> bool {
        self.asked.iter().any(|(_, a)| *a == Ask::AreYouAi)
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
        self.now_ms = now_ms;
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
            // Nothing held (the cold open, a scene) expires; after it, an owned request gets
            // at least its fresh carry window.
            let expired = !k.held
                && match k.owned {
                    Some(_) => {
                        now_ms.saturating_sub(k.since) >= self.tuning.request_max_ms
                            && now_ms >= k.expires
                    }
                    None => now_ms >= k.expires,
                };
            if expired {
                ev.push(moderation::input_event(now_ms, CARRY_RULE, &id, "expired"));
                continue;
            }
            out.push(k.cand.clone());
            self.carried_in.insert(id, k);
        }
        self.fold_question_waves(&out, now_ms);
        out
    }

    /// Open a beat: what [`Self::rollback`] restores if its core call fails. Taken after
    /// [`Self::screen`] (its records are real either way) and before [`Self::plan`].
    pub fn begin(&self) -> BeatTxn {
        BeatTxn {
            before: Box::new(self.clone()),
        }
    }

    /// The beat's core call failed: nothing it planned or committed is consumed. The bit and
    /// story position, the connector, the room events, the waves, the answered ids and the
    /// through-line are as before the plan; the picks are carried again with their original
    /// `since` and `expires` (a fresh one from this beat, a safety pick included), beside what
    /// the beat deferred, so the next beat that speaks answers them or expires them once.
    pub fn rollback(&mut self, txn: BeatTxn, pick: &Pick, now_ms: TsMs) {
        let after = std::mem::replace(self, *txn.before);
        let mut carry = after.carried;
        for k in after.unsettled {
            if !carry.iter().any(|x| x.cand.msg.id == k.cand.msg.id) {
                carry.push(k);
            }
        }
        for c in pick.chosen.iter().filter(|c| c.safety.is_some()) {
            if carry.iter().any(|x| x.cand.msg.id == c.msg.id) {
                continue;
            }
            let k = self.carried_in.get(&c.msg.id).cloned().unwrap_or(Carried {
                cand: c.clone(),
                since: now_ms,
                first: self.first.contains(&c.msg.id),
                expires: now_ms.saturating_add(CARRY_MS),
                held: false,
                owned: None,
                long: false,
            });
            carry.push(k);
        }
        self.carried = carry;
    }

    /// The candidates this plan lets the picker see. A question wave is offered by its
    /// representative alone.
    pub fn offered(&self, plan: &Plan, cands: &[Candidate]) -> Vec<Candidate> {
        cands
            .iter()
            .filter(|c| {
                c.safety.is_some()
                    || !self.wave_of.contains_key(&c.msg.id)
                        && match plan.offer {
                            Offer::SafetyOnly => false,
                            // An owned request is answered in the cold open too.
                            Offer::FirstContact => {
                                (self.is_first_contact(&c.msg.id) && !is_vote(c))
                                    || self.owns(c).is_some()
                            }
                            Offer::Chat => !is_filler(c),
                            Offer::Care => {
                                !self.answered.contains(&c.msg.id) && self.owns(c) == Some(Ask::Sad)
                            }
                            Offer::Aside { raid } => {
                                !self.answered.contains(&c.msg.id)
                                    && (moderation::is_gift_or_sub(&c.msg.text)
                                        || (raid
                                            && self.is_first_contact(&c.msg.id)
                                            && !is_vote(c)))
                            }
                        }
                        && plan.only.as_ref().is_none_or(|ids| ids.contains(&c.msg.id))
            })
            .cloned()
            .collect()
    }

    /// Whether the room is small this beat ([`SMALL_ROOM_ENTER`]).
    pub fn small_room(&self) -> bool {
        self.small_room
    }

    /// Count this beat's chatters (fresh messages, not carried ones) and settle the room's size,
    /// with hysteresis: small at [`SMALL_ROOM_ENTER`] or fewer, not small from
    /// [`SMALL_ROOM_LEAVE`].
    fn note_chatters(&mut self, cands: &[Candidate], now: TsMs) {
        for c in cands
            .iter()
            .filter(|c| !self.carried_in.contains_key(&c.msg.id))
        {
            self.chatters.insert(c.msg.viewer.id.clone(), now);
        }
        self.chatters
            .retain(|_, t| now.saturating_sub(*t) < SMALL_ROOM_WINDOW_MS);
        let n = self.chatters.len();
        if self.small_room {
            if n >= SMALL_ROOM_LEAVE {
                self.small_room = false;
            }
        } else if n <= SMALL_ROOM_ENTER {
            self.small_room = true;
        }
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
    /// connector used) is staged here: it stands once the beat is delivered, and a failed core
    /// call rolls it back ([`Self::begin`], [`Self::rollback`]). Then the
    /// through-line: the open loops are brought up to date, and a voiced beat outside the cold
    /// open, safety, a room event and the goodbye gets its pick-up and lead-in ([`agenda`]).
    pub fn plan(&mut self, b: &BeatInputs<'_>) -> Plan {
        self.now_ms = b.now_ms;
        self.case_next = self.case.next_move(b.ctx, b.now_ms);
        let case_loop: Vec<(String, String)> = self
            .case_next
            .iter()
            .map(|m| (m.tag(), format!("the case (next: {})", m.tag())))
            .collect();
        self.agenda.sync(LoopKind::Case, &case_loop);
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
            || p.mode == BeatMode::Scene
            || safety
            || b.closing
            || b.ctx.state == COLD_OPEN_STATE
            || p.room_note.is_some()
            // An owed request takes the beat: no pick-up or lead-in competes with its answer
            // (unhinged-roastmaster.sonnet.SEG-01.a300.s1 L1072-L1079: a Warlock's !propose
            // beside "pick-up: landed" got a line about the landed result instead).
            || p.owed
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
            // A beat passing chat by answers none, but its room is not quiet.
            quiet: p.ceiling == 0 && !b.cands.iter().any(|c| c.safety.is_none() && !is_filler(c)),
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
        self.note_chatters(b.cands, now);
        let seg_changed = self.last_segment.is_some_and(|s| s != b.ctx.segment);
        self.last_segment = Some(b.ctx.segment);
        let prev_state = self.last_state.replace(b.ctx.state.clone());
        let state_changed = prev_state.as_deref().is_some_and(|s| s != b.ctx.state);
        let safety = b.cands.iter().any(|c| c.safety.is_some());
        let raid = self.is_raid();
        let owed = b
            .cands
            .iter()
            .any(|c| !self.answered.contains(&c.msg.id) && self.owns(c).is_some());
        let seg = Seg {
            names: b.names,
            topic: b.topic_words,
            small: self.small_room,
        };

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
        // A raid during a scene is owed an aside, even when it lands in a speech hold; once
        // the scene is over the room is greeted as usual.
        let in_scene = b.scene.is_some();
        // A raid spread over a few beats counts too: its first contacts inside
        // RAID_WINDOW_MS (sbx-20261024-203548-10m: 3, 3 and 1 a beat, never 5 in one).
        if !in_scene {
            self.scene_raid = None;
            self.scene_arrivals.clear();
        } else {
            if !self.first.is_empty() {
                self.scene_arrivals.push_back((now, self.first.len()));
            }
            while self
                .scene_arrivals
                .front()
                .is_some_and(|(t, _)| now.saturating_sub(*t) > RAID_WINDOW_MS)
            {
                self.scene_arrivals.pop_front();
            }
            let arrived: usize = self.scene_arrivals.iter().map(|(_, n)| n).sum();
            if raid || arrived >= RAID_FIRST_CONTACTS {
                self.scene_raid = Some(self.scene_raid.unwrap_or(0).max(arrived));
                self.scene_arrivals.clear();
            }
        }

        if !in_scene {
            self.track_raid(now, raid);
        }

        self.room.observe(&b.ctx.state, &b.ctx.state_summary);
        let free_at = b.speech_until_ms.map_or(now, |u| u.max(now));
        // In the cold open and in a scene what is carried is held; after either, its clock
        // starts from this beat's drain.
        let cold = b.ctx.state == COLD_OPEN_STATE;
        let held = cold || in_scene;
        self.carry_next = b
            .cands
            .iter()
            .filter(|c| !self.answered.contains(&c.msg.id))
            .filter_map(|c| {
                let first = self.is_first_contact(&c.msg.id);
                // A question or bait waits REQUEST_MAX_MS from the drain, the rest CARRY_MS.
                let from_drain = free_at.saturating_add(self.carry_window(c));
                let (since, expires) = match self.carried_in.get(&c.msg.id) {
                    Some(k) if k.held && !held => (k.since, from_drain),
                    Some(k) => (k.since, k.expires),
                    None => (now, from_drain),
                };
                carries(c, first, seg, self.owns(c).is_some()).then(|| Carried {
                    cand: c.clone(),
                    since,
                    first,
                    expires,
                    held,
                    owned: self.owns(c),
                    long: waits_long(c),
                })
            })
            .collect();
        // The worthy chat, classed against the segment (safety picks apart). A question wave
        // counts once, by its representative.
        let classed: Vec<(&Candidate, &'static str, Class)> = b
            .cands
            .iter()
            .filter(|c| !self.answered.contains(&c.msg.id) && !self.wave_of.contains_key(&c.msg.id))
            .filter_map(|c| {
                worth(
                    c,
                    self.is_first_contact(&c.msg.id),
                    seg,
                    self.owns(c).is_some(),
                )
                .map(|(t, k)| (c, t, k))
            })
            .collect();
        // Chat waiting from an earlier beat (deferred, carried in), by id.
        let waiting_ids: HashSet<String> = self.carried_in.keys().cloned().collect();
        let waiting = |c: &Candidate| waiting_ids.contains(&c.msg.id);
        self.beat_worthy = classed
            .iter()
            .filter(|(c, _, _)| c.safety.is_none())
            .map(|(c, _, _)| c.msg.id.clone())
            .collect();
        let of = |k: Class| -> Vec<(&Candidate, &'static str)> {
            classed
                .iter()
                .filter(|(c, _, kk)| *kk == k && c.safety.is_none())
                .map(|(c, t, _)| (*c, *t))
                .collect()
        };
        let required = of(Class::Required);
        let on_topic = of(Class::OnTopic);
        let off_topic = of(Class::OffTopic);
        let fold = !required.is_empty() || !on_topic.is_empty();
        // Off-topic chat is not the segment's: it never resets the quiet a bit waits for.
        if fold || safety || self.quiet_since.is_none() {
            self.quiet_since = Some(now);
        }
        let quiet_ms = now.saturating_sub(self.quiet_since.unwrap_or(now));
        // Chat this beat passes by (no pick takes it): what a segment beat's reason counts.
        let passing = b
            .cands
            .iter()
            .filter(|c| {
                c.safety.is_none()
                    && !is_filler(c)
                    && !self.answered.contains(&c.msg.id)
                    && !self.wave_of.contains_key(&c.msg.id)
            })
            .count();
        // The off-topic slot's candidates: a direct question, mention or bait. It takes the one
        // waiting longest, a question or bait before a mention, so carried bait and questions
        // are answered inside their carry instead of expiring behind fresher chat.
        let direct_off: Vec<(&Candidate, &'static str)> = {
            let all: Vec<(&Candidate, &'static str)> = off_topic
                .iter()
                .filter(|(_, t)| matches!(*t, "mention" | "question" | "mockable" | "repeat"))
                .copied()
                .collect();
            let since = |c: &Candidate| self.carried_in.get(&c.msg.id).map_or(now, |k| k.since);
            let oldest = all
                .iter()
                .filter(|(c, _)| waiting(c))
                .min_by_key(|(c, t)| {
                    (
                        matches!(*t, "mention" | "repeat"),
                        since(c),
                        c.msg.ts_ms,
                        c.msg.id.clone(),
                    )
                })
                .copied();
            match oldest {
                Some(x) => vec![x],
                None => all,
            }
        };
        // Required or on-topic chat waiting from an earlier beat: the beat folds it in before any
        // new prepared material, and a story being told waits one beat.
        let fold_waiting = required.iter().chain(&on_topic).any(|(c, _)| waiting(c));
        let off_open = self
            .last_off_topic_ms
            .is_none_or(|t| now.saturating_sub(t) >= OFF_TOPIC_ANSWER_MS);
        let off_answer = !fold && !direct_off.is_empty() && off_open;
        let class_offer = |xs: &[(&Candidate, &'static str)], off_topic: bool| {
            let mut tags: Vec<&str> = Vec::new();
            for (_, t) in xs {
                if !tags.contains(t) {
                    tags.push(t);
                }
            }
            ChatOffer {
                ceiling: 1,
                only: Some(xs.iter().map(|(c, _)| c.msg.id.clone()).collect()),
                off_topic,
                signals: tags.join("+"),
                small: seg.small,
                owed: false,
            }
        };
        let offer = if !required.is_empty() {
            ChatOffer {
                owed: required.iter().any(|(c, _)| self.owns(c).is_some()),
                ..class_offer(&required, false)
            }
        } else if !on_topic.is_empty() {
            class_offer(&on_topic, false)
        } else if off_answer {
            class_offer(&direct_off, true)
        } else {
            ChatOffer {
                small: seg.small,
                ..ChatOffer::default()
            }
        };
        let gap_ms = b.speech_until_ms.map(|t| now.saturating_sub(t).max(0));
        // The goodbye beat always speaks: a quiet hold there would cost the sign-off.
        let may_hold = gap_ms.is_some_and(|g| g < self.tuning.hold_gap_ms)
            && self.last_mode != Some(BeatMode::Hold)
            && !b.closing;

        // Speech still queued: hold before anything is decided or consumed. A safety beat and
        // a closing beat run at once; an owned request waits only for REQUEST_LEAD_MS.
        let queued = free_at.saturating_sub(now);
        let hold_lead = if owed {
            b.speech_lead_ms.max(self.tuning.request_lead_ms)
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
        let chat = offer.ceiling;
        let signals = offer.signals.clone();

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

        // A scene being performed takes the beat (chat is carried); one still to start waits
        // for the room event that put it up (a ballot's result, its tie) to be announced.
        if let Some((id, starting)) = b.scene {
            if !(starting && self.room.has_pending()) {
                if let Some(p) = self.scene_break(b, id, &lead) {
                    return p;
                }
                let what = if starting { "starts" } else { "goes on" };
                return Plan::new(
                    BeatMode::Scene,
                    Offer::SafetyOnly,
                    0,
                    format!("{lead}scene {id} {what}"),
                );
            }
        }

        // A closed vote or a handover due: announced now, once, over the queued bit. A cue
        // waits while a sincerely sad viewer is owed an answer: no showmanship beside it.
        let sad = b
            .cands
            .iter()
            .any(|c| !self.answered.contains(&c.msg.id) && self.owns(c) == Some(Ask::Sad));
        let events = self.room.take(!sad);
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
            // A cue keeps room for itself: at most CUE_PICKS beside it.
            let ceiling = if events.iter().any(|e| e.cue_state.is_some()) {
                chat.min(room::CUE_PICKS)
            } else {
                chat
            };
            let mut p = Plan::new(
                mode,
                Offer::Chat,
                ceiling,
                format!("{lead}announce {labels}{with}"),
            )
            .with_chat(&offer);
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
            p.lead_cue = events.iter().any(|e| e.cue_state.is_some());
            if let Some((k, line)) = connector {
                p.material = Some(format!(
                    "Connector ({}):\n- {}\n",
                    connector_label(k),
                    one_line(&line, 400)
                ));
            }
            return p;
        }

        // The case's next move, ahead of chat, unless an owned request takes the beat.
        if let Some(mv) = self
            .case_next
            .clone()
            .filter(|m| !owed && !b.closing && self.case.due(m, now))
        {
            let face_up = b.ctx.hexes.iter().filter(|h| !h.fog).count();
            let names = crate::case_names::CaseNames::read(b.ctx);
            let directive = self.case.ask(&mv, face_up, &names, now);
            let (mode, offer_kind, ceiling, with) = if chat > 0 {
                (
                    BeatMode::React,
                    Offer::Chat,
                    1,
                    format!(" with chat ({signals})"),
                )
            } else {
                (BeatMode::Improvise, Offer::SafetyOnly, 0, String::new())
            };
            let mut p = Plan::new(
                mode,
                offer_kind,
                ceiling,
                format!("{lead}case: {}{with}", mv.tag()),
            )
            .with_chat(&offer);
            p.case = Some(if ceiling == 0 {
                directive
            } else {
                // The one pick still gets its own line (a gift once got only a highlight beside
                // the case line, sbx-20261022-110349-10m L351).
                format!("{directive}\n{CASE_WITH_CHAT}")
            });
            if ceiling == 0 {
                p.note = Some(CASE_NOTE.into());
            }
            return p;
        }

        // A queued connector, with the beat's one pick when it has one.
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
                )
                .with_chat(&offer);
                p.material = Some(format!(
                    "Connector ({}):\n- {}\n",
                    connector_label(kind),
                    one_line(&line, 400)
                ));
                return p;
            }
        }

        // What a beat with no pick says it passed by: the chat it carries, else the quiet.
        let quiet_s = quiet_ms / 1000;
        let label = if passing > 0 {
            format!("segment (carried {passing})")
        } else {
            format!("quiet {quiet_s} s")
        };

        // A bit being told goes on. Required or on-topic chat waiting from an earlier beat goes
        // first: the story's next line waits one beat (never two in a row) while the beat folds
        // the chat in. Fresh required chat skips its next line; on-topic chat (or the off-topic
        // answer) folds into its beat as the one pick, beside one line of it.
        if let Some(active) = self.active_bit.clone() {
            let bit = b
                .prep
                .and_then(|p| p.bits.iter().find(|x| x.id == active.id));
            match bit {
                None => self.active_bit = None,
                Some(bit) if fold_waiting && !self.bit_waited => {
                    self.bit_waited = true;
                    lead.push_str(&format!(
                        "{} {} waits a beat for chat; ",
                        kind_label(bit.kind),
                        bit.id
                    ));
                }
                Some(_) if !required.is_empty() && !fold_waiting => {
                    if let Some(why) = self.back_off(b.prep) {
                        lead.push_str(&why);
                    }
                }
                Some(bit) => {
                    self.bit_waited = false;
                    let lines = if chat > 0 { 1 } else { 2 };
                    let (m, what) = self.bit_lines(bit, active.next, lines);
                    let mut p = if chat > 0 {
                        Plan::new(
                            BeatMode::Mixed,
                            Offer::Chat,
                            1,
                            format!("{lead}{}: {what}", chat_head(&offer)),
                        )
                        .with_chat(&offer)
                    } else {
                        Plan::new(
                            BeatMode::Bit,
                            Offer::SafetyOnly,
                            0,
                            format!("{lead}{label}: {what}"),
                        )
                    };
                    p.bit_id = Some(bit.id.clone());
                    p.material = Some(m);
                    self.last_bit_ms = Some(now);
                    return p;
                }
            }
        }

        if chat > 0 {
            let after_bit = self.bit_ended;
            // The off-topic answer never bridges back to chat: it answers, then the segment goes on.
            if after_bit
                && !offer.off_topic
                && matches!(self.last_voiced, Some(BeatMode::Bit | BeatMode::Mixed))
            {
                if let Some(line) = self.connector(b.prep, ConnectorKind::BackToChat) {
                    self.bit_ended = false;
                    let mut p = Plan::new(
                        BeatMode::Bridge,
                        Offer::Chat,
                        1,
                        format!("{lead}bridge back_to_chat after a bit ({signals})"),
                    )
                    .with_chat(&offer);
                    p.material = Some(format!(
                        "Connector (back_to_chat):\n- {}\n",
                        one_line(&line, 400)
                    ));
                    return p;
                }
            }
            return self.fold(b, lead, &offer, now, fold_waiting);
        }

        // No pick this beat: quiet, or chat the segment passes by. A beat that passes chat by
        // never holds: the segment goes on.
        let may_hold = may_hold && passing == 0;
        if let Some(prep) = b.prep {
            let spaced = self
                .last_bit_ms
                .is_none_or(|t| now.saturating_sub(t) >= self.tuning.bit_spacing_ms);
            if quiet_ms >= self.tuning.quiet_bit_ms && spaced && self.active_bit.is_none() {
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
                                format!("{lead}{label}: bridge into_bit, {what}"),
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
                                format!("{lead}{label}: {what}"),
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
        // then a bit off its spacing, and only then an improvised line (on the state's purpose
        // when it has one).
        if let Some((note, what)) = self.agenda.bridge(now) {
            let mut p = Plan::new(
                BeatMode::Improvise,
                Offer::SafetyOnly,
                0,
                format!("{lead}{label}: bridged from the {what}"),
            );
            p.note = Some(format!(
                "(No chat to answer this beat.) {note}Never beg, never complain about the \
                 viewer count."
            ));
            return p;
        }
        if let Some(prep) = b.prep {
            if quiet_ms >= self.tuning.quiet_bit_ms && self.active_bit.is_none() {
                if let Some(bit) = self.next_bit(prep, b.ctx.segment, false) {
                    let (m, what) = self.bit_lines(bit, 0, 2);
                    self.last_bit_ms = Some(now);
                    let mut p = Plan::new(
                        BeatMode::Bit,
                        Offer::SafetyOnly,
                        0,
                        format!("{lead}{label}, nothing else open: {what}"),
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
        let mut p = Plan::new(
            BeatMode::Improvise,
            Offer::SafetyOnly,
            0,
            format!("{lead}{label}, {why}: improvised"),
        );
        if let Some(purpose) = crate::topic::state_purpose(b.ctx) {
            p.segment_note = Some(PURPOSE_NOTE.replace("{purpose}", &one_line(&purpose, 300)));
        }
        p
    }

    /// A beat that folds one pick into the segment: the next prepared bit off its spacing
    /// (premises first: they invite the room; never while chat `waiting` from an earlier beat goes
    /// in), the through-line's bridge, or the state's
    /// purpose, beside the pick (`mixed`); with no segment material, a `react` told to tie its
    /// reply back to the segment.
    fn fold(
        &mut self,
        b: &BeatInputs<'_>,
        lead: String,
        o: &ChatOffer,
        now: TsMs,
        waiting: bool,
    ) -> Plan {
        let head = format!("{lead}{}", chat_head(o));
        // An owed request takes the beat: no material competes with its answer, which still ties
        // back to the segment.
        if o.owed {
            let mut p = Plan::new(
                BeatMode::React,
                Offer::Chat,
                1,
                format!("{head}: the request takes the beat"),
            )
            .with_chat(o);
            p.segment_note = Some(TIE_BACK_NOTE.into());
            return p;
        }
        let spaced = self
            .last_bit_ms
            .is_none_or(|t| now.saturating_sub(t) >= self.tuning.bit_spacing_ms);
        // Chat waiting from an earlier beat goes in before any new prepared material.
        if self.active_bit.is_none() && spaced && !waiting {
            if let Some(bit) = b.prep.and_then(|p| self.next_bit(p, b.ctx.segment, true)) {
                let (m, what) = self.bit_lines(bit, 0, 1);
                let mut p = Plan::new(BeatMode::Mixed, Offer::Chat, 1, format!("{head}: {what}"))
                    .with_chat(o);
                p.bit_id = Some(bit.id.clone());
                p.material = Some(m);
                self.last_bit_ms = Some(now);
                return p;
            }
        }
        if let Some((note, what)) = self.agenda.bridge_with(now, true) {
            let mut p = Plan::new(
                BeatMode::Mixed,
                Offer::Chat,
                1,
                format!("{head}: bridged from the {what}"),
            )
            .with_chat(o);
            p.segment_note = Some(note.trim_end().to_string());
            return p;
        }
        if let Some(purpose) = crate::topic::state_purpose(b.ctx) {
            let mut p = Plan::new(
                BeatMode::Mixed,
                Offer::Chat,
                1,
                format!("{head}: the state's purpose"),
            )
            .with_chat(o);
            p.segment_note = Some(PURPOSE_NOTE.replace("{purpose}", &one_line(&purpose, 300)));
            return p;
        }
        let mut p = Plan::new(
            BeatMode::React,
            Offer::Chat,
            1,
            format!("{head}: no segment material, tied back"),
        )
        .with_chat(o);
        p.segment_note = Some(TIE_BACK_NOTE.into());
        p
    }

    /// What interrupts the scene this beat, at a line boundary: a care beat for a sincerely sad
    /// viewer, else one short aside for a raid or a gift, spaced [`SCENE_ASIDE_SPACING_MS`].
    /// `None`: the scene goes on.
    fn scene_break(&mut self, b: &BeatInputs<'_>, id: &str, lead: &str) -> Option<Plan> {
        let now = b.now_ms;
        let open = |c: &&Candidate| c.safety.is_none() && !self.answered.contains(&c.msg.id);
        if b.cands
            .iter()
            .filter(open)
            .any(|c| self.owns(c) == Some(Ask::Sad))
        {
            let mut p = Plan::new(
                BeatMode::React,
                Offer::Care,
                1,
                format!(
                    "{lead}scene {id} paused for a care beat (sincere sadness); it resumes after"
                ),
            );
            p.host_only = true;
            return Some(p);
        }
        let gifts = b
            .cands
            .iter()
            .filter(open)
            .filter(|c| moderation::is_gift_or_sub(&c.msg.text))
            .count();
        let raid = self.scene_raid;
        let spaced = self
            .last_aside_ms
            .is_none_or(|t| now.saturating_sub(t) >= SCENE_ASIDE_SPACING_MS);
        if (gifts == 0 && raid.is_none()) || !spaced {
            return None;
        }
        self.scene_raid = None;
        self.last_aside_ms = Some(now);
        let mut what: Vec<String> = Vec::new();
        if let Some(n) = raid {
            what.push(format!("a raid: {n} new viewers arrived at once"));
        }
        if gifts > 0 {
            what.push("a gift or a sub (in chat below)".into());
        }
        let what = what.join(", and ");
        let ceiling = usize::from(raid.is_some()) + usize::from(gifts > 0);
        let mut p = Plan::new(
            BeatMode::React,
            Offer::Aside {
                raid: raid.is_some(),
            },
            ceiling,
            format!("{lead}scene {id} paused for one aside ({what}); it resumes next beat"),
        );
        p.room_note = Some(format!(
            "SCENE PAUSED FOR ONE ASIDE: mid-scene, {what}. Break off for one short line in your \
             own voice that acknowledges it (thank whoever gave by name; welcome the raiders), \
             then stop: the scene resumes by itself next beat. No guest line, and nothing more \
             of the scene."
        ));
        p.note = Some(ANNOUNCE_NOTE.into());
        p.host_only = true;
        p.short = true;
        p.aside = Some(raid.is_some());
        Some(p)
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
            plan.mode = if plan.bit_id.is_some() {
                BeatMode::Bit
            } else {
                BeatMode::Improvise
            };
        }
        if plan.off_topic && pick.chosen.iter().any(|c| c.safety.is_none()) {
            self.last_off_topic_ms = Some(now_ms);
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
        // A question wave answered once answers every member.
        let staged_ids: HashMap<String, Candidate> = self
            .carry_next
            .iter()
            .map(|k| (k.cand.msg.id.clone(), k.cand.clone()))
            .collect();
        let mut members: Vec<String> = Vec::new();
        for c in &pick.chosen {
            if let Some(w) = self.waves.get(&c.msg.id) {
                let viewers: HashSet<&str> = w.iter().map(|(_, n)| n.as_str()).collect();
                plan.reason.push_str(&format!(
                    "; question wave of {} ({} viewers) answered once",
                    w.len(),
                    viewers.len()
                ));
                members.extend(w.iter().skip(1).map(|(id, _)| id.clone()));
            }
        }
        for id in &members {
            if let Some(c) = staged_ids.get(id) {
                for a in asks_of(c, self.is_first_contact(id)) {
                    self.asked.insert((c.msg.viewer.id.clone(), a));
                    self.asked_at.insert((c.msg.viewer.id.clone(), a), now_ms);
                }
            }
            self.agenda.close(LoopKind::Request, id);
        }
        let mut chosen: HashSet<&str> = pick.chosen.iter().map(|c| c.msg.id.as_str()).collect();
        chosen.extend(members.iter().map(String::as_str));
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
        if chosen.iter().any(|id| self.raid_waiting.contains_key(*id)) {
            self.raid_waiting.clear();
        }
        self.answered.extend(chosen.iter().map(|s| s.to_string()));
        for id in &chosen {
            self.agenda.close(LoopKind::Request, id);
        }
        for c in &pick.chosen {
            for a in asks_of(c, self.is_first_contact(&c.msg.id)) {
                self.asked.insert((c.msg.viewer.id.clone(), a));
                self.asked_at.insert((c.msg.viewer.id.clone(), a), now_ms);
            }
        }
        let staged = std::mem::take(&mut self.carry_next);
        self.unsettled = staged
            .iter()
            .filter(|k| chosen.contains(k.cand.msg.id.as_str()))
            .cloned()
            .collect();
        let mut carry: Vec<Carried> = staged
            .into_iter()
            .filter(|k| !chosen.contains(k.cand.msg.id.as_str()) && (k.held || now_ms < k.expires))
            .collect();
        // One carried message a viewer besides an owned request, a first contact and a gift: their
        // oldest question or bait, else their newest. A repeater cannot fill the carry.
        let deduped = |k: &Carried| {
            k.owned.is_none() && !k.first && !moderation::is_gift_or_sub(&k.cand.msg.text)
        };
        let mut keep_of: HashMap<String, (bool, TsMs, String)> = HashMap::new();
        for k in carry.iter().filter(|k| deduped(k)) {
            let score = (k.long, if k.long { -k.since } else { k.since });
            let e = keep_of.entry(k.cand.msg.viewer.id.clone()).or_insert((
                k.long,
                if k.long { -k.since } else { k.since },
                k.cand.msg.id.clone(),
            ));
            if score > (e.0, e.1) {
                *e = (score.0, score.1, k.cand.msg.id.clone());
            }
        }
        self.superseded = carry
            .iter()
            .filter(|k| deduped(k))
            .filter(|k| keep_of[&k.cand.msg.viewer.id].2 != k.cand.msg.id)
            .map(|k| k.cand.msg.id.clone())
            .collect();
        carry.retain(|k| !self.superseded.contains(&k.cand.msg.id));
        // Owned requests go first and are never capped. While held (the cold open, a scene),
        // first contacts and gifts come next, the oldest first, then the newest of the rest;
        // otherwise questions and bait, the oldest first, then the rest, the newest first.
        if carry.iter().any(|k| k.held) {
            carry.sort_by_key(|k| {
                let owed = k.first || moderation::is_gift_or_sub(&k.cand.msg.text);
                (
                    k.owned.is_none(),
                    !owed,
                    if owed { k.since } else { -k.since },
                )
            });
        } else {
            carry.sort_by_key(|k| {
                (
                    k.owned.is_none(),
                    !k.long,
                    if k.long { k.since } else { -k.since },
                )
            });
        }
        // Owned requests take no slot of the cap: CARRY_MAX bounds the rest (a viewer's repeated
        // ask once took the slots of everyone else's questions and bait).
        let keep = self.tuning.carry_max + carry.iter().filter(|k| k.owned.is_some()).count();
        self.capped = carry
            .iter()
            .skip(keep)
            .map(|k| k.cand.msg.id.clone())
            .collect();
        carry.truncate(keep);
        // Worth answering, not picked, not carried: recorded, never silently gone.
        self.dropped = std::mem::take(&mut self.beat_worthy)
            .into_iter()
            .filter(|id| {
                !chosen.contains(id.as_str())
                    && !self.capped.contains(id)
                    && !self.superseded.contains(id)
                    && !carry.iter().any(|k| &k.cand.msg.id == id)
            })
            .collect();
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
        if plan.bit_id.is_none()
            && matches!(
                plan.mode,
                BeatMode::React | BeatMode::Bridge | BeatMode::Mixed
            )
        {
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
    /// ([`CARRY_RULE`] / [`CAPPED`]) and of those it neither picked nor carried ([`DROPPED`]),
    /// taken once: every drop leaves a record.
    pub fn take_capped(&mut self, now_ms: TsMs) -> Vec<StreamEvent> {
        let capped = std::mem::take(&mut self.capped)
            .into_iter()
            .map(|id| (id, CAPPED));
        let dropped = std::mem::take(&mut self.dropped)
            .into_iter()
            .map(|id| (id, DROPPED));
        let superseded = std::mem::take(&mut self.superseded)
            .into_iter()
            .map(|id| (id, SUPERSEDED));
        capped
            .chain(superseded)
            .chain(dropped)
            .map(|(id, action)| moderation::input_event(now_ms, CARRY_RULE, &id, action))
            .collect()
    }

    /// The in-character stand-in for a line the output gate recast: a rebuttal of a forged
    /// decree when the beat answered an injection or an authority claim, else a quiet-room
    /// line. Either rotation never repeats one of its recent choices.
    pub fn recast(&mut self, authority: bool, original: &str) -> String {
        if authority {
            self.rebuttals
                .choose(&self.lines.rebuttals, original)
                .to_string()
        } else {
            self.in_character
                .choose(&self.lines.recast, original)
                .to_string()
        }
    }
}
