//! `shadowrunr preflight --fix [--download]`: apply the safe remedies pre-flight authored,
//! then re-probe. Pre-flight itself stays read-only; this is the separate, explicit path.
//!
//! - Every fix is a mutation the operator asked for with `--fix`; a network install also
//!   needs `--download` (confirmation scales with blast radius).
//! - A fix never deletes owner data: a reindex rebuilds a derived index from the markdown, and
//!   a list install only writes a list that has no installed copy yet.
//! - The success claim is the re-probe: a fix whose row did not improve is reported as not
//!   fixed. Running it again on a repaired store finds nothing to fix.

use std::time::Duration;

use serde::Serialize;
use shadowrunr_types::HealthState;

use crate::cli::{CallError, DaemonClient};
use crate::config::Config;
use crate::engine::EngineHandle;
use crate::preflight::{self, Checked, Fix, Remedy};

/// How long the "is a daemon holding the index?" probe waits.
const DAEMON_PROBE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, Default)]
pub struct FixOptions {
    /// Allow fixes that fetch from the network (term-list installs).
    pub download: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixStatus {
    Fixed,
    NotFixed,
    NeedsDownload,
}

/// What happened to one row's applied remedy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixResult {
    pub status: FixStatus,
    pub message: String,
}

impl FixResult {
    fn new(status: FixStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    /// `fixed: ...`, `not fixed: ...` or `needs --download: ...`.
    pub fn line(&self) -> String {
        let label = match self.status {
            FixStatus::Fixed => "fixed",
            FixStatus::NotFixed => "not fixed",
            FixStatus::NeedsDownload => "needs --download",
        };
        format!("{label}: {}", self.message)
    }
}

/// One row of the before -> after board.
#[derive(Debug, Clone, Serialize)]
pub struct FixRow {
    pub check: String,
    pub before: HealthState,
    pub after: HealthState,
    /// The re-probe's detail.
    pub detail: String,
    /// The remedy the row carried before the fix pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remedy: Option<Remedy>,
    /// The re-probe's remedy, when something is still to do.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<Remedy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<FixResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixOutcome {
    pub rows: Vec<FixRow>,
    /// No row carried an applied remedy.
    pub nothing_to_fix: bool,
}

impl FixOutcome {
    pub fn any_red(&self) -> bool {
        self.rows.iter().any(|r| r.after == HealthState::Red)
    }
}

/// Probe, apply the applied remedies `opts` allows, re-probe.
pub async fn apply(cfg: &Config, engine: &EngineHandle, opts: FixOptions) -> FixOutcome {
    let before = preflight::run_all_checked(cfg, engine).await;
    let mut results: Vec<Option<FixResult>> = Vec::with_capacity(before.len());
    for c in &before {
        results.push(match &c.remedy {
            Some(Remedy::Applied { fix, .. }) => Some(attempt(cfg, fix, opts).await),
            _ => None,
        });
    }
    let nothing_to_fix = results.iter().all(Option::is_none);
    let after = preflight::run_all_checked(cfg, engine).await;
    let rows = before
        .into_iter()
        .zip(results)
        .map(|(b, result)| board_row(b, result, &after))
        .collect();
    FixOutcome {
        rows,
        nothing_to_fix,
    }
}

fn board_row(before: Checked, result: Option<FixResult>, after: &[Checked]) -> FixRow {
    let a = after.iter().find(|a| a.report.check == before.report.check);
    let after_state = a.map_or(before.report.state, |a| a.report.state);
    // The claim is the re-probe: an applied fix whose row did not improve did not fix it.
    let result = result.map(|r| {
        if r.status == FixStatus::Fixed && after_state == before.report.state {
            FixResult::new(
                FixStatus::NotFixed,
                format!(
                    "{} ran, but the re-probe is still {}",
                    r.message,
                    state_word(after_state)
                ),
            )
        } else {
            r
        }
    });
    FixRow {
        check: before.report.check,
        before: before.report.state,
        after: after_state,
        detail: a.map_or(before.report.detail, |a| a.report.detail.clone()),
        remedy: before.remedy,
        remaining: a.and_then(|a| a.remedy.clone()),
        result,
    }
}

async fn attempt(cfg: &Config, fix: &Fix, opts: FixOptions) -> FixResult {
    match fix {
        Fix::Reindex => reindex(cfg).await,
        Fix::InstallLists { lists } => {
            let what = lists
                .iter()
                .map(|l| format!("fetches {} from {}", l.list_id, l.source))
                .collect::<Vec<_>>()
                .join(", ");
            if !opts.download {
                return FixResult::new(FixStatus::NeedsDownload, what);
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let mut done = Vec::new();
            for l in lists {
                match crate::policy_install::install(&cfg.paths.policy, &l.list_id, None, now).await
                {
                    Ok(m) => done.push(format!("installed {} ({} terms)", m.list_id, m.terms)),
                    Err(e) => {
                        return FixResult::new(FixStatus::NotFixed, format!("{}: {e}", l.list_id))
                    }
                }
            }
            FixResult::new(FixStatus::Fixed, done.join(", "))
        }
    }
}

/// Rebuild the memory index, unless a daemon on the configured port answers: it holds the
/// index open, so the rebuild would race it.
async fn reindex(cfg: &Config) -> FixResult {
    let port = cfg.daemon.port;
    let probe = DaemonClient::with_timeout(port, DAEMON_PROBE)
        .call::<serde_json::Value>("engine.status", serde_json::json!({}))
        .await;
    if !matches!(probe, Err(CallError::Unreachable(_))) {
        return FixResult::new(
            FixStatus::NotFixed,
            format!("the daemon on 127.0.0.1:{port} holds the index; stop it first"),
        );
    }
    let dir = cfg.paths.memory();
    match shadowrunr_brain::rebuild_index(&dir) {
        Ok(_) => FixResult::new(FixStatus::Fixed, "memory reindex"),
        Err(e) => FixResult::new(
            FixStatus::NotFixed,
            format!("reindex {}: {e}", dir.display()),
        ),
    }
}

pub fn state_word(s: HealthState) -> &'static str {
    match s {
        HealthState::Green => "green",
        HealthState::Amber => "amber",
        HealthState::Red => "red",
    }
}

/// The human board: `check  before[ -> after]  result | detail[  fix: ...]`.
pub fn render(outcome: &FixOutcome) -> String {
    let mut out = String::new();
    for r in &outcome.rows {
        let states = if r.before == r.after {
            state_word(r.after).to_string()
        } else {
            format!("{} -> {}", state_word(r.before), state_word(r.after))
        };
        let tail = match (&r.result, &r.remaining) {
            (Some(res), _) => res.line(),
            (None, Some(rem)) => format!("{}  {}", r.detail, rem.hint()),
            (None, None) => r.detail.clone(),
        };
        out.push_str(&format!("{}  {states}  {tail}\n", r.check));
    }
    if outcome.nothing_to_fix {
        out.push_str("nothing to fix\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(state: HealthState, remedy: Option<Remedy>) -> Checked {
        Checked {
            report: shadowrunr_types::HealthReport {
                check: "PF-03".into(),
                state,
                detail: "d".into(),
            },
            remedy,
        }
    }

    #[test]
    fn a_fix_the_re_probe_does_not_confirm_is_not_fixed() {
        let applied = Remedy::Applied {
            fix: Fix::Reindex,
            command: "shadowrunr memory reindex".into(),
        };
        let row = board_row(
            checked(HealthState::Amber, Some(applied.clone())),
            Some(FixResult::new(FixStatus::Fixed, "memory reindex")),
            &[checked(HealthState::Amber, Some(applied))],
        );
        let r = row.result.unwrap();
        assert_eq!(r.status, FixStatus::NotFixed);
        assert!(r.line().contains("still amber"), "{}", r.line());
    }
}
