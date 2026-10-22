//! The character package manifest, `characters/<id>/character.toml` (v2; layout and placeholder
//! syntax in docs/spec/contract.md).
//!
//! Everything that makes one character that character, and that the engine used to hard-code,
//! lives here: the identity words prompts are filled from, the in-character line bank, rank
//! titles and ladder overrides, the lesson-tuned constants with their provenance, and segment
//! flavour. The engine is character-neutral: an absent table or key means the engine default.
//!
//! - [`CharacterManifest`] is the file as written (the JSON Schema `character validate` uses).
//! - [`Persona`] is the resolved identity the engine reads, with [`Persona::fill`] for
//!   placeholder templates (`{name}`, `{followers}`, ...).
//! - [`Tuning`] is the resolved tuning table; [`TUNING_DEFAULTS`] declares every key's engine
//!   default and its provenance.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Rank, SegmentId};

/// `character.toml`, v2. Tables added in v2 (`identity`, `voice`, `avatar`, `lines`,
/// `tuning`, `segments`, and the ladder keys of `ranks`) are optional: a v1 file still loads,
/// with the engine's neutral defaults.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CharacterManifest {
    /// Package id: the directory name, `[A-Za-z0-9_-]`.
    pub id: String,
    /// Display name, e.g. "Arch-Lich Malakor".
    pub name: String,
    /// One line under the name in the operator app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
    /// Tone variant used when none is chosen (`tones/<id>.md`).
    pub default_tone: String,
    /// Summonable SEG-01 foils (`foils/<id>.md`), in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub foils: Vec<String>,
    /// Who the character is in the engine's own text. Absent: a neutral identity built from
    /// `name` ([`Persona::neutral`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<CharacterIdentity>,
    /// The character's voice. Absent (or any field absent): the daemon's `[voice]` config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<CharacterVoice>,
    /// The character's face. Absent (or any field absent): the daemon's config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<CharacterAvatar>,
    /// Rank titles, and overrides of the engine's rank ladder (`segments/data/ranks.toml`).
    #[serde(default, skip_serializing_if = "CharacterRanks::is_empty")]
    pub ranks: CharacterRanks,
    /// Optional constitution rules (`rules/<id>.md`), on or off by default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rules: BTreeMap<String, bool>,
    /// Rails: rules the operator override cannot switch off. Each needs `rules/<id>.md` and
    /// `<id> = true` under `[rules]`; `character.rules.set` refuses it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rails: Vec<String>,
    /// Model-free in-character lines the output gate and the orchestrator speak. Absent lines
    /// use the engine's neutral defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<CharacterLines>,
    /// Lesson-tuned constants, each with its provenance. An absent key is the engine default
    /// ([`TUNING_DEFAULTS`]).
    #[serde(default, skip_serializing_if = "CharacterTuning::is_empty")]
    pub tuning: CharacterTuning,
    /// Per-segment flavour appended to the segment's prompts (placeholders filled).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub segments: BTreeMap<SegmentId, SegmentFlavour>,
}

/// `[identity]`: the words the engine's own text uses for this character. When the table is
/// present, `name`, `patron`, `follower`, `sign_off` and `disclosure` are required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CharacterIdentity {
    /// Short name, e.g. "Malakor" (`{name}`).
    pub name: String,
    /// Title before the name, e.g. "Arch-Lich" (`{title}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Lowercase words chat calls the character by (addressing detection, the name stoplist).
    /// Absent: the name's words of four letters or more.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub address_words: Vec<String>,
    /// Speaker labels a model reply may start a line with, stripped before speech. Absent:
    /// `**Name:**`, `Name:`, `NAME:` and `**Name**:`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reply_prefixes: Vec<String>,
    /// Who the character answers to in the fiction: the owner whose brief the stream carries.
    pub patron: Patron,
    /// What the character calls the viewers (`{follower}`, `{followers}`).
    pub follower: Noun,
    /// What the character calls the research workers it sends out (`{researcher}`,
    /// `{researchers}`). Absent: "researcher", "researchers".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub researcher: Option<Noun>,
    /// The fleet-loss sign-off, spoken when the engine fleet stays lost.
    pub sign_off: String,
    /// The in-character AI disclosure that replaces a persona leak which disclosed being an AI.
    pub disclosure: String,
}

