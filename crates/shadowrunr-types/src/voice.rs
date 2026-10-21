//! Malakor's voice: what `GET /voice` pushes to the stage. The daemon hands each of Malakor's
//! `say` lines from a live run to the local voice sidecar (Kokoro through sherpa-onnx, see
//! `docs/spec/contract.md` appendix (voice)); the sidecar renders it a sentence at a time, and
//! each rendered sentence goes out as one [`VoiceFrame::Utterance`]: its PCM and its mouth
//! shapes. The stage plays the audio and drives the face from it; the audio is the one clock.
//!
//! The voice is optional and degrades cleanly: with `[voice] enabled = false`, a missing engine
//! or a dead sidecar, `/voice` sends `status` (`off` or `degraded`, with the reason) and the
//! stream runs as it does without a voice. A line that cannot be spoken in time is `skipped`,
//! never queued behind the stream.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The Oculus 15 mouth shapes. A sentence's timeline is a run of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Viseme {
    #[serde(rename = "sil")]
    Sil,
    PP,
    FF,
    TH,
    DD,
    #[serde(rename = "kk")]
    Kk,
    CH,
    SS,
    #[serde(rename = "nn")]
    Nn,
    RR,
    #[serde(rename = "aa")]
    Aa,
    E,
    #[serde(rename = "ih")]
    Ih,
    #[serde(rename = "oh")]
    Oh,
    #[serde(rename = "ou")]
    Ou,
}

/// One mouth shape over `[start_ms, end_ms)` of its utterance's audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VisemeSpan {
    pub v: Viseme,
    pub start_ms: u32,
    pub end_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceState {
    /// `[voice] enabled = false`: no voice by configuration.
    Off,
    /// The sidecar is warming up (the first render loads the model).
    Starting,
    /// Speaking.
    Ready,
    /// Enabled but not speaking: engine missing, sidecar exited, renders failing. `reason` says.
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSkip {
    /// Its audio could not start within `max_lag_ms` of its `say`.
    Late,
    /// More than `max_queue` lines were waiting; the oldest went.
    QueueFull,
    /// The render failed or timed out.
    Error,
}

/// Everything `GET /voice` may push.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VoiceFrame {
    /// Sent on connect and whenever the state changes.
    Status {
        state: VoiceState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// The voice id (e.g. `af_heart`), when one is configured.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        voice: Option<String>,
        /// Whether utterances carry mouth shapes (the durations model is present).
        visemes: bool,
    },
    /// One rendered sentence of a `say` line.
    Utterance {
        /// `<run_id>:<seq>:<part>`: unique per run.
        id: String,
        run_id: String,
        /// The `say` event's seq in its run (contract § event resume).
        seq: u64,
        /// 0-based sentence index within the line.
        part: u32,
        /// True on the line's last sentence.
        last: bool,
        /// This sentence's text, as spoken.
        text: String,
        sample_rate: u32,
        duration_ms: u32,
        /// Mono PCM16 little-endian, base64.
        pcm: String,
        /// Empty when the voice has no mouth shapes: the stage drives the mouth from loudness.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        visemes: Vec<VisemeSpan>,
    },
    /// A line that will not be spoken (or the rest of it, from `part`).
    Skipped {
        run_id: String,
        seq: u64,
        part: u32,
        reason: VoiceSkip,
    },
}

/// What a `/voice` client sends: which live run to voice. `run_id` absent = the newest live
/// run, following it as runs start. Each message replaces the previous choice.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceFollow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

/// The frame wrapping [`VoiceFollow`]: `{"follow": {...}}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceFollowFrame {
    pub follow: VoiceFollow,
}

/// `played`: a part's audio started playing on the stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoicePlayed {
    pub run_id: String,
    pub seq: u64,
    pub part: u32,
    /// When the audio started, in wall epoch ms (the stage's clock).
    pub at_wall_ms: i64,
}

/// `{"played": {...}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoicePlayedFrame {
    pub played: VoicePlayed,
}

/// Why the stage dropped a whole line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StageDropReason {
    /// Playing it would have put the stage more than its cap (`MAX_AHEAD_S`) behind.
    StageCap,
}

/// `stage_dropped`: the stage dropped a whole line and will not play it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceStageDropped {
    pub run_id: String,
    pub seq: u64,
    pub reason: StageDropReason,
}

/// `{"stage_dropped": {...}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceStageDroppedFrame {
    pub stage_dropped: VoiceStageDropped,
}

/// Which face the stage draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FaceState {
    /// The real layer rig.
    Rig,
    /// A stand-in: the silhouette or the placeholder.
    Silhouette,
}

/// `face`: which face is drawn; sent on load and on every change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceFace {
    pub run_id: String,
    pub state: FaceState,
    /// When it changed, in wall epoch ms (the stage's clock).
    pub at_wall_ms: i64,
}

