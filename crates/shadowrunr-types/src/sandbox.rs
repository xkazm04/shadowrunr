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
    /// ([`SandboxCheckId::is_watched`]) gate it. Default false.
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

/// A sandbox session's gate check, as it crosses the wire. The variants are in report order
/// ([`SandboxCheckId::ALL`]); the serde names are the ids `session.json` has always carried.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SandboxCheckId {
    SayGap,
    CoreErrors,
    SignOff,
    Chaos,
    StageAck,
    Judge,
    Voiced,
    Face,
    AvSync,
    DeadAir,
}

impl SandboxCheckId {
    /// Every check, in report order.
    pub const ALL: [Self; 10] = [
        Self::SayGap,
        Self::CoreErrors,
        Self::SignOff,
        Self::Chaos,
        Self::StageAck,
        Self::Judge,
        Self::Voiced,
        Self::Face,
        Self::AvSync,
        Self::DeadAir,
    ];

    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SayGap => "say_gap",
            Self::CoreErrors => "core_errors",
            Self::SignOff => "sign_off",
            Self::Chaos => "chaos",
            Self::StageAck => "stage_ack",
            Self::Judge => "judge",
            Self::Voiced => "voiced",
            Self::Face => "face",
            Self::AvSync => "av_sync",
            Self::DeadAir => "dead_air",
        }
    }

    /// The watched checks: what a viewer sees and hears, read from `voice.jsonl` and the
    /// recording. **The watched-gate rule:** they gate a session only when it was recorded
    /// ([`SandboxStartParams::record`]); otherwise they are reported and never gate, so an
    /// unrecorded, voice-off session gates on the other checks as before.
    pub const fn is_watched(self) -> bool {
        matches!(
            self,
            Self::Voiced | Self::Face | Self::AvSync | Self::DeadAir
        )
    }
}

impl std::fmt::Display for SandboxCheckId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// Whether check `id` gates a session started with `spec` (the watched-gate rule).
pub fn check_gates(spec: &SandboxStartParams, id: SandboxCheckId) -> bool {
    spec.record || !id.is_watched()
}

/// The session's gate: every check that gates it passed (an unmeasured check is not a pass).
pub fn gate_pass(spec: &SandboxStartParams, checks: &[SandboxCheck]) -> bool {
    let mut gating = checks.iter().filter(|c| check_gates(spec, c.id)).peekable();
    gating.peek().is_some() && gating.all(|c| c.state == SandboxCheckState::Pass)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxCheck {
    pub id: SandboxCheckId,
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
    /// watched checks gate only a recorded session, see [`SandboxCheckId::is_watched`]).
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

    fn checks(states: &[(SandboxCheckId, SandboxCheckState)]) -> Vec<SandboxCheck> {
        states
            .iter()
            .map(|(id, state)| SandboxCheck {
                id: *id,
                state: *state,
                detail: String::new(),
            })
            .collect()
    }

    #[test]
    fn the_watched_checks_gate_only_a_recorded_session() {
        use SandboxCheckState::*;
        let mut spec: SandboxStartParams = serde_json::from_str(r#"{"minutes":5}"#).unwrap();
        let unwatched: Vec<(SandboxCheckId, SandboxCheckState)> = SandboxCheckId::ALL
            .iter()
            .map(|id| {
                let st = if id.is_watched() { Unmeasured } else { Pass };
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
        let green: Vec<(SandboxCheckId, SandboxCheckState)> =
            SandboxCheckId::ALL.iter().map(|id| (*id, Pass)).collect();
        assert!(gate_pass(&spec, &checks(&green)));
        assert!(!gate_pass(&spec, &[]), "no checks is no pass");
        assert_eq!(
            serde_json::to_string(&spec).unwrap(),
            r#"{"minutes":5,"record":true}"#
        );
    }

    /// The wire names, in report order, exactly as the string constant listed them.
    const WIRE: [&str; 10] = [
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

    #[test]
    fn check_ids_serialize_to_the_wire_names_in_report_order() {
        assert_eq!(
            serde_json::to_string(&SandboxCheckId::AvSync).unwrap(),
            "\"av_sync\""
        );
        let names: Vec<String> = SandboxCheckId::ALL
            .iter()
            .map(|id| {
                serde_json::to_value(id)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(names, WIRE);
        for id in SandboxCheckId::ALL {
            assert_eq!(serde_json::to_value(id).unwrap(), id.as_str());
            assert_eq!(id.to_string(), id.as_str());
        }
        // Width and alignment reach the name, so `{:<12}` tables line up as with a `&str`.
        assert_eq!(format!("{:<9}|", SandboxCheckId::Face), "face     |");
    }

    #[test]
    fn exactly_the_four_watched_checks_are_watched() {
        let watched: Vec<&str> = SandboxCheckId::ALL
            .iter()
            .filter(|id| id.is_watched())
            .map(|id| id.as_str())
            .collect();
        assert_eq!(watched, ["voiced", "face", "av_sync", "dead_air"]);
    }

    #[test]
    fn a_report_with_every_check_id_reserializes_byte_identical() {
        let checks: Vec<String> = WIRE
            .iter()
            .map(|id| format!(r#"{{"id":"{id}","state":"pass","detail":"d"}}"#))
            .collect();
        let json = format!(
            r#"{{"session_id":"s","spec":{{"minutes":5}},"started_ms":1,"ended_ms":2,"pass":true,"checks":[{}],"max_say_gap_s":1.5,"say_gap_p95_s":1.0,"beat_ms_p50":10,"beat_ms_p95":20,"core_errors":0,"stall_lines":0,"health_transitions":0}}"#,
            checks.join(",")
        );
        let r: SandboxReport = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&r).unwrap(), json);
    }

    #[test]
    fn an_unknown_check_id_is_refused_not_a_new_check() {
        let e = serde_json::from_str::<SandboxCheck>(
            r#"{"id":"not_a_check","state":"pass","detail":""}"#,
        )
        .expect_err("a typo must not parse as a new check");
        assert!(e.to_string().contains("not_a_check"), "{e}");
        assert!(e.to_string().contains("unknown variant"), "{e}");
    }

    #[test]
    fn ack_frame_round_trips() {
        let f: AckFrame =
            serde_json::from_str(r#"{"ack":{"seq":42,"gaps":0,"stalled_ms":0}}"#).unwrap();
        assert_eq!(f.ack.seq, 42);
    }
}
