//! Segments as data. WP3 owns this crate; the skeleton fixes the surface sim and daemon call.

use shadowrunr_types::{ChatMsg, Command, Op, Rank, SegmentContext, SegmentId, StreamEvent, TsMs};

/// Rank from subscription facts (weighted ladder). Non-subscribers are `Thrall`.
pub fn rank_for(sub_tier: Option<u8>, months: u32, gifted: u32) -> Rank {
    let _ = (sub_tier, months, gifted);
    todo!("WP3: rank ladder")
}

/// Vote weight for a rank; grows sub-linearly. `Thrall` = 1.0.
pub fn vote_weight(rank: Rank) -> f64 {
    let _ = rank;
    todo!("WP3: vote weights")
}

/// Parse `!summon`, `!hint`, `!grade`, `!propose`, `!vote` from chat text.
pub fn parse_command(text: &str) -> Option<Command> {
    let _ = text;
    todo!("WP3: command parser")
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

pub struct SegmentRuntime {
    _segment: SegmentId,
}

impl SegmentRuntime {
    pub fn new(segment: SegmentId) -> Self {
        Self { _segment: segment }
    }

    pub fn context(&self, ccv: u32, elapsed_s: u32) -> SegmentContext {
        let _ = (ccv, elapsed_s);
        todo!("WP3: segment context")
    }

    /// Commands and votes from new chat; returns Vote/Moderation events.
    pub fn ingest_chat(&mut self, now_ms: TsMs, msgs: &[ChatMsg]) -> Vec<StreamEvent> {
        let _ = (now_ms, msgs);
        todo!("WP3: ingest chat")
    }

    /// Validate and apply the brain's ops (reject ops not allowed in this segment).
    pub fn apply_ops(&mut self, now_ms: TsMs, ops: &[Op]) -> Vec<StreamEvent> {
        let _ = (now_ms, ops);
        todo!("WP3: apply ops")
    }

    /// Clock: poll timeouts, handover rule (SEG-01 -> SEG-02A).
    pub fn tick(&mut self, now_ms: TsMs, ccv: u32) -> Vec<StreamEvent> {
        let _ = (now_ms, ccv);
        todo!("WP3: segment clock")
    }
}
