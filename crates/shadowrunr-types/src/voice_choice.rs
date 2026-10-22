//! Which provider speaks a character, with which voice, and the `voice.*` settings RPCs.
//!
//! A character's effective choice is built per field: the package `[voice]`, then the daemon's
//! `[voice]` for the kokoro fields, then the operator's local override,
//! `<data>/characters/<id>/voice.json` (gitignored), which wins. It is resolved at each stream
//! start. A provider never falls back to another: a failing provider leaves the voice
//! `degraded` with its reason, and captions carry the show. Provider keys are named by env var
//! in the daemon config; no RPC ever carries a key's value.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};

use crate::character::VoiceFx;
use crate::delivery::Delivery;
use crate::TsMs;

/// A text-to-speech provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceProvider {
    /// The local Kokoro sidecar (Kokoro Plus when its delivery controls are available).
    #[default]
    Kokoro,
    /// The ElevenLabs cloud API.
    Elevenlabs,
}

impl VoiceProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kokoro => "kokoro",
            Self::Elevenlabs => "elevenlabs",
        }
    }
}

/// Kokoro's voice: the same fields as the package `[voice]` kokoro keys.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KokoroVoice {
    /// Voice id, e.g. `af_heart`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// Speaker id in the voices file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<u32>,
    /// The voices file `sid` is read from, by name (in the daemon's `[voice] voices_dir`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voices_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fx: Option<VoiceFx>,
    /// Speaking-rate multiplier (1.0 is the voice's own rate).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "hundredths"
    )]
    pub speed: Option<f32>,
}

/// ElevenLabs' voice. A package carries `model` and `speed` only; `voice_id` names a voice in
/// the owner's account and normally lives in the local override, so sharing a character never
/// shares it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElevenlabsVoice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    /// Model id, e.g. `eleven_v3`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Speaking-rate multiplier (1.0 is the voice's own rate).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "hundredths"
    )]
    pub speed: Option<f32>,
}

/// A speed written to two decimals, as `Delivery::intensity` is, so a JSON value built from it
/// reads `1.08`, not the `f32` widened to `1.0800000429153442`.
fn hundredths<S: Serializer>(v: &Option<f32>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(x) => s.serialize_some(&((f64::from(*x) * 100.0).round() / 100.0)),
        None => s.serialize_none(),
    }
}

/// A provider and its voice. Only the chosen provider's table is read; the other is kept so
/// switching back keeps its settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VoiceChoice {
    pub provider: VoiceProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kokoro: Option<KokoroVoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevenlabs: Option<ElevenlabsVoice>,
}

/// A provider's key, by state only (never its value).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceKeyState {
    /// The provider needs no key (kokoro).
    NotNeeded,
    /// The configured env var is unset or empty.
    Missing,
    /// The env var is set; not verified (or a verify could not reach the provider: `reason`).
    Set,
    /// The provider accepted the key.
    Verified,
    /// The provider refused the key (401/403).
    Rejected,
}

/// One provider's readiness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderStatus {
    pub provider: VoiceProvider,
    /// It can speak now.
    pub ready: bool,
    pub key: VoiceKeyState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// When the key was last verified (epoch ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_ms: Option<TsMs>,
}

/// `voice.settings.get`, `voice.bench`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceSettingsGetParams {
    pub character: String,
}

/// `voice.settings.set`: replace the local override; `null` clears it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceSettingsSetParams {
    pub character: String,
    pub choice: Option<VoiceChoice>,
}

/// A character's voice settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceSettings {
    pub character: String,
    /// What the next stream start resolves to.
    pub effective: VoiceChoice,
    /// The package `[voice]` (with the daemon's `[voice]` under it).
    pub package: VoiceChoice,
    /// The local override, when one is stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#override: Option<VoiceChoice>,
    pub providers: Vec<ProviderStatus>,
    /// The human status label, e.g. "kokoro malakor-a (sid 52), fx light".
    pub label: String,
}

/// `voice.verify`: one cheap authenticated call to the provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceVerifyParams {
    pub provider: VoiceProvider,
}

/// `voice.preview`: speak `text` (gated by the brain's output gate, at most 400 chars) with
/// the character's effective choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoicePreviewParams {
    pub character: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
}

