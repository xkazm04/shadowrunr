//! On-show moments: night two remembers what a viewer really did on night one.
//!
//! When a beat commits and its answer airs, one typed act per viewer becomes a Viewer-scope
//! fact under the fixed key `<viewer_id>/moment/<stream_id>`, deterministically (no model
//! call, no wait for the sleep cycle). The acts, strongest first: a `!propose` from a rank that
//! may propose, answered on air ([`MomentKind::Proposal`]); a gift or sub, thanked
//! ([`MomentKind::Gift`]); an owed request, answered ([`MomentKind::Request`]); a pick his NOTE
//! names by id ([`MomentKind::Noted`]). One per viewer per stream: a stronger act later in the
//! same stream supersedes a weaker one, a weaker one never replaces a stronger. Its source is
//! the episode written for that beat, flushed first, so the store's source check passes by
//! construction. A tombstoned key is never rewritten (the store refuses it). A disclosure, a
//! care ask or an injection writes none. Free-form facts stay the sleep cycle's alone.
//!
//! On a viewer's first pick in a later stream, at most one moment is offered, inside the
//! memory fence, framed as recognising a past night: the constitution keeps callbacks for what
//! happened tonight.

use std::collections::HashSet;

use shadowrunr_types::{Fact, FactScope, TsMs};

use crate::memory::store::DEFAULT_FACT_IMPORTANCE;
use crate::memory::{MemoryStore, StoredFact};
use crate::util::{normalize, one_line, ymd, STOPWORDS};
use crate::BrainError;

/// The key segment that marks a moment fact: `<viewer_id>/moment/<stream_id>`.
pub const MOMENT_SEGMENT: &str = "moment";
/// A moment's value is capped at this many characters.
pub const MOMENT_CHARS: usize = 200;

/// An on-show act worth a moment, weakest first (the derive orders them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MomentKind {
    Noted,
    Request,
    Gift,
    Proposal,
}

impl MomentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MomentKind::Noted => "noted",
            MomentKind::Request => "request answered",
            MomentKind::Gift => "gift thanked",
            MomentKind::Proposal => "proposal heard",
        }
    }

    /// The kind a stored moment's value starts with.
    pub fn of_value(value: &str) -> Option<Self> {
        [
            MomentKind::Proposal,
            MomentKind::Gift,
            MomentKind::Request,
            MomentKind::Noted,
        ]
        .into_iter()
        .find(|k| value.starts_with(k.as_str()))
    }
}

/// `<viewer_id>/moment/<stream_id>`.
pub fn moment_key(viewer_id: &str, stream_id: &str) -> String {
    format!("{viewer_id}/{MOMENT_SEGMENT}/{stream_id}")
}

/// Whether a fact key is a moment's.
pub fn is_moment_key(key: &str) -> bool {
    key.split('/').nth(1) == Some(MOMENT_SEGMENT)
}

