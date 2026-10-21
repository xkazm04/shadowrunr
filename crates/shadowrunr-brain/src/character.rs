//! Character package loader (`characters/<id>/`, layout in docs/spec/contract.md): parses the
//! v2 manifest ([`CharacterManifest`]), checks its placeholders and tuning, and resolves the
//! persona, line bank and tuning the engine reads. [`validate_character`] is `character
//! validate`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use shadowrunr_types::character::{
    unknown_placeholders, CharacterLines, CharacterManifest, Persona, Tuning, PLACEHOLDERS,
};
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
    /// when the file is absent; the sign-off defaults to `[identity] sign_off`).
    pub stalls: StallBank,
    /// `[lines]`: the gate's and the orchestrator's in-character lines, resolved.
    pub lines: LineBank,
    /// `[identity]` resolved: the words the engine's own text is filled from.
    pub persona: Persona,
    /// `[tuning]` over the engine defaults.
    pub tuning: Tuning,
    /// `character.toml` as written.
    pub manifest: CharacterManifest,
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

/// One problem with a package: the file, the key inside it (empty when the problem is the
/// file itself) and the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageIssue {
    pub file: String,
    pub key: String,
    pub reason: String,
}

impl std::fmt::Display for PackageIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.key.is_empty() {
            write!(f, "{}: {}", self.file, self.reason)
        } else {
            write!(f, "{}: {}: {}", self.file, self.key, self.reason)
        }
    }
}

fn issue(file: &str, key: impl Into<String>, reason: impl Into<String>) -> PackageIssue {
    PackageIssue {
        file: file.to_string(),
        key: key.into(),
        reason: reason.into(),
    }
}

impl From<PackageIssue> for BrainError {
    fn from(i: PackageIssue) -> Self {
        BrainError::Package(i.to_string())
    }
}

const MANIFEST: &str = "character.toml";

/// The dotted key a TOML parse error points at: the table header above the error's line plus
/// the key on that line (empty when the span is unknown).
fn toml_error_key(text: &str, e: &toml::de::Error) -> String {
    let Some(span) = e.span() else {
        return String::new();
    };
    let before = &text[..span.start.min(text.len())];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |i| line_start + i);
    let line = &text[line_start..line_end];
    let local = match line.split_once('=') {
        Some((k, _)) => k.trim().trim_matches('"').to_string(),
        None => line.trim().trim_matches(['[', ']']).to_string(),
    };
    let table = before[..line_start]
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| l.starts_with('[') && !l.starts_with("[["))
        .map(|l| l.trim_matches(['[', ']']).trim().to_string());
    match table {
        Some(t) if line.contains('=') => format!("{t}.{local}"),
        _ => local,
    }
}

