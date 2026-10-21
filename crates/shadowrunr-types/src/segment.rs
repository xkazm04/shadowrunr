//! Segments, their lifecycle, and the SEG-02A investigation map.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::chat::EvidenceGrade;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum SegmentId {
    #[serde(rename = "SEG-01")]
    Seg01ImprovisedRoom,
    #[serde(rename = "SEG-02A")]
    Seg02aHexmap,
    /// Descoped (kept in the plan): Cartography Legion.
    #[serde(rename = "SEG-02B-I")]
    Seg02bCartography,
    /// Descoped (kept in the plan): Dependency Doctrine.
    #[serde(rename = "SEG-02B-II")]
    Seg02bDependency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SegmentStatus {
    Pilot,
    Trial,
    Regular,
    Flagship,
    Retired,
    Descoped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HexKind {
    Entity,
    Wallet,
    Domain,
    ShellCompany,
    Claim,
}

/// One hex of a SEG-02A case map. Axial coordinates (q, r).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Hex {
    pub id: String,
    pub q: i32,
    pub r: i32,
    pub kind: HexKind,
    pub label: String,
    /// Omitted while the hex is still fogged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<EvidenceGrade>,
    pub fog: bool,
    /// Source URLs; a DOC grade requires at least one allowlisted primary source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// What the card is in the case's story (appendix (screens and board)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<HexRole>,
    /// When it happened, `YYYY-MM-DD`: the board's timeline strip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

/// A card's part in the case's story, drawn as its glyph on the evidence board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HexRole {
    Victim,
    Promoter,
    Shell,
    Processor,
    Regulator,
    Claim,
    Wallet,
    Exchange,
    Person,
    Company,
}

/// What a string pinned between two cards says (`pin_link`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkRel {
    Paid,
    Owns,
    Controls,
    Promoted,
    Processed,
    Regulates,
    Claims,
    Supports,
    Contradicts,
}

/// One act of a case: a region of the evidence board and the cards in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CaseAct {
    pub name: String,
    pub hexes: Vec<String>,
}

/// What the segment runtime tells the brain each tick. Produced by
/// `shadowrunr_segments::SegmentRuntime::context`, consumed by `shadowrunr_brain::Brain::tick`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SegmentContext {
    pub segment: SegmentId,
    /// Rules of the segment as prompt text (stable within a segment, so cache-friendly).
    pub rules_prompt: String,
    /// Current state: open poll, active foil, revealed hexes, elapsed time...
    pub state_summary: String,
    /// The segment state machine's current state (`cold_open`, `opening`, `foil_vote`...);
    /// the orchestrator reads it to know the room is still filling.
    #[serde(default)]
    pub state: String,
    /// `Op` tags (serde names) the brain may emit in this segment.
    pub allowed_ops: Vec<String>,
    /// Concurrent viewers right now.
    pub ccv: u32,
    pub elapsed_s: u32,
    /// SEG-02A: the case's hexes as currently revealed; their grades drive the brain's
    /// "alleged" output gate. Empty outside SEG-02A.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hexes: Vec<Hex>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Case {
    pub id: String,
    pub title: String,
    /// True for fabricated demo or simulation cases. Must never be presented as real.
    pub fictional: bool,
    pub hexes: Vec<Hex>,
    /// Pairs of hex ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<(String, String)>,
    /// What the case asks, in one sentence: the board's heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thesis: Option<String>,
    /// The board's regions, in order. Absent: the runtime's arc plan names them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acts: Vec<CaseAct>,
}
