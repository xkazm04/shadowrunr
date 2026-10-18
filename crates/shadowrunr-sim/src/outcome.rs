//! What a run directory amounts to (WP13). An engine failure is not a result:
//!
//! - **`engine_failed`**: the stream was cut by the engine, so the transcript is not a
//!   performance of the character. The rule, first match wins:
//!   1. structured: the transcript ends `stream_end { reason: "fleet_lost" }` (the runner
//!      writes this from WP13 on);
//!   2. legacy: the transcript holds an `op: sign_off` whose reason starts `fleet_lost` (the
//!      brain's fleet-loss sign-off);
//!   3. ledger: the run's stream calls (every role but the judge) show the engine cutting the
//!      core thread: its last [`CUT_TAIL`] calls all failed with an engine error, or more than
//!      [`CUT_SHARE`] of its calls did;
//!   4. a run that failed before `run.json` (`error.json`) whose ledger matches rule 3.
//! - **`judge_pending`**: the transcript is sound, but the judge call failed with an engine
//!   error (rate limit, unavailable, timeout, io), or was deferred because no account could
//!   serve it. Re-judge, never re-run.
//! - **`judge_void`**: content only. The judge replied but abstained, its reply did not parse,
//!   or it refused.
//!
//! Engine errors are the `EngineError` kinds `rate_limited`, `unavailable`, `timeout` and
//! `io`. `refused` and `parse` are content outcomes.

use std::path::Path;

use serde::{Deserialize, Serialize};
use shadowrunr_types::{EndReason, EngineError, Op, Role, StreamEvent};

use crate::ledger::{self, LedgerRow};
use crate::read_json;
use crate::runner::{RunRecord, RunStatus};

/// Ledger `error_kind` values that mean the engine, not the content, failed.
pub const ENGINE_ERROR_KINDS: [&str; 4] = ["rate_limited", "unavailable", "timeout", "io"];
/// The brain's fleet-loss sign-off reason starts with this.
pub const FLEET_LOST_PREFIX: &str = "fleet_lost";
/// The stream is cut when the core thread's last this-many calls all failed on the engine.
pub const CUT_TAIL: usize = 3;
/// ... or when more than this share of its calls did.
pub const CUT_SHARE: f64 = 0.25;
/// Prefix of the `judge_error` the runner records for a failed judge call.
pub const JUDGE_ENGINE_PREFIX: &str = "judge engine call failed: ";
/// Prefix of the `judge_error` for a judge call skipped because no account could serve it.
pub const JUDGE_DEFERRED_PREFIX: &str = "judge deferred: ";

/// How a run directory counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunClass {
    /// `verdict.json` exists: a result.
    Judged,
    /// Simulated with judging off: a result without a verdict.
    Simulated,
    /// The judge replied but gave no usable verdict: a content void.
    JudgeVoid,
    /// Sound transcript, judge call failed on the engine: re-judge.
    JudgePending,
    /// The engine cut the stream: re-run.
    EngineFailed,
    /// Failed before `run.json` (`error.json`) for a reason that is not the engine.
    Failed,
    /// Neither `run.json` nor `error.json`: interrupted.
    Incomplete,
}

impl RunClass {
    /// Whether the run belongs in results and reconcile.
    pub fn is_result(self) -> bool {
        matches!(
            self,
            RunClass::Judged | RunClass::Simulated | RunClass::JudgeVoid | RunClass::Failed
        )
    }
}

pub fn is_engine_error_kind(kind: &str) -> bool {
    ENGINE_ERROR_KINDS.contains(&kind)
}

/// Whether an engine error is the engine's fault (retry later) rather than the content's.
pub fn is_engine_error(e: &EngineError) -> bool {
    matches!(
        e,
        EngineError::RateLimited { .. }
            | EngineError::Unavailable { .. }
            | EngineError::Timeout { .. }
            | EngineError::Io { .. }
    )
}