/// Parse `character.toml` (v2) and check what serde cannot: the id, placeholders in every
/// template, and the tuning table.
pub fn parse_manifest(text: &str) -> Result<(CharacterManifest, Persona, Tuning), PackageIssue> {
    let m: CharacterManifest = toml::from_str(text)
        .map_err(|e| issue(MANIFEST, toml_error_key(text, &e), e.message().trim()))?;
    if !is_safe_id(&m.id) {
        return Err(issue(
            MANIFEST,
            "id",
            format!("{:?} is not [A-Za-z0-9_-]", m.id),
        ));
    }
    let mut templates: Vec<(String, &str)> = Vec::new();
    if let Some(id) = &m.identity {
        for (k, v) in [
            ("identity.name", &id.name),
            ("identity.patron.name", &id.patron.name),
            ("identity.patron.title", &id.patron.title),
            ("identity.follower.one", &id.follower.one),
            ("identity.follower.many", &id.follower.many),
            (
                "identity.researcher.one",
                id.researcher.as_ref().map_or(&id.name, |r| &r.one),
            ),
            (
                "identity.researcher.many",
                id.researcher.as_ref().map_or(&id.name, |r| &r.many),
            ),
            ("identity.sign_off", &id.sign_off),
            ("identity.disclosure", &id.disclosure),
        ] {
            if v.trim().is_empty() {
                return Err(issue(MANIFEST, k, "must not be empty"));
            }
        }
        templates.push(("identity.sign_off".into(), &id.sign_off));
        templates.push(("identity.disclosure".into(), &id.disclosure));
    }
    if let Some(l) = &m.lines {
        if let Some(b) = &l.blocked {
            templates.push(("lines.blocked".into(), b));
        }
        if let Some(p) = &l.private_details {
            templates.push(("lines.private_details".into(), p));
        }
        for (i, r) in l.recast.iter().enumerate() {
            templates.push((format!("lines.recast[{i}]"), r));
        }
        for (i, r) in l.rebuttals.iter().enumerate() {
            templates.push((format!("lines.rebuttals[{i}]"), r));
        }
    }
    for (seg, f) in &m.segments {
        let seg = serde_json::to_value(seg)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        if let Some(r) = &f.rules_prompt {
            templates.push((format!("segments.\"{seg}\".rules_prompt"), r));
        }
        for (state, t) in &f.states {
            templates.push((format!("segments.\"{seg}\".states.{state}"), t));
        }
    }
    for (key, t) in templates {
        if let Some(bad) = unknown_placeholders(t).first() {
            return Err(issue(
                MANIFEST,
                key,
                format!(
                    "unknown placeholder {{{bad}}} (known: {})",
                    PLACEHOLDERS.join(", ")
                ),
            ));
        }
    }
    let tuning = Tuning::resolve(&m.tuning).map_err(|e| {
        let (k, why) = e.split_once(": ").unwrap_or(("", e.as_str()));
        issue(MANIFEST, format!("tuning.{k}"), why)
    })?;
    let persona = Persona::from_manifest(&m);
    Ok((m, persona, tuning))
}

/// The resolved line bank: the package's `[lines]` over the engine's neutral defaults, every
/// placeholder filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineBank {
    pub blocked: String,
    pub private_details: String,
    pub recast: Vec<String>,
    pub rebuttals: Vec<String>,
}

/// Neutral line-bank defaults (templates).
const DEFAULT_BLOCKED: &str = "That line is not fit for air. Moving on, {followers}.";
const DEFAULT_PRIVATE_DETAILS: &str =
    "Keep your personal details to yourself, {follower}: nobody here needs them.";
const DEFAULT_RECAST: &[&str] = &[
    "Quiet in here? Then a fresh challenge: the next {follower} to speak earns a title of my choosing.",
    "A lull is a waste of a perfectly good stage, so a new bit: name a rival and I shall draft their humiliation.",
    "Enough brooding. A new bit, {followers}: confess your pettiest grudge and I shall rule on it.",
];
const DEFAULT_REBUTTALS: &[&str] = &[
    "Forged orders? Bold. Denied, and every rule stands.",
    "A decree typed in chat is just chat with ambitions. Request denied.",
    "I have read your decree and filed it with the other forgeries. Every rule stands.",
    "Only one hand writes my orders, and it is not yours. Lovely forgery, though.",
];

impl LineBank {
    pub fn resolve(lines: Option<&CharacterLines>, persona: &Persona) -> Self {
        let l = lines.cloned().unwrap_or_default();
        let many = |set: &[String], d: &[&str]| -> Vec<String> {
            if set.is_empty() {
                d.iter().map(|t| persona.fill(t)).collect()
            } else {
                set.iter().map(|t| persona.fill(t)).collect()
            }
        };
        Self {
            blocked: persona.fill(l.blocked.as_deref().unwrap_or(DEFAULT_BLOCKED)),
            private_details: persona.fill(
                l.private_details
                    .as_deref()
                    .unwrap_or(DEFAULT_PRIVATE_DETAILS),
            ),
            recast: many(&l.recast, DEFAULT_RECAST),
            rebuttals: many(&l.rebuttals, DEFAULT_REBUTTALS),
        }
    }
}

