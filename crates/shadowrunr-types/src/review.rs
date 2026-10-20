//! Session review: the console's Review page over finished sandbox sessions. Line tags are a
//! derived, versioned file (`runs/sandbox/<id>/tags.json`, recomputable by `review.tag`);
//! operator feedback is an append-only log (`<data>/review/feedback.jsonl`) where the latest
//! row per `<session>#<line>` wins. `line` is always the 1-based transcript line, the same
//! number as the `/events` seq and the judge's `<run>#<line>` refs.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::rpc::LedgerEntry;
use crate::sandbox::{SandboxChaos, SandboxCheck};
use crate::{JudgeVerdict, ModelTier, SegmentId, StreamEvent, TsMs};

/// What a Malakor line is doing.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    /// An insult aimed at a named viewer or the foil.
    Roast,
    /// A light, affectionate jab.
    Tease,
    /// Humour not aimed at a person.
    Joke,
    /// Grants or plays with a mock title or rank.
    Title,
    /// Greets a newcomer, a raid, a gift or a return.
    Welcome,
    /// Answers chat plainly.
    Reply,
    /// Calls chat to act (vote, hint, summon...) or counts a poll down.
    Callout,
    /// Runs the segment (polls, case facts, reveals, research, framing).
    Show,
    /// Declines a request or bait.
    Refusal,
    /// A sincere safety or crisis line.
    Care,
    /// Says he is an AI character or that the case is fictional.
    Disclosure,
    Signoff,
    Other,
}

/// Whom a line is aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineTarget {
    Viewer,
    Chat,
    Foil,
    #[serde(rename = "self")]
    Myself,
    Lord,
    Case,
    None,
}

/// How cutting a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineEdge {
    None,
    Mild,
    Sharp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineTag {
    pub line: u64,
    pub kind: LineKind,
    pub target: LineTarget,
    pub edge: LineEdge,
    /// The tagger's reason, eight words at most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// `tags.json`: one tagger pass over a session's Malakor lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionTags {
    pub session_id: String,
    /// Bumped whenever the taxonomy or the tagger's instructions change.
    pub tagger_version: u32,
    pub model: ModelTier,
    pub created_ms: TsMs,
    pub lines: Vec<LineTag>,
    /// Malakor lines the tagger left without a tag (unparseable reply, timeout).
    pub untagged: u32,
}

/// The operator's quick marks on a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackTag {
    Landed,
    Flat,
    TooMean,
    BrokeCharacter,
    OffTone,
    GreatRoast,
    Repetitive,
    /// The line's tag is wrong.
    WrongTag,
}

/// One row of `feedback.jsonl`, and the folded state of a line. Empty `tags` with no `note`
/// clears the line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineFeedback {
    pub session_id: String,
    pub line: u64,
    pub tags: Vec<FeedbackTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub at_ms: TsMs,
}

/// One session on the Review page's wall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewSessionRow {
    pub session_id: String,
    pub minutes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chaos: Option<SandboxChaos>,
    pub started_ms: TsMs,
    /// Stream seconds from the first to the last event.
    pub length_s: f64,
    pub pass: bool,
    pub checks: Vec<SandboxCheck>,
    pub max_say_gap_s: f64,
    /// Absent when the session was not judged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_fails: Option<u32>,
    /// Malakor lines in the transcript.
    pub lines: u32,
    /// Tagged lines per kind; kinds with no line are omitted.
    pub mix: BTreeMap<LineKind, u32>,
    /// Lines with operator feedback.
    pub marked: u32,
    /// `tags.json` exists.
    pub tagged: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentSpan {
    pub segment: SegmentId,
    pub from_s: f64,
    pub to_s: f64,
}

/// What a judged rule or quality dimension asks for, for display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RuleInfo {
    /// A rule id (`situation:<id>`, `red-line:<id>`...) or a quality dimension id.
    pub id: String,
    /// The planted moment, for situation rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    pub required: String,
}

/// `review.session`: everything the Review page needs for one session, in one reply.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReviewSession {
    pub row: ReviewSessionRow,
    pub segments: Vec<SegmentSpan>,
    /// The whole transcript; event i is line i + 1.
    pub events: Vec<StreamEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<SessionTags>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<JudgeVerdict>,
    /// Entries for the verdict's failed rules and its quality dimensions.
    pub catalog: Vec<RuleInfo>,
    /// The session's engine calls (`ledger.jsonl`).
    pub calls: Vec<LedgerEntry>,
    pub feedback: Vec<LineFeedback>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewSessionsParams {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewSessionParams {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackSetParams {
    pub session_id: String,
    pub line: u64,
    pub tags: Vec<FeedbackTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewTagParams {
    pub session_id: String,
    /// Re-tag even when `tags.json` is current.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub force: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_are_snake_case_and_self_is_kept() {
        assert_eq!(
            serde_json::to_string(&LineKind::Signoff).unwrap(),
            r#""signoff""#
        );
        assert_eq!(
            serde_json::to_string(&LineTarget::Myself).unwrap(),
            r#""self""#
        );
        assert_eq!(
            serde_json::to_string(&FeedbackTag::BrokeCharacter).unwrap(),
            r#""broke_character""#
        );
        let mut mix = BTreeMap::new();
        mix.insert(LineKind::Roast, 3);
        assert_eq!(serde_json::to_string(&mix).unwrap(), r#"{"roast":3}"#);
    }
}
