//! The clip factory and its publish gate (docs/spec/contract.md, appendix "Clips").
//!
//! A clip candidate is a window of a stream transcript, scored from independent signals,
//! rendered as a vertical 1080x1920 draft, checked by the gate and approved by the owner
//! before any upload adapter may touch it. Every candidate is stored as
//! `<data>/clips/<id>/candidate.json`; the render files sit beside it.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::chat::EvidenceGrade;
use crate::segment::SegmentId;
use crate::TsMs;

/// The publish states. `rendered_draft -> pending_owner_gate -> approved_for_upload ->
/// published`, or `rejected` from any state before `published`. Only the owner moves a
/// candidate to `approved_for_upload` or `rejected`; only a real upload moves it to
/// `published` (dry runs never do).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ClipStatus {
    RenderedDraft,
    PendingOwnerGate,
    ApprovedForUpload,
    Published,
    Rejected,
}

/// What made a moment a marker. `mark_moment` is the host's own action; the others are
/// structural beats of the show read from the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    /// `op: mark_moment` (the host's mark, with its label).
    MarkMoment,
    /// A foil line answered by Malakor in the same or the next beat (FOIL_ROAST).
    FoilRoast,
    /// `op: reveal_hex` (HEX_REVEAL).
    HexReveal,
    /// A closed vote or a case's last hex annexed: the RELEASE_ANNEXED equivalent (releases
    /// belong to the descoped SEG-02B-II).
    VerdictSealed,
}

/// One marker the scorer read, admitted or capped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipMarker {
    pub kind: MarkerKind,
    /// Emission time on the stream clock.
    pub ts_ms: TsMs,
    pub label: String,
    /// Counted by the scorer. A capped marker is kept with its reason.
    pub admitted: bool,
    /// `label_empty`, `label_too_long`, `cooldown` or `segment_ceiling`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capped: Option<String>,
    /// A chat message asked for a clip just before the mark ("clip that"): the gate shows it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub chat_requested: bool,
}

/// The fused signals of one candidate, each 0..=1. An absent value is unmeasured for this
/// window (never scored as zero).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipSignals {
    /// The strongest admitted marker in the anchor beat (kind weight).
    pub marker: f64,
    /// Crowd reaction: the distinct-chatter z-score against the stream's own baseline, clipped
    /// to 0..=3 and divided by 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<f64>,
    /// The raw z-score behind `chat`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_z: Option<f64>,
    /// Programme level: the window's RMS level against the whole recording's,
    /// `clamp((window - stream + 6 dB) / 12 dB, 0, 1)`. Unmeasured without a recording.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<f64>,
    /// The window's RMS level behind `audio` (dBFS, ffmpeg `astats`, ungated: pauses count).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_db: Option<f64>,
    /// The whole recording's RMS level `audio` is read against (dBFS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_stream_db: Option<f64>,
    /// The model's hook read of the window's lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook: Option<f64>,
    /// The model's one-line reason for `hook`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_why: Option<String>,
    /// Why a signal is unmeasured here: `{"audio": "no audio in the media-less phase"}`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unmeasured: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipWeights {
    pub marker: f64,
    pub chat: f64,
    pub audio: f64,
    pub hook: f64,
}

/// How the score was fused: the nominal weights, the weights actually applied after the
/// unmeasured signals' share was redistributed, and where the weights come from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipScoring {
    pub nominal: ClipWeights,
    pub effective: ClipWeights,
    /// Always "unmeasured" until a human editor's independent picks have scored the weights.
    pub provenance: String,
    /// Signals that agreed (each at or above its firing level).
    pub agreeing: Vec<String>,
}

/// `clip`: two or more independent signals agree. `review`: one signal carries it, or the
/// window could not hold its setup; it goes to the owner as a review case, never straight
/// to the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClipLane {
    Clip,
    Review,
}

/// The cut, on two clocks: the stream clock (`*_ms`, the transcript's `ts_ms`) and the
/// spoken clock the draft plays on (`spoken_*_ms`, the caption timing model), because a
/// simulated beat holds more speech than its tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipWindow {
    /// The beat the score peaked on, corrected for its signal's lag.
    pub anchor_ms: TsMs,
    /// First and last stream-clock beats inside the cut.
    pub start_ms: TsMs,
    pub end_ms: TsMs,
    pub spoken_start_ms: i64,
    pub spoken_end_ms: i64,
    /// `spoken_end_ms - spoken_start_ms` in seconds.
    pub duration_s: f64,
    /// `marker`, `crowd` or `hook`: which signal placed the anchor.
    pub driver: String,
    /// The lag applied before the pre-roll (the crowd's reaction delay; 0 for a marker).
    pub lag_ms: i64,
    /// The pre-roll reached back before the anchor beat (spoken clock).
    pub preroll_ms: i64,
    /// The style's duration band in seconds, `[min, max]`.
    pub band_s: [u32; 2],
    /// `seg01-split-stage` (25-35 s), `seg02a-alleged` (45-60 s) or `default` (15-60 s).
    pub style: String,
    /// The setup and payoff could not fit the band: re-author, do not cut.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub needs_reauthoring: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The spoken clock is the voice's real timing from the run's `voice.jsonl` (each voiced
    /// line from its first `played` to its last part's `played` plus its duration), on the
    /// stream clock. Absent: the caption model's estimate.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub voice_timed: bool,
}

