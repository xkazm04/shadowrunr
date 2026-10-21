//! Where an MCP write or pre-flight runs. When a daemon answers on `[daemon] port`, `start_sim`
//! and `preflight` go to it over `/rpc`, so its guards (the sandbox/sim exclusion,
//! `max_live_runs`, one run per run_id, run-dir custody) and its engine fleet are the only
//! ones; an MCP call never builds a second fleet beside it. With no daemon the MCP server runs
//! them in-process, as before.
//!
//! The daemon is probed per call (`engine.status`, [`PROBE_TIMEOUT`]); read tools stay
//! in-process (they read the same files).

use std::time::Duration;

use serde_json::{json, Value};
use shadowrunr_types::ErrorCode;

use crate::cli::DaemonClient;
use crate::rpc::RpcError;

/// How long the probe waits for a daemon to answer.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// A daemon that answered the probe.
pub struct Daemon {
    port: u16,
    /// The daemon runs the scripted engine (no fleet).
    scripted: bool,
}

/// The daemon on `port`, `None` when nothing answers there. A daemon that answers with an
/// error is an error: the call is refused rather than run beside it.
pub async fn probe(port: u16) -> Result<Option<Daemon>, RpcError> {
    let probe = DaemonClient::with_timeout(port, PROBE_TIMEOUT);
    match probe.call_envelope("engine.status", json!({})).await {
        Err(_) => Ok(None),
        Ok(Err(e)) => Err(e),
        Ok(Ok(status)) => Ok(Some(Daemon {
            port,
            scripted: status["fleet"]["mode"] == "scripted",
        })),
    }
}

impl Daemon {
    /// `scripted` or `live`: the engine the daemon's runs use.
    pub fn engine(&self) -> &'static str {
        if self.scripted {
            "scripted"
        } else {
            "live"
        }
    }

    /// Refuse a sim whose `live` flag disagrees with the daemon's engine, in both directions:
    /// a live request must not build a second fleet, a scripted one must not spend live quota.
    pub fn check_engine(&self, live: bool) -> Result<(), RpcError> {
        if live == !self.scripted {
            return Ok(());
        }
        let asked = if live { "live" } else { "scripted" };
        Err(RpcError::new(
            ErrorCode::Conflict,
            format!(
                "the daemon on port {} runs the {} engine; start_sim asked for {asked} (pass live: {})",
                self.port,
                self.engine(),
                !self.scripted
            ),
        )
        .with_detail(json!({
            "reason": "engine_mismatch",
            "daemon_engine": self.engine(),
            "requested": asked,
        })))
    }

    /// One `/rpc` call; the daemon's error envelope comes back whole. A daemon that answered
    /// the probe and then went silent is `unavailable` (the call is not retried in-process:
    /// it may have reached the daemon).
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        DaemonClient::new(self.port)
            .call_envelope(method, params)
            .await
            .map_err(|m| {
                RpcError::new(
                    ErrorCode::Unavailable,
                    format!("daemon on port {}: {m}", self.port),
                )
            })?
    }
}
