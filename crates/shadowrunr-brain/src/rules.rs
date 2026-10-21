//! Package rules: `characters/<id>/rules/<rule-id>.md`, one rule per file, with defaults in
//! `character.toml` `[rules]` and the operator's override in
//! `<data>/characters/<id>/rules.json` (`{"<rule-id>": bool}`). Two kinds:
//!
//! - an optional rule: the override wins over the default; a toggle never rewrites the
//!   tracked package.
//! - a rail (`character.toml` `rails = [...]`): always on. It must have its file and a true
//!   default; the override cannot reach it (a stored value is ignored, never rewritten) and
//!   [`set_rule`] refuses it. Owners configure how a rail is delivered, never whether.
//!
//! Enabled rules join the stable prefix right after the constitution, sorted by id, so the
//! prefix stays byte-stable while the set is unchanged.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::BrainError;

/// One package rule with its effective state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterRule {
    pub id: String,
    /// The file's heading.
    pub title: String,
    /// The rule text below the heading.
    pub text: String,
    /// `character.toml` `[rules]` value (false when the table does not list it).
    pub default: bool,
    /// The operator's override when present, else the default; always the default for a rail.
    pub enabled: bool,
    /// A rail (`character.toml` `rails`): always on, out of the override's reach.
    pub locked: bool,
}

/// The wire shape of `character.rules` / `character.rules.set`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleState {
    pub id: String,
    pub title: String,
    pub enabled: bool,
    pub default: bool,
    pub locked: bool,
}

impl From<&CharacterRule> for RuleState {
    fn from(r: &CharacterRule) -> Self {
        RuleState {
            id: r.id.clone(),
            title: r.title.clone(),
            enabled: r.enabled,
            default: r.default,
            locked: r.locked,
        }
    }
}

/// Where the operator's override comes from.
#[derive(Debug, Clone, Default)]
pub enum RulesOverride {
    /// The repo convention: `<character_dir>/../../data/characters/<id>/rules.json`, i.e. the
    /// default `[paths] data` next to the default `[paths] characters`.
    #[default]
    Auto,
    File(PathBuf),
    /// Package defaults only.
    None,
}

/// `<data>/characters/<id>/rules.json`.
pub fn override_path(data_dir: &Path, character_id: &str) -> PathBuf {
    data_dir
        .join("characters")
        .join(character_id)
        .join("rules.json")
}

/// The override path [`RulesOverride::Auto`] resolves to.
pub fn auto_override_path(character_dir: &Path, character_id: &str) -> PathBuf {
    override_path(
        &character_dir.join("..").join("..").join("data"),
        character_id,
    )
}

fn pkg_err(m: impl Into<String>) -> BrainError {
    BrainError::Package(m.into())
}

fn is_safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Read an override file. A missing file is an empty override; a malformed one is an error
/// (silently falling back could switch a safety rule off).
pub fn read_overrides(path: &Path) -> Result<BTreeMap<String, bool>, BrainError> {
    match fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|e| pkg_err(format!("{}: {e}", path.display())))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(pkg_err(format!("{}: {e}", path.display()))),
    }
}

/// Split a rule file into its heading and text.
fn parse_rule_file(id: &str, text: &str) -> Result<(String, String), BrainError> {
    let text = text.replace("\r\n", "\n");
    let mut lines = text.lines().skip_while(|l| l.trim().is_empty()).peekable();
    let title = match lines.peek() {
        Some(l) if l.trim_start().starts_with('#') => {
            let t = l.trim_start().trim_start_matches('#').trim().to_string();
            lines.next();
            t
        }
        _ => id.to_string(),
    };
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    if body.is_empty() {
        return Err(pkg_err(format!("rules/{id}.md has no rule text")));
    }
    Ok((
        if title.is_empty() {
            id.to_string()
        } else {
            title
        },
        body,
    ))
}

