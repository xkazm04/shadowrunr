//! How a spoken line is delivered: the emotion, its intensity and the pace. The brain writes it
//! inline as a cue at the start of a line (`[sarcasm 0.8 slow] Oh, wonderful.`), parses it out
//! of the text, and carries it on `StreamEvent::Say`; the voice turns it into its provider's
//! controls (Kokoro Plus speed, pauses and blends; ElevenLabs audio tags). The emotion set is
//! closed, so a cue the voice cannot read never reaches it.
//!
//! The same cue may also direct the face: a [`Gesture`] and an eye-light level
//! (`[menace 0.9 slow glare eyes 2] Kneel.`). The voice ignores both; the stage plays them.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The closed emotion set a cue may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Emotion {
    Neutral,
    Menace,
    Glee,
    Deadpan,
    Hushed,
    Outrage,
    Sarcasm,
    Weary,
    Triumph,
    Tender,
}

impl Emotion {
    /// Every emotion, in contract order.
    pub const ALL: [Emotion; 10] = [
        Self::Neutral,
        Self::Menace,
        Self::Glee,
        Self::Deadpan,
        Self::Hushed,
        Self::Outrage,
        Self::Sarcasm,
        Self::Weary,
        Self::Triumph,
        Self::Tender,
    ];

    /// The wire (and cue) word.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Menace => "menace",
            Self::Glee => "glee",
            Self::Deadpan => "deadpan",
            Self::Hushed => "hushed",
            Self::Outrage => "outrage",
            Self::Sarcasm => "sarcasm",
            Self::Weary => "weary",
            Self::Triumph => "triumph",
            Self::Tender => "tender",
        }
    }

    /// The emotion a cue word names (case-insensitive); `None` for a word outside the set.
    pub fn parse(word: &str) -> Option<Self> {
        let w = word.to_ascii_lowercase();
        Self::ALL.into_iter().find(|e| e.as_str() == w)
    }
}

/// The pace a line is spoken at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Pace {
    Slow,
    #[default]
    Normal,
    Fast,
}

impl Pace {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Slow => "slow",
            Self::Normal => "normal",
            Self::Fast => "fast",
        }
    }

    /// The pace a cue word names (case-insensitive).
    pub fn parse(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "slow" => Some(Self::Slow),
            "normal" => Some(Self::Normal),
            "fast" => Some(Self::Fast),
            _ => None,
        }
    }
}

/// A head gesture the face plays once, as the stage's performance channels define it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Gesture {
    Nod,
    Tilt,
    LeanIn,
    Recoil,
    Glare,
    Flare,
}

impl Gesture {
    /// Every gesture, in contract order.
    pub const ALL: [Gesture; 6] = [
        Self::Nod,
        Self::Tilt,
        Self::LeanIn,
        Self::Recoil,
        Self::Glare,
        Self::Flare,
    ];

    /// The wire (and cue) word.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nod => "nod",
            Self::Tilt => "tilt",
            Self::LeanIn => "lean_in",
            Self::Recoil => "recoil",
            Self::Glare => "glare",
            Self::Flare => "flare",
        }
    }

    /// The gesture a cue word names (case-insensitive); `None` outside the set.
    pub fn parse(word: &str) -> Option<Self> {
        let w = word.to_ascii_lowercase();
        Self::ALL.into_iter().find(|g| g.as_str() == w)
    }
}

/// The brightest eye light a cue may ask for (1.0 is the rig's own).
pub const EYE_LIGHT_MAX: f32 = 2.5;

/// The intensity a cue without one gets.
pub const DEFAULT_INTENSITY: f32 = 0.6;

/// A line's delivery. `intensity` is 0.0..=1.0, clamped when read.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Delivery {
    pub emotion: Emotion,
    #[serde(deserialize_with = "clamped", serialize_with = "hundredths")]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub intensity: f32,
    pub pace: Pace,
    /// A head gesture for the face, played once when the line is heard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gesture: Option<Gesture>,
    /// The eye light the face settles to for this line, 0.0..=2.5 (1.0 is the rig's own),
    /// clamped when read. Absent: the emotion's own level.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "eye_clamped",
        serialize_with = "eye_hundredths"
    )]
    #[schemars(range(min = 0.0, max = 2.5))]
    pub eye_light: Option<f32>,
}

impl Delivery {
    /// A delivery with `intensity` clamped into 0.0..=1.0 (NaN reads as the default).
    pub fn new(emotion: Emotion, intensity: f32, pace: Pace) -> Self {
        Self {
            emotion,
            intensity: clamp_intensity(intensity),
            pace,
            gesture: None,
            eye_light: None,
        }
    }

    /// This delivery with a face gesture.
    pub fn with_gesture(mut self, gesture: Option<Gesture>) -> Self {
        self.gesture = gesture;
        self
    }

    /// This delivery with an eye light, clamped into 0.0..=2.5 (NaN: none).
    pub fn with_eye_light(mut self, eye_light: Option<f32>) -> Self {
        self.eye_light = eye_light.and_then(clamp_eye);
        self
    }
}

