//! Long-term memory: markdown truth, SQLite FTS5 index, sleep-cycle consolidation.

pub mod consolidate;
pub mod markdown;
pub mod store;

pub use consolidate::{consolidate, should_consolidate, ConsolidationReport};
pub use store::{
    habit_key, rebuild_index, verify_index, CycleRecord, DecayReport, FactStatus, HabitStatus,
    IndexCheck, MemoryStore, NewHabit, Procedural, RebuildReport, StoredFact,
};
