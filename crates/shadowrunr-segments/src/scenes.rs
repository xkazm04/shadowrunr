//! SEG-03 scene files: `<data>/scenes/<guest>/<id>.toml` (contract appendix "SEG-03 Table
//! Read"). Loading and the shape check only; the output gate's check runs where the policy
//! lives (the daemon), and playing a scene is the brain's.

use std::path::{Path, PathBuf};

use shadowrunr_types::scene::{Scene, SceneProblem, SceneSpeaker, SceneStatus};

/// The longest ballot line.
pub const PITCH_MAX: usize = 80;

/// A scene file as read from disk, with the problems the shape check found.
#[derive(Debug, Clone)]
pub struct LoadedScene {
    pub path: PathBuf,
    pub scene: Scene,
    pub problems: Vec<SceneProblem>,
}

impl LoadedScene {
    /// Approved and clean: the only scenes a ballot may offer.
    pub fn playable(&self) -> bool {
        self.scene.status == SceneStatus::Approved && self.problems.is_empty()
    }
}

/// `<data>/scenes`.
pub fn scenes_dir(data: &Path) -> PathBuf {
    data.join("scenes")
}

/// Read and parse one scene file. The error is a one-line reason for the CLI.
pub fn load_scene(path: &Path) -> Result<Scene, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every `*.toml` scene in `<data>/scenes/<guest>/`, sorted by id, each shape-checked. A file
/// that does not parse is returned as a `shape` problem on a placeholder, never dropped silently.
pub fn load_guest(data: &Path, guest: &str, known_foils: &[String]) -> Vec<LoadedScene> {
    let dir = scenes_dir(data).join(guest);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<LoadedScene> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .map(|path| match load_scene(&path) {
            Ok(scene) => {
                let mut problems = check_shape(&scene, known_foils);
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if stem != scene.id {
                    problems.push(problem(
                        None,
                        "shape",
                        format!("id `{}` is not the file name `{stem}`", scene.id),
                    ));
                }
                if scene.guest != guest {
                    problems.push(problem(
                        None,
                        "shape",
                        format!("guest `{}` is not the folder `{guest}`", scene.guest),
                    ));
                }
                LoadedScene {
                    path,
                    scene,
                    problems,
                }
            }
            Err(reason) => {
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                LoadedScene {
                    scene: Scene {
                        id,
                        title: String::new(),
                        pitch: String::new(),
                        guest: guest.to_string(),
                        status: SceneStatus::Draft,
                        notes: Vec::new(),
                        lines: Vec::new(),
                        fictional: false,
                    },
                    problems: vec![problem(None, "shape", reason)],
                    path,
                }
            }
        })
        .collect();
    out.sort_by(|a, b| a.scene.id.cmp(&b.scene.id));
    out
}

/// The checks that need no policy: shape, pitch length, a known guest, no empty scene, and
/// never ending on the guest.
pub fn check_shape(scene: &Scene, known_foils: &[String]) -> Vec<SceneProblem> {
    let mut out = Vec::new();
    if scene.id.is_empty() || scene.title.trim().is_empty() {
        out.push(problem(
            None,
            "shape",
            "a scene needs an id and a title".into(),
        ));
    }
    if scene.pitch.chars().count() > PITCH_MAX {
        out.push(problem(
            None,
            "pitch_too_long",
            format!(
                "pitch is {} chars, at most {PITCH_MAX}",
                scene.pitch.chars().count()
            ),
        ));
    }
    if !known_foils.iter().any(|f| f == &scene.guest) {
        out.push(problem(
            None,
            "guest_unknown",
            format!("`{}` is not a declared foil", scene.guest),
        ));
    }
    if scene.lines.is_empty() {
        out.push(problem(
            None,
            "no_lines",
            "a scene needs at least one line".into(),
        ));
    }
    for (i, l) in scene.lines.iter().enumerate() {
        let i = Some(i as u32);
        match (&l.text, &l.live) {
            (Some(t), None) if !t.trim().is_empty() => {
                if l.fallback.is_some() {
                    out.push(problem(
                        i,
                        "shape",
                        "`fallback` belongs to a `live` line".into(),
                    ));
                }
            }
            (None, Some(_)) => {
                if l.speaker != SceneSpeaker::Host {
                    out.push(problem(i, "shape", "only the host has live lines".into()));
                }
                if l.fallback.as_deref().is_none_or(|f| f.trim().is_empty()) {
                    out.push(problem(
                        i,
                        "shape",
                        "a `live` line needs a `fallback`".into(),
                    ));
                }
            }
            _ => out.push(problem(
                i,
                "shape",
                "a line has exactly one of `text` and `live`".into(),
            )),
        }
    }
    if scene
        .lines
        .last()
        .is_some_and(|l| l.speaker == SceneSpeaker::Guest)
    {
        out.push(problem(
            Some(scene.lines.len() as u32 - 1),
            "ends_on_guest",
            "the host has the last word".into(),
        ));
    }
    out
}

fn problem(line: Option<u32>, code: &str, detail: String) -> SceneProblem {
    SceneProblem {
        line,
        code: code.into(),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::scene::SceneLine;

    fn line(speaker: SceneSpeaker, text: &str) -> SceneLine {
        SceneLine {
            speaker,
            text: Some(text.into()),
            live: None,
            fallback: None,
        }
    }

    fn scene(lines: Vec<SceneLine>) -> Scene {
        Scene {
            id: "s".into(),
            title: "S".into(),
            pitch: "p".into(),
            guest: "the-painter".into(),
            status: SceneStatus::Approved,
            notes: Vec::new(),
            lines,
            fictional: true,
        }
    }

    #[test]
    fn a_clean_scene_has_no_problems() {
        let s = scene(vec![
            line(SceneSpeaker::Guest, "a"),
            line(SceneSpeaker::Host, "b"),
        ]);
        assert!(check_shape(&s, &["the-painter".into()]).is_empty());
    }

    #[test]
    fn ending_on_the_guest_an_unknown_guest_and_a_live_guest_line_are_problems() {
        let mut s = scene(vec![
            SceneLine {
                speaker: SceneSpeaker::Guest,
                text: None,
                live: Some("x".into()),
                fallback: Some("y".into()),
            },
            line(SceneSpeaker::Guest, "a"),
        ]);
        s.guest = "nobody".into();
        let codes: Vec<String> = check_shape(&s, &["the-painter".into()])
            .into_iter()
            .map(|p| p.code)
            .collect();
        assert!(codes.contains(&"ends_on_guest".to_string()));
        assert!(codes.contains(&"guest_unknown".to_string()));
        assert!(codes.contains(&"shape".to_string()));
    }
}