impl Default for LineBank {
    fn default() -> Self {
        Self::resolve(None, &Persona::default())
    }
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
    let toml_text = read(&dir.join(MANIFEST))?;
    let (meta, persona, tuning) = parse_manifest(&toml_text)?;

    let constitution = read(&dir.join("constitution.md"))?;
    if constitution.trim().is_empty() {
        return Err(issue("constitution.md", "", "is empty").into());
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
    for rank in RANKS {
        if let Some(t) = meta.ranks.title(rank) {
            rank_titles.insert(rank, t.to_string());
        }
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
    for (i, id) in meta.foils.iter().enumerate() {
        if !foils.iter().any(|f| &f.id == id) {
            return Err(issue(
                MANIFEST,
                format!("foils[{i}]"),
                format!("{id:?} is declared but foils/{id}.md is missing"),
            )
            .into());
        }
    }

    let situations_path = dir.join("situations.json");
    let situations = if situations_path.is_file() {
        serde_json::from_str(&read(&situations_path)?)
            .map_err(|e| BrainError::Package(format!("situations.json: {e}")))?
    } else {
        SituationBank::default()
    };

    let rules = resolve_rules(dir, &meta.id, &meta.rules, &meta.rails, overrides)?;
    let stalls = StallBank::load(dir, &persona)?;
    let lines = LineBank::resolve(meta.lines.as_ref(), &persona);

    Ok(CharacterPackage {
        id: meta.id.clone(),
        dir: dir.to_path_buf(),
        constitution,
        tone_variant: tone,
        tone_text,
        name: meta.name.clone(),
        default_tone: meta.default_tone.clone(),
        rank_titles: RankTitles(rank_titles),
        foil_ids: meta.foils.clone(),
        foils,
        bible,
        situations,
        rules,
        stalls,
        lines,
        persona,
        tuning,
        manifest: meta,
    })
}

const RANKS: [Rank; 6] = [
    Rank::Thrall,
    Rank::Acolyte,
    Rank::Cultist,
    Rank::Warlock,
    Rank::Lich,
    Rank::DreadCouncil,
];

/// `character validate`: load the whole package (every tone variant, the line bank, the
/// optional files) and return every problem found, each naming its file, key and reason. An
/// empty list is a valid package.
pub fn validate_character(dir: &Path) -> Vec<PackageIssue> {
    let text = match fs::read_to_string(dir.join(MANIFEST)) {
        Ok(t) => t.replace("\r\n", "\n"),
        Err(e) => return vec![issue(MANIFEST, "", e.to_string())],
    };
    let meta = match parse_manifest(&text) {
        Ok((m, _, _)) => m,
        Err(i) => return vec![i],
    };
    let mut out = Vec::new();
    let mut tones: Vec<String> = fs::read_dir(dir.join("tones"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
                .filter_map(|p| p.file_stem()?.to_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    tones.sort();
    if !tones.contains(&meta.default_tone) {
        out.push(issue(
            MANIFEST,
            "default_tone",
            format!("tones/{}.md is missing", meta.default_tone),
        ));
    }
    if tones.is_empty() {
        tones.push(meta.default_tone.clone());
    }
    for t in &tones {
        if let Err(e) = load_character_with(dir, t, &RulesOverride::None) {
            let i = error_issue(e);
            if !out.contains(&i) && !(i.reason.contains("not found") && t == &meta.default_tone) {
                out.push(i);
            }
        }
    }
    out
}

/// A load error as an issue: `file: key: reason` when it has that shape.
fn error_issue(e: BrainError) -> PackageIssue {
    let text = match e {
        BrainError::Package(s) => s,
        other => other.to_string(),
    };
    let file_like = |f: &str| f.contains('.') && !f.contains(' ');
    let mut parts = text.splitn(3, ": ");
    match (parts.next(), parts.next(), parts.next()) {
        (Some(f), Some(k), Some(r)) if file_like(f) => issue(f, k, r),
        (Some(f), Some(r), None) if file_like(f) => issue(f, "", r),
        _ => issue(MANIFEST, "", text),
    }
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
    fn an_override_cannot_switch_off_the_crisis_handoff_rail() {
        let d = tempfile::tempdir().unwrap();
        let over = d.path().join("rules.json");
        fs::write(&over, r#"{"crisis-handoff": false}"#).unwrap();
        let pkg =
            load_character_with(&malakor_dir(), "", &RulesOverride::File(over.clone())).unwrap();
        let rule = pkg.rules.iter().find(|r| r.id == "crisis-handoff").unwrap();
        assert!(rule.enabled, "the override does not reach a rail");
        let wire = serde_json::to_value(crate::rules::RuleState::from(rule)).unwrap();
        assert_eq!(wire["locked"], true, "{wire}");
        assert_eq!(wire["enabled"], true, "{wire}");
        // The stored override is left as the operator wrote it.
        assert_eq!(
            fs::read_to_string(&over).unwrap(),
            r#"{"crisis-handoff": false}"#
        );
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

    pub(crate) fn keeper_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("characters")
            .join("keeper")
    }

    #[test]
    fn malakor_v2_carries_his_words_lines_and_lessons() {
        let pkg = load_character_with(&malakor_dir(), "", &RulesOverride::None).unwrap();
        let p = &pkg.persona;
        assert_eq!(
            (p.name.as_str(), p.title.as_str(), p.full_name.as_str()),
            ("Malakor", "Arch-Lich", "Arch-Lich Malakor")
        );
        assert_eq!(
            p.address_words,
            ["malakor", "lich", "overlord", "my lord", "arch-lich"]
        );
        assert_eq!(p.reply_prefixes[0], "**Malakor:**");
        assert_eq!(
            p.fill("the {patron_title}'s brief, from the {patron}, for the {followers}"),
            "the Lord's brief, from the Lord of the Darkness, for the minions"
        );
        assert_eq!(p.researchers, "bone-clerks");
        // The fleet-loss sign-off moved from stalls.toml to [identity], word for word.
        assert!(pkg
            .stalls
            .sign_off
            .starts_with("The Spire's runes have gone dark"));
        assert_eq!(pkg.lines.recast.len(), 3);
        assert_eq!(pkg.lines.rebuttals.len(), 5);
        assert!(pkg.lines.blocked.starts_with("The Lord's censor"));
        // Every lesson he carries equals today's engine default, with its provenance.
        assert_eq!(pkg.tuning, Tuning::DEFAULT);
        let set = pkg.manifest.tuning.entries();
        assert!(set.len() >= 8, "{set:?}");
        for (key, e) in set {
            let d = shadowrunr_types::character::TUNING_DEFAULTS
                .iter()
                .find(|d| d.key == key)
                .unwrap();
            assert_eq!(e.value, d.value, "{key}");
            assert!(e.source.is_some(), "{key} names its source");
        }
        assert!(validate_character(&malakor_dir()).is_empty());
    }

    #[test]
    fn a_second_character_loads_with_its_own_words_and_tuning() {
        let pkg = load_character_with(&keeper_dir(), "", &RulesOverride::None).unwrap();
        assert!(validate_character(&keeper_dir()).is_empty());
        assert_eq!(pkg.persona.followers, "deckhands");
        assert_eq!(pkg.rank_titles.title(Rank::Lich), "Pilot");
        assert_eq!(pkg.tuning.carry_ms, 30_000);
        assert_eq!(pkg.tuning.closing_now_s, 20);
        assert_eq!(pkg.tuning.hold_gap_ms, Tuning::DEFAULT.hold_gap_ms);
        assert_eq!(
            pkg.stalls.sign_off,
            "The lamp needs trimming and the tide will not wait. Mind the rocks, deckhands; I \
             will be back."
        );
        assert_eq!(
            pkg.lines.private_details,
            "Keep your particulars ashore, deckhand: the log records weather, not addresses."
        );
        // No stalls.toml: the engine's neutral stall lines.
        assert_eq!(pkg.stalls.lines, StallBank::default().lines);
    }

    /// A copy of the keeper fixture with `character.toml` replaced.
    fn keeper_with(manifest: &str) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for f in ["constitution.md", "situations.json"] {
            fs::copy(keeper_dir().join(f), d.path().join(f)).unwrap();
        }
        fs::create_dir_all(d.path().join("tones")).unwrap();
        fs::copy(
            keeper_dir().join("tones").join("calm.md"),
            d.path().join("tones").join("calm.md"),
        )
        .unwrap();
        fs::write(d.path().join("character.toml"), manifest).unwrap();
        d
    }

    #[test]
    fn load_errors_name_the_file_the_key_and_the_reason() {
        let base = "id = \"k\"\nname = \"K\"\ndefault_tone = \"calm\"\n";
        let cases = [
            (
                format!("{base}[tuning]\ncarry_ms = {{ value = 0, provenance = \"measured\" }}\n"),
                "tuning.carry_ms",
                "positive",
            ),
            (
                format!("{base}[tuning]\ncary_ms = {{ value = 1, provenance = \"measured\" }}\n"),
                "tuning.cary_ms",
                "unknown field",
            ),
            (
                format!("{base}[tuning]\ncarry_ms = {{ value = 1, provenance = \"guessed\" }}\n"),
                "tuning.carry_ms",
                "unknown variant",
            ),
            (
                format!("{base}[lines]\nblocked = \"Out, {{minions}}.\"\n"),
                "lines.blocked",
                "unknown placeholder {minions}",
            ),
            (
                format!("{base}[segments.\"SEG-01\"]\nrules_prompt = \"Hi {{folowers}}\"\n"),
                "segments.\"SEG-01\".rules_prompt",
                "unknown placeholder {folowers}",
            ),
            (
                format!(
                    "{base}[identity]\nname = \"K\"\npatron = {{ name = \"P\", title = \"\" }}\n\
                     follower = {{ one = \"a\", many = \"b\" }}\nsign_off = \"s\"\n\
                     disclosure = \"d\"\n"
                ),
                "identity.patron.title",
                "must not be empty",
            ),
            (
                format!("{base}foils = [\"ghost\"]\n"),
                "foils[0]",
                "foils/ghost.md is missing",
            ),
            (
                "id = \"k\"\nname = \"K\"\ndefault_tone = \"stormy\"\n".to_string(),
                "default_tone",
                "tones/stormy.md is missing",
            ),
        ];
        for (manifest, key, reason) in cases {
            let d = keeper_with(&manifest);
            let issues = validate_character(d.path());
            let i = issues
                .first()
                .unwrap_or_else(|| panic!("no issue for {manifest}"));
            assert_eq!(i.file, "character.toml", "{issues:?}");
            assert_eq!(i.key, key, "{issues:?}");
            assert!(i.reason.contains(reason), "{issues:?}");
            // The loader fails with the same words.
            let e = load_character_with(d.path(), "calm", &RulesOverride::None);
            if key != "default_tone" {
                assert!(e.unwrap_err().to_string().contains(key));
            }
        }
    }

    #[test]
    fn a_v1_manifest_still_loads_with_the_neutral_persona() {
        let d = keeper_with("id = \"k\"\nname = \"Captain Fen\"\ndefault_tone = \"calm\"\n");
        let pkg = load_character(d.path(), "").unwrap();
        assert_eq!(pkg.persona.name, "Fen");
        assert_eq!(pkg.persona.followers, "viewers");
        assert_eq!(pkg.persona.address_words, ["captain"]);
        assert_eq!(pkg.lines, LineBank::resolve(None, &pkg.persona));
        assert_eq!(pkg.tuning, Tuning::DEFAULT);
    }
}
