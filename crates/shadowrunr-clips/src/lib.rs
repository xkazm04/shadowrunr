//! The clip factory and its publish gate (docs/spec/contract.md, appendix "Clips";
//! docs/design/obsidian-program-digest.md § 7).
//!
//! A stream transcript is read as beats of spoken lines, on the voice's real timing when the
//! run has a `voice.jsonl`. Each beat is scored from independent signals, each normalised to
//! its own stream: the host's moment markers (capped in rate and label length), the crowd's
//! distinct-chatter z-score, programme loudness (measured on the show's recording when the run
//! has one, else unmeasured) and a model's hook read of the window (one cheapest-tier call per
//! run). The score is `0.35·marker + 0.25·chat + 0.15·audio + 0.25·hook` with the weights of
//! unmeasured signals redistributed, and it is reported as an unmeasured design hypothesis.
//! A peak becomes a window that reaches back by a pre-roll for its setup, snaps to whole
//! lines and is clamped to its segment's style band. The best five per stream are drafted,
//! checked by the gate, and wait for the owner's signature. A render cuts a recorded run's
//! footage (a 16:9 trim and a 9:16 reframe around the face and captions); a run without a
//! recording is re-drawn by the stage (a vertical 1080x1920 video when the tools exist, else a
//! poster frame plus caption tracks). Upload adapters only write the request they would send.

pub mod footage;
pub mod gate;
pub mod hook;
pub mod host;
pub mod render;
pub mod scan;
pub mod signals;
pub mod store;
pub mod transcript;
pub mod upload;
pub mod voice;
pub mod window;

#[cfg(test)]
mod tests;

pub use host::ClipHost;
pub use render::{probe_tools, RenderTools, ToolProbe};
pub use scan::{scan, RunInput, ScanOptions};
pub use store::ClipStore;
pub use upload::{adapter_for, UploadAdapter};

use shadowrunr_types::clips::ClipWeights;
use shadowrunr_types::TsMs;

/// The digest's blend (§ 7). A design hypothesis: no editor's independent picks have scored
/// it yet, so every ranking carries [`WEIGHTS_NOTE`].
pub const NOMINAL_WEIGHTS: ClipWeights = ClipWeights {
    marker: 0.35,
    chat: 0.25,
    audio: 0.15,
    hook: 0.25,
};

/// The provenance every scoring record and scan report carries.
pub const WEIGHTS_PROVENANCE: &str = "unmeasured: design hypothesis from docs/design/obsidian-program-digest.md § 7; not yet scored against an editor's independent picks or clip retention";

/// The label beside every ranking.
pub const WEIGHTS_NOTE: &str = "weights unmeasured (0.35 marker, 0.25 chat, 0.15 audio, 0.25 hook; an unmeasured signal's share, such as audio for a run without a recording, is redistributed over the measured ones)";

/// Candidates kept per stream.
pub const TOP_PER_STREAM: usize = 5;

/// The vertical draft's frame.
pub const WIDTH: u32 = 1080;
pub const HEIGHT: u32 = 1920;

#[derive(Debug, thiserror::Error)]
pub enum ClipError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    /// The request conflicts with the candidate's state (approving a held clip, publishing
    /// one the owner has not approved).
    #[error("{0}")]
    Conflict(String),
    #[error("io: {0}")]
    Io(String),
}

impl ClipError {
    /// The contract's error code name for this error.
    pub fn code(&self) -> &'static str {
        match self {
            ClipError::NotFound(_) => "not_found",
            ClipError::Invalid(_) => "invalid_params",
            ClipError::Conflict(_) => "conflict",
            ClipError::Io(_) => "io",
        }
    }
}

pub type Result<T> = std::result::Result<T, ClipError>;

pub fn now_ms() -> TsMs {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as TsMs)
        .unwrap_or(0)
}

/// `m:ss` on a stream clock.
pub fn clock(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}
