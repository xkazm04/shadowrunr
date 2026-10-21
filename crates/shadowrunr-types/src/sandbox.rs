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

fn is_false(b: &bool) -> bool {
    !*b
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
    /// The session is recorded (`stream sandbox --record`), so the watched checks
    /// ([`SANDBOX_WATCHED_CHECKS`]) gate it. Default false.
    #[serde(default, skip_serializing_if = "is_false")]
    pub record: bool,
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
pub const SANDBOX_CHECKS: [&str; 10] = [
    "say_gap",
    "core_errors",
    "sign_off",
    "chaos",
    "stage_ack",
    "judge",
    "voiced",
    "face",
    "av_sync",
    "dead_air",
];

/// The watched checks: what a viewer sees and hears, read from `voice.jsonl` and the recording.
/// **The watched-gate rule:** they gate a session only when it was recorded
/// ([`SandboxStartParams::record`]); otherwise they are reported and never gate, so an
/// unrecorded, voice-off session gates on the other checks as before.
pub const SANDBOX_WATCHED_CHECKS: [&str; 4] = ["voiced", "face", "av_sync", "dead_air"];

/// Whether check `id` gates a session started with `spec` (the watched-gate rule).
pub fn check_gates(spec: &SandboxStartParams, id: &str) -> bool {
    spec.record || !SANDBOX_WATCHED_CHECKS.contains(&id)
}

/// The session's gate: every check that gates it passed (an unmeasured check is not a pass).
pub fn gate_pass(spec: &SandboxStartParams, checks: &[SandboxCheck]) -> bool {
    let mut gating = checks
        .iter()
        .filter(|c| check_gates(spec, &c.id))
        .peekable();
    gating.peek().is_some() && gating.all(|c| c.state == SandboxCheckState::Pass)
}

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
    /// Every check that gates the session passed (an unmeasured check is not a pass; the
    /// watched checks gate only a recorded session, see [`SANDBOX_WATCHED_CHECKS`]).
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

    fn checks(states: &[(&str, SandboxCheckState)]) -> Vec<SandboxCheck> {
        states
            .iter()
            .map(|(id, state)| SandboxCheck {
                id: (*id).into(),
                state: *state,
                detail: String::new(),
            })
            .collect()
    }

    #[test]
    fn the_watched_checks_gate_only_a_recorded_session() {
        use SandboxCheckState::*;
        let mut spec: SandboxStartParams = serde_json::from_str(r#"{"minutes":5}"#).unwrap();
        let unwatched: Vec<(&str, SandboxCheckState)> = SANDBOX_CHECKS
            .iter()
            .map(|id| {
                let st = if SANDBOX_WATCHED_CHECKS.contains(id) {
                    Unmeasured
                } else {
                    Pass
                };
                (*id, st)
            })
            .collect();
        // Unrecorded: the watched checks are unmeasured and the gate passes as before.
        assert!(gate_pass(&spec, &checks(&unwatched)));
        let mut red = unwatched.clone();
        red[0].1 = Fail;
        assert!(!gate_pass(&spec, &checks(&red)));
        // Recorded: the same unmeasured checks fail the gate; all green passes it.
        spec.record = true;
        assert!(!gate_pass(&spec, &checks(&unwatched)));
        let green: Vec<(&str, SandboxCheckState)> =
            SANDBOX_CHECKS.iter().map(|id| (*id, Pass)).collect();
        assert!(gate_pass(&spec, &checks(&green)));
        assert!(!gate_pass(&spec, &[]), "no checks is no pass");
        assert_eq!(
            serde_json::to_string(&spec).unwrap(),
            r#"{"minutes":5,"record":true}"#
        );
    }

    #[test]
    fn ack_frame_round_trips() {
        let f: AckFrame =
            serde_json::from_str(r#"{"ack":{"seq":42,"gaps":0,"stalled_ms":0}}"#).unwrap();
        assert_eq!(f.ack.seq, 42);
    }
}
