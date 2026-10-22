//! The daemon's side of a character package (`characters/<id>/`, docs/guide/characters.md):
//! `character validate` with the template's fill-in warnings, `character new` from a
//! template, and what the package overrides in the daemon's own config (the voice, the face
//! renderer, the served rig).

use std::path::{Path, PathBuf};

use shadowrunr_brain::{parse_manifest, validate_character, CharacterPackage};
use shadowrunr_segments::{RankTable, SegmentConfig};
use shadowrunr_types::catalog::{CharacterIssue, IssueSeverity};
use shadowrunr_types::character::{CharacterManifest, Persona};

use crate::config::{Config, FaceVariant};
use crate::voice::VoiceConfig;

/// The text a template leaves for the author to replace. `character validate` reports every
/// value that still holds it as a warning, never an error.
pub const FILL_MARKER: &str = "FILL:";

/// The scaffold `character new` copies by default (`<characters>/_template`).
pub const TEMPLATE_ID: &str = "_template";

/// Text files `validate` scans for [`FILL_MARKER`].
const TEXT_EXTENSIONS: [&str; 3] = ["toml", "md", "json"];

/// `character validate`: every error the brain's loader finds (`file: key: reason`), then a
/// warning for each value that still holds [`FILL_MARKER`].
pub fn validate(dir: &Path) -> Vec<CharacterIssue> {
    let mut out: Vec<CharacterIssue> = validate_character(dir)
        .into_iter()
        .map(|i| CharacterIssue {
            file: i.file,
            key: i.key,
            reason: i.reason,
            severity: IssueSeverity::Error,
        })
        .collect();
    for (rel, path) in text_files(dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let text = text.replace("\r\n", "\n");
        let keys = if rel.ends_with(".toml") {
            toml_fill_keys(&text)
        } else {
            line_fill_keys(&text)
        };
        for key in keys {
            out.push(CharacterIssue {
                file: rel.clone(),
                key,
                reason: "still a template placeholder: fill this in".into(),
                severity: IssueSeverity::Warning,
            });
        }
    }
    out
}

/// Whether `issues` holds an error (a warning alone passes).
pub fn has_errors(issues: &[CharacterIssue]) -> bool {
    issues.iter().any(|i| i.severity == IssueSeverity::Error)
}

/// `file: key: reason`, or `file: reason` when the issue is the file.
pub fn issue_line(i: &CharacterIssue) -> String {
    let level = match i.severity {
        IssueSeverity::Error => "",
        IssueSeverity::Warning => " (warning)",
    };
    if i.key.is_empty() {
        format!("{}: {}{level}", i.file, i.reason)
    } else {
        format!("{}: {}: {}{level}", i.file, i.key, i.reason)
    }
}

/// `(relative path with / separators, path)` of every text file in the package, sorted.
fn text_files(dir: &Path) -> Vec<(String, PathBuf)> {
    fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() && depth < 4 {
                walk(root, &p, depth + 1, out);
            } else if p
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| TEXT_EXTENSIONS.contains(&x))
            {
                let rel = p
                    .strip_prefix(root)
                    .unwrap_or(&p)
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push((rel, p));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, 0, &mut out);
    out.sort();
    out
}