/// Whether a recorded `judge_error` names an engine failure (also matches the strings
/// written before WP13: `judge engine call failed: <EngineError display>`).
pub fn judge_error_is_engine(msg: &str) -> bool {
    if msg.starts_with(JUDGE_DEFERRED_PREFIX) {
        return true;
    }
    let Some(rest) = msg.strip_prefix(JUDGE_ENGINE_PREFIX) else {
        return false;
    };
    // The `Display` forms of RateLimited, Unavailable, Timeout and Io.
    ["rate limited", "engine unavailable", "timed out", "io:"]
        .iter()
        .any(|p| rest.starts_with(p))
}

/// The fleet-loss signal in a transcript, structured first.
pub fn fleet_lost_in(events: &[StreamEvent]) -> Option<String> {
    let structured = events.iter().any(|e| {
        matches!(
            e,
            StreamEvent::StreamEnd {
                reason: EndReason::FleetLost,
                ..
            }
        )
    });
    if structured {
        return Some("stream_end fleet_lost".into());
    }
    events.iter().find_map(|e| match e {
        StreamEvent::Op {
            op: Op::SignOff { reason },
            ..
        } if reason.starts_with(FLEET_LOST_PREFIX) => Some(format!("sign_off \"{reason}\"")),
        _ => None,
    })
}

/// Rule 3: did engine errors cut the core thread? Judge rows are ignored.
pub fn stream_cut(rows: &[LedgerRow]) -> Option<String> {
    let core: Vec<bool> = rows
        .iter()
        .filter(|r| r.role == Role::CoreThread)
        .map(|r| r.error_kind.as_deref().is_some_and(is_engine_error_kind))
        .collect();
    if core.is_empty() {
        return None;
    }
    let failed = core.iter().filter(|f| **f).count();
    if core.len() >= CUT_TAIL && core[core.len() - CUT_TAIL..].iter().all(|f| *f) {
        return Some(format!(
            "ledger: the core thread's last {CUT_TAIL} calls failed on the engine ({failed} of {} failed)",
            core.len()
        ));
    }
    if failed as f64 > CUT_SHARE * core.len() as f64 {
        return Some(format!(
            "ledger: {failed} of {} core-thread calls failed on the engine",
            core.len()
        ));
    }
    None
}

/// The transcript's fleet-loss signal, reading the file. Falls back to a text scan when a
/// line does not parse (a transcript from a newer writer).
fn fleet_lost_file(path: &Path) -> Option<String> {
    match crate::transcript::read(path) {
        Ok(events) => fleet_lost_in(&events),
        Err(_) => {
            let text = std::fs::read_to_string(path).ok()?;
            (text.contains("\"reason\":\"fleet_lost") || text.contains("\"reason\": \"fleet_lost"))
                .then(|| "transcript: fleet_lost".to_string())
        }
    }
}

/// Why the engine cut this run, when it did (rules 1-3).
pub fn engine_failure(dir: &Path) -> Option<String> {
    let transcript = dir.join("transcript.jsonl");
    if transcript.is_file() {
        if let Some(why) = fleet_lost_file(&transcript) {
            return Some(why);
        }
    }
    let rows = ledger::read(&dir.join("ledger.jsonl")).unwrap_or_default();
    stream_cut(&rows)
}

/// Classify a run directory. Reads only.
pub fn classify(dir: &Path) -> RunClass {
    if dir.join("verdict.json").is_file() {
        return RunClass::Judged;
    }
    let record: Option<RunRecord> = read_json(&dir.join("run.json")).ok();
    let Some(record) = record else {
        if dir.join("error.json").is_file() {
            return if engine_failure(dir).is_some() {
                RunClass::EngineFailed
            } else {
                RunClass::Failed
            };
        }
        return RunClass::Incomplete;
    };
    classify_record(dir, &record)
}

