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
    /// A prerequisite: the bit is offered only while the prep's [`ArcDef`] stands in this
    /// state. Absent: offered whatever the arc (and with no arc).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc_state: Option<String>,
}

/// Whether an arc may run. Only the owner approves one; a draft is never loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArcStatus {
    /// Written, pending the owner's approval: never run.
    Draft,
    /// Approved by the owner: a stream with this prep runs it.
    Approved,
}

/// The vote a state puts to the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArcVote {
    pub question: String,
    /// The options, exactly as the poll offers them; each permitted transition names one.
    pub options: Vec<String>,
}

/// One state of an arc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArcStateDef {
    pub id: String,
    /// What is true in this state, one line, for the host.
    pub summary: String,
    /// The consequence owed on the night after this state is entered; delivered once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payoff: Option<String>,
    /// The vote that moves the arc on from this state (after its payoff). A state without
    /// one ends the arc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vote: Option<ArcVote>,
}

/// A permitted move: when the vote of `from` is won by `option`, the arc goes to `to`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArcTransition {
    pub id: String,
    pub from: String,
    pub option: String,
    pub to: String,
    /// What the host announces when it happens (owner-approved; he says it in his words).
    pub announce: String,
}

/// One bounded audience-authored arc: viewers' confirmed votes move it from night to night
/// through the states and transitions the owner approved, and nothing else does (no chat text,
/// no model line). See `shadowrunr_brain::arc`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArcDef {
    pub id: String,
    pub title: String,
    pub status: ArcStatus,
    /// A short factual recap a newcomer gets before taking part.
    pub recap: String,
    /// The state the arc starts in.
    pub start: String,
    pub states: Vec<ArcStateDef>,
    pub transitions: Vec<ArcTransition>,
}

/// An arc stays small: at most this many states...
pub const ARC_MAX_STATES: usize = 8;
/// ...and this many transitions.
pub const ARC_MAX_TRANSITIONS: usize = 16;

impl ArcDef {
    /// The arc is well formed: bounded, its start and every transition's states exist, unique
    /// ids, and every transition's option is one of its `from` state's vote options.
    pub fn validate(&self) -> Result<(), String> {
        if self.states.is_empty() || self.states.len() > ARC_MAX_STATES {
            return Err(format!("an arc has 1-{ARC_MAX_STATES} states"));
        }
        if self.transitions.len() > ARC_MAX_TRANSITIONS {
            return Err(format!(
                "an arc has at most {ARC_MAX_TRANSITIONS} transitions"
            ));
        }
        let state = |id: &str| self.states.iter().find(|s| s.id == id);
        if state(&self.start).is_none() {
            return Err(format!("start state {:?} is not defined", self.start));
        }
        for (i, s) in self.states.iter().enumerate() {
            if self.states[..i].iter().any(|o| o.id == s.id) {
                return Err(format!("state {:?} is defined twice", s.id));
            }
        }
        for (i, t) in self.transitions.iter().enumerate() {
            if self.transitions[..i].iter().any(|o| o.id == t.id) {
                return Err(format!("transition {:?} is defined twice", t.id));
            }
            let Some(from) = state(&t.from) else {
                return Err(format!("transition {:?}: no state {:?}", t.id, t.from));
            };
            if state(&t.to).is_none() {
                return Err(format!("transition {:?}: no state {:?}", t.id, t.to));
            }
            let offered = from
                .vote
                .as_ref()
                .is_some_and(|v| v.options.iter().any(|o| o == &t.option));
            if !offered {
                return Err(format!(
                    "transition {:?}: {:?} is not an option of {:?}'s vote",
                    t.id, t.option, t.from
                ));
            }
        }
        Ok(())
    }
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
    /// One audience-authored arc the stream runs, when its `status` is `approved`. Omitted
    /// when the stream runs none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<ArcDef>,
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
