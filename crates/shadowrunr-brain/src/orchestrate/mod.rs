//! The orchestrator: one decision per beat, between the picker and the composer, on what the
//! beat is for ([`BeatMode`]) and so which chat the picker may pass and which prepared material
//! the composer voices. Every beat that reaches it writes exactly one `beat_plan` event, before
//! the beat's lines, with a reason a reviewer can read.
//!
//! - **Cold open** (`ctx.state == "cold_open"`): the prep's story, one or two lines a beat, in
//!   order; then its handoff. Only first contacts and safety reach the voice (at most one first
//!   contact a beat). Without a prep: one short "the room is gathering" open with no premise.
//!   Once the story is told he holds or greets first contacts until the state changes.
//! - **After it**, per beat: `react` when chat is worth answering; `bit` when the room is quiet
//!   (a prepared bit for the current segment, shuffled, never repeated in the stream); `bridge`
//!   for a connector line between the two (into a bit, back to chat, into a poll or a handover);
//!   `mixed` (a short bit line and one pick) after a run of reacts in a thin room; `hold` when
//!   there is nothing worth saying and the last line is recent. Without a prep there is no bit,
//!   bridge, mixed or hold: a quiet beat is improvised as before.
//!
//! Rules: a bit is offered once and marked used; the room answering a bit skips its next line;
//! a raid, a safety pick or a segment change expires the queued bit and connector; a safety beat
//! never voices prepared material. Votes and lone emotes never fill a pick, and a wave once
//! answered is not picked again this stream.
//!
//! The thresholds are assumptions to tune against sandbox runs, not measurements:
//! [`QUIET_BIT_MS`], [`BIT_SPACING_MS`], [`HOLD_GAP_MS`], [`THIN_REACT_STREAK`],
//! [`THIN_ROOM_MAX`], [`RAID_FIRST_CONTACTS`].

pub mod requests;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use shadowrunr_types::prep::{BeatMode, BitKind, ConnectorKind, PrepBit, StreamPrep};
use shadowrunr_types::{ChatMsg, SegmentContext, StreamEvent, TsMs};

use crate::moderation::{self, Candidate, IN_CHARACTER_LINES};
use crate::picker::{is_bare_emote, is_question, is_vote, mentions, Pick};
use crate::util::{fnv1a64, normalize, one_line, similarity};

use requests::{Rotation, REBUTTALS};

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

/// The beat note for an open with no prepared story, and for gathering lines after one.
pub const GATHERING_NOTE: &str = "(The room is still gathering. One short in-character line \
that welcomes whoever is arriving: no premise, no poll, no call to action, no count.)";

/// What one beat needs from the brain.
pub struct BeatInputs<'a> {
    pub now_ms: TsMs,
    pub ctx: &'a SegmentContext,
    /// The beat's candidates after [`Orchestrator::screen`].
    pub cands: &'a [Candidate],
    pub prep: Option<&'a StreamPrep>,
    /// When Malakor last spoke.
    pub last_say_ms: Option<TsMs>,
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
            reason,
        }
    }
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
}