fn clamp_eye(e: f32) -> Option<f32> {
    (!e.is_nan()).then(|| e.clamp(0.0, EYE_LIGHT_MAX))
}

fn eye_clamped<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f32>, D::Error> {
    Option::<f32>::deserialize(d).map(|o| o.and_then(clamp_eye))
}

fn eye_hundredths<S: Serializer>(e: &Option<f32>, s: S) -> Result<S::Ok, S::Error> {
    match e {
        Some(v) => s.serialize_f64((f64::from(*v) * 100.0).round() / 100.0),
        None => s.serialize_none(),
    }
}

fn clamp_intensity(i: f32) -> f32 {
    if i.is_nan() {
        DEFAULT_INTENSITY
    } else {
        i.clamp(0.0, 1.0)
    }
}

fn clamped<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    f32::deserialize(d).map(clamp_intensity)
}

/// Written to two decimals, so a JSON value built from it reads `0.8`, not the `f32` widened
/// to `0.800000011920929`.
fn hundredths<S: Serializer>(i: &f32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64((f64::from(*i) * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_is_snake_case_and_clamps_intensity() {
        let d = Delivery::new(Emotion::Sarcasm, 0.8, Pace::Slow);
        assert_eq!(
            serde_json::to_string(&d).unwrap(),
            r#"{"emotion":"sarcasm","intensity":0.8,"pace":"slow"}"#
        );
        assert_eq!(serde_json::to_value(d).unwrap()["intensity"], 0.8);
        let over: Delivery =
            serde_json::from_str(r#"{"emotion":"glee","intensity":3.5,"pace":"fast"}"#).unwrap();
        assert_eq!(over.intensity, 1.0);
        let under: Delivery =
            serde_json::from_str(r#"{"emotion":"weary","intensity":-1,"pace":"normal"}"#).unwrap();
        assert_eq!(under.intensity, 0.0);
        assert!(serde_json::from_str::<Delivery>(
            r#"{"emotion":"joy","intensity":1,"pace":"fast"}"#
        )
        .is_err());
    }

    #[test]
    fn a_say_omits_its_delivery_when_absent() {
        let say = |delivery| crate::StreamEvent::Say {
            ts_ms: 5,
            speaker: crate::Speaker::Host,
            foil: None,
            text: "Kneel.".into(),
            replying_to: Vec::new(),
            first_text_ms: None,
            delivery,
        };
        let plain = serde_json::to_string(&say(None)).unwrap();
        assert_eq!(
            plain,
            r#"{"type":"say","ts_ms":5,"speaker":"host","text":"Kneel."}"#
        );
        let cued =
            serde_json::to_string(&say(Some(Delivery::new(Emotion::Menace, 0.5, Pace::Slow))))
                .unwrap();
        assert!(
            cued.ends_with(r#""delivery":{"emotion":"menace","intensity":0.5,"pace":"slow"}}"#),
            "{cued}"
        );
        // A transcript written before the field reads back with none.
        match serde_json::from_str::<crate::StreamEvent>(&plain).unwrap() {
            crate::StreamEvent::Say { delivery, .. } => assert_eq!(delivery, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn face_fields_are_optional_and_clamped() {
        let d = Delivery::new(Emotion::Menace, 0.9, Pace::Slow)
            .with_gesture(Some(Gesture::LeanIn))
            .with_eye_light(Some(1.8));
        assert_eq!(
            serde_json::to_string(&d).unwrap(),
            r#"{"emotion":"menace","intensity":0.9,"pace":"slow","gesture":"lean_in","eye_light":1.8}"#
        );
        let hot: Delivery =
            serde_json::from_str(r#"{"emotion":"glee","intensity":1,"pace":"fast","eye_light":9}"#)
                .unwrap();
        assert_eq!(hot.eye_light, Some(EYE_LIGHT_MAX));
        assert_eq!(hot.gesture, None);
        assert_eq!(
            Delivery::new(Emotion::Glee, 1.0, Pace::Fast)
                .with_eye_light(Some(f32::NAN))
                .eye_light,
            None
        );
        assert!(serde_json::from_str::<Delivery>(
            r#"{"emotion":"glee","intensity":1,"pace":"fast","gesture":"wink"}"#
        )
        .is_err());
        for g in Gesture::ALL {
            assert_eq!(Gesture::parse(g.as_str()), Some(g));
            assert_eq!(
                serde_json::to_string(&g).unwrap(),
                format!("\"{}\"", g.as_str())
            );
        }
    }

    #[test]
    fn every_emotion_round_trips_its_word() {
        for e in Emotion::ALL {
            assert_eq!(Emotion::parse(e.as_str()), Some(e));
            assert_eq!(
                serde_json::to_string(&e).unwrap(),
                format!("\"{}\"", e.as_str())
            );
        }
        assert_eq!(Emotion::parse("MENACE"), Some(Emotion::Menace));
        assert_eq!(Emotion::parse("laughs"), None);
        assert_eq!(Pace::parse("Fast"), Some(Pace::Fast));
    }
}