/// The dotted key of every TOML value holding [`FILL_MARKER`] (comments are skipped; a line
/// inside a multi-line array names the array's key).
fn toml_fill_keys(text: &str) -> Vec<String> {
    let mut table = String::new();
    let mut array_key: Option<String> = None;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('#') || t.is_empty() {
            continue;
        }
        if array_key.is_none() && t.starts_with('[') && !t.contains('=') {
            table = t.trim_matches(['[', ']']).trim().to_string();
            continue;
        }
        let key = match (&array_key, t.split_once('=')) {
            (Some(k), _) => k.clone(),
            (None, Some((k, _))) => {
                let k = k.trim().trim_matches('"');
                if table.is_empty() {
                    k.to_string()
                } else {
                    format!("{table}.{k}")
                }
            }
            (None, None) => continue,
        };
        if array_key.is_none() && t.ends_with('[') {
            array_key = Some(key.clone());
        } else if array_key.is_some() && t.starts_with(']') {
            array_key = None;
        }
        if value_part(t).contains(FILL_MARKER) && !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

/// The line without a trailing `# comment` outside a string.
fn value_part(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

/// `line <n>` (1-based) of every line holding [`FILL_MARKER`].
fn line_fill_keys(text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| l.contains(FILL_MARKER))
        .map(|(i, _)| format!("line {}", i + 1))
        .collect()
}

/// `character new`: copy `template` to `<characters>/<id>/` with its `id` set, then validate
/// the copy. Refuses an unsafe id and an existing directory; never overwrites a file.
pub fn new_character(
    characters: &Path,
    id: &str,
    template: &Path,
) -> Result<(PathBuf, Vec<CharacterIssue>), String> {
    crate::runs::check_id("character", id).map_err(|e| e.message)?;
    if id.starts_with('_') {
        return Err(format!(
            "character id {id:?} starts with '_', which marks a template"
        ));
    }
    let manifest = template.join("character.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| format!("template {}: {e}", manifest.display()))?;
    let dest = characters.join(id);
    if dest.exists() {
        return Err(format!(
            "{} already exists; character new never overwrites",
            dest.display()
        ));
    }
    let text = set_id(&text, id)
        .ok_or_else(|| format!("template {}: no top-level id line", manifest.display()))?;
    copy_dir(template, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    std::fs::write(dest.join("character.toml"), text)
        .map_err(|e| format!("{}: {e}", dest.display()))?;
    let issues = validate(&dest);
    Ok((dest, issues))
}

/// The manifest with its top-level `id = "..."` line set to `id`.
fn set_id(text: &str, id: &str) -> Option<String> {
    let mut done = false;
    let lines: Vec<String> = text
        .lines()
        .map(|l| {
            let t = l.trim_start();
            if !done && t.starts_with("id") && t[2..].trim_start().starts_with('=') {
                done = true;
                format!("id = \"{id}\"")
            } else {
                l.to_string()
            }
        })
        .collect();
    done.then(|| lines.join("\n") + "\n")
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else if target.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} exists", target.display()),
            ));
        } else {
            std::fs::copy(e.path(), &target)?;
        }
    }
    Ok(())
}