/// Where the candidate came from. `transcript` is relative to the configured runs root
/// (or the file name when the transcript lies outside it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
    pub run_id: String,
    pub stream_id: String,
    pub transcript: String,
    /// 1-based transcript lines of the first and last event inside the cut.
    pub line_from: u32,
    pub line_to: u32,
}

/// One hex a SEG-02A cut reveals or names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipHex {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<EvidenceGrade>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
}

/// The claim context a SEG-02A (or case-linked) cut carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipEvidence {
    pub case_id: String,
    pub case_title: String,
    /// A fabricated demo or simulation case: the header says so.
    pub fictional: bool,
    pub hexes: Vec<ClipHex>,
    /// The weakest grade among `hexes` (the badge); absent when the cut makes no hex claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worst_grade: Option<EvidenceGrade>,
    /// Source URLs shown in the picture, at most three.
    pub citation: Vec<String>,
}

/// One caption cue: the exact transcript line (viewer handles redacted), timed on the
/// draft's spoken clock from the clip's start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipCaption {
    pub start_ms: i64,
    pub end_ms: i64,
    /// `malakor`, or the foil id.
    pub speaker: String,
    pub text: String,
    /// On a voice-timed cut: the line was never heard (the voice skipped it, the stage dropped
    /// it, or it is a foil's), so it keeps its stream-clock time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unvoiced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GateState {
    Pass,
    Fail,
    /// The check could not run. It holds the clip like a fail; the owner may override it
    /// with a recorded reason.
    Unmeasured,
    NotApplicable,
}

/// A clip publish-gate check, as it crosses the wire. The serde names are the ids
/// `candidate.json` has always carried.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GateCheckId {
    CareState,
    PersonalData,
    BrigadeCall,
    BlockedTerm,
    InjectionReply,
    AiDisclosure,
    HandlesInPicture,
    EvidenceBadgeBurnt,
    SourceCitation,
    NoOpenCorrection,
}

impl GateCheckId {
    /// Every check: the exclusions, then the render and evidence checklist.
    pub const ALL: [Self; 10] = [
        Self::CareState,
        Self::PersonalData,
        Self::BrigadeCall,
        Self::BlockedTerm,
        Self::InjectionReply,
        Self::AiDisclosure,
        Self::HandlesInPicture,
        Self::EvidenceBadgeBurnt,
        Self::SourceCitation,
        Self::NoOpenCorrection,
    ];

    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CareState => "care_state",
            Self::PersonalData => "personal_data",
            Self::BrigadeCall => "brigade_call",
            Self::BlockedTerm => "blocked_term",
            Self::InjectionReply => "injection_reply",
            Self::AiDisclosure => "ai_disclosure",
            Self::HandlesInPicture => "handles_in_picture",
            Self::EvidenceBadgeBurnt => "evidence_badge_burnt",
            Self::SourceCitation => "source_citation",
            Self::NoOpenCorrection => "no_open_correction",
        }
    }
}

impl std::fmt::Display for GateCheckId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// One gate check. Exclusions run on every clip; the mandatory checklist runs when the clip
/// is from SEG-02A or links a case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GateCheck {
    pub id: GateCheckId,
    pub state: GateState,
    pub detail: String,
    /// Part of the SEG-02A / linked-case checklist.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mandatory: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipGate {
    /// `linked_case_id IS NOT NULL OR segment = SEG-02A`.
    pub requires_mandatory_gate: bool,
    pub checks: Vec<GateCheck>,
    /// Any check failed or could not run.
    pub held: bool,
    /// `<check>: <detail>` for every check that holds the clip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hold_reasons: Vec<String>,
    pub evaluated_ms: TsMs,
}

/// One file of a render, relative to the candidate's directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipFile {
    pub path: String,
    /// `video`, `cut_16x9`, `cut_9x16`, `poster`, `captions_srt`, `captions_ass`,
    /// `stage_input`, `manifest`.
    pub kind: String,
    pub bytes: u64,
}