/// Every rule in `<dir>/rules/`, sorted by id, with defaults and the override applied. A rail
/// must have its file and a true default, and the override does not reach it.
pub(crate) fn resolve_rules(
    dir: &Path,
    character_id: &str,
    defaults: &BTreeMap<String, bool>,
    rails: &[String],
    overrides: &RulesOverride,
) -> Result<Vec<CharacterRule>, BrainError> {
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let rules_dir = dir.join("rules");
    if rules_dir.is_dir() {
        let entries = fs::read_dir(&rules_dir)
            .map_err(|e| pkg_err(format!("{}: {e}", rules_dir.display())))?;
        for entry in entries {
            let path = entry.map_err(|e| pkg_err(format!("rules: {e}")))?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !is_safe_id(id) {
                return Err(pkg_err(format!("rule id {id:?} is not [A-Za-z0-9_-]")));
            }
            files.insert(id.to_string(), path);
        }
    }
    for id in defaults.keys() {
        if !files.contains_key(id) {
            return Err(pkg_err(format!(
                "rule {id:?} is listed in character.toml [rules] but rules/{id}.md is missing"
            )));
        }
    }
    for id in rails {
        if !files.contains_key(id) {
            return Err(pkg_err(format!(
                "rail {id:?} is listed in character.toml rails but rules/{id}.md is missing"
            )));
        }
        if defaults.get(id) != Some(&true) {
            return Err(pkg_err(format!(
                "rail {id:?} must be `{id} = true` in character.toml [rules]: a rail is always on"
            )));
        }
    }
    let over = match overrides {
        RulesOverride::Auto => read_overrides(&auto_override_path(dir, character_id))?,
        RulesOverride::File(p) => read_overrides(p)?,
        RulesOverride::None => BTreeMap::new(),
    };
    let mut out = Vec::with_capacity(files.len());
    for (id, path) in files {
        let text =
            fs::read_to_string(&path).map_err(|e| pkg_err(format!("{}: {e}", path.display())))?;
        let (title, text) = parse_rule_file(&id, &text)?;
        let default = defaults.get(&id).copied().unwrap_or(false);
        let locked = rails.contains(&id);
        // A rail ignores the override: a stored `false` stays in the file, unread.
        let enabled = if locked {
            default
        } else {
            over.get(&id).copied().unwrap_or(default)
        };
        out.push(CharacterRule {
            id,
            title,
            text,
            default,
            enabled,
            locked,
        });
    }
    Ok(out)
}

#[derive(Deserialize)]
struct RulesToml {
    id: String,
    #[serde(default)]
    rules: BTreeMap<String, bool>,
    #[serde(default)]
    rails: Vec<String>,
}

/// The rules of a package without loading the rest of it (the daemon and the sim instrument).
pub fn load_rules(
    character_dir: &Path,
    overrides: &RulesOverride,
) -> Result<Vec<CharacterRule>, BrainError> {
    let path = character_dir.join("character.toml");
    let text =
        fs::read_to_string(&path).map_err(|e| pkg_err(format!("{}: {e}", path.display())))?;
    let meta: RulesToml =
        toml::from_str(&text).map_err(|e| pkg_err(format!("character.toml: {e}")))?;
    if !is_safe_id(&meta.id) {
        return Err(pkg_err(format!(
            "character id {:?} is not [A-Za-z0-9_-]",
            meta.id
        )));
    }
    resolve_rules(character_dir, &meta.id, &meta.rules, &meta.rails, overrides)
}