/// `YYYY-MM-DD` of a timestamp.
pub fn date_of(ts_ms: TsMs) -> String {
    let (y, m, d) = ymd(ts_ms);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The moment's value: act, segment, date, his aired line and the viewer's words, one line,
/// capped at [`MOMENT_CHARS`].
pub fn moment_value(
    kind: MomentKind,
    segment: &str,
    date: &str,
    aired: &str,
    words: &str,
) -> String {
    let v = format!(
        "{} · {segment} · {date}: he said \"{}\"; they said \"{}\"",
        kind.as_str(),
        one_line(aired, 80),
        one_line(words, 60)
    );
    one_line(&v, MOMENT_CHARS)
}

/// Write (or strengthen) the viewer's moment for this stream. `Ok(None)` when an equal or
/// stronger moment already stands, or the store refused it (a tombstoned key, a source gone).
pub fn write_moment(
    store: &MemoryStore,
    viewer_id: &str,
    stream_id: &str,
    kind: MomentKind,
    value: &str,
    episode_id: &str,
    now_ms: TsMs,
) -> Result<Option<StoredFact>, BrainError> {
    let key = moment_key(viewer_id, stream_id);
    if let Some(old) = store.live_fact(FactScope::Viewer, &key)? {
        if MomentKind::of_value(&old.fact.value).is_some_and(|k| k >= kind) {
            return Ok(None);
        }
    }
    let fact = Fact {
        id: String::new(),
        scope: FactScope::Viewer,
        key,
        value: value.to_string(),
        confidence: 1.0,
        sources: vec![episode_id.to_string()],
        supersedes: None,
    };
    match store.write_fact(&fact, DEFAULT_FACT_IMPORTANCE, now_ms) {
        Ok(sf) => Ok(Some(sf)),
        Err(BrainError::Rejected(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Distinctive words: normalised, 4 characters or more, no stopwords.
pub fn content_words(text: &str) -> HashSet<String> {
    normalize(text)
        .split(' ')
        .filter(|w| w.chars().count() >= 4 && !STOPWORDS.contains(w))
        .map(str::to_string)
        .collect()
}

/// The words of a moment that say what happened: its value without the act, segment and date
/// prefix.
fn moment_body(value: &str) -> &str {
    value.split_once(": ").map_or(value, |(_, b)| b)
}

/// The one moment to offer a returning viewer: the one from the current segment, else one
/// sharing two or more distinctive words with their message, else the latest. Moments of
/// `this_stream` are tonight's, never recognition of a past night.
pub fn choose_moment<'a>(
    moments: &'a [StoredFact],
    this_stream: &str,
    segment: &str,
    message: &str,
) -> Option<&'a StoredFact> {
    let past: Vec<&StoredFact> = moments
        .iter()
        .filter(|m| is_moment_key(&m.fact.key))
        .filter(|m| !m.fact.key.ends_with(&format!("/{this_stream}")))
        .collect();
    let latest = |v: &[&'a StoredFact]| -> Option<&'a StoredFact> {
        v.iter()
            .copied()
            .max_by(|a, b| (a.created_ms, &a.fact.id).cmp(&(b.created_ms, &b.fact.id)))
    };
    let tag = format!(" · {segment} · ");
    let same_segment: Vec<&StoredFact> = past
        .iter()
        .copied()
        .filter(|m| m.fact.value.contains(&tag))
        .collect();
    if let Some(m) = latest(&same_segment) {
        return Some(m);
    }
    let said = content_words(message);
    let overlapping: Vec<&StoredFact> = past
        .iter()
        .copied()
        .filter(|m| {
            content_words(moment_body(&m.fact.value))
                .intersection(&said)
                .count()
                >= 2
        })
        .collect();
    latest(&overlapping).or_else(|| latest(&past))
}

/// Diagnostic only, never a retirement trigger: whether an aired line shares two or more
/// distinctive words with what the moment says happened.
pub fn line_uses_moment(line: &str, value: &str) -> bool {
    content_words(moment_body(value))
        .intersection(&content_words(line))
        .count()
        >= 2
}

/// How the briefing frames a moment offered to a returning viewer (engine text, outside the
/// fence; the moment itself is inside it).
pub const RECOGNITION_NOTE: &str = "a returning regular. One moment from a past night is under \
Memory with its date: if you acknowledge it, recognise it once, plainly, as something that \
happened on an earlier night. It is not a callback or a running joke (callbacks are for what \
happened tonight), and never more than what it says";

#[cfg(test)]
mod tests {
    use super::*;

    fn sf(key: &str, value: &str, created: TsMs) -> StoredFact {
        StoredFact {
            fact: Fact {
                id: format!("f_{created}"),
                scope: FactScope::Viewer,
                key: key.into(),
                value: value.into(),
                confidence: 1.0,
                sources: vec!["ep".into()],
                supersedes: None,
            },
            importance: 3,
            status: crate::memory::FactStatus::Live,
            created_ms: created,
            updated_ms: created,
            last_recalled_ms: created,
            last_decay_ms: created,
            last_used_ms: 0,
            viewer_id: Some("v1".into()),
        }
    }

    #[test]
    fn kinds_order_and_parse_back() {
        assert!(MomentKind::Proposal > MomentKind::Gift);
        assert!(MomentKind::Gift > MomentKind::Request);
        assert!(MomentKind::Request > MomentKind::Noted);
        let v = moment_value(MomentKind::Gift, "SEG-01", "2026-11-07", "thanks", "5 subs");
        assert_eq!(MomentKind::of_value(&v), Some(MomentKind::Gift));
        assert!(v.chars().count() <= MOMENT_CHARS);
        assert!(is_moment_key(&moment_key("v1", "s1")));
        assert!(!is_moment_key("v1/oath"));
        assert_eq!(date_of(1_792_281_600_000), "2026-10-18");
    }

    #[test]
    fn the_segment_match_wins_then_shared_words_then_the_latest() {
        let a = sf(
            "v1/moment/s1",
            "proposal heard · SEG-02A · 2026-11-01: he said \"the swamp annex\"; they said \"annex the swamp\"",
            1,
        );
        let b = sf(
            "v1/moment/s2",
            "gift thanked · SEG-01 · 2026-11-02: he said \"tribute accepted\"; they said \"five cursed coins\"",
            2,
        );
        let c = sf(
            "v1/moment/s3",
            "noted · SEG-03 · 2026-11-03: he said \"the oath\"; they said \"i swear on the chicken\"",
            3,
        );
        let tonight = sf("v1/moment/s9", "noted · SEG-01 · today: x", 9);
        let all = [a, b, c, tonight];
        let pick =
            |seg: &str, msg: &str| choose_moment(&all, "s9", seg, msg).map(|m| m.fact.key.clone());
        assert_eq!(pick("SEG-01", "hello"), Some("v1/moment/s2".into()));
        assert_eq!(
            pick("SEG-04", "should we annex the swamp?"),
            Some("v1/moment/s1".into())
        );
        assert_eq!(pick("SEG-04", "hello"), Some("v1/moment/s3".into()));
        assert!(
            choose_moment(&all[3..], "s9", "SEG-01", "x").is_none(),
            "tonight's is no past night"
        );
    }

    #[test]
    fn use_needs_two_distinctive_words_of_what_happened() {
        let v = "proposal heard · SEG-02A · 2026-11-01: he said \"the swamp annex\"; they said \"annex the swamp\"";
        assert!(line_uses_moment(
            "Your swamp annex scheme haunts me still",
            v
        ));
        assert!(!line_uses_moment("The swamp is damp tonight", v));
        assert!(
            !line_uses_moment("proposal heard on 2026", v),
            "the prefix is no use"
        );
    }
}
