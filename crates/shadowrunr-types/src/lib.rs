//! Wire contract for Shadowrunr OS. Every identifier here is load-bearing: the console, the
//! stage and the transcript files read these exact serde names. Change one only together with
//! `docs/spec/contract.md`.
//!
//! Absent-value convention: optional fields are omitted from JSON (`skip_serializing_if`),
//! never written as `null`.

pub mod chat;
pub mod engine;
pub mod event;
pub mod memory;
pub mod segment;
pub mod sim;

pub use chat::*;
pub use engine::*;
pub use event::*;
pub use memory::*;
pub use segment::*;
pub use sim::*;

/// Milliseconds since the Unix epoch, or since stream start inside a simulation.
pub type TsMs = i64;