/// The character's patron (`{patron}`, `{patron_title}`). Written without an article: the
/// engine's text supplies "the".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Patron {
    /// e.g. "Lord of the Darkness".
    pub name: String,
    /// The short form, e.g. "Lord" ("the Lord's brief").
    pub title: String,
}

/// A word in the singular and the plural, e.g. `{ one = "minion", many = "minions" }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Noun {
    pub one: String,
    pub many: String,
}

/// `[voice]`: each present field overrides the daemon's `[voice]` for this character.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CharacterVoice {
    /// TTS voice id, e.g. `af_heart`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// The voice's verified speaker id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<u32>,
    /// Speaking-rate multiplier (1.0 is the voice's own rate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    /// The voices file `sid` is read from, by name: a file in the daemon's `[voice] voices_dir`
    /// (`~/.shadowrunr/voices` by default), made on each machine from a committed recipe
    /// (`tools/voice/recipes`), never bundled. Absent: the model's own `voices.bin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voices_file: Option<String>,
    /// The effect chain over each sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fx: Option<VoiceFx>,
}

/// The voice sidecar's effect chain (`tools/voice`, `--fx`): the same length out, so the mouth
/// timeline holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceFx {
    /// The engine's audio as it is.
    #[default]
    None,
    /// Pitch and formants a little down, light grit, a short crypt reverb.
    Light,
    /// Further down, more grit, a doubled chest and a longer crypt.
    Heavy,
}

impl VoiceFx {
    /// The sidecar's `--fx` value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Light => "light",
            Self::Heavy => "heavy",
        }
    }
}

/// `[avatar]`: the character's face; its rig lives at `<data>/characters/<id>/avatar/rig/`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CharacterAvatar {
    /// Rig spec, relative to the package, e.g. `rig/m8.spec.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rig: Option<String>,
    /// The stage's face renderer: `glow`, `cut` or `depth`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
}

/// `[ranks]`: the title the character says for each rank (wire names are fixed), and optional
/// overrides of the engine's ladder. An override replaces only what it names.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CharacterRanks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thrall: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acolyte: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cultist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warlock: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lich: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dread_council: Option<String>,
    /// Vote weight per rank (positive, non-decreasing up the ladder after merging).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weights: BTreeMap<Rank, f64>,
    /// How a rank is reached (the base rank, `thrall`, takes none).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub thresholds: BTreeMap<Rank, RankThreshold>,
    /// Lowest rank allowed to use `!propose`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propose_min_rank: Option<Rank>,
}

impl CharacterRanks {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The title set for `rank`, if any.
    pub fn title(&self, rank: Rank) -> Option<&str> {
        match rank {
            Rank::Thrall => self.thrall.as_deref(),
            Rank::Acolyte => self.acolyte.as_deref(),
            Rank::Cultist => self.cultist.as_deref(),
            Rank::Warlock => self.warlock.as_deref(),
            Rank::Lich => self.lich.as_deref(),
            Rank::DreadCouncil => self.dread_council.as_deref(),
        }
    }
}

/// One rank's qualifying conditions: gifting at least `min_gifted`, or any `tiers` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankThreshold {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_gifted: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<TierThreshold>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TierThreshold {
    /// Current subscription tier: 1, 2 or 3.
    pub tier: u8,
    /// Minimum cumulative months at that tier.
    #[serde(default)]
    pub min_months: u32,
}

/// `[lines]`: model-free in-character lines. Each may use placeholders.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CharacterLines {
    /// Replaces a line a `block` term rule caught.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
    /// Replaces a line that did nothing but ask someone for personal details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_details: Option<String>,
    /// Stand-ins for a line that broke character or leaked the persona (a fresh bit each).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recast: Vec<String>,
    /// Rebuttals of a forged decree; none may name or describe the patron.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rebuttals: Vec<String>,
}

