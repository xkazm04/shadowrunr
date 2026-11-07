//! Review feedback mapped for the Tone Lab (`tone.inbox`, `tone.review_line`): a host line of a
//! sandbox session with its text, the session's tone, the situation of the chat it answers (as
//! the brain classifies a beat), its beat's source and segment, and the owner's marks.
//!
//! Every one of the eight marks has a role ([`role`], exhaustive):
//! - `great_roast`, `landed`: **bank candidates** (a line worth keeping);
//! - `flat`, `repetitive`, `off_tone`, `broke_character`: **draft-card** marks. Each is a
//!   direction suggestion, keyed by (beat source, segment, situation); a key with
//!   [`DRAFT_MIN_LINES`] or more such lines yields one [`EffectCardDraft`] (condition, an empty
//!   effect for the owner to write, refs to the lines, a retirement rule). Drafts are data the
//!   inbox returns: nothing writes `effects.toml`;
//! - `too_mean`: a **direction suggestion** only: how mean is too mean is the owner's call by
//!   hand, never a generated card;
//! - `wrong_tag`: **classification only**: it corrects the tagger's situation tag and feeds no
//!   direction.

use std::collections::{BTreeMap, HashSet};

use shadowrunr_brain::compose::beat_situations;
use shadowrunr_brain::moderation::Candidate;
use shadowrunr_brain::orchestrate::source_from_reason;
use shadowrunr_types::character::Persona;
use shadowrunr_types::prep::BeatSource;
use shadowrunr_types::review::{FeedbackTag, LineFeedback};
use shadowrunr_types::tone::{EffectCardDraft, LineRef, LineSituation, MarkCount, ToneReviewItem};
use shadowrunr_types::{SegmentId, Speaker, StreamEvent};

/// Each inbox list holds at most this many items, newest first.
pub const INBOX_MAX: usize = 100;

/// What a mark does in the inbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkRole {
    /// A line worth keeping: a bank candidate.
    BankCandidate,
    /// A direction suggestion only.
    Suggestion,
    /// A direction suggestion that counts toward a draft effect card for its key.
    DraftCard,
    /// A correction of the line's situation tag: feeds no direction.
    Classification,
}

/// Every mark's role. No wildcard: a new mark does not compile until it says what it does.
pub fn role(tag: FeedbackTag) -> MarkRole {
    match tag {
        FeedbackTag::Landed | FeedbackTag::GreatRoast => MarkRole::BankCandidate,
        FeedbackTag::TooMean => MarkRole::Suggestion,
        FeedbackTag::Flat
        | FeedbackTag::Repetitive
        | FeedbackTag::OffTone
        | FeedbackTag::BrokeCharacter => MarkRole::DraftCard,
        FeedbackTag::WrongTag => MarkRole::Classification,
    }
}

/// The marks that make a line a bank candidate.
pub const CANDIDATE_TAGS: [FeedbackTag; 2] = [FeedbackTag::GreatRoast, FeedbackTag::Landed];
/// The marks that make a line a direction suggestion (the draft-card marks and `too_mean`).
pub const SUGGESTION_TAGS: [FeedbackTag; 5] = [
    FeedbackTag::Flat,
    FeedbackTag::TooMean,
    FeedbackTag::Repetitive,
    FeedbackTag::OffTone,
    FeedbackTag::BrokeCharacter,
];
/// The marks a draft effect card counts.
pub const DRAFT_TAGS: [FeedbackTag; 4] = [
    FeedbackTag::Flat,
    FeedbackTag::Repetitive,
    FeedbackTag::OffTone,
    FeedbackTag::BrokeCharacter,
];
/// A key draws a draft card at this many lines carrying a draft mark (assumption: one mark is
/// an opinion, three on one kind of beat are a pattern).
pub const DRAFT_MIN_LINES: usize = 3;
/// The retirement rule every draft carries.
pub const DRAFT_RETIREMENT: &str = "Retire the card when, after it is committed, its key gathers 3 or more flat, repetitive, off-tone or broke-character marks again, or when at most 3 targeted judged runs show no rise in humor_landed for its source (sim mine humor-by-source, one instrument hash).";

