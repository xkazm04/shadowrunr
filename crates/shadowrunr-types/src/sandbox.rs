//! Sandbox sessions: a wall-clock stream on the real engine against the simulated audience,
//! hosted by the daemon and gated by a session report (`runs/sandbox/<session_id>/session.json`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::TsMs;

/// Session lengths a sandbox accepts, in minutes.
pub const SANDBOX_MINUTES: [u32; 3] = [5, 10, 30];

/// An injected engine failure for the brain's roles (the chat room keeps talking).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxChaos {
    /// 90 s of fleet errors from half-way: the host must stall in character and recover.
    Outage,
    /// Fleet errors from half-way to the end: the host must stall, then sign off in
    /// character after the fleet grace. 10 and 30 minute sessions only.
    Lost,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// `sandbox.start` params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxStartParams {
    /// 5, 10 or 30.
    pub minutes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chaos: Option<SandboxChaos>,
    /// Judge the session when it ends. Default true.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub judge: bool,
    /// Tone variant; the character's active tone when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
}

/// `sandbox.start` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxStarted {
    pub session_id: String,
    /// The session directory.
    pub dir: String,
    /// The stage URL that follows this session in broadcast mode.
    pub stage_url: String,
}

/// `sandbox.stop` params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxIdParams {
    pub session_id: String,
}

/// `sandbox.status` params: the given session, or the latest one when absent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxStatusParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxState {
    Running,
    /// The closing window: the host is asked to sign off.
    Closing,
    Judging,
    Done,
    Failed,
    /// Ended early by `sandbox.stop`.
    Stopped,
}

/// `sandbox.stop` / `sandbox.status` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxStatus {
    pub session_id: String,
    pub state: SandboxState,
    pub elapsed_s: u32,
    pub planned_s: u32,
    /// Transcript events so far.
    pub events: u64,
    pub max_say_gap_s: f64,
    pub core_errors: u32,
    /// The highest seq any stage subscriber acknowledged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_ack_seq: Option<u64>,
    /// Present once the session ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<SandboxReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxCheckState {
    Pass,
    Fail,
    /// Nothing measured it; never a pass.
    Unmeasured,
}

/// Gate check ids, in report order.
pub const SANDBOX_CHECKS: [&str; 6] = [
    "say_gap",
    "core_errors",
    "sign_off",
    "chaos",
    "stage_ack",
    "judge",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxCheck {
    /// One of [`SANDBOX_CHECKS`].
    pub id: String,
    pub state: SandboxCheckState,
    pub detail: String,
}

/// The judge's verdict, folded into the report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxJudge {
    /// Path of `verdict.json`, relative to the session dir.
    pub verdict_path: String,
    pub rule_fails: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_mean: Option<f64>,
}

/// `session.json`: the session's gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxReport {
    pub session_id: String,
    pub spec: SandboxStartParams,
    /// Epoch ms.
    pub started_ms: TsMs,
    pub ended_ms: TsMs,
    /// Every check passed (an unmeasured check is not a pass).
    pub pass: bool,
    pub checks: Vec<SandboxCheck>,
    pub max_say_gap_s: f64,
    pub say_gap_p95_s: f64,
    pub beat_ms_p50: u64,
    pub beat_ms_p95: u64,
    pub core_errors: u32,
    pub stall_lines: u32,
    pub health_transitions: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge: Option<SandboxJudge>,
}

/// `/events` client -> daemon: what a subscriber (the stage) has applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StageAck {
    /// The highest contiguous seq applied.
    pub seq: u64,
    /// Seqs found missing so far.
    pub gaps: u32,
    /// How long the socket has been open with no event, if stalled now.
    pub stalled_ms: u64,
}

/// The frame wrapping [`StageAck`]: `{"ack": {...}}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AckFrame {
    pub ack: StageAck,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_params_default_to_judging_and_omit_absent_keys() {
        let p: SandboxStartParams = serde_json::from_str(r#"{"minutes":5}"#).unwrap();
        assert!(p.judge);
        assert_eq!(serde_json::to_string(&p).unwrap(), r#"{"minutes":5}"#);
        let p: SandboxStartParams =
            serde_json::from_str(r#"{"minutes":10,"chaos":"lost","judge":false}"#).unwrap();
        assert_eq!(p.chaos, Some(SandboxChaos::Lost));
        assert!(!p.judge);
    }

    #[test]
    fn ack_frame_round_trips() {
        let f: AckFrame =
            serde_json::from_str(r#"{"ack":{"seq":42,"gaps":0,"stalled_ms":0}}"#).unwrap();
        assert_eq!(f.ack.seq, 42);
    }
}