/// The segment runtime's config for a stream of `pkg`, as the sim runner and the sandbox build
/// it: its foils, persona, tuning view, segment flavour and rank-ladder overrides over
/// `<segments>/ranks.toml`.
pub fn segment_config(cfg: &Config, pkg: &CharacterPackage) -> Result<SegmentConfig, String> {
    let path = cfg.paths.segments.join("ranks.toml");
    let ranks = RankTable::load_or_builtin(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .with_overrides(&pkg.manifest.ranks)
        .map_err(|e| format!("character {} [ranks]: {e}", pkg.id))?;
    Ok(SegmentConfig {
        foils: pkg.foil_ids.clone(),
        ranks,
        persona: pkg.persona.clone(),
        tuning: pkg.tuning,
        flavour: pkg.manifest.segments.clone(),
        ..SegmentConfig::default()
    })
}

/// A package's manifest, when `<characters>/<id>/character.toml` reads and parses.
pub fn manifest(characters: &Path, id: &str) -> Option<CharacterManifest> {
    if crate::runs::check_id("character", id).is_err() {
        return None;
    }
    let text = std::fs::read_to_string(characters.join(id).join("character.toml")).ok()?;
    parse_manifest(&text.replace("\r\n", "\n"))
        .ok()
        .map(|(m, _, _)| m)
}

/// The persona of `<characters>/<id>/`; the neutral one when its package does not load.
pub fn persona_of(characters: &Path, id: &str) -> Persona {
    manifest(characters, id)
        .map(|m| Persona::from_manifest(&m))
        .unwrap_or_default()
}

/// The configured character's (`[sim] character`) persona.
pub fn persona(cfg: &Config) -> Persona {
    persona_of(&cfg.paths.characters, &cfg.sim.character)
}

/// The daemon's `[voice]` with the package's `[voice]` over it, field by field. `speed` is
/// not applied: the voice sidecar takes no rate flag yet.
pub fn voice_for(base: &VoiceConfig, m: Option<&CharacterManifest>) -> VoiceConfig {
    let mut v = base.clone();
    if let Some(cv) = m.and_then(|m| m.voice.as_ref()) {
        if let Some(voice) = &cv.voice {
            v.voice = voice.clone();
        }
        if let Some(sid) = cv.sid {
            v.sid = sid;
        }
        if let Some(file) = &cv.voices_file {
            v.voices_file = Some(file.clone());
        }
        if let Some(fx) = cv.fx {
            v.fx = Some(fx);
        }
    }
    v
}

/// The daemon's `[stage] avatar` unless the package's `[avatar] face` names a renderer.
pub fn face_for(base: FaceVariant, m: Option<&CharacterManifest>) -> FaceVariant {
    m.and_then(|m| m.avatar.as_ref())
        .and_then(|a| a.face.as_deref())
        .and_then(|f| serde_json::from_value(serde_json::Value::String(f.to_string())).ok())
        .unwrap_or(base)
}

/// Apply the configured character's (`[sim] character`) `[voice]` and `[avatar]` to `cfg`.
/// A package that does not load leaves the daemon's own values (pre-flight reports it).
pub fn apply_overrides(cfg: &mut Config) {
    let m = manifest(&cfg.paths.characters, &cfg.sim.character);
    cfg.voice = voice_for(&cfg.voice, m.as_ref());
    cfg.stage.avatar = face_for(cfg.stage.avatar, m.as_ref());
}

/// Where `/avatar/{file}` serves `character`'s rig from: `<data>/characters/<id>/avatar/rig/`
/// when it exists, else the shared `<data>/avatar/rig/` (the pre-package location, kept as the
/// fallback; nothing is moved automatically).
pub fn rig_dir(data: &Path, character: &str) -> PathBuf {
    if crate::runs::check_id("character", character).is_ok() {
        let own = data
            .join("characters")
            .join(character)
            .join("avatar")
            .join("rig");
        if own.is_dir() {
            return own;
        }
    }
    crate::voice::rig_dir(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::character::VoiceFx;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
    }

    fn template() -> PathBuf {
        repo().join("characters").join(TEMPLATE_ID)
    }

    fn keeper() -> PathBuf {
        repo()
            .join("crates")
            .join("shadowrunr-brain")
            .join("tests")
            .join("fixtures")
            .join("characters")
            .join("keeper")
    }

    /// One documented parameter of a template file: `# ? question`, `# options: ...`,
    /// `# default: ...`, then its key line (`key = value` or `# key = value`).
    struct Param {
        key: String,
        options: String,
        default: String,
        written: String,
    }

    fn params(text: &str) -> Vec<Param> {
        let lines: Vec<&str> = text.lines().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let Some(o) = lines[i].strip_prefix("# options: ") else {
                i += 1;
                continue;
            };
            let d = lines[i + 1]
                .strip_prefix("# default: ")
                .unwrap_or_else(|| panic!("no default after line {}", i + 1));
            let key_line = lines[i + 2].trim_start_matches("# ");
            let (key, value) = key_line
                .split_once(" = ")
                .unwrap_or_else(|| panic!("no key line after line {}", i + 2));
            let mut written = value.to_string();
            let mut j = i + 3;
            if written.ends_with('[') {
                while !lines[j].starts_with(']') {
                    written.push_str(lines[j]);
                    j += 1;
                }
                written.push(']');
            }
            assert!(
                lines[i - 1].starts_with("# ? "),
                "{key}: no question before line {}",
                i + 1
            );
            out.push(Param {
                key: key.to_string(),
                options: o.to_string(),
                default: d.to_string(),
                written,
            });
            i = j;
        }
        out
    }

    fn value(text: &str) -> toml::Value {
        toml::from_str::<toml::Table>(&format!("v = {text}"))
            .unwrap_or_else(|e| panic!("{text}: {e}"))["v"]
            .clone()
    }

    #[test]
    fn every_template_default_is_one_of_its_options_and_is_what_it_writes() {
        use shadowrunr_types::character::TUNING_DEFAULTS;
        let text = std::fs::read_to_string(template().join("character.toml")).unwrap();
        let ps = params(&text.replace("\r\n", "\n"));
        assert!(ps.len() >= 50, "{} parameters", ps.len());
        for p in &ps {
            let d = value(&p.default);
            let w = value(&p.written);
            if let Some(t) = w.as_table().filter(|t| t.contains_key("value")) {
                // A tuning key: the engine default, with the default's provenance.
                let row = TUNING_DEFAULTS
                    .iter()
                    .find(|r| r.key == p.key)
                    .unwrap_or_else(|| panic!("{} is no tuning key", p.key));
                assert_eq!(d.as_integer(), Some(row.value as i64), "{}", p.key);
                assert_eq!(t["value"], d, "{}", p.key);
                assert_eq!(
                    t.get("source").and_then(|s| s.as_str()),
                    row.source,
                    "{}",
                    p.key
                );
            } else {
                assert_eq!(w, d, "{}: the default is what the template writes", p.key);
            }
            if p.options.contains('|')
                || !p.options.starts_with('a') && !p.options.starts_with("free")
            {
                let opts: Vec<toml::Value> =
                    p.options.split('|').map(|o| value(o.trim())).collect();
                assert!(
                    opts.contains(&d),
                    "{}: {} not in {}",
                    p.key,
                    p.default,
                    p.options
                );
            } else if p.options.starts_with("a positive whole number") {
                assert!(d.as_integer().is_some_and(|n| n > 0), "{}", p.key);
            } else if p.options.starts_with("a whole number") {
                assert!(d.as_integer().is_some(), "{}", p.key);
            } else if p.options.starts_with("a list") {
                assert!(d.as_array().is_some(), "{}", p.key);
            } else {
                assert!(
                    p.options.starts_with("free text"),
                    "{}: {}",
                    p.key,
                    p.options
                );
                let strings = d.as_str().is_some()
                    || d.as_table()
                        .is_some_and(|t| t.values().all(|v| v.as_str().is_some()));
                assert!(strings, "{}", p.key);
            }
        }
        // Every tuning key is documented.
        for row in TUNING_DEFAULTS {
            assert!(ps.iter().any(|p| p.key == row.key), "{} missing", row.key);
        }
        // The other documented files carry their parameters the same way.
        assert_eq!(
            params(&std::fs::read_to_string(template().join("stalls.toml")).unwrap()).len(),
            1
        );
    }

    #[test]
    fn the_template_carries_no_author_paths_and_no_malakor_words() {
        let malakor =
            shadowrunr_brain::load_character(&repo().join("characters").join("malakor"), "")
                .unwrap();
        let mut words: Vec<String> = malakor.persona.words();
        words.extend(
            [
                &malakor.persona.followers,
                &malakor.persona.researchers,
                &malakor.persona.patron_title,
            ]
            .map(|w| w.to_string()),
        );
        words.extend(["Ninth Dark", "Spire", "Phylactery"].map(String::from));
        for (rel, path) in text_files(&template()) {
            let text = std::fs::read_to_string(&path).unwrap().to_lowercase();
            // Rank wire names are the engine's, not his: `lich = "..."` and `"lich"`.
            let text = text.replace("\nlich = ", "\n").replace("\"lich\"", "");
            for w in &words {
                let w = w.to_lowercase();
                let hit = text.match_indices(&w).any(|(i, _)| {
                    let before = text[..i].chars().last();
                    let after = text[i + w.len()..].chars().next();
                    !before.is_some_and(char::is_alphanumeric)
                        && !after.is_some_and(char::is_alphanumeric)
                });
                assert!(!hit, "{rel} says {w:?}");
            }
            for p in [":\\", ":/", "/users/", "/home/", "\\users\\"] {
                assert!(!text.contains(p), "{rel} holds a path ({p})");
            }
        }
    }

    #[test]
    fn fill_keys_name_the_dotted_toml_key_and_skip_comments() {
        let t = "# FILL: not this\nid = \"x\"\nname = \"FILL: the name\" # FILL: nor this\n\
                 [identity]\nfollower = { one = \"FILL: one\", many = \"FILL: many\" }\n\
                 [lines]\nrecast = [\n    \"ok\",\n    \"FILL: a bit\",\n]\nblocked = \"fine\"\n\
                 [segments.\"SEG-01\"]\nrules_prompt = \"FILL: flavour\"\n";
        assert_eq!(
            toml_fill_keys(t),
            [
                "name",
                "identity.follower",
                "lines.recast",
                "segments.\"SEG-01\".rules_prompt"
            ]
        );
        assert_eq!(line_fill_keys("a\nFILL: b\nc"), ["line 2"]);
    }

    #[test]
    fn new_then_validate_leaves_only_fill_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, issues) = new_character(tmp.path(), "nova", &template()).unwrap();
        assert_eq!(dir, tmp.path().join("nova"));
        assert!(!has_errors(&issues), "{issues:#?}");
        assert!(
            !issues.is_empty(),
            "the template's placeholders are reported"
        );
        assert!(issues
            .iter()
            .all(|i| i.severity == IssueSeverity::Warning && i.reason.contains("placeholder")));
        assert!(issues
            .iter()
            .any(|i| i.file == "character.toml" && i.key == "identity.name"));
        let m = manifest(tmp.path(), "nova").unwrap();
        assert_eq!(m.id, "nova");
        // It refuses to overwrite, and a template id.
        let again = new_character(tmp.path(), "nova", &template()).unwrap_err();
        assert!(again.contains("already exists"), "{again}");
        assert!(new_character(tmp.path(), "_mine", &template()).is_err());
        assert!(new_character(tmp.path(), "../x", &template()).is_err());
    }

    #[test]
    fn a_filled_package_validates_clean() {
        assert!(validate(&keeper()).is_empty(), "{:#?}", validate(&keeper()));
        let malakor = repo().join("characters").join("malakor");
        assert!(validate(&malakor).is_empty(), "{:#?}", validate(&malakor));
    }

    #[test]
    fn validate_reports_errors_as_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, _) = new_character(tmp.path(), "bad", &template()).unwrap();
        let path = dir.join("character.toml");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text + "\nwhat = 1\n").unwrap();
        let issues = validate(&dir);
        assert!(has_errors(&issues), "{issues:#?}");
        let e = issues
            .iter()
            .find(|i| i.severity == IssueSeverity::Error)
            .unwrap();
        assert_eq!(e.file, "character.toml");
        assert!(issue_line(e).starts_with("character.toml: "), "{e:?}");
    }

    #[test]
    fn the_package_voice_and_face_override_the_daemon_field_by_field() {
        let base = VoiceConfig::default();
        let m: CharacterManifest = toml::from_str(
            "id = \"k\"\nname = \"K\"\ndefault_tone = \"calm\"\n[voice]\nsid = 7\n\
             [avatar]\nface = \"depth\"\n",
        )
        .unwrap();
        let v = voice_for(&base, Some(&m));
        assert_eq!((v.voice.as_str(), v.sid), (base.voice.as_str(), 7));
        assert_eq!(v.max_queue, base.max_queue);
        assert_eq!(voice_for(&base, None).sid, base.sid);
        assert_eq!((v.voices_file.as_deref(), v.fx), (None, None));
        let blended: CharacterManifest = toml::from_str(
            "id = \"k\"\nname = \"K\"\ndefault_tone = \"calm\"\n[voice]\nvoice = \"k-a\"\nsid = 52\n\
             voices_file = \"k-voices.bin\"\nfx = \"light\"\n",
        )
        .unwrap();
        let mine = VoiceConfig {
            voices_dir: Some("V".into()),
            fx: Some(VoiceFx::Heavy),
            ..VoiceConfig::default()
        };
        let v = voice_for(&mine, Some(&blended));
        assert_eq!(
            (v.voice.as_str(), v.sid, v.voices_file.as_deref(), v.fx),
            ("k-a", 52, Some("k-voices.bin"), Some(VoiceFx::Light))
        );
        assert_eq!(
            v.voices_dir.as_deref(),
            Some("V"),
            "the folder is the machine's"
        );
        assert!(
            toml::from_str::<CharacterManifest>(
                "id = \"k\"\nname = \"K\"\ndefault_tone = \"calm\"\n[voice]\nfx = \"loud\"\n"
            )
            .is_err(),
            "an fx that is not a preset is refused"
        );
        assert_eq!(face_for(FaceVariant::Cut, Some(&m)), FaceVariant::Depth);
        assert_eq!(face_for(FaceVariant::Glow, None), FaceVariant::Glow);
        let odd: CharacterManifest = toml::from_str(
            "id = \"k\"\nname = \"K\"\ndefault_tone = \"calm\"\n[avatar]\nface = \"mesh\"\n",
        )
        .unwrap();
        assert_eq!(face_for(FaceVariant::Glow, Some(&odd)), FaceVariant::Glow);
    }

    #[test]
    fn the_rig_is_the_character_s_own_else_the_shared_one() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        assert_eq!(rig_dir(data, "keeper"), data.join("avatar").join("rig"));
        let own = data
            .join("characters")
            .join("keeper")
            .join("avatar")
            .join("rig");
        std::fs::create_dir_all(&own).unwrap();
        assert_eq!(rig_dir(data, "keeper"), own);
        assert_eq!(rig_dir(data, "malakor"), data.join("avatar").join("rig"));
        assert_eq!(rig_dir(data, "../x"), data.join("avatar").join("rig"));
    }
}
