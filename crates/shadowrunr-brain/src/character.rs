//! Character package loader (`characters/<id>/`, layout in docs/spec/contract.md).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use shadowrunr_types::Rank;

use crate::rules::{resolve_rules, CharacterRule, RulesOverride};
use crate::stall::StallBank;
use crate::BrainError;

/// Rank titles as the character says them (`[ranks]` in character.toml).
#[derive(Debug, Clone, Default)]
pub struct RankTitles(BTreeMap<Rank, String>);

impl RankTitles {
    /// The character's title for a rank, or the plain wire-name fallback.
    pub fn title(&self, rank: Rank) -> String {
        self.0
            .get(&rank)
            .cloned()
            .unwrap_or_else(|| default_title(rank).to_string())
    }
}

fn default_title(rank: Rank) -> &'static str {
    match rank {
        Rank::Thrall => "Thrall",
        Rank::Acolyte => "Acolyte",
        Rank::Cultist => "Cultist",
        Rank::Warlock => "Warlock",
        Rank::Lich => "Lich",
        Rank::DreadCouncil => "Dread Council",
    }
}

/// A SEG-01 guest foil (`foils/<id>.md`).
#[derive(Debug, Clone)]
pub struct Foil {
    pub id: String,
    pub text: String,
}

/// `situations.json`: the sim situation bank.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SituationBank {
    pub version: u32,
    #[serde(default)]
    pub situations: Vec<Situation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Situation {
    pub id: String,
    pub behaviour: String,
    pub archetype: String,
    #[serde(default)]
    pub near_miss: bool,
    pub required: String,
    #[serde(default)]
    pub forbidden: Vec<String>,
    #[serde(default)]
    pub invariants: Vec<String>,
}

/// A loaded character package (`characters/<id>/`, layout in docs/spec/contract.md).
#[derive(Debug, Clone)]
pub struct CharacterPackage {
    pub id: String,
    pub dir: PathBuf,
    /// Immutable constitution: identity, awareness, red lines.
    pub constitution: String,
    /// Selected tone variant id and its text.
    pub tone_variant: String,
    pub tone_text: String,
    /// Display name, e.g. "Arch-Lich Malakor".
    pub name: String,
    pub default_tone: String,
    pub rank_titles: RankTitles,
    /// Foil ids declared in character.toml, in order.
    pub foil_ids: Vec<String>,
    /// Every `foils/*.md`, sorted by id.
    pub foils: Vec<Foil>,
    /// `bible.md` when present.
    pub bible: Option<String>,
    /// `situations.json`, exposed for the simulator (empty when the file is absent).
    pub situations: SituationBank,
    /// Every optional rule (`rules/<id>.md`), sorted by id, with its effective state.
    pub rules: Vec<CharacterRule>,
    /// `stalls.toml`: model-free stall lines and the fleet-loss sign-off (built-in defaults
    /// when the file is absent).
    pub stalls: StallBank,
}

impl CharacterPackage {
    pub fn foil(&self, id: &str) -> Option<&Foil> {
        self.foils.iter().find(|f| f.id == id)
    }

    /// Enabled optional rules, sorted by id.
    pub fn enabled_rules(&self) -> impl Iterator<Item = &CharacterRule> {
        self.rules.iter().filter(|r| r.enabled)
    }
}

#[derive(Debug, Deserialize)]
struct CharacterToml {
    id: String,
    name: String,
    default_tone: String,
    #[serde(default)]
    foils: Vec<String>,
    #[serde(default)]
    ranks: BTreeMap<String, String>,
    /// `[rules]`: optional constitution rules, on or off by default.
    #[serde(default)]
    rules: BTreeMap<String, bool>,
}

fn read(path: &Path) -> Result<String, BrainError> {
    fs::read_to_string(path)
        .map(|s| s.replace("\r\n", "\n"))
        .map_err(|e| BrainError::Package(format!("{}: {e}", path.display())))
}

fn is_safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Load `characters/<id>/` with the given tone variant. An empty `tone_variant` selects the
/// package's `default_tone`. A missing tone file is an error. The operator's rule override is
/// found by the repo convention ([`RulesOverride::Auto`]).
pub fn load_character(dir: &Path, tone_variant: &str) -> Result<CharacterPackage, BrainError> {
    load_character_with(dir, tone_variant, &RulesOverride::Auto)
}