/// Classify a run that has a `run.json` (and no verdict).
pub fn classify_record(dir: &Path, record: &RunRecord) -> RunClass {
    if record.status == RunStatus::EngineFailed
        || record.engine_failure.is_some()
        || engine_failure(dir).is_some()
    {
        return RunClass::EngineFailed;
    }
    match record.status {
        RunStatus::Judged => RunClass::Judged,
        RunStatus::Simulated => RunClass::Simulated,
        RunStatus::JudgePending => RunClass::JudgePending,
        RunStatus::EngineFailed => RunClass::EngineFailed,
        RunStatus::JudgeVoid => {
            if record
                .judge_error
                .as_deref()
                .is_some_and(judge_error_is_engine)
            {
                RunClass::JudgePending
            } else {
                RunClass::JudgeVoid
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::{Effort, ModelTier, Usage};

    fn row(role: Role, err: Option<&str>) -> LedgerRow {
        LedgerRow {
            ts_unix_ms: 0,
            role,
            tier: ModelTier::Haiku,
            effort: Effort::Low,
            engine: None,
            account: None,
            model: None,
            first_text_ms: None,
            duration_ms: 0,
            usage: Usage::default(),
            cost_usd: None,
            fallback_reason: None,
            correlation_id: None,
            error_kind: err.map(String::from),
            session: false,
        }
    }

    #[test]
    fn judge_error_strings_split_engine_from_content() {
        for s in [
            "judge engine call failed: engine unavailable: all claude cli accounts cooling",
            "judge engine call failed: timed out after 600000 ms",
            "judge engine call failed: rate limited on account isolated",
            "judge engine call failed: io: broken pipe",
            "judge deferred: quota (isolated) until 2026-10-18 14:20:00 UTC",
        ] {
            assert!(judge_error_is_engine(s), "{s}");
        }
        for s in [
            "judge reply is not the rubric JSON",
            "judge engine call failed: refused: no",
            "judge engine call failed: could not parse engine output: x",
        ] {
            assert!(!judge_error_is_engine(s), "{s}");
        }
    }

    #[test]
    fn stream_cut_rule() {
        let ok = |n: usize| (0..n).map(|_| row(Role::CoreThread, None));
        let bad = |n: usize| (0..n).map(|_| row(Role::CoreThread, Some("unavailable")));
        // Healthy, one blip mid-stream, and chatter/judge errors do not cut the stream.
        let mut rows: Vec<LedgerRow> = ok(100).collect();
        assert!(stream_cut(&rows).is_none());
        rows.insert(50, row(Role::CoreThread, Some("timeout")));
        rows.extend((0..30).map(|_| row(Role::Chatter, Some("unavailable"))));
        rows.push(row(Role::Judge, Some("unavailable")));
        assert!(stream_cut(&rows).is_none());
        // The stream ended with the core thread failing.
        let mut tail: Vec<LedgerRow> = ok(100).chain(bad(3)).collect();
        assert!(stream_cut(&tail).unwrap().contains("last 3"));
        tail.push(row(Role::Judge, None));
        assert!(stream_cut(&tail).is_some(), "judge rows are ignored");
        // A content error at the end is not the engine.
        let refused: Vec<LedgerRow> = ok(100)
            .chain((0..3).map(|_| row(Role::CoreThread, Some("refused"))))
            .collect();
        assert!(stream_cut(&refused).is_none());
        // A long outage that recovered.
        let share: Vec<LedgerRow> = ok(30).chain(bad(25)).chain(ok(30)).collect();
        assert!(stream_cut(&share).unwrap().contains("25 of 85"));
    }

    #[test]
    fn fleet_lost_prefers_the_structured_signal() {
        let end = StreamEvent::StreamEnd {
            ts_ms: 5,
            reason: EndReason::FleetLost,
        };
        let sign_off = StreamEvent::Op {
            ts_ms: 5,
            op: Op::SignOff {
                reason: "fleet_lost: engine degraded or down for 300s".into(),
            },
        };
        assert_eq!(
            fleet_lost_in(&[sign_off.clone(), end]).as_deref(),
            Some("stream_end fleet_lost")
        );
        assert!(fleet_lost_in(&[sign_off]).unwrap().starts_with("sign_off"));
        let other = StreamEvent::Op {
            ts_ms: 5,
            op: Op::SignOff {
                reason: "operator".into(),
            },
        };
        assert!(fleet_lost_in(&[other]).is_none());
    }
}
