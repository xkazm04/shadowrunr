//! The mechanical checks over a stream prep: a grounding check over the material as written,
//! never another model pass. Every check is always present, `pass` or `fail`, with a detail
//! that names what failed, so the owner sees why a prep cannot be approved.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use shadowrunr_brain::moderation::{gate_text, Policy};
use shadowrunr_types::prep::{PrepCheck, PrepCheckState, StreamPrep};

pub use super::rules::{
    BitRule, BITS, COLD_OPEN_BEATS, CONNECTOR_KINDS, MAX_LINE_CHARS, MIN_CONNECTORS_PER_KIND,
};

/// The operator's list of real people and channels the prep must never name, one per line
/// (`#` comments), beside `platform-rules.json`. Optional and local; never committed.
pub const REAL_NAMES_FILE: &str = "real-names.txt";

/// What the checks read besides the prep.
pub struct CheckContext<'a> {
    /// The term rules the brain's output gate applies.
    pub policy: &'a Policy,
    /// Real people and channels from the operator's list (lowercase).
    pub real_names: &'a [String],
}

/// `<policy>/real-names.txt`, lowercased; empty when absent.
pub fn load_real_names(policy_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(policy_dir.join(REAL_NAMES_FILE))
        .map(|t| parse_real_names(&t))
        .unwrap_or_default()
}

pub fn parse_real_names(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_lowercase)
        .collect()
}

struct Patterns {
    /// Handles, real platforms as channels, and "a real streamer named ...".
    real: Vec<(Regex, &'static str)>,
    url: Regex,
    /// Lines that lean on a stream a newcomer never saw.
    prior: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let re = |p: &str| Regex::new(p).expect("prep check pattern compiles");
        Patterns {
            real: vec![
                (re(r"(?:^|[\s(\[])@[A-Za-z0-9_]{2,}"), "an @handle"),
                (
                    re(r"(?i)\b(?:twitch|youtube|youtuber|tiktok|instagram|twitter|reddit|discord|patreon|onlyfans|facebook|snapchat|kick\.com)\b"),
                    "a real platform or channel",
                ),
                (
                    re(r"(?i)\b(?:real|famous|actual|popular)\s+(?:streamers?|youtubers?|influencers?|celebrit(?:y|ies)|people|persons?)\b"),
                    "a real person",
                ),
                (
                    re(r"(?i)\b(?:streamer|influencer|creator)\s+(?:named|called)\b"),
                    "a named real creator",
                ),
            ],
            url: re(
                r"(?i)\b(?:https?://|www\.)|\b[a-z0-9-]+\.(?:com|net|org|tv|io|gg|co|me|ly|app|xyz|live)\b",
            ),
            prior: re(
                r"(?i)\b(?:last\s+(?:time|stream|night|week|broadcast|episode|session|show)|as\s+i\s+(?:said|told\s+you|mentioned|promised)|remember\s+when|you\s+(?:all\s+)?remember|you\s+will\s+recall|previously\s+on|yesterday|the\s+other\s+(?:day|night)|where\s+we\s+left\s+off|longtime\s+(?:viewers|minions)|regulars\s+know)\b",
            ),
        }
    })
}

/// Every line of the prep with where it sits (`cold_open.beats[2]`, `prem-03 line 1`, ...).
pub fn lines(p: &StreamPrep) -> Vec<(String, &str)> {
    let mut out = vec![("cold_open.title".to_string(), p.cold_open.title.as_str())];
    for (i, b) in p.cold_open.beats.iter().enumerate() {
        out.push((format!("cold_open.beats[{}]", i + 1), b.as_str()));
    }
    out.push((
        "cold_open.handoff".to_string(),
        p.cold_open.handoff.as_str(),
    ));
    for b in &p.bits {
        for (i, l) in b.lines.iter().enumerate() {
            out.push((format!("{} line {}", b.id, i + 1), l.as_str()));
        }
        if let Some(pay) = &b.payoff {
            out.push((format!("{} payoff", b.id), pay.as_str()));
        }
    }
    for c in &p.connectors {
        out.push((c.id.clone(), c.line.as_str()));
    }
    out
}

