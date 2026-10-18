//! The `shadowrunr` binary as a library: CLI, config, pre-flight, and the daemon
//! (`/events` WebSocket + `/rpc`). See docs/guide/cli.md and docs/spec/contract.md.

pub mod cli;
pub mod config;
pub mod engine;
pub mod live;
pub mod preflight;
pub mod rpc;
pub mod runs;
pub mod server;
