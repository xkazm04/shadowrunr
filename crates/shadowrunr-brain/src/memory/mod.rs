//! Long-term memory: markdown truth, SQLite FTS5 index, sleep-cycle consolidation.

pub mod consolidate;
pub mod markdown;
pub mod store;

pub use consolidate::{consolidate, should_consolidate, ConsolidationReport};
pub use store::{
    rebuild_index, CycleRecord, DecayReport, FactStatus, MemoryStore, Procedural, RebuildReport,
    StoredFact,
};
