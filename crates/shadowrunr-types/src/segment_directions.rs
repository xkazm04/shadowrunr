//! Segment directions: the owner's notes on how a character runs one segment, written while
//! reviewing a show. One markdown file per segment in the character package,
//! `segments/<SEG-ID>.md`, read into that segment's per-turn section while it is live. The
//! daemon reads and writes it inside `paths.characters` only; the owner commits it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{SegmentId, TsMs};

/// `segments.directions.get`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentDirectionsGetParams {
    pub character: String,
    pub segment_id: SegmentId,
}

/// `segments.directions.set`: replace `segments/<SEG-ID>.md` with `text`. An empty text
/// leaves an empty file, which the brain ignores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentDirectionsSetParams {
    pub character: String,
    pub segment_id: SegmentId,
    pub text: String,
}

/// `segments/<SEG-ID>.md` as it stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SegmentDirections {
    pub character: String,
    pub segment_id: SegmentId,
    /// Empty when the file does not exist.
    pub text: String,
    /// The file's last write; absent when the file does not exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_ms: Option<TsMs>,
}