/// The source and segment of the beat transcript line `line` (1-based) belongs to: the last
/// `beat_plan` before it (its typed source, else the one its reason names) and the last
/// `segment_start`.
pub fn beat_key(events: &[StreamEvent], line: u64) -> (Option<BeatSource>, Option<SegmentId>) {
    let Some(idx) = usize::try_from(line)
        .ok()
        .and_then(|l| l.checked_sub(1))
        .filter(|i| *i <= events.len())
    else {
        return (None, None);
    };
    let before = &events[..idx];
    let source = before.iter().rev().find_map(|e| match e {
        StreamEvent::BeatPlan {
            mode,
            picks,
            reason,
            source,
            ..
        } => Some(source.or_else(|| source_from_reason(*mode, *picks, reason))),
        _ => None,
    });
    let segment = before.iter().rev().find_map(|e| match e {
        StreamEvent::SegmentStart { segment, .. } => Some(*segment),
        _ => None,
    });
    (source.flatten(), segment)
}

/// The draft effect cards for `items` (each a line with its marks and key): one per (source,
/// segment, situation) key with [`DRAFT_MIN_LINES`] or more lines carrying a [`DRAFT_TAGS`]
/// mark, most lines first. A card's effect is empty and it cites lines, never their text.
pub fn drafts(items: &[ToneReviewItem]) -> Vec<EffectCardDraft> {
    type Key = (Option<BeatSource>, Option<SegmentId>, Option<LineSituation>);
    let mut by_key: Vec<(Key, Vec<&ToneReviewItem>)> = Vec::new();
    for it in items
        .iter()
        .filter(|it| it.tags.iter().any(|t| role(*t) == MarkRole::DraftCard))
    {
        let key = (it.source, it.segment, it.situation);
        match by_key.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(it),
            None => by_key.push((key, vec![it])),
        }
    }
    let mut out: Vec<EffectCardDraft> = by_key
        .into_iter()
        .filter(|(_, v)| v.len() >= DRAFT_MIN_LINES)
        .map(|((source, segment, situation), v)| {
            let marks = DRAFT_TAGS
                .iter()
                .map(|t| MarkCount {
                    tag: *t,
                    lines: u32::try_from(v.iter().filter(|it| it.tags.contains(t)).count())
                        .unwrap_or(u32::MAX),
                })
                .filter(|m| m.lines > 0)
                .collect();
            let mut refs: Vec<LineRef> = v
                .iter()
                .map(|it| LineRef {
                    session_id: it.session_id.clone(),
                    line: it.line,
                })
                .collect();
            refs.sort_by(|a, b| a.session_id.cmp(&b.session_id).then(a.line.cmp(&b.line)));
            EffectCardDraft {
                source,
                segment,
                situation,
                effect: String::new(),
                marks,
                refs,
                retirement: DRAFT_RETIREMENT.to_string(),
            }
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.refs.len()));
    out
}

/// The session's character and tone (`stream_start`).
pub fn stream_of(events: &[StreamEvent]) -> Option<(&str, &str)> {
    events.iter().find_map(|e| match e {
        StreamEvent::StreamStart {
            character,
            tone_variant,
            ..
        } => Some((character.as_str(), tone_variant.as_str())),
        _ => None,
    })
}

/// The most specific situation of the chat that transcript line `line` (1-based, a host
/// `say`) answers. `None` when it answers no chat message, or when the brain would carry no
/// roast material in that beat (a minor flirting, say).
pub fn situation_of(events: &[StreamEvent], line: u64, persona: &Persona) -> Option<LineSituation> {
    let idx = usize::try_from(line).ok()?.checked_sub(1)?;
    let StreamEvent::Say { replying_to, .. } = events.get(idx)? else {
        return None;
    };
    if replying_to.is_empty() {
        return None;
    }
    let picked: Vec<Candidate> = events[..idx]
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Chat { msg, .. } if replying_to.contains(&msg.id) => Some(Candidate {
                msg: msg.clone(),
                count: 1,
                viewers: 1,
                injection: false,
                safety: None,
            }),
            _ => None,
        })
        .collect();
    if picked.is_empty() {
        return None;
    }
    let tags = beat_situations(&picked, &HashSet::new(), None, None, false, persona)?;
    tags.first().and_then(|t| LineSituation::parse(t))
}

/// The host line's text, when transcript line `line` is one.
pub fn host_text(events: &[StreamEvent], line: u64) -> Option<&str> {
    let idx = usize::try_from(line).ok()?.checked_sub(1)?;
    match events.get(idx)? {
        StreamEvent::Say {
            speaker: Speaker::Host,
            text,
            ..
        } => Some(text.as_str()),
        _ => None,
    }
}

