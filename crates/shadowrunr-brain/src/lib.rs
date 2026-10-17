//! The character's mind. WP2 owns this crate; the skeleton fixes the surface sim and daemon
//! call in parallel.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use shadowrunr_types::{ChatMsg, Engine, ModelTier, SegmentContext, StreamEvent, TsMs};

#[derive(Debug, thiserror::Error)]
pub enum BrainError {
    #[error("character package: {0}")]
    Package(String),
    #[error("memory store: {0}")]
    Store(String),
    #[error(transparent)]
    Engine(#[from] shadowrunr_types::EngineError),
}

/// A loaded character package (`characters/<id>/`, layout in docs/spec/contract.md).
#[derive(Debug, Clone)]
pub struct CharacterPackage {
    pub id: String,
    pub dir: PathBuf,
    /// Immutable constitution: identity, awareness, red lines.
    pub constitution: String,
    /// Selected tone variant id and its text.
    pub tone_variant: String,
    pub tone_text: String,
}

pub fn load_character(dir: &Path, tone_variant: &str) -> Result<CharacterPackage, BrainError> {
    let _ = (dir, tone_variant);
    todo!("WP2: load characters/<id>/ package")
}

#[derive(Debug, Clone)]
pub struct BrainConfig {
    pub character: CharacterPackage,
    /// Memory root (markdown truth + SQLite index). A sim run gets a throwaway directory.
    pub memory_dir: PathBuf,
    pub stream_id: String,
    pub core_tier: ModelTier,
    /// Max concurrent background jobs.
    pub pool_size: usize,
}

pub struct Brain {
    _cfg: BrainConfig,
    _engine: Arc<dyn Engine>,
}

impl Brain {
    pub async fn new(cfg: BrainConfig, engine: Arc<dyn Engine>) -> Result<Self, BrainError> {
        Ok(Self {
            _cfg: cfg,
            _engine: engine,
        })
    }

    /// One beat of the core thread: given new chat since the last tick and the segment
    /// context, pick messages (anti-bait), speak, emit ops, dispatch/collect jobs.
    /// Returns transcript events in order.
    pub async fn tick(
        &mut self,
        now_ms: TsMs,
        inbox: Vec<ChatMsg>,
        ctx: &SegmentContext,
    ) -> Result<Vec<StreamEvent>, BrainError> {
        let _ = (now_ms, inbox, ctx);
        todo!("WP2: core thread tick")
    }

    /// End of stream: write episodes, maybe trigger consolidation.
    pub async fn end_stream(&mut self, now_ms: TsMs) -> Result<(), BrainError> {
        let _ = now_ms;
        todo!("WP2: end-of-stream memory write")
    }
}
