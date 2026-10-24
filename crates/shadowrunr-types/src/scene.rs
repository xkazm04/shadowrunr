//! SEG-03 (Table Read): scenes the host performs with a scheduled guest.
//!
//! A scene is an owner-authored file, `<data>/scenes/<guest>/<id>.toml`, never part of the
//! character package (owner-local material stays out of the repository). Its fixed lines are
//! spoken as written, after the output gate; its live slots are written by the host's core
//! model when the scene starts, and fall back to the slot's own host line when that fails.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Who speaks a scene line. The guest is the stream's scheduled foil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SceneSpeaker {
    Host,
    Guest,
}

/// Whether a scene may go on a ballot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SceneStatus {
    Draft,
    Approved,
}

/// One line of a scene: fixed `text`, or (host only) a live slot with a `live` direction and
/// a `fallback` line. Exactly one of `text` and `live` is set; `scene check` enforces it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneLine {
    pub speaker: SceneSpeaker,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// What the host's live line must do, e.g. "the closing speech: what we learned today".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<String>,
    /// Spoken when the live slot's call fails or times out. Required with `live`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

/// A scene file (`<data>/scenes/<guest>/<id>.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Scene {
    /// Kebab-case, unique within the guest's folder; equals the file stem.
    pub id: String,
    pub title: String,
    /// The ballot line, at most 80 chars.
    pub pitch: String,
    /// The foil id that plays the guest.
    pub guest: String,
    pub status: SceneStatus,
    /// Talking points for the free talk after the scene.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(rename = "line")]
    pub lines: Vec<SceneLine>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fictional: bool,
}

/// One problem `scene check` found. A scene with any problem cannot be approved and never
/// reaches a ballot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneProblem {
    /// 0-based line index; omitted for a whole-scene problem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Stable code: `shape`, `pitch_too_long`, `guest_unknown`, `gate_blocked`, `gate_recast`,
    /// `no_lines`, `ends_on_guest`.
    pub code: String,
    pub detail: String,
}

/// One row of `scene.list` / the `scene list` CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneSummary {
    pub id: String,
    pub guest: String,
    pub title: String,
    pub pitch: String,
    pub status: SceneStatus,
    pub lines: u32,
    pub live_slots: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<SceneProblem>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fictional: bool,
}

/// The stream prep's SEG-03 plan: the scheduled guest and up to three scenes for its ballots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TableReadPlan {
    /// Foil id of the guest.
    pub guest: String,
    /// 1-3 scene ids from `<data>/scenes/<guest>/`, in ballot order.
    pub scenes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_toml_shape() {
        let src = r#"
id = "the-gallery"
title = "The Gallery"
pitch = "The painter tries to sell Malakor a forged masterpiece."
guest = "the-painter"
status = "approved"
fictional = true
notes = ["Was the forgery better than the original?"]

[[line]]
speaker = "guest"
text = "Behold. My masterpiece."

[[line]]
speaker = "host"
live = "the closing speech: what we learned today"
fallback = "You know, I learned something today. Nothing. I learned nothing."
"#;
        let s: Scene = toml::from_str(src).unwrap();
        assert_eq!(s.lines.len(), 2);
        assert_eq!(s.lines[0].speaker, SceneSpeaker::Guest);
        assert!(s.lines[1].live.is_some());
        let j = serde_json::to_value(&s).unwrap();
        assert!(j["line"][0].get("live").is_none());
    }
}
