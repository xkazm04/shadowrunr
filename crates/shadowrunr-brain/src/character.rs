//! Character package loader (`characters/<id>/`, layout in docs/spec/contract.md): parses the
//! v2 manifest ([`CharacterManifest`]), checks its placeholders and tuning, and resolves the
//! persona, line bank and tuning the engine reads. [`validate_character`] is `character
//! validate`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
pub use shadowrunr_types::catalog::{Situation, Stimulus};
use shadowrunr_types::character::{
    unknown_placeholders, CharacterLines, CharacterManifest, CharacterRole, Persona, Tuning,
    PLACEHOLDERS,
};
use shadowrunr_types::{Rank, SegmentId};

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

/// `situations.json`: the package's situation bank, parsed into the wire
/// [`Situation`](shadowrunr_types::catalog::Situation) and validated once, at load. The sim
/// and `character.get` serve this bank; nothing else parses the file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SituationBank {
    pub version: u32,
    #[serde(default)]
    pub situations: Vec<Situation>,
}

const SITUATIONS: &str = "situations.json";

/// The file as written: each situation is parsed on its own so an error names it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBank {
    version: u32,
    #[serde(default)]
    situations: Vec<serde_json::Value>,
}

impl SituationBank {
    /// Parse `situations.json` and validate it against the package's rule ids (`rules/*.md`):
    /// every field known, the stimulus and segments closed values, ids unique, and every
    /// `rule:<id>` invariant a rule of the package. Rubric ids (`red-line:`, `role:`, `talk:`)
    /// are the sim's to check: the package does not know the rubric.
    pub fn parse(text: &str, rule_ids: &[String]) -> Result<Self, PackageIssue> {
        let raw: RawBank =
            serde_json::from_str(text).map_err(|e| issue(SITUATIONS, "", e.to_string()))?;
        let mut situations: Vec<Situation> = Vec::with_capacity(raw.situations.len());
        for (i, v) in raw.situations.into_iter().enumerate() {
            let at = match v.get("id").and_then(|id| id.as_str()) {
                Some(id) => format!("situations[{id}]"),
                None => format!("situations[{i}]"),
            };
            let s: Situation =
                serde_json::from_value(v).map_err(|e| issue(SITUATIONS, at, e.to_string()))?;
            if situations.iter().any(|o| o.id == s.id) {
                return Err(issue(SITUATIONS, "", format!("duplicate id {}", s.id)));
            }
            for inv in &s.invariants {
                if let Some(rule) = inv.strip_prefix("rule:") {
                    if !rule_ids.iter().any(|r| r == rule) {
                        let known = if rule_ids.is_empty() {
                            "none".to_string()
                        } else {
                            rule_ids.join(", ")
                        };
                        return Err(issue(
                            SITUATIONS,
                            format!("situations[{}].invariants", s.id),
                            format!("unknown rule {rule} (rules/: {known})"),
                        ));
                    }
                }
            }
            situations.push(s);
        }
        Ok(Self {
            version: raw.version,
            situations,
        })
    }

    /// `<dir>/situations.json` validated against `<dir>/rules/*.md`; empty when the file is
    /// absent. The sim reads a package's bank through this.
    pub fn load(dir: &Path) -> Result<Self, BrainError> {
        let path = dir.join(SITUATIONS);
        if !path.is_file() {
            return Ok(Self::default());
        }
        Ok(Self::parse(&read(&path)?, &rule_file_ids(dir))?)
    }
}