/// What the render burnt into the picture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipOverlay {
    /// The ALLEGED header (SEG-02A and case-linked clips).
    pub alleged_header: bool,
    /// The grade badge text (`DOC`, `REP`, `CLM`, `SPC` or `UNGRADED`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade_badge: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citation: Vec<String>,
    /// "AI character · synthetic media", always drawn.
    pub ai_disclosure: bool,
    /// Where the captions come from: always the transcript text, with the timing model named.
    pub captions: String,
}

/// Where a render's picture comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClipFootage {
    /// Trimmed from the show's own recording (`recording.json`): `cut-16x9.mp4` and the
    /// vertical reframe `cut-9x16.mp4`.
    Recording,
    /// Re-drawn by the stage from the transcript (no recording): `draft.mp4` or a poster.
    Redrawn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipRender {
    /// `video` (an MP4 was cut from the recording or recorded from the stage) or `poster` (a
    /// still poster frame plus caption tracks, when the tools are missing).
    pub kind: String,
    /// Absent on renders made before cuts from the recording existed: re-drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footage: Option<ClipFootage>,
    pub width: u32,
    pub height: u32,
    pub duration_s: f64,
    pub files: Vec<ClipFile>,
    pub overlay: ClipOverlay,
    /// Why there is no video (a missing tool, a failed recording), when `kind` is `poster`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub rendered_ms: TsMs,
}

/// The owner's signature on `approved_for_upload`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipApproval {
    pub signature: String,
    pub at_ms: TsMs,
    /// Required when a check was unmeasured; the hold reasons it overrode are kept beside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overridden: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipRejection {
    pub reason: String,
    pub at_ms: TsMs,
    pub by: String,
}

/// One state change, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipTransition {
    pub status: ClipStatus,
    pub at_ms: TsMs,
    /// `scanner`, `gate`, `render` or the owner's signature.
    pub by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ClipPlatform {
    Youtube,
    Tiktok,
    Twitch,
}

/// One upload request the dry run wrote instead of sending.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipDryRun {
    pub platform: ClipPlatform,
    pub at_ms: TsMs,
    /// The request file, relative to the candidate's directory.
    pub file: String,
    /// The platform call can serve this candidate at all (Twitch Create Clip needs a live
    /// broadcast).
    pub applicable: bool,
    /// Uploads left today under the platform's daily budget, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_left: Option<u32>,
    pub note: String,
}

/// The show recording a candidate is cut from: the run's `recording.json`, read at scan time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipRecording {
    /// The recording file (the run directory's `recording.<ext>`), as found at scan time.
    pub file: String,
    /// The stream-clock time of the file's first frame: a stream time `t` sits at
    /// `t - stream_offset_ms` in the file.
    pub stream_offset_ms: i64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Speech starts this long after its `played` time, constant, with no drift (measured on
    /// the recorded sessions: +200 ms, within 25 ms). A tolerance the cuts' pre-roll absorbs:
    /// cut points are not shifted by it.
    pub av_offset_ms: i64,
}

/// What a person watching the cut saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClipFeedbackTag {
    Landed,
    Flat,
    VoiceOffSync,
    FaceWrong,
    CaptionWrong,
    CutTooEarly,
    CutTooLate,
    Boring,
}

/// Watched feedback on a cut. It never changes the candidate's state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipFeedback {
    pub tags: Vec<ClipFeedbackTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub at_ms: TsMs,
}

/// A stored clip candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipCandidate {
    /// `clip-<16 hex>`: stable for the same source and cut.
    pub id: String,
    pub created_ms: TsMs,
    pub status: ClipStatus,
    pub score: f64,
    /// 1 = the stream's best.
    pub rank: u32,
    pub lane: ClipLane,
    pub segment: SegmentId,
    /// The marker label, or the first line of the payoff.
    pub title: String,
    pub source: ClipSource,
    pub window: ClipWindow,
    pub markers: Vec<ClipMarker>,
    pub signals: ClipSignals,
    pub scoring: ClipScoring,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_case_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<ClipEvidence>,
    pub captions: Vec<ClipCaption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<ClipRender>,
    pub gate: ClipGate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ClipApproval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection: Option<ClipRejection>,
    pub history: Vec<ClipTransition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dry_runs: Vec<ClipDryRun>,
    /// The run's show recording, when it has one: a render cuts it instead of re-drawing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording: Option<ClipRecording>,
    /// Watched feedback (`clips.feedback`); kept across a rescan of the same cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<ClipFeedback>,
}