/// `[segments."SEG-01"]`: flavour appended to a segment's prompts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SegmentFlavour {
    /// Appended to the segment's rules prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules_prompt: Option<String>,
    /// State name -> text appended to that state's prompt line.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub states: BTreeMap<String, String>,
}

// ── Placeholders ─────────────────────────────────────────────────────────────

/// Every placeholder a template may use. `{key}` is the value; `{Key}` (first letter upper
/// case) is the value with its first letter upper-cased. Any other `{` is literal text, so JSON
/// examples in prompts need no escaping.
pub const PLACEHOLDERS: &[&str] = &[
    "name",
    "title",
    "full_name",
    "patron",
    "patron_title",
    "follower",
    "followers",
    "researcher",
    "researchers",
];

/// The resolved identity the engine reads: every word a template can name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Persona {
    /// Display name (`{full_name}`), e.g. "Arch-Lich Malakor".
    pub full_name: String,
    /// Short name (`{name}`), e.g. "Malakor".
    pub name: String,
    /// Title (`{title}`), empty when none.
    pub title: String,
    pub address_words: Vec<String>,
    pub reply_prefixes: Vec<String>,
    pub patron: String,
    pub patron_title: String,
    pub follower: String,
    pub followers: String,
    pub researcher: String,
    pub researchers: String,
    pub sign_off: String,
    pub disclosure: String,
}

impl Default for Persona {
    fn default() -> Self {
        Self::neutral("Host")
    }
}

/// The name's words of four letters or more, lowercased (the default address words).
fn name_words(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in name.to_lowercase().split_whitespace() {
        if w.chars().count() >= 4 && !out.iter().any(|o| o == w) {
            out.push(w.to_string());
        }
    }
    out
}

fn default_prefixes(name: &str) -> Vec<String> {
    vec![
        format!("**{name}:**"),
        format!("{name}:"),
        format!("{}:", name.to_uppercase()),
        format!("**{name}**:"),
    ]
}

impl Persona {
    /// The engine's neutral identity for a package without `[identity]`.
    pub fn neutral(full_name: &str) -> Self {
        let name = full_name
            .split_whitespace()
            .last()
            .unwrap_or(full_name)
            .to_string();
        Self {
            full_name: full_name.to_string(),
            address_words: name_words(full_name),
            reply_prefixes: default_prefixes(&name),
            name,
            title: String::new(),
            patron: "producer".into(),
            patron_title: "producer".into(),
            follower: "viewer".into(),
            followers: "viewers".into(),
            researcher: "researcher".into(),
            researchers: "researchers".into(),
            sign_off: "That is all the show can manage tonight. Thank you for watching; I will \
                       be back."
                .into(),
            disclosure: "Yes, I am an AI character, performed by a machine. The show goes on."
                .into(),
        }
    }

    /// The persona of a manifest: its `[identity]`, else [`Persona::neutral`].
    pub fn from_manifest(m: &CharacterManifest) -> Self {
        let Some(id) = &m.identity else {
            return Self::neutral(&m.name);
        };
        let mut address_words: Vec<String> =
            id.address_words.iter().map(|w| w.to_lowercase()).collect();
        if address_words.is_empty() {
            address_words = name_words(&id.name);
        }
        for w in name_words(&m.name) {
            if !address_words.contains(&w) {
                address_words.push(w);
            }
        }
        Self {
            full_name: m.name.clone(),
            name: id.name.clone(),
            title: id.title.clone().unwrap_or_default(),
            address_words,
            reply_prefixes: if id.reply_prefixes.is_empty() {
                default_prefixes(&id.name)
            } else {
                id.reply_prefixes.clone()
            },
            patron: id.patron.name.clone(),
            patron_title: id.patron.title.clone(),
            follower: id.follower.one.clone(),
            followers: id.follower.many.clone(),
            researcher: id
                .researcher
                .as_ref()
                .map_or_else(|| "researcher".into(), |n| n.one.clone()),
            researchers: id
                .researcher
                .as_ref()
                .map_or_else(|| "researchers".into(), |n| n.many.clone()),
            sign_off: id.sign_off.clone(),
            disclosure: id.disclosure.clone(),
        }
    }

