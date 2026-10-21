//! Segments as data: the rank ladder and vote weights, the chat command parser, table-driven
//! segments (SEG-01 Improvised Room, SEG-02A Hexmap Investigation, descoped SEG-02B-I/II),
//! the SEG-02A case file and re-search cap, the deterministic [`SegmentRuntime`], and the
//! segment portfolio lifecycle.
//!
//! Data files live under `data/` and are compiled in; ranks, the allowlist, cases and case
//! files can also be loaded from disk at runtime. Real cases staged by the off-stream research
//! pipeline live in the [`cases`] store under `<data>/cases/`.

pub mod case;
pub mod cases;
pub mod command;
pub mod evidence;
pub mod portfolio;
pub mod ranks;
pub mod research;
pub mod runtime;
pub mod table;

use shadowrunr_types::{Rank, SegmentId};

pub use case::{demo_case, load_case, parse_case};
pub use cases::{CaseMeta, CaseStatus, CaseStore, StagedCase, StreamKind, SubjectCategory};
pub use command::{normalize_hex, parse_command, parse_grade};
pub use evidence::{requires_alleged, ResearchAllowlist, SourceAllowlist};
pub use portfolio::{next_status, PortfolioEntry};
pub use ranks::RankTable;
pub use research::{target_hex, CaseFile, CaseRecord};
pub use runtime::{Lead, PinnedLink, Proposal, SegmentConfig, SegmentRuntime};
pub use table::{
    active_segments, descoped_segments, segment_is_descoped, segment_name, segment_table,
    HandoverRule, SegmentTable, DEFAULT_MAX_SEARCHES_PER_HEX,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SegmentError {
    #[error("segment {} ({title}) is descoped and cannot be started", segment_name(*segment))]
    Descoped { segment: SegmentId, title: String },
    #[error("data file {file}: {detail}")]
    Data { file: String, detail: String },
    #[error("config: {detail}")]
    Config { detail: String },
    #[error("io {path}: {detail}")]
    Io { path: String, detail: String },
    /// A live stream asked for a real case the owner has not approved for stream (or whose
    /// content changed since the approval).
    #[error(
        "case {case_id} is a real case not approved for stream: the owner approves it with `shadowrunr case approve {case_id}`"
    )]
    NotApproved { case_id: String },
}

/// Rank from subscription facts, using the built-in ladder (`data/ranks.toml`).
/// Non-subscribers are `Thrall`. Use [`RankTable::rank_for`] for a table loaded at runtime.
pub fn rank_for(sub_tier: Option<u8>, months: u32, gifted: u32) -> Rank {
    RankTable::builtin().rank_for(sub_tier, months, gifted)
}

/// Vote weight for a rank from the built-in ladder; grows sub-linearly. `Thrall` = 1.0.
pub fn vote_weight(rank: Rank) -> f64 {
    RankTable::builtin().vote_weight(rank)
}

/// Inputs to the Segment Engagement Score; each is already z-normalised by the caller.
#[derive(Debug, Clone, Copy, Default)]
pub struct SesInputs {
    pub chat_msgs_min_z: f64,
    pub poll_vote_rate_z: f64,
    pub ccv_retention_delta_z: f64,
    pub clip_48h_watch_pct_z: f64,
}

/// SES = 0.30·chat + 0.25·poll + 0.25·retention + 0.20·clip (weights are assumptions).
pub fn ses(i: SesInputs) -> f64 {
    0.30 * i.chat_msgs_min_z
        + 0.25 * i.poll_vote_rate_z
        + 0.25 * i.ccv_retention_delta_z
        + 0.20 * i.clip_48h_watch_pct_z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_functions_use_builtin_ladder() {
        assert_eq!(rank_for(None, 0, 0), Rank::Thrall);
        assert_eq!(rank_for(Some(3), 12, 0), Rank::DreadCouncil);
        assert_eq!(vote_weight(Rank::Thrall), 1.0);
        assert_eq!(vote_weight(Rank::DreadCouncil), 2.4);
    }

    #[test]
    fn ses_weights() {
        let s = ses(SesInputs {
            chat_msgs_min_z: 1.0,
            poll_vote_rate_z: 1.0,
            ccv_retention_delta_z: 1.0,
            clip_48h_watch_pct_z: 1.0,
        });
        assert!((s - 1.0).abs() < 1e-12);
    }
}
