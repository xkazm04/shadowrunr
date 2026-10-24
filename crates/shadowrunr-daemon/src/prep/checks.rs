//! The mechanical checks over a stream prep: a grounding check over the material as written,
//! never another model pass. Every check is always present, `pass` or `fail`, with a detail
//! that names what failed, so the owner sees why a prep cannot be approved. The one exception
//! is `table_read`: present exactly when the prep plans a SEG-03 Table Read (there is nothing
//! to check without one, and a prep without one keeps the checks it always had).

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
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
    /// Where the `table_read` check reads the scenes and the prep character's foils. `None`:
    /// a prep with a table read fails that check (a prep without one passes it either way).
    pub scenes: Option<ScenePaths<'a>>,
}

/// `<data>` (its `scenes/<guest>/`) and the character packages (the prep character's foils).
#[derive(Debug, Clone, Copy)]
pub struct ScenePaths<'a> {
    pub data: &'a Path,
    pub characters: &'a Path,
}

/// The `table_read` check's problems (the check runs only with a table read).
fn table_read(p: &StreamPrep, ctx: &CheckContext) -> Vec<Problem> {
    let Some(plan) = &p.table_read else {
        return Vec::new();
    };
    let Some(paths) = ctx.scenes else {
        return vec![Problem::bare(
            "the scenes cannot be read where these checks run".into(),
        )];
    };
    let foils = crate::character::manifest(paths.characters, &p.character)
        .map(|m| m.foils)
        .unwrap_or_default();
    crate::scenes::table_read_problems(plan, &p.character, paths.data, &foils, ctx.policy)
        .into_iter()
        .map(Problem::bare)
        .collect()
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

fn check(id: &str, problems: &[Problem], pass_detail: String) -> PrepCheck {
    if problems.is_empty() {
        PrepCheck {
            id: id.to_string(),
            state: PrepCheckState::Pass,
            detail: pass_detail,
        }
    } else {
        let n = problems.len();
        let mut detail = problems
            .iter()
            .take(8)
            .map(|x| x.msg.as_str())
            .collect::<Vec<_>>()
            .join("; ");
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

/// Room a count shortfall leaves: `missing` more items of `kind` (a connector kind's wire
/// name, a bit kind's singular, or `beat`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub kind: String,
    pub missing: usize,
}

/// One thing a check caught, located: `at` is the edit target (`conn-01`, `prem-03`,
/// `cold_open.beats[2]`) or, for a count shortfall, the insert slot
/// (`insert:connector:into_poll`, `insert:premise`, `insert:cold_open.beat`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub check: String,
    pub at: String,
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<Slot>,
}

/// One problem as a check reports it (`msg`), with the targets it names and, for a shortfall,
/// the slot it leaves. [`run`] prints the messages; [`findings`] keeps the targets.
struct Problem {
    msg: String,
    targets: Vec<String>,
    slot: Option<(String, Slot)>,
}

impl Problem {
    /// A problem on one line: the target is the line's item (`prem-03 line 1` -> `prem-03`).
    fn at(locator: &str, msg: String) -> Self {
        Self {
            msg,
            targets: vec![target_of(locator)],
            slot: None,
        }
    }

    /// A problem in nothing the material holds (an item generation dropped).
    fn bare(msg: String) -> Self {
        Self {
            msg,
            targets: Vec::new(),
            slot: None,
        }
    }

    fn slot(msg: String, at: String, kind: &str, missing: usize) -> Self {
        Self {
            msg,
            targets: Vec::new(),
            slot: Some((
                at,
                Slot {
                    kind: kind.to_string(),
                    missing,
                },
            )),
        }
    }
}

/// The edit target a line locator sits in: `prem-03 line 1` and `prem-03 payoff` are
/// `prem-03`; `conn-01` and `cold_open.beats[2]` are themselves.
pub fn target_of(locator: &str) -> String {
    locator.split(' ').next().unwrap_or(locator).to_string()
}

/// Every edit target `p` holds: the cold open's title, beats and handoff, each bit, each
/// connector.
pub fn targets(p: &StreamPrep) -> Vec<String> {
    let mut out = vec!["cold_open.title".to_string()];
    for i in 0..p.cold_open.beats.len() {
        out.push(format!("cold_open.beats[{}]", i + 1));
    }
    out.push("cold_open.handoff".into());
    out.extend(p.bits.iter().map(|b| b.id.clone()));
    out.extend(p.connectors.iter().map(|c| c.id.clone()));
    out
}

/// Every check's problems, in check order, with the check's pass detail.
fn assess(
    p: &StreamPrep,
    ctx: &CheckContext,
    dropped: &Dropped,
) -> Vec<(&'static str, Vec<Problem>, String)> {
    let all = lines(p);

    let long: Vec<Problem> = all
        .iter()
        .filter_map(|(at, l)| {
            let n = l.chars().count();
            (n > MAX_LINE_CHARS).then(|| Problem::at(at, format!("{at} is {n} characters")))
        })
        .collect();
    let empty: Vec<Problem> = all
        .iter()
        .filter(|(_, l)| l.trim().is_empty())
        .map(|(at, _)| Problem::at(at, format!("{at} is empty")))
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
        let msg = format!(
            "cold open has {beats} beats ({}-{})",
            COLD_OPEN_BEATS.0, COLD_OPEN_BEATS.1
        );
        counts.push(if beats < COLD_OPEN_BEATS.0 {
            Problem::slot(
                msg,
                "insert:cold_open.beat".into(),
                "beat",
                COLD_OPEN_BEATS.0 - beats,
            )
        } else {
            Problem {
                msg,
                targets: (1..=beats)
                    .map(|i| format!("cold_open.beats[{i}]"))
                    .collect(),
                slot: None,
            }
        });
    }
    for (r, n) in &bit_counts {
        if r.count_ok(*n) {
            continue;
        }
        let msg = format!("{n} {} ({})", r.plural, r.count_text());
        counts.push(if *n < r.count.0 {
            Problem::slot(
                msg,
                format!("insert:{}", r.singular),
                r.singular,
                r.count.0 - n,
            )
        } else {
            Problem {
                msg,
                targets: p
                    .bits
                    .iter()
                    .filter(|b| b.kind == r.kind)
                    .map(|b| b.id.clone())
                    .collect(),
                slot: None,
            }
        });
    }
    for (k, n) in &per_kind {
        if *n < MIN_CONNECTORS_PER_KIND {
            counts.push(Problem::slot(
                format!("{n} {k} connectors (at least {MIN_CONNECTORS_PER_KIND})"),
                format!("insert:connector:{k}"),
                k,
                MIN_CONNECTORS_PER_KIND - n,
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

    let mut shape: Vec<Problem> = dropped.shape.iter().cloned().map(Problem::bare).collect();
    shape.extend(empty);
    for b in &p.bits {
        let r = super::rules::rule(b.kind);
        let n = b.lines.len();
        if !r.lines_ok(n) {
            shape.push(Problem::at(
                &b.id,
                format!(
                    "{} is a {} with {n} lines ({})",
                    b.id,
                    r.singular,
                    r.lines_text()
                ),
            ));
        }
        if r.payoff_required && b.payoff.as_deref().is_none_or(|x| x.trim().is_empty()) {
            shape.push(Problem::at(
                &b.id,
                format!("{} is a {} without a payoff", b.id, r.singular),
            ));
        }
    }

    let real: Vec<Problem> = all
        .iter()
        .filter_map(|(at, l)| {
            names_real(l, ctx.real_names).map(|w| Problem::at(at, format!("{at} names {w}")))
        })
        .collect();
    let urls: Vec<Problem> = all
        .iter()
        .filter_map(|(at, l)| {
            patterns()
                .url
                .find(l)
                .map(|m| Problem::at(at, format!("{at} has {:?}", m.as_str())))
        })
        .collect();
    let prior: Vec<Problem> = all
        .iter()
        .filter_map(|(at, l)| {
            patterns().prior.find(l).map(|m| {
                Problem::at(
                    at,
                    format!("{at} leans on an earlier stream ({:?})", m.as_str()),
                )
            })
        })
        .collect();

    let mut segments: Vec<Problem> = dropped
        .segments
        .iter()
        .cloned()
        .map(Problem::bare)
        .collect();
    for b in &p.bits {
        if shadowrunr_segments::segment_is_descoped(b.segment) {
            segments.push(Problem::at(
                &b.id,
                format!("{} is for {}, which is descoped", b.id, wire(&b.segment)),
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
            dupes.push(Problem::at(at, format!("{at} repeats {first}")));
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
            .map(|id| Problem::at(id, format!("id {id} is used twice"))),
    );

    let gated: Vec<Problem> = all
        .iter()
        .filter_map(|(at, l)| {
            let (_, fired) = gate_text(l, ctx.policy, &[]);
            (!fired.is_empty()).then(|| {
                let rules: Vec<String> = fired.iter().map(|(r, a)| format!("{r} {a}")).collect();
                Problem::at(at, format!("{at}: {}", rules.join(", ")))
            })
        })
        .collect();

    let n = all.len();
    let mut out = vec![
        (
            "line_length",
            long,
            format!("{n} lines, each at most {MAX_LINE_CHARS} characters"),
        ),
        ("counts", counts, counts_detail),
        (
            "shape",
            shape,
            "every bit has the lines its kind needs".into(),
        ),
        (
            "real_names",
            real,
            "no real person, handle or channel".into(),
        ),
        ("urls", urls, "no links".into()),
        (
            "prior_streams",
            prior,
            "nothing leans on a stream a newcomer missed".into(),
        ),
        (
            "segments",
            segments,
            "every bit is for an active segment".into(),
        ),
        ("duplicates", dupes, "no line or id repeats".into()),
        (
            "output_gate",
            gated,
            format!("{n} lines pass the brain's output gate unchanged"),
        ),
    ];
    if let Some(t) = &p.table_read {
        out.push((
            "table_read",
            table_read(p, ctx),
            format!(
                "guest {}: {} scene(s), each approved and clean",
                t.guest,
                t.scenes.len()
            ),
        ));
    }
    out
}

/// Run every check over `p`. `dropped` lists what generation could not keep (a bit naming no
/// segment, an unknown kind); it fails `shape` or `segments`. A failed check's detail is
/// prose for the owner, at most eight problems; [`findings`] has every one.
pub fn run(p: &StreamPrep, ctx: &CheckContext, dropped: &Dropped) -> Vec<PrepCheck> {
    assess(p, ctx, dropped)
        .into_iter()
        .map(|(id, problems, pass)| check(id, &problems, pass))
        .collect()
}

/// What the checks catch in `p`, located and untruncated: the predicates [`run`] uses, one
/// finding per target a problem names and one insert slot per count shortfall (what
/// `prep revise` may edit).
pub fn findings(p: &StreamPrep, ctx: &CheckContext) -> Vec<Finding> {
    let mut out = Vec::new();
    for (id, problems, _) in assess(p, ctx, &Dropped::default()) {
        for x in problems {
            if let Some((at, slot)) = x.slot {
                out.push(Finding {
                    check: id.into(),
                    at,
                    why: x.msg.clone(),
                    slot: Some(slot),
                });
            }
            for at in x.targets {
                out.push(Finding {
                    check: id.into(),
                    at,
                    why: x.msg.clone(),
                    slot: None,
                });
            }
        }
    }
    out
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
