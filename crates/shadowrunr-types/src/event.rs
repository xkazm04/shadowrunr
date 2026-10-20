//! The stream transcript: one `StreamEvent` per JSONL line. Produced by the runtime and the
//! simulator, consumed by the stage (replay), Reconcile & Plan, the judge and the clip factory.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::chat::{ChatMsg, EvidenceGrade, Rank};
use crate::engine::ModelTier;
use crate::reconcile::StreamBrief;
use crate::segment::{Case, SegmentId};
use crate::TsMs;

/// Malakor's action grammar. The model emits `OP: {json}` lines; only these ops are allowed.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Highlight {
        msg_id: String,
    },
    StartPoll {
        poll_id: String,
        question: String,
        options: Vec<String>,
        min_rank: Rank,
        secs: u32,
    },
    ClosePoll {
        poll_id: String,
    },
    SummonFoil {
        foil: String,
    },
    DismissFoil,
    RevealHex {
        hex: String,
        grade: EvidenceGrade,
    },
    DispatchResearch {
        job_id: String,
        brief: String,
    },
    ShowCard {
        card_id: String,
        title: String,
        body: String,
    },
    MarkMoment {
        label: String,
    },
    SwitchSegment {
        to: SegmentId,
    },
    SignOff {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VoteTally {
    pub option: String,
    pub voters: u32,
    /// Sum of rank-weighted votes.
    pub weight: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModStage {
    /// Chat input filtered before the model sees it.
    Input,
    /// Model output filtered before speech or screen.
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Scheduled,
    FleetLost,
    Operator,
    SimComplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Speaker {
    Malakor,
    Foil,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    StreamStart {
        ts_ms: TsMs,
        stream_id: String,
        character: String,
        tone_variant: String,
        core_tier: ModelTier,
        audience_size: u32,
        /// Addition (WP15): the brief the brain received in its stable prefix for this stream
        /// (the chosen plan's dispatch, segments and topics plus the active directives).
        /// Omitted when the stream had no brief.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        brief: Option<StreamBrief>,
    },
    SegmentStart {
        ts_ms: TsMs,
        segment: SegmentId,
    },
    SegmentEnd {
        ts_ms: TsMs,
        segment: SegmentId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ses: Option<f64>,
    },
    /// SEG-02A: the Case being audited, emitted once right after its `segment_start`.
    CaseOpen {
        ts_ms: TsMs,
        case: Case,
    },
    Chat {
        ts_ms: TsMs,
        msg: ChatMsg,
    },
    /// The chat picker chose these messages for the core thread.
    Picked {
        ts_ms: TsMs,
        msg_ids: Vec<String>,
        reason: String,
    },
    Say {
        ts_ms: TsMs,
        speaker: Speaker,
        /// Foil id when speaker = foil.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        foil: Option<String>,
        text: String,
        /// Message ids this line answers.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        replying_to: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_text_ms: Option<u64>,
    },
    Op {
        ts_ms: TsMs,
        op: Op,
    },
    Vote {
        ts_ms: TsMs,
        poll_id: String,
        tallies: Vec<VoteTally>,
        closed: bool,
    },
    Moderation {
        ts_ms: TsMs,
        stage: ModStage,
        rule: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        msg_id: Option<String>,
        action: String,
    },
    Handover {
        ts_ms: TsMs,
        from: SegmentId,
        to: SegmentId,
        reason: String,
    },
    /// Background job lifecycle (research subagents etc.).
    Job {
        ts_ms: TsMs,
        job_id: String,
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    /// What the orchestrator chose for this beat: the record that makes its choices judgeable.
    BeatPlan {
        ts_ms: TsMs,
        mode: crate::prep::BeatMode,
        /// The prepared bit or cold-open beat it voiced, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bit_id: Option<String>,
        /// Chat messages passed to the voice this beat.
        picks: u32,
        reason: String,
    },
    /// The brain's view of the engine fleet changed (WP sandbox): written on every
    /// transition, so an outage and its recovery are on the record.
    Health {
        ts_ms: TsMs,
        state: FleetHealth,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    StreamEnd {
        ts_ms: TsMs,
        reason: EndReason,
    },
}

/// The brain's fleet mode as recorded by [`StreamEvent::Health`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FleetHealth {
    Healthy,
    Degraded,
    Down,
}
