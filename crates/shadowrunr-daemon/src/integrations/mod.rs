//! Integrations with the operator's other tools. Every one is opt-in, and none is ever fatal
//! or blocking: a missing or unreachable peer degrades to a status line, never to a failed
//! stream, sim or command.
//!
//! Personas (the operator's desktop agent platform), docs/guide/personas.md:
//! - [`door`]: the loopback door (`~/.personas/local-http.json`) and `.personas/project.json`,
//!   read by `shadowrunr personas status`;
//! - [`activity`]: matching daemon and CLI events forwarded as HMAC-signed webhook calls;
//! - [`mcp`]: `shadowrunr mcp`, a stdio MCP server over the same handlers as `/rpc`.
//!
//! The `[personas]` section of `shadowrunr.toml` is read here ([`settings`]), not by
//! `crate::config`, so the integration stays self-contained.

pub mod activity;
pub mod door;
pub mod forwarder;
pub mod mcp;
pub mod mcp_schema;
pub mod mcp_tools;
pub mod settings;
pub mod signing;
pub mod status;

pub use settings::PersonasSettings;