/// Set one rule in the operator's override file (`<data>/characters/<id>/rules.json`) and
/// return the updated list. An unknown rule id and a rail are [`BrainError::Rejected`] and write
/// nothing; the package is never written.
pub fn set_rule(
    character_dir: &Path,
    character_id: &str,
    override_file: &Path,
    rule_id: &str,
    enabled: bool,
) -> Result<Vec<RuleState>, BrainError> {
    let rules = load_rules(
        character_dir,
        &RulesOverride::File(override_file.to_path_buf()),
    )?;
    if rules.iter().any(|r| r.id == rule_id && r.locked) {
        return Err(BrainError::Rejected(format!(
            "{rule_id} is a rail of character {character_id}: always on, the operator override \
             cannot switch it (character.toml rails)"
        )));
    }
    if !rules.iter().any(|r| r.id == rule_id) {
        let known: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        return Err(BrainError::Rejected(format!(
            "character {character_id} has no rule {rule_id:?} (known: {})",
            if known.is_empty() {
                "none".to_string()
            } else {
                known.join(", ")
            }
        )));
    }
    let mut over = read_overrides(override_file)?;
    over.insert(rule_id.to_string(), enabled);
    if let Some(parent) = override_file.parent() {
        fs::create_dir_all(parent).map_err(|e| pkg_err(format!("{}: {e}", parent.display())))?;
    }
    let json = serde_json::to_string_pretty(&over).map_err(|e| pkg_err(e.to_string()))?;
    let tmp = override_file.with_extension("json.tmp");
    fs::write(&tmp, format!("{json}\n")).map_err(|e| pkg_err(format!("{}: {e}", tmp.display())))?;
    fs::rename(&tmp, override_file)
        .map_err(|e| pkg_err(format!("{}: {e}", override_file.display())))?;
    let rules = load_rules(
        character_dir,
        &RulesOverride::File(override_file.to_path_buf()),
    )?;
    Ok(rules.iter().map(RuleState::from).collect())
}