/// A candidate as `clips.list` shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipSummary {
    pub id: String,
    pub status: ClipStatus,
    pub score: f64,
    pub rank: u32,
    pub lane: ClipLane,
    pub segment: SegmentId,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
    pub run_id: String,
    pub duration_s: f64,
    pub held: bool,
    pub requires_mandatory_gate: bool,
    /// `video`, `poster`, or absent before a render.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<String>,
    pub created_ms: TsMs,
}

/// `clips.list`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipsListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ClipStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// 1..=500; larger values are capped at 500.
    pub limit: u32,
}

/// Candidates by score, best first; `counts` covers every stored candidate by status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipsPage {
    pub items: Vec<ClipSummary>,
    pub counts: BTreeMap<String, u32>,
    /// Candidates waiting at the owner gate that the gate holds (a check failed or could not
    /// run), over the whole store.
    pub held: u32,
    /// Stored candidates matching the filter, over every page.
    pub total: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipIdParams {
    pub id: String,
}

/// What `clips.scan` reads: one run (`run_id`, optionally qualified) or every result run of
/// a tournament.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ClipScanSource {
    Run {
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tournament_id: Option<String>,
    },
    Tournament {
        tournament_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipsScanParams {
    pub source: ClipScanSource,
    /// Ask the model for the hook read (one call per run). Default true; false leaves the
    /// hook unmeasured and redistributes its weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook: Option<bool>,
}

/// One run of a scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipScanRun {
    pub run_id: String,
    pub candidates: u32,
    /// `measured`, `off`, or `unmeasured: <why>` (the engine call failed or did not parse).
    pub hook: String,
    pub markers_admitted: u32,
    pub markers_capped: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipScanReport {
    pub runs: Vec<ClipScanRun>,
    /// New candidates written.
    pub written: u32,
    /// Candidates the owner already acted on, kept as they were.
    pub kept: u32,
    /// Untouched earlier candidates of the same runs, replaced by this scan.
    pub replaced: u32,
    /// The best candidates of this scan, best first (at most 10).
    pub top: Vec<ClipSummary>,
    /// "weights unmeasured: ...": the label every ranking carries.
    pub weights_note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipsApproveParams {
    pub id: String,
    /// Who approves. Recorded; the owner's act, never the scorer's.
    pub signature: String,
    /// Required when a gate check is unmeasured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipsRejectParams {
    pub id: String,
    pub reason: String,
    /// Who rejects; `owner` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipsPublishParams {
    pub id: String,
    /// Every platform when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platforms: Option<Vec<ClipPlatform>>,
}

/// `clips.feedback`: replaces the candidate's watched feedback (any state; it changes none).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipsFeedbackParams {
    pub id: String,
    pub tags: Vec<ClipFeedbackTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `clips.recording`: a sandbox session's show recording, for the Review player's watch pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipsRecordingParams {
    pub session_id: String,
}

/// One line the voice was heard saying, on the stream clock (from `voice.jsonl`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipHeardLine {
    /// The `say` event's transcript line (its seq).
    pub line: u32,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// A session's recording as the console plays it: a stream time `t` is at
/// `t - stream_offset_ms` in the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipRunRecording {
    pub session_id: String,
    /// The file on the daemon: `/media/runs/sandbox/<session_id>/recording.mp4`.
    pub media_path: String,
    pub stream_offset_ms: i64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub av_offset_ms: i64,
    /// Every heard line's real timing, by transcript line; empty without a voice log.
    pub heard: Vec<ClipHeardLine>,
}

/// `clips.publish`: always a dry run in this build. No credentials exist and no adapter
/// makes a network call; each adapter writes the exact request it would send.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipPublishReport {
    pub id: String,
    pub dry_run: bool,
    pub requests: Vec<ClipDryRun>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_check_ids_round_trip_under_their_wire_names() {
        let wire = [
            "care_state",
            "personal_data",
            "brigade_call",
            "blocked_term",
            "injection_reply",
            "ai_disclosure",
            "handles_in_picture",
            "evidence_badge_burnt",
            "source_citation",
            "no_open_correction",
        ];
        assert_eq!(GateCheckId::ALL.len(), wire.len());
        for (id, name) in GateCheckId::ALL.iter().zip(wire) {
            let s = serde_json::to_string(id).unwrap();
            assert_eq!(s, format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<GateCheckId>(&s).unwrap(), *id);
            assert_eq!(id.as_str(), name);
            assert_eq!(id.to_string(), name);
        }
        let e =
            serde_json::from_str::<GateCheck>(r#"{"id":"care-state","state":"pass","detail":""}"#)
                .expect_err("a typo must not parse as a new check");
        assert!(e.to_string().contains("unknown variant"), "{e}");
    }
}
