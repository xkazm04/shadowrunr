//! Segments, their lifecycle, and the SEG-02A investigation map.

use serde::{Deserialize, Serialize};

use crate::chat::EvidenceGrade;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentStatus {
    Pilot,
    Trial,
    Regular,
    Flagship,
    Retired,
    Descoped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HexKind {
    Entity,
    Wallet,
    Domain,
    ShellCompany,
    Claim,
}

/// One hex of a SEG-02A case map. Axial coordinates (q, r).
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// What the segment runtime tells the brain each tick. Produced by
/// `shadowrunr_segments::SegmentRuntime::context`, consumed by `shadowrunr_brain::Brain::tick`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentContext {
    pub segment: SegmentId,
    /// Rules of the segment as prompt text (stable within a segment, so cache-friendly).
    pub rules_prompt: String,
    /// Current state: open poll, active foil, revealed hexes, elapsed time...
    pub state_summary: String,
    /// `Op` tags (serde names) the brain may emit in this segment.
    pub allowed_ops: Vec<String>,
    /// Concurrent viewers right now.
    pub ccv: u32,
    pub elapsed_s: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    pub title: String,
    /// True for fabricated demo or simulation cases. Must never be presented as real.
    pub fictional: bool,
    pub hexes: Vec<Hex>,
    /// Pairs of hex ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<(String, String)>,
}
