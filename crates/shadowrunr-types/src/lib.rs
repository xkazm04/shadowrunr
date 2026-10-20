//! Wire contract for Shadowrunr OS. Every identifier here is load-bearing: the console, the
//! stage and the transcript files read these exact serde names. Change one only together with
//! `docs/spec/contract.md`.
//!
//! `rpc`, `catalog`, `reconcile`, `results` and `clips` hold the daemon protocol types (reach them by module path);
//! `schema` exports the whole contract as JSON Schema for `@shadowrunr/contract`.
//!
//! Absent-value convention: optional fields are omitted from JSON (`skip_serializing_if`),
//! never written as `null`.

pub mod catalog;
pub mod chat;
pub mod clips;
pub mod engine;
pub mod error;
pub mod event;
pub mod memory;
pub mod reconcile;
pub mod results;
pub mod rpc;
pub mod sandbox;
pub mod schema;
pub mod segment;
pub mod sim;

pub use chat::*;
pub use engine::*;
pub use error::*;
pub use event::*;
pub use memory::*;
pub use segment::*;
pub use sim::*;

/// Milliseconds since the Unix epoch, or since stream start inside a simulation.
pub type TsMs = i64;