/// Why a candidate is worth answering, if it is.
fn worth(c: &Candidate, first: bool, names: &[String]) -> Option<&'static str> {
    if c.safety.is_some() {
        return Some("safety");
    }
    if is_filler(c) {
        return None;
    }
    if first {
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

    /// Whether a message was its viewer's first this stream.
    pub fn is_first_contact(&self, msg_id: &str) -> bool {
        self.first.contains(msg_id)
    }

    /// Drop repeats of a wave already answered (an `input` record, `wave_repeat` / `dropped`)
    /// and fold near-duplicate pastes into one candidate, so several pasters are never picked.
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
        out.into_iter().map(|(_, c)| c).collect()
    }

    /// The candidates this plan lets the picker see.
    pub fn offered(&self, plan: &Plan, cands: &[Candidate]) -> Vec<Candidate> {
        cands
            .iter()
            .filter(|c| {
                c.safety.is_some()
                    || match plan.offer {
                        Offer::SafetyOnly => false,
                        Offer::FirstContact => self.first.contains(&c.msg.id) && !is_vote(c),
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

    /// Decide this beat. State that a decision consumes (a bit offered, a story line voiced, a
    /// connector used) is consumed here: the beat goes ahead whatever the engine does.
    pub fn plan(&mut self, b: &BeatInputs<'_>) -> Plan {
        self.beats += 1;
        let now = b.now_ms;
        let seg_changed = self.last_segment.is_some_and(|s| s != b.ctx.segment);
        self.last_segment = Some(b.ctx.segment);
        let prev_state = self.last_state.replace(b.ctx.state.clone());
        let state_changed = prev_state.as_deref().is_some_and(|s| s != b.ctx.state);
        let safety = b.cands.iter().any(|c| c.safety.is_some());
        let raid = self.first.len() >= RAID_FIRST_CONTACTS;

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

        let worthy: Vec<&'static str> = b
            .cands
            .iter()
            .filter_map(|c| worth(c, self.first.contains(&c.msg.id), b.names))
            .collect();
        if !worthy.is_empty() || self.quiet_since.is_none() {
            self.quiet_since = Some(now);
        }
        let quiet_ms = now.saturating_sub(self.quiet_since.unwrap_or(now));
        let gap_ms = b.last_say_ms.map(|t| now.saturating_sub(t));
        let may_hold =
            gap_ms.is_some_and(|g| g < HOLD_GAP_MS) && self.last_mode != Some(BeatMode::Hold);

        if b.ctx.state == COLD_OPEN_STATE {
            let mut p = self.plan_cold_open(b, safety, may_hold);
            p.reason = format!("{lead}{}", p.reason);
            return p;
        }

        // A state change queues its connector: the story cut short by a full room, a poll, a
        // handover.
        if state_changed && !safety {
            let st = b.ctx.state.as_str();
            if prev_state.as_deref() == Some(COLD_OPEN_STATE) && !self.handoff_done {
                self.pending = Some(ConnectorKind::IntoRoom);
            } else if st.contains("poll") || st.contains("vote") {
                self.pending = Some(ConnectorKind::IntoPoll);
            } else if st.contains("handover") {
                self.pending = Some(ConnectorKind::IntoHandover);
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

        if safety {
            return Plan::new(
                BeatMode::React,
                Offer::Chat,
                max,
                format!("{lead}safety beat: prepared material held back"),
            );
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
                Some(bit) if !worthy.is_empty() => {
                    let next = active.next + 1;
                    lead.push_str(&format!(
                        "room answered {} {}: line {} skipped; ",
                        kind_label(bit.kind),
                        bit.id,
                        active.next + 1
                    ));
                    if next >= bit.lines.len() {
                        self.active_bit = None;
                        self.bit_ended = true;
                    } else {
                        self.active_bit = Some(ActiveBit {
                            id: active.id,
                            next,
                        });
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

    fn plan_cold_open(&mut self, b: &BeatInputs<'_>, safety: bool, may_hold: bool) -> Plan {
        let firsts = b
            .cands
            .iter()
            .any(|c| self.first.contains(&c.msg.id) && !is_vote(c));
        if safety {
            return Plan::new(
                BeatMode::React,
                Offer::FirstContact,
                1,
                "room filling: safety beat, story paused".into(),
            );
        }
        let fc = if firsts {
            ", greeting a first contact"
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
            return Plan::new(
                BeatMode::React,
                Offer::FirstContact,
                1,
                "room filling: first contact".into(),
            );
        }
        if may_hold {
            return Plan::new(
                BeatMode::Hold,
                Offer::SafetyOnly,
                0,
                "room filling: story told, hold".into(),
            );
        }
        let mut p = Plan::new(
            BeatMode::ColdOpen,
            Offer::SafetyOnly,
            0,
            "room filling: gathering line".into(),
        );
        p.note = Some(GATHERING_NOTE.into());
        p
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
