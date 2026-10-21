//! Review feedback mapped for the Tone Lab (`tone.inbox`, `tone.review_line`): a host line of a
//! sandbox session with its text, the session's tone, the situation of the chat it answers (as
//! the brain classifies a beat) and the owner's marks. `great_roast` / `landed` lines are bank
//! candidates; `flat` / `too_mean` lines are direction suggestions.

use std::collections::{BTreeMap, HashSet};

use shadowrunr_brain::compose::beat_situations;
use shadowrunr_brain::moderation::Candidate;
use shadowrunr_types::character::Persona;
use shadowrunr_types::review::{FeedbackTag, LineFeedback};
use shadowrunr_types::tone::{LineSituation, ToneReviewItem};
use shadowrunr_types::{Speaker, StreamEvent};

/// Each inbox list holds at most this many items, newest first.
pub const INBOX_MAX: usize = 100;

/// The marks that make a line a bank candidate.
pub const CANDIDATE_TAGS: [FeedbackTag; 2] = [FeedbackTag::GreatRoast, FeedbackTag::Landed];
/// The marks that make a line a direction suggestion.
pub const SUGGESTION_TAGS: [FeedbackTag; 2] = [FeedbackTag::Flat, FeedbackTag::TooMean];

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
    Some(ToneReviewItem {
        session_id: session_id.to_string(),
        line,
        text: text.to_string(),
        tone: tone.to_string(),
        situation: situation_of(events, line, persona),
        tags: feedback.map(|f| f.tags.clone()).unwrap_or_default(),
        note: feedback.and_then(|f| f.note.clone()),
        at_ms: feedback.map(|f| f.at_ms),
    })
}

/// Feedback rows that belong in the inbox, grouped by session.
pub fn by_session(feedback: Vec<LineFeedback>) -> BTreeMap<String, Vec<LineFeedback>> {
    let mut out: BTreeMap<String, Vec<LineFeedback>> = BTreeMap::new();
    for f in feedback {
        if f.tags
            .iter()
            .any(|t| CANDIDATE_TAGS.contains(t) || SUGGESTION_TAGS.contains(t))
        {
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