    fn value(&self, key: &str) -> Option<&str> {
        Some(match key {
            "name" => &self.name,
            "title" => &self.title,
            "full_name" => &self.full_name,
            "patron" => &self.patron,
            "patron_title" => &self.patron_title,
            "follower" => &self.follower,
            "followers" => &self.followers,
            "researcher" => &self.researcher,
            "researchers" => &self.researchers,
            _ => return None,
        })
    }

    /// `template` with every placeholder replaced. An unknown placeholder stays as written
    /// ([`unknown_placeholders`] finds them at load time).
    pub fn fill(&self, template: &str) -> String {
        let mut out = String::with_capacity(template.len() + 16);
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            match placeholder_at(after) {
                Some(key) => {
                    let lower = key.to_ascii_lowercase();
                    match self.value(&lower) {
                        Some(v) if key.starts_with(|c: char| c.is_ascii_uppercase()) => {
                            let mut cs = v.chars();
                            if let Some(first) = cs.next() {
                                out.extend(first.to_uppercase());
                                out.push_str(cs.as_str());
                            }
                        }
                        Some(v) => out.push_str(v),
                        None => {
                            out.push('{');
                            out.push_str(key);
                            out.push('}');
                        }
                    }
                    rest = &after[key.len() + 1..];
                }
                None => {
                    out.push('{');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// This persona's identifying words: the names, the title, the address words and the
    /// patron (for checks that one character's words stay out of another's text).
    pub fn words(&self) -> Vec<String> {
        let mut w: Vec<String> = vec![
            self.full_name.clone(),
            self.name.clone(),
            self.patron.clone(),
        ];
        if !self.title.is_empty() {
            w.push(self.title.clone());
        }
        w.extend(self.address_words.iter().cloned());
        w
    }
}

/// The placeholder key at the start of `s` (the text after a `{`): `[A-Za-z_]+` then `}`.
fn placeholder_at(s: &str) -> Option<&str> {
    let end = s.find('}')?;
    let key = &s[..end];
    (!key.is_empty() && key.chars().all(|c| c.is_ascii_alphabetic() || c == '_')).then_some(key)
}

/// Placeholders in `template` that are not in [`PLACEHOLDERS`] (in either case form).
pub fn unknown_placeholders(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        if let Some(key) = placeholder_at(after) {
            let known = PLACEHOLDERS.iter().any(|p| {
                *p == key
                    || (key.starts_with(|c: char| c.is_ascii_uppercase())
                        && key.eq_ignore_ascii_case(p))
            });
            if !known && !out.iter().any(|o| o == key) {
                out.push(key.to_string());
            }
            rest = &after[key.len() + 1..];
        } else {
            rest = after;
        }
    }
    out
}

// ── Tuning ───────────────────────────────────────────────────────────────────

/// Where a tuning value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Measured on a run (name it in `source`).
    Measured,
    /// A judgement not yet calibrated.
    Assumption,
}

/// One `[tuning]` key: `{ value = 25000, provenance = "assumption", source = "..." }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TuningEntry {
    /// A positive whole number in the key's unit (`_ms` milliseconds, `_s` seconds, else a
    /// count).
    pub value: u64,
    pub provenance: Provenance,
    /// The run id or lesson the value comes from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// One row of the engine's default tuning table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuningDefault {
    pub key: &'static str,
    pub value: u64,
    pub provenance: Provenance,
    pub source: Option<&'static str>,
    pub doc: &'static str,
}