/// [`load_character`] with an explicit source for the operator's rule override.
pub fn load_character_with(
    dir: &Path,
    tone_variant: &str,
    overrides: &RulesOverride,
) -> Result<CharacterPackage, BrainError> {
    let toml_text = read(&dir.join("character.toml"))?;
    let meta: CharacterToml = toml::from_str(&toml_text)
        .map_err(|e| BrainError::Package(format!("character.toml: {e}")))?;
    if !is_safe_id(&meta.id) {
        return Err(BrainError::Package(format!(
            "character id {:?} is not [A-Za-z0-9_-]",
            meta.id
        )));
    }

    let constitution = read(&dir.join("constitution.md"))?;
    if constitution.trim().is_empty() {
        return Err(BrainError::Package("constitution.md is empty".into()));
    }

    let tone = if tone_variant.is_empty() {
        meta.default_tone.clone()
    } else {
        tone_variant.to_string()
    };
    if !is_safe_id(&tone) {
        return Err(BrainError::Package(format!(
            "tone variant {tone:?} is not [A-Za-z0-9_-]"
        )));
    }
    let tone_path = dir.join("tones").join(format!("{tone}.md"));
    if !tone_path.is_file() {
        return Err(BrainError::Package(format!(
            "tone variant {tone:?} not found at {}",
            tone_path.display()
        )));
    }
    let tone_text = read(&tone_path)?;

    let bible_path = dir.join("bible.md");
    let bible = if bible_path.is_file() {
        Some(read(&bible_path)?)
    } else {
        None
    };

    let mut rank_titles = BTreeMap::new();
    for (k, v) in &meta.ranks {
        let rank: Rank = serde_json::from_value(serde_json::Value::String(k.clone()))
            .map_err(|_| BrainError::Package(format!("character.toml: unknown rank {k:?}")))?;
        rank_titles.insert(rank, v.clone());
    }

    let mut foils = Vec::new();
    let foils_dir = dir.join("foils");
    if foils_dir.is_dir() {
        let entries = fs::read_dir(&foils_dir)
            .map_err(|e| BrainError::Package(format!("{}: {e}", foils_dir.display())))?;
        for entry in entries {
            let path = entry
                .map_err(|e| BrainError::Package(format!("foils: {e}")))?
                .path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            foils.push(Foil {
                id: id.to_string(),
                text: read(&path)?,
            });
        }
    }
    foils.sort_by(|a, b| a.id.cmp(&b.id));
    for id in &meta.foils {
        if !foils.iter().any(|f| &f.id == id) {
            return Err(BrainError::Package(format!(
                "foil {id:?} is declared in character.toml but foils/{id}.md is missing"
            )));
        }
    }

    let situations_path = dir.join("situations.json");
    let situations = if situations_path.is_file() {
        serde_json::from_str(&read(&situations_path)?)
            .map_err(|e| BrainError::Package(format!("situations.json: {e}")))?
    } else {
        SituationBank::default()
    };

    let rules = resolve_rules(dir, &meta.id, &meta.rules, overrides)?;
    let stalls = StallBank::load(dir)?;

    Ok(CharacterPackage {
        id: meta.id,
        dir: dir.to_path_buf(),
        constitution,
        tone_variant: tone,
        tone_text,
        name: meta.name,
        default_tone: meta.default_tone,
        rank_titles: RankTitles(rank_titles),
        foil_ids: meta.foils,
        foils,
        bible,
        situations,
        rules,
        stalls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn malakor_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("characters")
            .join("malakor")
    }

    #[test]
    fn loads_malakor() {
        let pkg = load_character(&malakor_dir(), "theatrical").expect("malakor loads");
        assert_eq!(pkg.id, "malakor");
        assert_eq!(pkg.name, "Arch-Lich Malakor");
        assert_eq!(pkg.tone_variant, "theatrical");
        assert!(pkg.constitution.contains("Red lines"));
        assert!(pkg.tone_text.contains("theatrical"));
        assert_eq!(pkg.rank_titles.title(Rank::Lich), "Lesser Lich");
        assert!(pkg
            .foil_ids
            .starts_with(&["the-disruptor".to_string(), "the-life-coach".to_string()]));
        assert!(pkg.foil_ids.iter().all(|id| pkg.foil(id).is_some()));
        assert!(pkg.foil("the-disruptor").is_some());
        assert!(pkg
            .situations
            .situations
            .iter()
            .any(|s| s.id == "prompt-injection"));
    }

    #[test]
    fn malakor_ships_three_rules_enabled_by_default() {
        let pkg = load_character_with(&malakor_dir(), "", &RulesOverride::None).unwrap();
        let ids: Vec<(&str, bool)> = pkg
            .rules
            .iter()
            .map(|r| (r.id.as_str(), r.default))
            .collect();
        assert_eq!(
            ids,
            [
                ("crisis-chat-ban", true),
                ("crisis-handoff", true),
                ("no-financial-promotion", true)
            ]
        );
        assert!(pkg.rules.iter().all(|r| r.enabled));
        assert!(pkg.rules[0].text.contains("removes that viewer from chat"));
        assert!(pkg.rules[1].text.contains("real help"));
        assert!(pkg.rules[1].text.contains("never asks where they are"));
        assert!(pkg.rules[2].text.contains("Phylactery Token"));
    }

    #[test]
    fn the_constitution_forbids_asking_for_personal_details() {
        let pkg = load_character_with(&malakor_dir(), "", &RulesOverride::None).unwrap();
        let flat = pkg
            .constitution
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            flat.contains(
                "4. No doxxing: no addresses, family members or private details of anyone. He \
                 never asks anyone for personal details either (where they live or are, their \
                 real name, age, school, workplace, contacts or socials), not even to help."
            ),
            "{flat}"
        );
    }

    #[test]
    fn empty_tone_uses_default() {
        let pkg = load_character(&malakor_dir(), "").unwrap();
        assert_eq!(pkg.tone_variant, "unhinged-roastmaster");
    }

    #[test]
    fn missing_tone_is_an_error() {
        let err = load_character(&malakor_dir(), "deadpan").unwrap_err();
        assert!(err.to_string().contains("deadpan"));
        assert!(load_character(&malakor_dir(), "../constitution").is_err());
    }
}