/// A rendered preview line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoicePreview {
    pub provider: VoiceProvider,
    pub label: String,
    pub sample_rate: u32,
    /// Mono PCM16 little-endian, base64.
    pub pcm: String,
    pub duration_ms: u32,
    /// Characters sent to the provider.
    pub chars: u32,
    /// Whether the provider produced mouth shapes.
    pub visemes: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<Delivery>,
}

/// `voice.bench`: render `characters/<id>/voice/bench.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceBenchParams {
    pub character: String,
}

/// The rendered emotion bench.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceBench {
    pub provider: VoiceProvider,
    pub label: String,
    pub lines: Vec<VoiceBenchLine>,
}

/// One bench line. On `error` the audio fields are empty (`pcm` "", zero lengths).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceBenchLine {
    pub id: String,
    pub text: String,
    pub delivery: Delivery,
    pub sample_rate: u32,
    pub pcm: String,
    pub duration_ms: u32,
    pub chars: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `voice/bench.toml` in a character package: the lines the emotion bench renders.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VoiceBenchFile {
    #[serde(default, rename = "line")]
    pub lines: Vec<VoiceBenchEntry>,
}

/// `[[line]]` in `voice/bench.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VoiceBenchEntry {
    pub id: String,
    pub text: String,
    pub emotion: crate::delivery::Emotion,
    pub intensity: f32,
    pub pace: crate::delivery::Pace,
}

impl VoiceBenchEntry {
    pub fn delivery(&self) -> Delivery {
        Delivery::new(self.emotion, self.intensity, self.pace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_omit_absent_tables_and_name_providers_in_snake_case() {
        let c = VoiceChoice {
            provider: VoiceProvider::Elevenlabs,
            kokoro: None,
            elevenlabs: Some(ElevenlabsVoice {
                voice_id: Some("v".into()),
                model: None,
                speed: Some(1.1),
            }),
        };
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(
            s,
            r#"{"provider":"elevenlabs","elevenlabs":{"voice_id":"v","speed":1.1}}"#
        );
        assert_eq!(serde_json::from_str::<VoiceChoice>(&s).unwrap(), c);
        assert!(serde_json::from_str::<VoiceChoice>(r#"{"provider":"openai"}"#).is_err());
    }

    #[test]
    fn speeds_reach_a_json_value_at_two_decimals() {
        let c = VoiceChoice {
            provider: VoiceProvider::Elevenlabs,
            kokoro: Some(KokoroVoice {
                speed: Some(0.95),
                ..Default::default()
            }),
            elevenlabs: Some(ElevenlabsVoice {
                speed: Some(1.08),
                ..Default::default()
            }),
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["elevenlabs"]["speed"], serde_json::json!(1.08));
        assert_eq!(v["kokoro"]["speed"], serde_json::json!(0.95));
        assert_eq!(serde_json::from_value::<VoiceChoice>(v).unwrap(), c);
    }

    #[test]
    fn settings_set_reads_null_or_absent_as_clear() {
        let p: VoiceSettingsSetParams =
            serde_json::from_str(r#"{"character":"m","choice":null}"#).unwrap();
        assert_eq!(p.choice, None);
        let p: VoiceSettingsSetParams = serde_json::from_str(r#"{"character":"m"}"#).unwrap();
        assert_eq!(p.choice, None);
    }

    #[test]
    fn settings_name_the_override_key_plainly() {
        let s = VoiceSettings {
            character: "m".into(),
            effective: VoiceChoice::default(),
            package: VoiceChoice::default(),
            r#override: Some(VoiceChoice::default()),
            providers: vec![ProviderStatus {
                provider: VoiceProvider::Kokoro,
                ready: true,
                key: VoiceKeyState::NotNeeded,
                reason: None,
                checked_ms: None,
            }],
            label: "kokoro".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["override"]["provider"], "kokoro");
        assert_eq!(v["providers"][0]["key"], "not_needed");
        assert!(v["providers"][0].get("reason").is_none());
    }

    #[test]
    fn a_bench_file_reads_its_lines() {
        let f: VoiceBenchFile = toml_like(
            r#"{"line":[{"id":"a","text":"Kneel.","emotion":"menace","intensity":0.7,"pace":"slow"}]}"#,
        );
        assert_eq!(f.lines.len(), 1);
        assert_eq!(
            f.lines[0].delivery(),
            Delivery::new(
                crate::delivery::Emotion::Menace,
                0.7,
                crate::delivery::Pace::Slow
            )
        );
    }

    fn toml_like(json: &str) -> VoiceBenchFile {
        serde_json::from_str(json).unwrap()
    }
}