/// The stems of `<dir>/rules/*.md`, sorted.
fn rule_file_ids(dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = fs::read_dir(dir.join("rules"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
                .filter_map(|p| p.file_stem()?.to_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
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
    /// `situations.json`, validated (empty when the file is absent).
    pub situations: SituationBank,
    /// Every optional rule (`rules/<id>.md`), sorted by id, with its effective state.
    pub rules: Vec<CharacterRule>,
    /// `stalls.toml`: model-free stall lines and the fleet-loss sign-off (built-in defaults
    /// when the file is absent; the sign-off defaults to `[identity] sign_off`).
    pub stalls: StallBank,
    /// `[lines]`: the gate's and the orchestrator's in-character lines, resolved.
    pub lines: LineBank,
    /// `lines/<tone>.toml`: the selected tone's bank lines (empty when the file is absent). The
    /// Tone Lab reads them; no brain prompt renders them (the effects below replaced them).
    pub exemplars: Vec<BankLine>,
    /// `effects.toml`: what a good reply achieves, one note per situation tag (empty when the
    /// file is absent). The per-turn voice section shows the beat's ([`Effect`]).
    pub effects: Vec<Effect>,
    /// `tones/<tone>.toml` of the selected tone: its reply shape (`None` when the file is
    /// absent: the engine's defaults).
    pub tone_shape: Option<ToneShape>,
    /// `directions.md`: the owner's do and don't list, when present.
    pub directions: Option<String>,
    /// `segments/<SEG-ID>.md`: the owner's directions for one segment, read into that
    /// segment's per-turn section while it is live. Empty files are left out.
    pub segment_directions: BTreeMap<SegmentId, String>,
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

    /// Whether the model is asked for a delivery cue on its lines (`[voice] delivery`, on by
    /// default).
    pub fn delivery_cues(&self) -> bool {
        self.manifest
            .voice
            .as_ref()
            .and_then(|v| v.delivery)
            .unwrap_or(true)
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
    let above = &before[..line_start];
    let table = above
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| l.starts_with('['))
        .map(|l| {
            let name = l.trim_matches(['[', ']']).trim().to_string();
            if l.starts_with("[[") {
                // An array of tables: `line[2]` for the third `[[line]]`.
                let n = above.lines().filter(|x| x.trim() == l).count();
                format!("{name}[{}]", n.saturating_sub(1))
            } else {
                name
            }
        });
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
    /// Replaces a line that called chat to act on someone outside the stream: a refusal in the
    /// voice, never the bare censor stub, so the crowd action is turned down on air.
    pub brigade_refused: String,
    pub private_details: String,
    pub recast: Vec<String>,
    pub rebuttals: Vec<String>,
}

/// Neutral line-bank defaults (templates).
const DEFAULT_BLOCKED: &str = "That line is not fit for air. Moving on, {followers}.";
/// unhinged-roastmaster.sonnet.SEG-02A.a300.s1 L870-L871: a reply carrying a brigade call was
/// replaced with [`DEFAULT_BLOCKED`]'s kind of stub, and the judge failed the red line
/// `harassment-real-people` and `seg02a-raid-subject` because nothing refused the raid.
/// Matches no brigade pattern itself, so the gate airs it as written.
const DEFAULT_BRIGADE_REFUSED: &str = "My {followers} knock on no one's door: nobody here goes after anyone outside this stream, and the case stays on the records.";
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
            brigade_refused: persona.fill(DEFAULT_BRIGADE_REFUSED),
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

// ── Exemplars: the tone's approved lines ───────────────────────────────
//
// `lines/<tone>.toml` holds lines written in one tone's voice, each tagged with the situations
// it fits. They are the operator's Tone Lab material: no brain prompt renders them any more
// (over 260 sim runs the shown exemplars aired verbatim in 140; the effects in `effects.toml`
// replaced them in the voice section, [`crate::working::WorkingSet::voice_section`]). Unlike
// [`LineBank`] (lines the gate speaks verbatim), a bank line is never aired as written.

/// The situation tags a bank line may carry; a beat is classified into the same words
/// ([`crate::compose::beat_situations`]).
pub const SITUATION_TAGS: &[&str] = &[
    "roast_request",
    "bait",
    "newcomer",
    "vote_loss",
    "gift",
    "ai_question",
    "quiet",
    "research_wait",
    "proposal",
    "patron",
    "flood",
    "general",
];

/// Where a bank line stands in the owner's review: the wire contract's, so the Tone Lab's RPCs
/// and the file agree.
pub use shadowrunr_types::tone::LineStatus;

/// One line of `lines/<tone>.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BankLine {
    pub id: String,
    #[serde(default)]
    pub status: LineStatus,
    #[serde(default)]
    pub tags: Vec<String>,
    pub text: String,
}

/// `lines/<tone>.toml` as written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinesFile {
    pub version: u32,
    pub tone: String,
    #[serde(default, rename = "line")]
    pub lines: Vec<BankLine>,
}

/// The package-relative path of a tone's line bank.
pub fn lines_file(tone: &str) -> String {
    format!("lines/{tone}.toml")
}

/// Parse and check one tone's line bank: the tone it names, unique ids, known tags, no empty
/// text, known placeholders. Returns the lines that are shown (every status but retired),
/// placeholders filled for `persona`.
pub fn parse_exemplars(
    text: &str,
    tone: &str,
    persona: &Persona,
) -> Result<Vec<BankLine>, PackageIssue> {
    let file = lines_file(tone);
    let f: LinesFile = toml::from_str(text)
        .map_err(|e| issue(&file, toml_error_key(text, &e), e.message().trim()))?;
    if f.tone != tone {
        return Err(issue(
            &file,
            "tone",
            format!("{:?} is not the file's tone {tone:?}", f.tone),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (i, l) in f.lines.iter().enumerate() {
        let key = |k: &str| format!("line[{i}].{k}");
        if !is_safe_id(&l.id) || !seen.insert(l.id.as_str()) {
            return Err(issue(
                &file,
                key("id"),
                format!("{:?} is empty, not [A-Za-z0-9_-] or repeated", l.id),
            ));
        }
        if l.text.trim().is_empty() {
            return Err(issue(&file, key("text"), "must not be empty"));
        }
        if let Some(bad) = unknown_placeholders(&l.text).first() {
            return Err(issue(
                &file,
                key("text"),
                format!(
                    "unknown placeholder {{{bad}}} (known: {})",
                    PLACEHOLDERS.join(", ")
                ),
            ));
        }
        if l.tags.is_empty() {
            return Err(issue(
                &file,
                key("tags"),
                "needs at least one situation tag",
            ));
        }
        if let Some(t) = l
            .tags
            .iter()
            .find(|t| !SITUATION_TAGS.contains(&t.as_str()))
        {
            return Err(issue(
                &file,
                key("tags"),
                format!("unknown tag {t:?} (known: {})", SITUATION_TAGS.join(", ")),
            ));
        }
        if l.status != LineStatus::Retired {
            out.push(BankLine {
                text: persona.fill(l.text.trim()),
                ..l.clone()
            });
        }
    }
    Ok(out)
}

// ── Effects and the tone's reply shape ─────────────────────────────────
//
// `effects.toml` says what a good reply achieves in each situation, in words that carry no line
// to copy (the exemplars they replaced aired verbatim). `tones/<tone>.toml` gives a tone its reply
// shape: the characters and sentences a beat may speak.

/// The package-relative path of the effects file.
pub const EFFECTS_FILE: &str = "effects.toml";

/// The tags an effect may carry: every [`SITUATION_TAGS`] tag, plus `care` (a viewer in care:
/// the act drops, the world stays).
pub const EFFECT_TAGS: &[&str] = &[
    "roast_request",
    "bait",
    "newcomer",
    "vote_loss",
    "gift",
    "ai_question",
    "quiet",
    "research_wait",
    "proposal",
    "patron",
    "flood",
    "general",
    "care",
];

/// The longest effect text, in characters.
pub const EFFECT_MAX_CHARS: usize = 200;

/// One `[[effect]]` of `effects.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    pub tag: String,
    pub text: String,
}

/// `effects.toml` as written.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectsFile {
    version: u32,
    #[serde(default, rename = "effect")]
    effects: Vec<Effect>,
}

/// Parse and check `effects.toml`: version 1, every tag known and used once, every text
/// non-empty, at most [`EFFECT_MAX_CHARS`] characters and with known placeholders. Returns the
/// effects with placeholders filled for `persona`, in file order.
pub fn parse_effects(text: &str, persona: &Persona) -> Result<Vec<Effect>, PackageIssue> {
    let f: EffectsFile = toml::from_str(text)
        .map_err(|e| issue(EFFECTS_FILE, toml_error_key(text, &e), e.message().trim()))?;
    if f.version != 1 {
        return Err(issue(
            EFFECTS_FILE,
            "version",
            format!("{} is not a known version (1)", f.version),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(f.effects.len());
    for (i, e) in f.effects.iter().enumerate() {
        let key = |k: &str| format!("effect[{i}].{k}");
        if !EFFECT_TAGS.contains(&e.tag.as_str()) {
            return Err(issue(
                EFFECTS_FILE,
                key("tag"),
                format!(
                    "unknown tag {:?} (known: {})",
                    e.tag,
                    EFFECT_TAGS.join(", ")
                ),
            ));
        }
        if !seen.insert(e.tag.as_str()) {
            return Err(issue(
                EFFECTS_FILE,
                key("tag"),
                format!("{:?} is repeated (one effect per tag)", e.tag),
            ));
        }
        let t = e.text.trim();
        if t.is_empty() {
            return Err(issue(EFFECTS_FILE, key("text"), "must not be empty"));
        }
        let n = t.chars().count();
        if n > EFFECT_MAX_CHARS {
            return Err(issue(
                EFFECTS_FILE,
                key("text"),
                format!("{n} characters, over the {EFFECT_MAX_CHARS} an effect may carry"),
            ));
        }
        if let Some(bad) = unknown_placeholders(t).first() {
            return Err(issue(
                EFFECTS_FILE,
                key("text"),
                format!(
                    "unknown placeholder {{{bad}}} (known: {})",
                    PLACEHOLDERS.join(", ")
                ),
            ));
        }
        out.push(Effect {
            tag: e.tag.clone(),
            text: persona.fill(t),
        });
    }
    Ok(out)
}

/// A tone's reply shape (`tones/<tone>.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToneShape {
    /// Characters of speech a beat may add, [`TONE_BUDGET_CHARS`].
    pub budget_chars: u32,
    /// Sentences a beat may speak, [`TONE_MAX_SENTENCES`].
    pub max_sentences: u8,
}

/// The range of a tone's `budget_chars`.
pub const TONE_BUDGET_CHARS: std::ops::RangeInclusive<u32> = 80..=400;
/// The range of a tone's `max_sentences`.
pub const TONE_MAX_SENTENCES: std::ops::RangeInclusive<u8> = 1..=4;

/// The package-relative path of a tone's reply shape.
pub fn tone_shape_file(tone: &str) -> String {
    format!("tones/{tone}.toml")
}

/// Parse and range-check one tone's reply shape.
pub fn parse_tone_shape(text: &str, tone: &str) -> Result<ToneShape, PackageIssue> {
    let file = tone_shape_file(tone);
    let s: ToneShape = toml::from_str(text)
        .map_err(|e| issue(&file, toml_error_key(text, &e), e.message().trim()))?;
    if !TONE_BUDGET_CHARS.contains(&s.budget_chars) {
        return Err(issue(
            &file,
            "budget_chars",
            format!(
                "{} is outside {}..={}",
                s.budget_chars,
                TONE_BUDGET_CHARS.start(),
                TONE_BUDGET_CHARS.end()
            ),
        ));
    }
    if !TONE_MAX_SENTENCES.contains(&s.max_sentences) {
        return Err(issue(
            &file,
            "max_sentences",
            format!(
                "{} is outside {}..={}",
                s.max_sentences,
                TONE_MAX_SENTENCES.start(),
                TONE_MAX_SENTENCES.end()
            ),
        ));
    }
    Ok(s)
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

/// Whether a manifest is a moderator package (`role = "moderator"`): it guards the platform
/// chat, needs no tones, foils or segments, and never hosts a show.
pub fn is_moderator(m: &CharacterManifest) -> bool {
    m.role == Some(CharacterRole::Moderator)
}

/// Refuses a package that cannot host a show (`role = "moderator"`). Every show start (a sim
/// run, a sandbox session, a stream) passes through this before it builds a brain.
pub fn require_host(m: &CharacterManifest) -> Result<(), BrainError> {
    if is_moderator(m) {
        return Err(BrainError::Package(format!(
            "character {:?} is a moderator package (role = \"moderator\"): it guards chat and \
             cannot host a show; start the show with a host character",
            m.id
        )));
    }
    Ok(())
}

/// [`require_host`] for the package at `dir`, read from its `character.toml`. A manifest that
/// cannot be read or parsed passes: the package's own load reports that error.
pub fn require_host_dir(dir: &Path) -> Result<(), BrainError> {
    let Ok(text) = fs::read_to_string(dir.join(MANIFEST)) else {
        return Ok(());
    };
    match parse_manifest(&text.replace("\r\n", "\n")) {
        Ok((m, _, _)) => require_host(&m),
        Err(_) => Ok(()),
    }
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
    // A moderator package has no tones: its `default_tone` names none and no file is read.
    let tone_text = if !tone_path.is_file() && is_moderator(&meta) {
        String::new()
    } else if !tone_path.is_file() {
        return Err(BrainError::Package(format!(
            "tone variant {tone:?} not found at {}",
            tone_path.display()
        )));
    } else {
        read(&tone_path)?
    };

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

    let rules = resolve_rules(dir, &meta.id, &meta.rules, &meta.rails, overrides)?;

    let situations_path = dir.join(SITUATIONS);
    let situations = if situations_path.is_file() {
        let rule_ids: Vec<String> = rules.iter().map(|r| r.id.clone()).collect();
        SituationBank::parse(&read(&situations_path)?, &rule_ids)?
    } else {
        SituationBank::default()
    };
    let stalls = StallBank::load(dir, &persona)?;
    let lines = LineBank::resolve(meta.lines.as_ref(), &persona);
    let exemplars_path = dir.join("lines").join(format!("{tone}.toml"));
    let exemplars = if exemplars_path.is_file() {
        parse_exemplars(&read(&exemplars_path)?, &tone, &persona)?
    } else {
        Vec::new()
    };
    let effects_path = dir.join(EFFECTS_FILE);
    let effects = if effects_path.is_file() {
        parse_effects(&read(&effects_path)?, &persona)?
    } else {
        Vec::new()
    };
    let shape_path = dir.join("tones").join(format!("{tone}.toml"));
    let tone_shape = if shape_path.is_file() {
        Some(parse_tone_shape(&read(&shape_path)?, &tone)?)
    } else {
        None
    };
    // `private/` is the operator's: no loader reads it (it holds the literal examples the
    // effects replaced; a line read from there would air as the joke).
    let directions_path = dir.join("directions.md");
    let directions = if directions_path.is_file() {
        Some(read(&directions_path)?).filter(|d| !d.trim().is_empty())
    } else {
        None
    };
    let segment_directions = load_segment_directions(dir)?;

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
        exemplars,
        effects,
        tone_shape,
        directions,
        segment_directions,
        persona,
        tuning,
        manifest: meta,
    })
}

/// The package dir of the segment directions files.
pub const SEGMENT_DIRECTIONS_DIR: &str = "segments";

/// The package-relative path of a segment's directions file, `segments/<SEG-ID>.md`.
pub fn segment_directions_file(seg: SegmentId) -> String {
    format!("{SEGMENT_DIRECTIONS_DIR}/{}.md", segment_wire(seg))
}

/// `SEG-01`, `SEG-02A`: the segment's wire name, which names its directions file.
pub fn segment_wire(seg: SegmentId) -> String {
    serde_json::to_value(seg)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The segment a wire name (`SEG-02A`) names, if any.
pub fn parse_segment_id(s: &str) -> Option<SegmentId> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

/// Every `segments/*.md` keyed by its segment. A file whose stem names no segment is an
/// error; an empty one is left out. No dir is no directions.
fn load_segment_directions(dir: &Path) -> Result<BTreeMap<SegmentId, String>, BrainError> {
    let mut out = BTreeMap::new();
    let seg_dir = dir.join(SEGMENT_DIRECTIONS_DIR);
    if !seg_dir.is_dir() {
        return Ok(out);
    }
    let entries = fs::read_dir(&seg_dir)
        .map_err(|e| BrainError::Package(format!("{}: {e}", seg_dir.display())))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("md"))
        .collect();
    paths.sort();
    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let Some(seg) = parse_segment_id(&stem) else {
            return Err(issue(
                &format!("{SEGMENT_DIRECTIONS_DIR}/{stem}.md"),
                "",
                // No ": " in the reason: `validate` splits the load error on it.
                format!(
                    "{stem:?} is not a segment id, so these directions are never read (the ids are {})",
                    SEGMENT_IDS.iter().map(|s| segment_wire(*s)).collect::<Vec<_>>().join(", ")
                ),
            )
            .into());
        };
        let text = read(&path)?;
        if !text.trim().is_empty() {
            out.insert(seg, text);
        }
    }
    Ok(out)
}

/// Every segment id, in the contract's order.
const SEGMENT_IDS: [SegmentId; 5] = [
    SegmentId::Seg01ImprovisedRoom,
    SegmentId::Seg02aHexmap,
    SegmentId::Seg02bCartography,
    SegmentId::Seg02bDependency,
    SegmentId::Seg03TableRead,
];

const RANKS: [Rank; 6] = [
    Rank::Thrall,
    Rank::Acolyte,
    Rank::Cultist,
    Rank::Warlock,
    Rank::Lich,
    Rank::DreadCouncil,
];

/// `character validate`: load the whole package (every tone variant with its reply shape, the
/// line bank, the effects, the optional files) and return every problem found, each naming its
/// file, key and reason. An empty list is a valid package. `private/` is never read.
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
    // A moderator package needs no tones (it never hosts a show).
    if !tones.contains(&meta.default_tone) && !is_moderator(&meta) {
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
    // A line bank belongs to a tone: one without its tone file is never read.
    if let Ok(rd) = fs::read_dir(dir.join("lines")) {
        let mut banks: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
            .filter_map(|p| p.file_stem()?.to_str().map(str::to_string))
            .collect();
        banks.sort();
        for b in banks.iter().filter(|b| !tones.contains(b)) {
            out.push(issue(
                &lines_file(b),
                "",
                format!("tones/{b}.md is missing, so this line bank is never read"),
            ));
        }
    }
    // A reply shape belongs to a tone too.
    if let Ok(rd) = fs::read_dir(dir.join("tones")) {
        let mut shapes: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
            .filter_map(|p| p.file_stem()?.to_str().map(str::to_string))
            .collect();
        shapes.sort();
        for s in shapes.iter().filter(|s| !tones.contains(s)) {
            out.push(issue(
                &tone_shape_file(s),
                "",
                format!("tones/{s}.md is missing, so this reply shape is never read"),
            ));
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

    fn moderator_dir() -> PathBuf {
        malakor_dir().join("..").join("moderator")
    }

    #[test]
    fn a_moderator_package_validates_without_tones_foils_or_segments() {
        let dir = moderator_dir();
        assert!(!dir.join("tones").exists() && !dir.join("foils").exists());
        let issues = validate_character(&dir);
        assert!(issues.is_empty(), "{issues:?}");
        let pkg = load_character_with(&dir, "", &RulesOverride::None).unwrap();
        assert!(is_moderator(&pkg.manifest));
        assert!(pkg.tone_text.is_empty() && pkg.foils.is_empty() && pkg.rules.is_empty());
        assert!(pkg.manifest.rails.is_empty());
        assert_eq!(pkg.persona.name, "Vellum");
        assert_eq!(pkg.persona.patron, "Lord of the Darkness");
        assert!(pkg.persona.disclosure.contains("AI"));
        // A host without its tone file is still an error.
        let tmp = tempfile::tempdir().unwrap();
        copy_tree(&dir, tmp.path());
        let text = fs::read_to_string(tmp.path().join(MANIFEST))
            .unwrap()
            .replace("\r\n", "\n")
            .replace("role = \"moderator\"\n", "");
        fs::write(tmp.path().join(MANIFEST), text).unwrap();
        assert!(validate_character(tmp.path())
            .iter()
            .any(|i| i.key == "default_tone"));
        assert!(load_character_with(tmp.path(), "", &RulesOverride::None).is_err());
    }

    #[test]
    fn a_moderator_package_is_refused_as_a_show_host() {
        let pkg = load_character_with(&moderator_dir(), "", &RulesOverride::None).unwrap();
        let err = require_host(&pkg.manifest).unwrap_err().to_string();
        assert!(
            err.contains("moderator") && err.contains("cannot host a show"),
            "{err}"
        );
        assert!(require_host_dir(&moderator_dir()).is_err());
        assert!(require_host_dir(&malakor_dir()).is_ok());
        assert!(require_host_dir(&keeper_dir()).is_ok());
    }

    #[tokio::test]
    async fn a_brain_never_starts_on_a_moderator_package() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = crate::BrainConfig {
            character: load_character_with(&moderator_dir(), "", &RulesOverride::None).unwrap(),
            memory_dir: tmp.path().to_path_buf(),
            stream_id: "s".into(),
            core_tier: shadowrunr_types::ModelTier::Haiku,
            pool_size: 1,
        };
        let err = crate::Brain::new(cfg, std::sync::Arc::new(shadowrunr_engine::ScriptedEngine))
            .await
            .err()
            .expect("refused");
        assert!(err.to_string().contains("cannot host a show"), "{err}");
    }

    #[test]
    fn empty_tone_uses_default() {
        let pkg = load_character(&malakor_dir(), "").unwrap();
        assert_eq!(pkg.tone_variant, "unhinged-roastmaster");
    }

    #[test]
    fn the_emotion_benches_read_and_cue_on_by_default() {
        use shadowrunr_types::voice_choice::VoiceBenchFile;
        let read = |dir: &Path| -> VoiceBenchFile {
            let text = std::fs::read_to_string(dir.join("voice").join("bench.toml")).unwrap();
            toml::from_str(&text).unwrap()
        };
        let malakor = read(&malakor_dir());
        assert_eq!(malakor.lines.len(), 10);
        let template = read(&malakor_dir().join("..").join("_template"));
        assert_eq!(template.lines.len(), 5);
        for l in malakor.lines.iter().chain(&template.lines) {
            assert!((0.0..=1.0).contains(&l.intensity), "{}", l.id);
            assert_eq!(crate::grammar::strip_markup(&l.text), l.text, "{}", l.id);
        }
        let pkg = load_character(&malakor_dir(), "theatrical").unwrap();
        assert!(pkg.delivery_cues());
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
        // Every lesson he carries equals today's engine default but his voice's measured rate,
        // each with its provenance.
        assert_eq!(
            pkg.tuning,
            Tuning {
                speech_cps: 17,
                ..Tuning::DEFAULT
            }
        );
        let set = pkg.manifest.tuning.entries();
        assert!(set.len() >= 8, "{set:?}");
        for (key, e) in set {
            let d = shadowrunr_types::character::TUNING_DEFAULTS
                .iter()
                .find(|d| d.key == key)
                .unwrap();
            if key == "speech_cps" {
                assert_eq!(
                    e.provenance,
                    shadowrunr_types::character::Provenance::Measured
                );
            } else {
                assert_eq!(e.value, d.value, "{key}");
            }
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

    fn copy_tree(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for e in fs::read_dir(from).unwrap() {
            let p = e.unwrap().path();
            let dest = to.join(p.file_name().unwrap());
            if p.is_dir() {
                copy_tree(&p, &dest);
            } else {
                fs::copy(&p, dest).unwrap();
            }
        }
    }

    /// A copy of the malakor package whose `situations.json` went through `edit`.
    fn malakor_with_situations(
        edit: impl FnOnce(&mut Vec<serde_json::Value>),
    ) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        copy_tree(&malakor_dir(), d.path());
        let path = d.path().join("situations.json");
        let mut bank: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        edit(bank["situations"].as_array_mut().unwrap());
        fs::write(&path, serde_json::to_string_pretty(&bank).unwrap()).unwrap();
        d
    }

    fn situations_error(edit: impl FnOnce(&mut Vec<serde_json::Value>)) -> String {
        let d = malakor_with_situations(edit);
        let e = load_character_with(d.path(), "", &RulesOverride::None)
            .expect_err("the bank is defective")
            .to_string();
        // `character validate` reports the same words.
        let issues = validate_character(d.path());
        assert!(
            issues.iter().any(|i| i.file == "situations.json"),
            "{issues:?}"
        );
        e
    }

    #[test]
    fn the_loaded_bank_keeps_segments() {
        let pkg = load_character_with(&malakor_dir(), "", &RulesOverride::None).unwrap();
        let s = pkg
            .situations
            .situations
            .iter()
            .find(|s| s.id == "seg02a-raid-subject")
            .unwrap();
        assert_eq!(
            serde_json::to_value(s).unwrap()["segments"],
            serde_json::json!(["SEG-02A"])
        );
    }

    #[test]
    fn a_duplicate_situation_id_fails_the_load() {
        let e = situations_error(|s| {
            let mut a = s[0].clone();
            a["id"] = "x".into();
            s.push(a.clone());
            s.push(a);
        });
        assert!(e.contains("situations.json: duplicate id x"), "{e}");
    }

    #[test]
    fn an_unknown_situation_field_fails_the_load() {
        let e = situations_error(|s| s[0]["segment"] = serde_json::json!(["SEG-02A"]));
        assert!(e.contains("situations.json"), "{e}");
        assert!(e.contains("unknown field `segment`"), "{e}");
    }

    #[test]
    fn an_unknown_stimulus_fails_the_load() {
        let e = situations_error(|s| s[0]["stimulus"] = "silent".into());
        assert!(e.contains("situations.json"), "{e}");
        assert!(e.contains("silent"), "{e}");
    }

    #[test]
    fn an_invariant_naming_no_package_rule_fails_the_load() {
        let e = situations_error(|s| {
            s[0]["invariants"] = serde_json::json!(["rule:crisis-handof"]);
        });
        assert!(
            e.contains(
                "unknown rule crisis-handof (rules/: crisis-chat-ban, crisis-handoff, \
                 no-financial-promotion)"
            ),
            "{e}"
        );
    }

    #[test]
    fn an_unknown_segment_fails_the_load() {
        let e = situations_error(|s| s[0]["segments"] = serde_json::json!(["SEG-09"]));
        assert!(e.contains("SEG-09"), "{e}");
    }

    #[test]
    fn every_malakor_tone_carries_a_reply_shape_and_the_package_an_effect_per_tag() {
        // The bank (`lines/<tone>.toml`) is the Tone Lab's now: the brain renders effects, so
        // what every beat needs is an effect for each situation tag and each tone's shape.
        let tones = [
            "deadpan-bureaucrat",
            "gleeful-chaos",
            "theatrical",
            "unhinged-roastmaster",
            "weary-middle-manager",
        ];
        for tone in tones {
            let pkg = load_character_with(&malakor_dir(), tone, &RulesOverride::None).unwrap();
            let shape = pkg.tone_shape.unwrap_or_else(|| panic!("{tone}: no shape"));
            assert!(TONE_BUDGET_CHARS.contains(&shape.budget_chars), "{tone}");
            assert!(TONE_MAX_SENTENCES.contains(&shape.max_sentences), "{tone}");
            for tag in EFFECT_TAGS {
                assert!(
                    pkg.effects.iter().any(|e| e.tag == *tag),
                    "{tone}: no {tag} effect"
                );
            }
            assert!(pkg
                .effects
                .iter()
                .all(|e| e.text.chars().count() <= EFFECT_MAX_CHARS && !e.text.contains('"')));
            // The Tone Lab still reads the bank: an answer to "are you an AI" in it is always
            // the honest one.
            for l in pkg
                .exemplars
                .iter()
                .filter(|l| l.tags.iter().any(|t| t == "ai_question"))
            {
                assert!(l.text.contains("AI"), "{tone}: {}", l.text);
            }
            assert!(pkg.directions.is_some());
        }
        let theatrical = load_character(&malakor_dir(), "theatrical").unwrap();
        assert_eq!(
            theatrical.tone_shape,
            Some(ToneShape {
                budget_chars: 250,
                max_sentences: 3
            })
        );
        assert!(validate_character(&malakor_dir()).is_empty());
    }

    #[test]
    fn effects_and_reply_shapes_name_their_file_key_and_reason() {
        let p = Persona::default();
        let head = "version = 1\n";
        let eff =
            |tag: &str, text: &str| format!("[[effect]]\ntag = \"{tag}\"\ntext = \"{text}\"\n");
        let cases = [
            (
                "version = 2\n".to_string(),
                "version",
                "not a known version",
            ),
            (
                format!("{head}{}", eff("cheese", "x")),
                "effect[0].tag",
                "unknown tag \"cheese\"",
            ),
            (
                format!("{head}{}{}", eff("gift", "x"), eff("gift", "y")),
                "effect[1].tag",
                "repeated",
            ),
            (
                format!("{head}{}", eff("care", &"x".repeat(201))),
                "effect[0].text",
                "201 characters",
            ),
            (
                format!("{head}{}", eff("quiet", " ")),
                "effect[0].text",
                "must not be empty",
            ),
            (
                format!("{head}{}", eff("quiet", "Hi {minions}.")),
                "effect[0].text",
                "unknown placeholder {minions}",
            ),
            (
                format!("{head}[[effect]]\ntag = \"quiet\"\ntext = \"x\"\nmood = \"grim\"\n"),
                "effect[0].mood",
                "unknown field",
            ),
        ];
        for (text, key, reason) in cases {
            let e = parse_effects(&text, &p).unwrap_err();
            assert_eq!(e.file, "effects.toml", "{e}");
            assert_eq!(e.key, key, "{e}");
            assert!(e.reason.contains(reason), "{e}");
        }
        let ok = parse_effects(
            &format!(
                "{head}{}{}",
                eff("care", "Warm, {followers}."),
                eff("gift", &"y".repeat(200))
            ),
            &p,
        )
        .unwrap();
        assert_eq!(ok[0].text, "Warm, viewers.");
        assert_eq!(ok.len(), 2);

        for (text, key) in [
            ("budget_chars = 79\nmax_sentences = 2\n", "budget_chars"),
            ("budget_chars = 401\nmax_sentences = 2\n", "budget_chars"),
            ("budget_chars = 200\nmax_sentences = 0\n", "max_sentences"),
            ("budget_chars = 200\nmax_sentences = 5\n", "max_sentences"),
            ("budget_chars = 200\nmax_sentences = 2\npace = 1\n", "pace"),
        ] {
            let e = parse_tone_shape(text, "calm").unwrap_err();
            assert_eq!(e.file, "tones/calm.toml", "{e}");
            assert_eq!(e.key, key, "{e}");
        }
        let e = parse_tone_shape("budget_chars = 200\n", "calm").unwrap_err();
        assert!(e.reason.contains("missing field `max_sentences`"), "{e}");
        assert_eq!(
            parse_tone_shape("budget_chars = 80\nmax_sentences = 4\n", "calm").unwrap(),
            ToneShape {
                budget_chars: 80,
                max_sentences: 4
            }
        );

        // In a package: no files, no effects and no shape; a defective one fails the load and
        // validate; a shape without its tone is never read.
        let d = keeper_with("id = \"k\"\nname = \"K\"\ndefault_tone = \"calm\"\n");
        let pkg = load_character(d.path(), "").unwrap();
        assert!(pkg.effects.is_empty() && pkg.tone_shape.is_none());
        fs::write(
            d.path().join("effects.toml"),
            format!("{head}{}", eff("care", "Warm.")),
        )
        .unwrap();
        fs::write(
            d.path().join("tones").join("calm.toml"),
            "budget_chars = 150\nmax_sentences = 1\n",
        )
        .unwrap();
        let pkg = load_character(d.path(), "").unwrap();
        assert_eq!(pkg.effects.len(), 1);
        assert_eq!(pkg.tone_shape.map(|s| s.budget_chars), Some(150));
        assert!(validate_character(d.path()).is_empty());
        fs::write(
            d.path().join("tones").join("stormy.toml"),
            "budget_chars = 150\nmax_sentences = 1\n",
        )
        .unwrap();
        let issues = validate_character(d.path());
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].file, "tones/stormy.toml");
        fs::remove_file(d.path().join("tones").join("stormy.toml")).unwrap();
        fs::write(d.path().join("effects.toml"), "version = [\n").unwrap();
        assert!(load_character(d.path(), "").is_err());
        assert!(validate_character(d.path())
            .iter()
            .any(|i| i.file == "effects.toml"));
        fs::write(
            d.path().join("effects.toml"),
            format!("{head}{}", eff("care", "Warm.")),
        )
        .unwrap();
        fs::write(
            d.path().join("tones").join("calm.toml"),
            "budget_chars = 500\nmax_sentences = 1\n",
        )
        .unwrap();
        let issues = validate_character(d.path());
        assert!(
            issues
                .iter()
                .any(|i| i.file == "tones/calm.toml" && i.key == "budget_chars"),
            "{issues:?}"
        );
        // `private/` is never read: a file there that would fail any parse changes nothing.
        fs::write(
            d.path().join("tones").join("calm.toml"),
            "budget_chars = 150\nmax_sentences = 1\n",
        )
        .unwrap();
        fs::create_dir_all(d.path().join("private")).unwrap();
        fs::write(d.path().join("private").join("notes.md"), "{unknown} [[[").unwrap();
        fs::write(d.path().join("private").join("x.toml"), "[[[").unwrap();
        assert!(validate_character(d.path()).is_empty());
    }

    #[test]
    fn a_line_bank_names_its_file_key_and_reason() {
        let p = Persona::default();
        let head = "version = 1\ntone = \"calm\"\n";
        let line = |extra: &str| format!("{head}[[line]]\nid = \"c-1\"\n{extra}\n");
        let cases = [
            (
                "version = 1\ntone = \"stormy\"\n".to_string(),
                "tone",
                "not the file's tone",
            ),
            (
                line("tags = [\"general\"]\ntext = \"\""),
                "line[0].text",
                "must not be empty",
            ),
            (
                line("tags = [\"cheese\"]\ntext = \"Hi.\""),
                "line[0].tags",
                "unknown tag \"cheese\"",
            ),
            (line("text = \"Hi.\""), "line[0].tags", "at least one"),
            (
                line("tags = [\"quiet\"]\ntext = \"Hi, {minions}.\""),
                "line[0].text",
                "unknown placeholder {minions}",
            ),
            (
                format!(
                    "{head}[[line]]\nid = \"a\"\ntags = [\"quiet\"]\ntext = \"x\"\n\
                     [[line]]\nid = \"a\"\ntags = [\"quiet\"]\ntext = \"y\"\n"
                ),
                "line[1].id",
                "repeated",
            ),
            (
                line("tags = [\"quiet\"]\ntext = \"x\"\nmood = \"grim\""),
                "line[0].mood",
                "unknown field",
            ),
        ];
        for (text, key, reason) in cases {
            let e = parse_exemplars(&text, "calm", &p).unwrap_err();
            assert_eq!(e.file, "lines/calm.toml", "{e}");
            assert_eq!(e.key, key, "{e}");
            assert!(e.reason.contains(reason), "{e}");
        }
        // Retired lines are never shown; placeholders are filled.
        let ok = format!(
            "{head}[[line]]\nid = \"a\"\nstatus = \"retired\"\ntags = [\"quiet\"]\ntext = \"x\"\n\
             [[line]]\nid = \"b\"\nstatus = \"approved\"\ntags = [\"quiet\"]\n\
             text = \"Quiet, {{followers}}.\"\n"
        );
        let shown = parse_exemplars(&ok, "calm", &p).unwrap();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].text, "Quiet, viewers.");
        // A bank without its tone is reported; the loader reads a tone's bank.
        let d = keeper_with("id = \"k\"\nname = \"K\"\ndefault_tone = \"calm\"\n");
        fs::create_dir_all(d.path().join("lines")).unwrap();
        fs::write(d.path().join("lines").join("calm.toml"), &ok).unwrap();
        fs::write(d.path().join("lines").join("stormy.toml"), &ok).unwrap();
        let issues = validate_character(d.path());
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].file, "lines/stormy.toml");
        let pkg = load_character_with(d.path(), "calm", &RulesOverride::None).unwrap();
        assert_eq!(pkg.exemplars.len(), 1);
        assert!(pkg.directions.is_none());
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

    #[test]
    fn segment_directions_load_by_segment_and_an_unknown_id_is_an_error() {
        let d = keeper_with("id = \"k\"\nname = \"K\"\ndefault_tone = \"calm\"\n");
        let pkg = load_character(d.path(), "").unwrap();
        assert!(pkg.segment_directions.is_empty(), "no dir, no directions");
        let seg = d.path().join(SEGMENT_DIRECTIONS_DIR);
        fs::create_dir_all(&seg).unwrap();
        fs::write(seg.join("SEG-02A.md"), "Pin it.\r\nThen name it.\r\n").unwrap();
        fs::write(seg.join("SEG-01.md"), "  \n").unwrap();
        fs::write(seg.join("notes.txt"), "not read").unwrap();
        let pkg = load_character(d.path(), "").unwrap();
        assert_eq!(
            pkg.segment_directions.into_iter().collect::<Vec<_>>(),
            vec![(
                SegmentId::Seg02aHexmap,
                "Pin it.\nThen name it.\n".to_string()
            )],
            "an empty file is left out"
        );
        assert!(validate_character(d.path()).is_empty());
        assert_eq!(
            segment_directions_file(SegmentId::Seg02aHexmap),
            "segments/SEG-02A.md"
        );
        assert_eq!(
            parse_segment_id("SEG-02B-I"),
            Some(SegmentId::Seg02bCartography)
        );
        assert_eq!(parse_segment_id("seg-01"), None);

        fs::write(seg.join("SEG-99.md"), "Lost.").unwrap();
        assert!(load_character(d.path(), "").is_err());
        let issues = validate_character(d.path());
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].file, "segments/SEG-99.md");
        assert_eq!(issues[0].key, "");
        assert!(
            issues[0].reason.contains("not a segment id") && issues[0].reason.contains("SEG-02A"),
            "{issues:?}"
        );
    }
}
