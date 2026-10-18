//! Optional constitution rules: `characters/<id>/rules/<rule-id>.md`, one rule per file, with
//! defaults in `character.toml` `[rules]` and the operator's override in
//! `<data>/characters/<id>/rules.json` (`{"<rule-id>": bool}`). The override wins; a toggle
//! never rewrites the tracked package. Enabled rules join the stable prefix right after the
//! constitution, sorted by id, so the prefix stays byte-stable while the set is unchanged.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::BrainError;

/// One optional rule with its effective state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterRule {
    pub id: String,
    /// The file's heading.
    pub title: String,
    /// The rule text below the heading.
    pub text: String,
    /// `character.toml` `[rules]` value (false when the table does not list it).
    pub default: bool,
    /// The operator's override when present, else the default.
    pub enabled: bool,
}

/// The wire shape of `character.rules` / `character.rules.set`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleState {
    pub id: String,
    pub title: String,
    pub enabled: bool,
    pub default: bool,
}

impl From<&CharacterRule> for RuleState {
    fn from(r: &CharacterRule) -> Self {
        RuleState {
            id: r.id.clone(),
            title: r.title.clone(),
            enabled: r.enabled,
            default: r.default,
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

/// Every rule in `<dir>/rules/`, sorted by id, with defaults and the override applied.
pub(crate) fn resolve_rules(
    dir: &Path,
    character_id: &str,
    defaults: &BTreeMap<String, bool>,
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
        let enabled = over.get(&id).copied().unwrap_or(default);
        out.push(CharacterRule {
            id,
            title,
            text,
            default,
            enabled,
        });
    }
    Ok(out)
}

#[derive(Deserialize)]
struct RulesToml {
    id: String,
    #[serde(default)]
    rules: BTreeMap<String, bool>,
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
    resolve_rules(character_dir, &meta.id, &meta.rules, overrides)
}

/// Set one rule in the operator's override file (`<data>/characters/<id>/rules.json`) and
/// return the updated list. An unknown rule id is an error; the package is never written.
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
}