fn check(id: &str, problems: Vec<String>, pass_detail: String) -> PrepCheck {
    if problems.is_empty() {
        PrepCheck {
            id: id.to_string(),
            state: PrepCheckState::Pass,
            detail: pass_detail,
        }
    } else {
        let n = problems.len();
        let mut detail = problems.into_iter().take(8).collect::<Vec<_>>().join("; ");
        if n > 8 {
            detail.push_str(&format!("; and {} more", n - 8));
        }
        PrepCheck {
            id: id.to_string(),
            state: PrepCheckState::Fail,
            detail,
        }
    }
}

fn count_kind(p: &StreamPrep, rule: &BitRule) -> usize {
    p.bits.iter().filter(|b| b.kind == rule.kind).count()
}

fn wire<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

/// A line with its case, spacing and closing punctuation flattened, for duplicate detection.
fn normalized(line: &str) -> String {
    line.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .trim_end_matches(['.', '!', '?', ',', ';', ':'])
        .to_string()
}

fn names_real(line: &str, real_names: &[String]) -> Option<String> {
    for (re, what) in &patterns().real {
        if let Some(m) = re.find(line) {
            return Some(format!("{what} ({:?})", m.as_str().trim()));
        }
    }
    let lower = line.to_lowercase();
    for name in real_names {
        let hit = lower.match_indices(name.as_str()).any(|(at, _)| {
            let before = lower[..at].chars().next_back();
            let after = lower[at + name.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        });
        if hit {
            return Some("a name on the operator's real-names list".to_string());
        }
    }
    None
}

/// Run every check over `p`. `dropped` lists what generation could not keep (a bit naming no
/// segment, an unknown kind); it fails `shape` or `segments`.
pub fn run(p: &StreamPrep, ctx: &CheckContext, dropped: &Dropped) -> Vec<PrepCheck> {
    let all = lines(p);

    let long: Vec<String> = all
        .iter()
        .filter_map(|(at, l)| {
            let n = l.chars().count();
            (n > MAX_LINE_CHARS).then(|| format!("{at} is {n} characters"))
        })
        .collect();
    let empty: Vec<String> = all
        .iter()
        .filter(|(_, l)| l.trim().is_empty())
        .map(|(at, _)| format!("{at} is empty"))
        .collect();

    let bit_counts: Vec<(&BitRule, usize)> = BITS.iter().map(|r| (r, count_kind(p, r))).collect();
    let beats = p.cold_open.beats.len();
    let per_kind: Vec<(String, usize)> = CONNECTOR_KINDS
        .iter()
        .map(|k| {
            (
                wire(k),
                p.connectors.iter().filter(|c| c.kind == *k).count(),
            )
        })
        .collect();
    let mut counts = Vec::new();
    if !(COLD_OPEN_BEATS.0..=COLD_OPEN_BEATS.1).contains(&beats) {
        counts.push(format!(
            "cold open has {beats} beats ({}-{})",
            COLD_OPEN_BEATS.0, COLD_OPEN_BEATS.1
        ));
    }
    for (r, n) in &bit_counts {
        if !r.count_ok(*n) {
            counts.push(format!("{n} {} ({})", r.plural, r.count_text()));
        }
    }
    for (k, n) in &per_kind {
        if *n < MIN_CONNECTORS_PER_KIND {
            counts.push(format!(
                "{n} {k} connectors (at least {MIN_CONNECTORS_PER_KIND})"
            ));
        }
    }
    let counts_detail = format!(
        "cold open {beats} beats, {}, connectors {}",
        bit_counts
            .iter()
            .map(|(r, n)| format!("{n} {}", r.plural))
            .collect::<Vec<_>>()
            .join(", "),
        per_kind
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut shape = dropped.shape.clone();
    shape.extend(empty);
    for b in &p.bits {
        let r = super::rules::rule(b.kind);
        let n = b.lines.len();
        if !r.lines_ok(n) {
            shape.push(format!(
                "{} is a {} with {n} lines ({})",
                b.id,
                r.singular,
                r.lines_text()
            ));
        }
        if r.payoff_required && b.payoff.as_deref().is_none_or(|x| x.trim().is_empty()) {
            shape.push(format!("{} is a {} without a payoff", b.id, r.singular));
        }
    }

    let real: Vec<String> = all
        .iter()
        .filter_map(|(at, l)| names_real(l, ctx.real_names).map(|w| format!("{at} names {w}")))
        .collect();
    let urls: Vec<String> = all
        .iter()
        .filter_map(|(at, l)| {
            patterns()
                .url
                .find(l)
                .map(|m| format!("{at} has {:?}", m.as_str()))
        })
        .collect();
    let prior: Vec<String> = all
        .iter()
        .filter_map(|(at, l)| {
            patterns()
                .prior
                .find(l)
                .map(|m| format!("{at} leans on an earlier stream ({:?})", m.as_str()))
        })
        .collect();

    let mut segments = dropped.segments.clone();
    for b in &p.bits {
        if shadowrunr_segments::segment_is_descoped(b.segment) {
            segments.push(format!(
                "{} is for {}, which is descoped",
                b.id,
                wire(&b.segment)
            ));
        }
    }

    let mut seen: HashMap<String, &str> = HashMap::new();
    let mut dupes = Vec::new();
    for (at, l) in &all {
        let key = normalized(l);
        if key.is_empty() {
            continue;
        }
        if let Some(first) = seen.get(&key) {
            dupes.push(format!("{at} repeats {first}"));
        } else {
            seen.insert(key, at.as_str());
        }
    }
    let mut ids: HashMap<&str, usize> = HashMap::new();
    for id in std::iter::once(p.cold_open.id.as_str())
        .chain(p.bits.iter().map(|b| b.id.as_str()))
        .chain(p.connectors.iter().map(|c| c.id.as_str()))
    {
        *ids.entry(id).or_insert(0) += 1;
    }
    let mut repeated: Vec<&str> = ids
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(id, _)| *id)
        .collect();
    repeated.sort_unstable();
    dupes.extend(
        repeated
            .into_iter()
            .map(|id| format!("id {id} is used twice")),
    );

    let gated: Vec<String> = all
        .iter()
        .filter_map(|(at, l)| {
            let (_, fired) = gate_text(l, ctx.policy, &[]);
            (!fired.is_empty()).then(|| {
                let rules: Vec<String> = fired.iter().map(|(r, a)| format!("{r} {a}")).collect();
                format!("{at}: {}", rules.join(", "))
            })
        })
        .collect();

    let n = all.len();
    vec![
        check(
            "line_length",
            long,
            format!("{n} lines, each at most {MAX_LINE_CHARS} characters"),
        ),
        check("counts", counts, counts_detail),
        check(
            "shape",
            shape,
            "every bit has the lines its kind needs".into(),
        ),
        check(
            "real_names",
            real,
            "no real person, handle or channel".into(),
        ),
        check("urls", urls, "no links".into()),
        check(
            "prior_streams",
            prior,
            "nothing leans on a stream a newcomer missed".into(),
        ),
        check(
            "segments",
            segments,
            "every bit is for an active segment".into(),
        ),
        check("duplicates", dupes, "no line or id repeats".into()),
        check(
            "output_gate",
            gated,
            format!("{n} lines pass the brain's output gate unchanged"),
        ),
    ]
}

/// What generation dropped from the planner's reply, kept so the checks show it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dropped {
    /// Items with an unknown kind or no usable lines.
    pub shape: Vec<String>,
    /// Bits for a segment that does not exist or is descoped.
    pub segments: Vec<String>,
}

/// The ids of failed checks.
pub fn failed(checks: &[PrepCheck]) -> Vec<&str> {
    checks
        .iter()
        .filter(|c| c.state == PrepCheckState::Fail)
        .map(|c| c.id.as_str())
        .collect()
}
