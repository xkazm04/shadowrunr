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
    /// Pin a labelled string between two face-up cards on the evidence board, citing the
    /// returned record that shows it, filed under one of them (appendix (screens and board)).
    PinLink {
        from: String,
        to: String,
        rel: crate::segment::LinkRel,
        record_id: String,
    },
    /// Switch the broadcast's main screen (appendix (screens and board)). Only the screens the
    /// current segment (and state) allows; `record_id` names a staged case record for
    /// `browser`/`desk`. `focus` names a case hex the map or board zooms to on
    /// `hexmap`/`board`/`desk` (absent: the overview).
    ShowScreen {
        screen: ScreenId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focus: Option<String>,
    },
}

/// What fills the broadcast's main screen. `host` is the big-avatar room; the others put the
/// content full-size with Malakor cut out in the bottom-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreenId {
    /// The big-avatar room (the cold open, the improvised room).
    Host,
    /// The case hexmap full-size. No segment shows it now; kept for older transcripts.
    Hexmap,
    /// A browser-styled window on one staged case record.
    Browser,
    /// Both windows: the browser in front of the board.
    Desk,
    /// The evidence board: the case's cards and the strings pinned between them.
    Board,
}

/// One staged case record as the stage shows it in the browser window: the segment runtime's
/// record (PII-scrubbed for a real case) with nothing added. Absent provenance is omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ShownRecord {
    /// `DOC-01`, `REP-02`... (the prefix is the grade).
    pub id: String,
    pub hex: String,
    pub grade: EvidenceGrade,
    pub title: String,
    /// The publisher's URL (a reserved `*.example` URL for a fictional record).
    pub source: String,
    pub summary: String,
    /// `YYYY-MM-DD` the research pipeline fetched it (real cases only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieved: Option<String>,
    /// The Wayback capture it came through, when the publisher refused the fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    pub fictional: bool,
    /// When the record's event happened, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
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
    /// The character the stream runs (`stream_start.character` names it). Reads the v1 wire
    /// value `malakor` too, so old transcripts and fixtures still load.
    #[serde(alias = "malakor")]
    Host,
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
    /// A staged case record the stage may show (sent before the `show_screen` that names it).
    RecordShown {
        ts_ms: TsMs,
        record: ShownRecord,
    },
    /// A string the runtime accepted onto the evidence board (`pin_link`), with its record's
    /// grade so the stage draws below-DOC as alleged.
    LinkPinned {
        ts_ms: TsMs,
        from: String,
        to: String,
        rel: crate::segment::LinkRel,
        record_id: String,
        grade: EvidenceGrade,
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