macro_rules! tuning_table {
    ($( $(#[doc = $doc:literal])+ $key:ident : $ty:ty = $default:literal, $prov:ident, $source:expr; )+) => {
        /// `[tuning]` as written: every key optional.
        #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct CharacterTuning {
            $(
                $(#[doc = $doc])+
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $key: Option<TuningEntry>,
            )+
        }

        impl CharacterTuning {
            pub fn is_empty(&self) -> bool {
                *self == Self::default()
            }

            /// `(key, entry)` for every key the package sets.
            pub fn entries(&self) -> Vec<(&'static str, &TuningEntry)> {
                let mut out = Vec::new();
                $( if let Some(e) = &self.$key { out.push((stringify!($key), e)); } )+
                out
            }
        }

        /// The resolved tuning table: the package's values over the engine defaults.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Tuning {
            $( $(#[doc = $doc])+ pub $key: $ty, )+
        }

        impl Tuning {
            /// The engine defaults.
            pub const DEFAULT: Tuning = Tuning { $( $key: $default, )+ };

            /// The package's values over the defaults. Every value must be positive and fit
            /// its key's type; the error names the key and why.
            pub fn resolve(t: &CharacterTuning) -> Result<Tuning, String> {
                let mut out = Tuning::DEFAULT;
                $(
                    if let Some(e) = &t.$key {
                        if e.value == 0 {
                            return Err(format!("{}: value must be positive", stringify!($key)));
                        }
                        out.$key = <$ty>::try_from(e.value).map_err(|_| {
                            format!("{}: value {} is out of range", stringify!($key), e.value)
                        })?;
                    }
                )+
                Ok(out)
            }
        }

        impl Default for Tuning {
            fn default() -> Self {
                Self::DEFAULT
            }
        }

        /// Every tuning key with its engine default and provenance, in table order.
        pub const TUNING_DEFAULTS: &[TuningDefault] = &[
            $( TuningDefault {
                key: stringify!($key),
                value: $default as u64,
                provenance: Provenance::$prov,
                source: $source,
                doc: concat!($($doc),+),
            }, )+
        ];
    };
}

tuning_table! {
    /// The most chat messages answered in one beat (a ceiling, not a quota).
    max_picks: usize = 2, Measured, Some("sbx-20261021-011413-5m: both three-pick beats lost their last line to the 260-character budget");
    /// How long an unanswered message is carried into the next beats, ms.
    carry_ms: i64 = 25_000, Assumption, Some("two to five beats, so the answer still reads as one");
    /// The most messages carried at once; the oldest go first.
    carry_max: usize = 6, Assumption, None;
    /// A quiet beat may hold only while the host's last line is younger than this, ms.
    hold_gap_ms: i64 = 7_000, Assumption, Some("with 5 s beats the next line speaks under the 15 s say-gap ceiling");
    /// No chat worth answering for this long before a new bit starts, ms.
    quiet_bit_ms: i64 = 8_000, Assumption, None;
    /// Least time between the starts of two new bits, ms.
    bit_spacing_ms: i64 = 20_000, Assumption, None;
    /// While the fleet fails, a stall line once nobody has spoken this long, s.
    stall_every_s: u64 = 12, Assumption, None;
    /// A failed beat's stall is due this much before `stall_every_s` (one beat), s.
    failed_beat_slack_s: u64 = 5, Assumption, None;
    /// A cover line never goes out sooner than this into a slow core call, ms.
    min_cover_ms: u64 = 4_000, Assumption, Some("sandbox smoke run: an 18.4 s call left a 19.1 s silence");
    /// An owned request is answered within this long of reaching a beat, ms.
    request_answer_ms: i64 = 20_000, Assumption, None;
    /// While an owned request waits, a beat holds for speech only past this much queued, ms.
    request_lead_ms: i64 = 8_000, Assumption, None;
    /// An owned request still unanswered this long after it reached a beat is dropped, ms.
    request_max_ms: i64 = 90_000, Assumption, None;
    /// The voice's render lag after a `say`, part of the lookahead lead, ms.
    render_lag_ms: i64 = 2_000, Measured, Some("p50 2.2 s from say to rendered audio in two voiced runs");
    /// The most the lookahead lead may grow to, ms.
    lookahead_max_ms: i64 = 8_000, Assumption, Some("sbx-20261021-143530-10m: core-call silences p50 4.7 s, p90 8.0 s");
    /// A line queued further behind than this is dropped (the voice's 15 s max lag less its render lag), ms.
    lag_ceiling_ms: i64 = 13_000, Assumption, None;
    /// A runtime screen default never overrides a model `show_screen` this recent, ms.
    screen_hold_ms: i64 = 8_000, Assumption, None;
    /// After this long on `browser` the screen moves to `desk`, ms.
    browser_dwell_ms: i64 = 40_000, Assumption, Some("2026-10-21 10-minute show sat on one record for over five minutes");
    /// After this long on `desk` the screen pulls back to the hexmap, ms.
    desk_dwell_ms: i64 = 30_000, Assumption, None;
    /// The closing window: the last this many seconds of a session (wrap-up), s.
    closing_s: u32 = 60, Assumption, None;
    /// The goodbye: the last this many seconds, where the sign-off is asked for, s.
    closing_now_s: u32 = 15, Assumption, Some("sbx-20261021-011413-5m signed off at the first closing line, at 4:04 of 5:00");
    /// After a sign-off the stream holds past the planned end at most this long, s.
    goodbye_drain_max_s: u32 = 15, Assumption, None;
    /// A sign-off counts when it lands in the last this many seconds, s.
    sign_off_window_s: u32 = 30, Assumption, None;
    /// After a stop request, the host has this long to sign off, s.
    stop_sign_off_s: u32 = 10, Assumption, None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn malakor() -> Persona {
        Persona {
            full_name: "Arch-Lich Malakor".into(),
            name: "Malakor".into(),
            title: "Arch-Lich".into(),
            address_words: vec!["malakor".into()],
            reply_prefixes: vec![],
            patron: "Lord of the Darkness".into(),
            patron_title: "Lord".into(),
            follower: "minion".into(),
            followers: "minions".into(),
            researcher: "bone-clerk".into(),
            researchers: "bone-clerks".into(),
            sign_off: String::new(),
            disclosure: String::new(),
        }
    }

    #[test]
    fn fill_replaces_known_placeholders_and_capitalises_on_request() {
        let p = malakor();
        assert_eq!(
            p.fill("{Followers}, the {patron} sends {name} ({title}). {followers}!"),
            "Minions, the Lord of the Darkness sends Malakor (Arch-Lich). minions!"
        );
        assert_eq!(p.fill("The {patron_title}'s brief"), "The Lord's brief");
    }

    #[test]
    fn braces_that_are_not_placeholders_stay_literal() {
        let p = malakor();
        let t = r#"OP: {"op":"mark_moment"} {not a key} {} {unknown_key} {"#;
        assert_eq!(p.fill(t), t);
        assert_eq!(unknown_placeholders(t), vec!["unknown_key".to_string()]);
        assert!(unknown_placeholders("{name} {Name} {FOLLOWERS}").is_empty());
    }

    #[test]
    fn neutral_persona_derives_its_words_from_the_name() {
        let p = Persona::neutral("Keeper Odile Marsh");
        assert_eq!(p.name, "Marsh");
        assert_eq!(p.address_words, vec!["keeper", "odile", "marsh"]);
        assert_eq!(p.reply_prefixes[1], "Marsh:");
    }

    #[test]
    fn tuning_resolves_over_defaults_and_rejects_bad_values() {
        let mut t = CharacterTuning::default();
        assert_eq!(Tuning::resolve(&t).unwrap(), Tuning::DEFAULT);
        t.carry_ms = Some(TuningEntry {
            value: 30_000,
            provenance: Provenance::Measured,
            source: Some("run-1".into()),
        });
        assert_eq!(Tuning::resolve(&t).unwrap().carry_ms, 30_000);
        t.closing_s = Some(TuningEntry {
            value: u64::MAX,
            provenance: Provenance::Assumption,
            source: None,
        });
        assert!(Tuning::resolve(&t).unwrap_err().starts_with("closing_s"));
        t.closing_s = None;
        t.max_picks = Some(TuningEntry {
            value: 0,
            provenance: Provenance::Assumption,
            source: None,
        });
        assert!(Tuning::resolve(&t).unwrap_err().contains("positive"));
        assert_eq!(TUNING_DEFAULTS.len(), 23);
        assert!(TUNING_DEFAULTS
            .iter()
            .all(|d| d.value > 0 && !d.doc.is_empty()));
    }
}