/// `{"face": {...}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceFaceFrame {
    pub face: VoiceFace,
}

/// Everything a `/voice` client may send, told apart by its one key. Only `follow` changes what
/// the socket sends; the reports (`played`, `stage_dropped`, `face`) go to the run's
/// `voice.jsonl` and are never answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum VoiceClientFrame {
    Follow(VoiceFollowFrame),
    Played(VoicePlayedFrame),
    StageDropped(VoiceStageDroppedFrame),
    Face(VoiceFaceFrame),
}

/// What a `voice.jsonl` line records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceLogEvent {
    /// An utterance frame went out (`part`, `duration_ms`, `text`).
    Rendered,
    /// The service skipped the line from `part` (`reason`: `late`, `queue_full` or `error`).
    Skipped,
    /// The stage started playing `part` (its `played` frame).
    Played,
    /// The stage dropped the whole line (`reason`: `stage_cap`).
    StageDropped,
    /// The stage changed the face it draws (`reason`: `rig` or `silhouette`).
    Face,
}

/// One line of a run's `voice.jsonl` (contract § run-directory artifacts): what the voice did,
/// on the session's stream clock. Absent keys are omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceLogLine {
    /// Stream-clock ms: wall epoch ms minus the session's `started_ms`.
    pub ts_ms: i64,
    /// The `say` event's seq (0 for `face`, which belongs to no line).
    pub seq: u64,
    pub event: VoiceLogEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viseme_names_are_the_oculus_set() {
        let all = [
            (Viseme::Sil, "sil"),
            (Viseme::PP, "PP"),
            (Viseme::FF, "FF"),
            (Viseme::TH, "TH"),
            (Viseme::DD, "DD"),
            (Viseme::Kk, "kk"),
            (Viseme::CH, "CH"),
            (Viseme::SS, "SS"),
            (Viseme::Nn, "nn"),
            (Viseme::RR, "RR"),
            (Viseme::Aa, "aa"),
            (Viseme::E, "E"),
            (Viseme::Ih, "ih"),
            (Viseme::Oh, "oh"),
            (Viseme::Ou, "ou"),
        ];
        for (v, name) in all {
            assert_eq!(serde_json::to_string(&v).unwrap(), format!("\"{name}\""));
        }
    }

    #[test]
    fn frames_are_tagged_and_omit_absent_keys() {
        let s = VoiceFrame::Status {
            state: VoiceState::Off,
            reason: None,
            voice: None,
            visemes: false,
        };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"type":"status","state":"off","visemes":false}"#
        );
        let k = VoiceFrame::Skipped {
            run_id: "r".into(),
            seq: 7,
            part: 0,
            reason: VoiceSkip::QueueFull,
        };
        assert_eq!(
            serde_json::to_string(&k).unwrap(),
            r#"{"type":"skipped","run_id":"r","seq":7,"part":0,"reason":"queue_full"}"#
        );
        let f: VoiceFollowFrame = serde_json::from_str(r#"{"follow":{}}"#).unwrap();
        assert_eq!(f.follow.run_id, None);
    }

    #[test]
    fn client_frames_are_told_apart_by_their_key() {
        let f = |t: &str| serde_json::from_str::<VoiceClientFrame>(t).ok();
        assert_eq!(
            f(r#"{"follow":{"run_id":"r"}}"#),
            Some(VoiceClientFrame::Follow(VoiceFollowFrame {
                follow: VoiceFollow {
                    run_id: Some("r".into())
                }
            }))
        );
        let played = r#"{"played":{"run_id":"r","seq":4,"part":1,"at_wall_ms":1700000000123}}"#;
        assert_eq!(
            f(played),
            Some(VoiceClientFrame::Played(VoicePlayedFrame {
                played: VoicePlayed {
                    run_id: "r".into(),
                    seq: 4,
                    part: 1,
                    at_wall_ms: 1_700_000_000_123
                }
            }))
        );
        assert_eq!(serde_json::to_string(&f(played).unwrap()).unwrap(), played);
        let dropped = r#"{"stage_dropped":{"run_id":"r","seq":5,"reason":"stage_cap"}}"#;
        assert!(matches!(
            f(dropped),
            Some(VoiceClientFrame::StageDropped(_))
        ));
        let face = r#"{"face":{"run_id":"r","state":"silhouette","at_wall_ms":5}}"#;
        assert_eq!(serde_json::to_string(&f(face).unwrap()).unwrap(), face);
        assert_eq!(
            f(r#"{"face":{"run_id":"r","state":"mask","at_wall_ms":5}}"#),
            None
        );
        assert_eq!(f(r#"{"played":{"run_id":"r","seq":4}}"#), None);
        assert_eq!(f(r#"{"subscribe":"live"}"#), None);
    }
}
