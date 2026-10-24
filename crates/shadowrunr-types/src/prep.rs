//! Stream prep: the material a stream opens and runs on, written by the planner before each
//! stream and approved by the owner (like a real case), stored at `<data>/prep/<prep_id>/prep.json`.
//! The brain's orchestrator voices it: the cold-open story while the room fills, quiet-room
//! premises per segment, and connector lines that bridge between prepared bits and chat. Prep is
//! never generated live in a quiet moment; only an approved prep reaches a stream.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{SegmentId, TsMs};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrepStatus {
    /// Written and mechanically checked; not yet seen by the owner.
    Staged,
    /// The owner approved it: streams may use it.
    Approved,
    /// Rejected or superseded; never used.
    Retired,
}

/// The cold open: a short story told in beats while the room fills.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColdOpen {
    pub id: String,
    pub title: String,
    /// The story's lines in order, one or two voiced per beat.
    pub beats: Vec<String>,
    /// The line that hands over from the story to the room.
    pub handoff: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BitKind {
    /// A one-line invitation chat can join in one line (quiet-room deck).
    Premise,
    /// A short told story (2-5 lines) with a payoff.
    Story,
    /// A running gag from the character's bible, re-dressed for tonight.
    Callback,
}

/// One prepared bit, for one segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrepBit {
    pub id: String,
    pub segment: SegmentId,
    pub kind: BitKind,
    /// The lines in order (a premise has one).
    pub lines: Vec<String>,
    /// What lands at the end, so the host knows where the bit goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payoff: Option<String>,
}

/// Where a connector line bridges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorKind {
    /// From the cold open into the room.
    IntoRoom,
    /// From answering chat back to a prepared bit.
    IntoBit,
    /// From a bit back to chat.
    BackToChat,
    /// Into a poll or vote.
    IntoPoll,
    /// Toward a segment handover.
    IntoHandover,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Connector {
    pub id: String,
    pub kind: ConnectorKind,
    pub line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrepCheckState {
    Pass,
    Fail,
}

/// A mechanical check over the prep (lengths, no real people, no callbacks a newcomer missed...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrepCheck {
    pub id: String,
    pub state: PrepCheckState,
    pub detail: String,
}

/// `prep.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StreamPrep {
    pub prep_id: String,
    pub character: String,
    pub created_ms: TsMs,
    pub status: PrepStatus,
    /// The Reconcile report whose plan it was written from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_report: Option<String>,
    pub cold_open: ColdOpen,
    pub bits: Vec<PrepBit>,
    pub connectors: Vec<Connector>,
    /// SEG-03: the scheduled guest and the scenes for its ballots. Omitted when the stream
    /// plays no Table Read; SEG-01 then hands over to SEG-02A.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_read: Option<crate::scene::TableReadPlan>,
    pub checks: Vec<PrepCheck>,
    /// Set on approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_ms: Option<TsMs>,
    /// True for a fabricated sample (tests, sims without an approved prep).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fictional: bool,
}

/// One row of `prep.list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrepSummary {
    pub prep_id: String,
    pub created_ms: TsMs,
    pub status: PrepStatus,
    pub title: String,
    pub bits: u32,
    pub failed_checks: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrepGenerateParams {
    /// Write from this Reconcile report's plan; the current plan when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrepIdParams {
    pub prep_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrepApproveParams {
    pub prep_id: String,
    /// The owner's signature.
    pub signature: String,
}

/// What the orchestrator chose for one beat (`StreamEvent::BeatPlan`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BeatMode {
    /// A cold-open story beat (room still filling).
    ColdOpen,
    /// Answer picked chat.
    React,
    /// Voice a prepared bit.
    Bit,
    /// A connector line, then chat or a bit.
    Bridge,
    /// A bit and picked chat in the same beat.
    Mixed,
    /// Nothing to say this beat (the gap is still under the ceiling).
    Hold,
    /// A quiet beat with nothing prepared that fits: the host fills it in his own words.
    Improvise,
    /// SEG-03: the next lines of the scene being performed.
    Scene,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names() {
        assert_eq!(
            serde_json::to_string(&BeatMode::ColdOpen).unwrap(),
            r#""cold_open""#
        );
        assert_eq!(
            serde_json::to_string(&ConnectorKind::BackToChat).unwrap(),
            r#""back_to_chat""#
        );
        assert_eq!(
            serde_json::to_string(&PrepStatus::Approved).unwrap(),
            r#""approved""#
        );
    }
}
