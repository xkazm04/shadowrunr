//! The `shadowrunr` binary as a library: CLI, config, pre-flight, and the daemon
//! (`/events` WebSocket + `/rpc`). See docs/guide/cli.md and docs/spec/contract.md.

pub mod case_cli;
pub mod casework;
pub mod catalog;
pub mod cli;
pub mod clips;
pub mod clips_cli;
pub mod config;
pub mod engine;
pub mod events;
pub mod integrations;
pub mod live;
pub mod paging;
pub mod policy_install;
pub mod preflight;
pub mod report_wire;
pub mod review;
pub mod rpc;
pub mod runs;
pub mod sandbox;
pub mod server;
pub mod voice;

#[cfg(test)]
mod e2e;