/// A stable text of the effective enabled set (`id\n` per enabled rule, sorted), for hashing.
pub fn enabled_set_text(rules: &[CharacterRule]) -> String {
    let mut ids: Vec<&str> = rules
        .iter()
        .filter(|r| r.enabled)
        .map(|r| r.id.as_str())
        .collect();
    ids.sort_unstable();
    ids.iter().map(|id| format!("{id}\n")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(root: &Path) -> PathBuf {
        let dir = root.join("characters").join("x");
        fs::create_dir_all(dir.join("rules")).unwrap();
        fs::write(
            dir.join("character.toml"),
            "id = \"x\"\n[rules]\nalpha = true\nbeta = false\n",
        )
        .unwrap();
        fs::write(
            dir.join("rules/alpha.md"),
            "# Alpha rule\r\n\r\nDo alpha.\r\n",
        )
        .unwrap();
        fs::write(dir.join("rules/beta.md"), "# Beta rule\n\nDo beta.\n").unwrap();
        fs::write(dir.join("rules/gamma.md"), "Gamma without heading.\n").unwrap();
        dir
    }

    #[test]
    fn defaults_then_override_wins() {
        let d = tempfile::tempdir().unwrap();
        let dir = pkg(d.path());
        let rules = load_rules(&dir, &RulesOverride::None).unwrap();
        let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["alpha", "beta", "gamma"]);
        assert_eq!(rules[0].title, "Alpha rule");
        assert_eq!(rules[0].text, "Do alpha.");
        assert!(rules[0].enabled && rules[0].default);
        assert!(!rules[1].enabled && !rules[1].default);
        assert_eq!(rules[2].title, "gamma");
        assert!(!rules[2].enabled, "unlisted rules are off by default");

        // The Auto override lives under the repo's data dir.
        let over = auto_override_path(&dir, "x");
        fs::create_dir_all(over.parent().unwrap()).unwrap();
        fs::write(&over, r#"{"alpha": false, "beta": true, "unknown": true}"#).unwrap();
        let rules = load_rules(&dir, &RulesOverride::Auto).unwrap();
        assert!(!rules[0].enabled && rules[0].default);
        assert!(rules[1].enabled && !rules[1].default);
        assert_eq!(enabled_set_text(&rules), "beta\n");
    }

    #[test]
    fn set_rule_writes_only_the_override() {
        let d = tempfile::tempdir().unwrap();
        let dir = pkg(d.path());
        let toml_before = fs::read_to_string(dir.join("character.toml")).unwrap();
        let file = override_path(&d.path().join("data"), "x");
        let list = set_rule(&dir, "x", &file, "alpha", false).unwrap();
        assert!(!list.iter().find(|r| r.id == "alpha").unwrap().enabled);
        assert_eq!(read_overrides(&file).unwrap().get("alpha"), Some(&false));
        assert_eq!(
            fs::read_to_string(dir.join("character.toml")).unwrap(),
            toml_before
        );
        let err = set_rule(&dir, "x", &file, "nope", true).unwrap_err();
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn listed_rule_without_file_and_bad_override_are_errors() {
        let d = tempfile::tempdir().unwrap();
        let dir = pkg(d.path());
        fs::remove_file(dir.join("rules/beta.md")).unwrap();
        assert!(load_rules(&dir, &RulesOverride::None)
            .unwrap_err()
            .to_string()
            .contains("beta"));
        let d = tempfile::tempdir().unwrap();
        let dir = pkg(d.path());
        let bad = d.path().join("bad.json");
        fs::write(&bad, "{not json").unwrap();
        assert!(load_rules(&dir, &RulesOverride::File(bad)).is_err());
    }

    fn malakor() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("characters")
            .join("malakor")
    }

    #[test]
    fn set_rule_refuses_a_rail_and_writes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let file = override_path(d.path(), "malakor");
        let err = set_rule(&malakor(), "malakor", &file, "crisis-handoff", false)
            .expect_err("a rail is not a switch");
        assert!(matches!(err, BrainError::Rejected(_)), "{err:?}");
        assert!(
            err.to_string().contains("crisis-handoff is a rail"),
            "{err}"
        );
        assert!(!file.exists(), "rules.json is not written for a rail");
    }

    /// GUARD: only rails lock; the optional rules stay operator switches.
    #[test]
    fn optional_rules_beside_a_rail_stay_switches() {
        let d = tempfile::tempdir().unwrap();
        let file = override_path(d.path(), "malakor");
        let list = set_rule(
            &malakor(),
            "malakor",
            &file,
            "no-financial-promotion",
            false,
        )
        .expect("an optional rule toggles");
        let list2 = set_rule(&malakor(), "malakor", &file, "crisis-chat-ban", false)
            .expect("an optional rule toggles");
        assert!(
            !list
                .iter()
                .find(|r| r.id == "no-financial-promotion")
                .unwrap()
                .enabled
        );
        assert!(
            !list2
                .iter()
                .find(|r| r.id == "crisis-chat-ban")
                .unwrap()
                .enabled
        );
        let saved = read_overrides(&file).unwrap();
        assert_eq!(saved.get("crisis-chat-ban"), Some(&false));
        assert_eq!(saved.get("no-financial-promotion"), Some(&false));
    }

    #[test]
    fn a_rail_must_have_its_file_and_a_true_default() {
        // Listed under `rails` but `[rules]` sets it false.
        let d = tempfile::tempdir().unwrap();
        let dir = pkg(d.path());
        fs::write(
            dir.join("character.toml"),
            "id = \"x\"\nrails = [\"beta\"]\n[rules]\nalpha = true\nbeta = false\n",
        )
        .unwrap();
        let err = load_rules(&dir, &RulesOverride::None).expect_err("a rail defaulting off");
        assert!(err.to_string().contains("beta"), "{err}");

        // Listed under `rails` with no rules/<id>.md.
        let d = tempfile::tempdir().unwrap();
        let dir = pkg(d.path());
        fs::write(
            dir.join("character.toml"),
            "id = \"x\"\nrails = [\"delta\"]\n[rules]\nalpha = true\n",
        )
        .unwrap();
        let err = load_rules(&dir, &RulesOverride::None).expect_err("a rail with no file");
        assert!(err.to_string().contains("delta"), "{err}");
    }
}