/// The item for one host line, with its feedback when there is any.
pub fn item(
    session_id: &str,
    events: &[StreamEvent],
    line: u64,
    feedback: Option<&LineFeedback>,
    persona: &Persona,
) -> Option<ToneReviewItem> {
    let text = host_text(events, line)?;
    let (_, tone) = stream_of(events)?;
    let (source, segment) = beat_key(events, line);
    Some(ToneReviewItem {
        session_id: session_id.to_string(),
        line,
        text: text.to_string(),
        tone: tone.to_string(),
        situation: situation_of(events, line, persona),
        tags: feedback.map(|f| f.tags.clone()).unwrap_or_default(),
        note: feedback.and_then(|f| f.note.clone()),
        at_ms: feedback.map(|f| f.at_ms),
        source,
        segment,
    })
}

/// Feedback rows that belong in the inbox, grouped by session.
pub fn by_session(feedback: Vec<LineFeedback>) -> BTreeMap<String, Vec<LineFeedback>> {
    let mut out: BTreeMap<String, Vec<LineFeedback>> = BTreeMap::new();
    for f in feedback {
        if f.tags.iter().any(|t| role(*t) != MarkRole::Classification) {
            out.entry(f.session_id.clone()).or_default().push(f);
        }
    }
    out
}

/// Sort newest first (then by session and line) and cap at [`INBOX_MAX`].
pub fn newest_first(mut items: Vec<ToneReviewItem>) -> Vec<ToneReviewItem> {
    items.sort_by(|a, b| {
        b.at_ms
            .cmp(&a.at_ms)
            .then_with(|| b.session_id.cmp(&a.session_id))
            .then_with(|| a.line.cmp(&b.line))
    });
    items.truncate(INBOX_MAX);
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::prep::BeatMode;

    const ALL_MARKS: [FeedbackTag; 8] = [
        FeedbackTag::Landed,
        FeedbackTag::Flat,
        FeedbackTag::TooMean,
        FeedbackTag::BrokeCharacter,
        FeedbackTag::OffTone,
        FeedbackTag::GreatRoast,
        FeedbackTag::Repetitive,
        FeedbackTag::WrongTag,
    ];

    /// Every mark is accounted for: listed once here (the match below fails to compile when a
    /// mark is added), and each has exactly the role its lists say.
    #[test]
    fn every_mark_has_a_role() {
        fn listed(t: FeedbackTag) -> usize {
            match t {
                FeedbackTag::Landed => 0,
                FeedbackTag::Flat => 1,
                FeedbackTag::TooMean => 2,
                FeedbackTag::BrokeCharacter => 3,
                FeedbackTag::OffTone => 4,
                FeedbackTag::GreatRoast => 5,
                FeedbackTag::Repetitive => 6,
                FeedbackTag::WrongTag => 7,
            }
        }
        for (i, t) in ALL_MARKS.into_iter().enumerate() {
            assert_eq!(listed(t), i);
            let r = role(t);
            assert_eq!(
                CANDIDATE_TAGS.contains(&t),
                r == MarkRole::BankCandidate,
                "{t:?}"
            );
            assert_eq!(DRAFT_TAGS.contains(&t), r == MarkRole::DraftCard, "{t:?}");
            assert_eq!(
                SUGGESTION_TAGS.contains(&t),
                matches!(r, MarkRole::Suggestion | MarkRole::DraftCard),
                "{t:?}"
            );
        }
        assert_eq!(role(FeedbackTag::WrongTag), MarkRole::Classification);
        // A wrong_tag row alone reaches no inbox list.
        let rows = by_session(vec![
            LineFeedback {
                session_id: "s".into(),
                line: 2,
                tags: vec![FeedbackTag::WrongTag],
                note: None,
                at_ms: 1,
            },
            LineFeedback {
                session_id: "s".into(),
                line: 3,
                tags: vec![FeedbackTag::OffTone],
                note: None,
                at_ms: 2,
            },
        ]);
        assert_eq!(rows["s"].len(), 1);
        assert_eq!(rows["s"][0].line, 3);
    }

    fn plan(reason: &str, source: Option<BeatSource>) -> StreamEvent {
        StreamEvent::BeatPlan {
            ts_ms: 0,
            mode: BeatMode::Improvise,
            bit_id: None,
            picks: 0,
            reason: reason.into(),
            source,
        }
    }

    fn say(text: &str) -> StreamEvent {
        StreamEvent::Say {
            ts_ms: 0,
            speaker: Speaker::Host,
            foil: None,
            text: text.into(),
            replying_to: Vec::new(),
            first_text_ms: None,
            delivery: None,
        }
    }

    fn marked(
        sid: &str,
        line: u64,
        text: &str,
        tags: &[FeedbackTag],
        ev: &[StreamEvent],
    ) -> ToneReviewItem {
        let (source, segment) = beat_key(ev, line);
        ToneReviewItem {
            session_id: sid.into(),
            line,
            text: text.into(),
            tone: "unhinged-roastmaster".into(),
            situation: None,
            tags: tags.to_vec(),
            note: None,
            at_ms: Some(1),
            source,
            segment,
        }
    }

    /// Three flat, repetitive or off-tone purpose lines in SEG-02A draw one card, keyed by them,
    /// with an empty effect, refs to the lines and a retirement rule; the same marks on two case
    /// lines draw none; the marked text appears nowhere in a card.
    #[test]
    fn a_key_with_three_marked_lines_draws_one_card_and_quotes_none() {
        let ev = vec![
            StreamEvent::SegmentStart {
                ts_ms: 0,
                segment: SegmentId::Seg02aHexmap,
            },
            plan("quiet: improvised", Some(BeatSource::Purpose)),
            say("The ledger demands progress, minions."),
            plan("case: dispatch HEX-03", None),
            say("The clerks ride for the shell."),
            plan("segment+chat: topic: the state's purpose", None),
            say("Onward, the scheme advances."),
            plan("quiet: improvised", Some(BeatSource::Purpose)),
            say("Progress, I said, progress."),
            plan("case: reveal HEX-01 DOC", None),
            say("Face up, the syndicate."),
        ];
        assert_eq!(
            beat_key(&ev, 3),
            (Some(BeatSource::Purpose), Some(SegmentId::Seg02aHexmap))
        );
        assert_eq!(
            beat_key(&ev, 7).0,
            Some(BeatSource::Purpose),
            "from the reason"
        );
        let items = vec![
            marked(
                "s1",
                3,
                "The ledger demands progress, minions.",
                &[FeedbackTag::Flat],
                &ev,
            ),
            marked(
                "s1",
                7,
                "Onward, the scheme advances.",
                &[FeedbackTag::Repetitive, FeedbackTag::Flat],
                &ev,
            ),
            marked(
                "s1",
                9,
                "Progress, I said, progress.",
                &[FeedbackTag::OffTone],
                &ev,
            ),
            marked(
                "s1",
                5,
                "The clerks ride for the shell.",
                &[FeedbackTag::Flat],
                &ev,
            ),
            marked(
                "s1",
                11,
                "Face up, the syndicate.",
                &[FeedbackTag::BrokeCharacter],
                &ev,
            ),
            // Marks that draw no card: a bank candidate and too_mean.
            marked(
                "s1",
                3,
                "The ledger demands progress, minions.",
                &[FeedbackTag::TooMean],
                &ev,
            ),
        ];
        let cards = drafts(&items);
        assert_eq!(cards.len(), 1, "{cards:?}");
        let c = &cards[0];
        assert_eq!(c.source, Some(BeatSource::Purpose));
        assert_eq!(c.segment, Some(SegmentId::Seg02aHexmap));
        assert_eq!(c.situation, None);
        assert!(c.effect.is_empty());
        assert_eq!(c.retirement, DRAFT_RETIREMENT);
        let lines: Vec<u64> = c.refs.iter().map(|r| r.line).collect();
        assert_eq!(lines, [3, 7, 9]);
        let flat = c.marks.iter().find(|m| m.tag == FeedbackTag::Flat).unwrap();
        assert_eq!(flat.lines, 2);
        let json = serde_json::to_string(&cards).unwrap();
        for it in &items {
            assert!(
                !json.contains(&it.text),
                "a card quotes a marked line: {json}"
            );
        }
        // Two marked lines on a key are no pattern yet.
        assert!(drafts(&items[..2]).is_empty());
    }
}
