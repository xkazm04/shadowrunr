//! Pre-flight checks PF-01..PF-08: the media-less mapping of the design's pre-flight board.
//! Every check returns exactly one [`HealthReport`]; a red check blocks `sim.start` and makes
//! `shadowrunr preflight` exit non-zero. Nothing here spends quota.
//!
//! Every non-green row is authored with its [`Remedy`] beside the check ([`Checked`]): applied
//! (`shadowrunr preflight --fix` may run it, see `preflight_fix`), guided (a command or file
//! the operator acts on) or external (outside shadowrunr). The checks themselves stay
//! read-only; a remedy is only a description until `--fix` applies it.

use std::path::Path;

use serde::Serialize;
use shadowrunr_brain::moderation::{ListStatus, Policy};
use shadowrunr_segments::{SegmentConfig, SegmentError, SegmentRuntime};
use shadowrunr_sim::calibration::{self, CalibrationOrigin};
use shadowrunr_types::{HealthReport, HealthState, SegmentId};

use crate::config::Config;
use crate::engine::EngineHandle;

/// Free space below this is red; below `AMBER_FREE_BYTES` amber.
pub const RED_FREE_BYTES: u64 = 1 << 30;
pub const AMBER_FREE_BYTES: u64 = 5 << 30;

/// How a non-green row is repaired: the remediation ladder as data, never prose to parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Remedy {
    /// `shadowrunr preflight --fix` applies it (`command` is the manual equivalent).
    Applied {
        #[serde(flatten)]
        fix: Fix,
        command: String,
    },
    /// The operator acts: a command to run and/or a file to write or repair.
    Guided {
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        text: String,
    },
    /// Outside shadowrunr (the engine fleet, the disk, the media stack).
    External { text: String },
}

/// A repair `preflight --fix` knows how to apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "fix", rename_all = "snake_case")]
pub enum Fix {
    /// Rebuild `<data>/memory/index.sqlite` from the markdown (derived data only).
    Reindex,
    /// Install term lists from their known sources: network, so it also needs `--download`.
    InstallLists { lists: Vec<ListSource> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListSource {
    pub list_id: String,
    pub source: String,
}

impl Remedy {
    fn guided(command: Option<String>, file: Option<&Path>, text: impl Into<String>) -> Self {
        Self::Guided {
            command,
            file: file.map(|f| f.display().to_string()),
            text: text.into(),
        }
    }

    fn external(text: impl Into<String>) -> Self {
        Self::External { text: text.into() }
    }

    /// The suffix a human pre-flight row ends with.
    pub fn hint(&self) -> String {
        match self {
            Self::Applied {
                fix: Fix::Reindex, ..
            } => "fix: shadowrunr preflight --fix".into(),
            Self::Applied {
                fix: Fix::InstallLists { .. },
                ..
            } => "fix: shadowrunr preflight --fix --download".into(),
            Self::Guided {
                command: Some(c), ..
            } => format!("fix: {c}"),
            Self::Guided { text, .. } => format!("fix: {text}"),
            Self::External { text } => format!("external: {text}"),
        }
    }
}

/// One check's report and its remedy (none when green or when nothing is known to help).
#[derive(Debug, Clone, Serialize)]
pub struct Checked {
    #[serde(flatten)]
    pub report: HealthReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remedy: Option<Remedy>,
}

impl Checked {
    fn fix(mut self, remedy: Remedy) -> Self {
        self.remedy = Some(remedy);
        self
    }
}

fn rep(check: &str, state: HealthState, detail: impl Into<String>) -> Checked {
    Checked {
        report: HealthReport {
            check: check.into(),
            state,
            detail: detail.into(),
        },
        remedy: None,
    }
}

fn worst(a: HealthState, b: HealthState) -> HealthState {
    use HealthState::*;
    match (a, b) {
        (Red, _) | (_, Red) => Red,
        (Amber, _) | (_, Amber) => Amber,
        _ => Green,
    }
}

pub fn any_red(reports: &[HealthReport]) -> bool {
    reports.iter().any(|r| r.state == HealthState::Red)
}

/// Run all eight checks, in order.
pub async fn run_all(cfg: &Config, engine: &EngineHandle) -> Vec<HealthReport> {
    run_all_checked(cfg, engine)
        .await
        .into_iter()
        .map(|c| c.report)
        .collect()
}

/// [`run_all`] with each row's remedy. Read-only, like every check.
pub async fn run_all_checked(cfg: &Config, engine: &EngineHandle) -> Vec<Checked> {
    let mut rows = vec![
        pf01(engine).await,
        pf02(cfg),
        pf03(cfg),
        pf04(cfg),
        pf05(cfg),
        pf06(cfg),
        pf07(cfg),
        pf08(),
    ];
    // PF-09 (integrations) appears only when Personas activity is configured.
    rows.extend(
        crate::integrations::activity::preflight_line(cfg)
            .await
            .map(|report| Checked {
                report,
                remedy: None,
            }),
    );
    rows
}

/// PF-01: the engine's own health (WP1), folded into one report.
pub async fn pf01_engine(engine: &EngineHandle) -> HealthReport {
    pf01(engine).await.report
}

async fn pf01(engine: &EngineHandle) -> Checked {
    let reports = engine.engine.health().await;
    let external =
        || Remedy::external("repair the engine named in the detail (CLI install, account login)");
    if reports.is_empty() {
        return rep("PF-01", HealthState::Amber, "engine reported no health").fix(external());
    }
    let state = reports
        .iter()
        .fold(HealthState::Green, |s, r| worst(s, r.state));
    let detail = reports
        .iter()
        .map(|r| r.detail.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    let c = rep("PF-01", state, detail);
    if state == HealthState::Green {
        c
    } else {
        c.fix(external())
    }
}

/// PF-02: the default character package loads and its constitution is present.
pub fn pf02_character(cfg: &Config) -> HealthReport {
    pf02(cfg).report
}

fn pf02(cfg: &Config) -> Checked {
    let id = &cfg.sim.character;
    let dir = cfg.paths.characters.join(id);
    match shadowrunr_brain::load_character(&dir, "") {
        Ok(pkg) => rep(
            "PF-02",
            HealthState::Green,
            format!(
                "{} ({}): constitution {} chars, tone {}, {} foils, {} situations",
                pkg.name,
                pkg.id,
                pkg.constitution.chars().count(),
                pkg.tone_variant,
                pkg.foils.len(),
                pkg.situations.situations.len()
            ),
        ),
        Err(e) => rep(
            "PF-02",
            HealthState::Red,
            format!("character {id} at {}: {e}", dir.display()),
        )
        .fix(Remedy::guided(
            None,
            Some(&dir),
            "repair the character package, or point [sim] character / [paths] characters at one that loads",
        )),
    }
}

/// The amber hint when the memory index disagrees with the markdown.
pub const REINDEX_HINT: &str = "index stale: run `shadowrunr memory reindex`";

/// The nearest existing directory at or above `dir`, and whether it is marked read-only.
/// A check never creates the directory it checks.
fn nearest_dir_readonly(dir: &Path) -> Option<(std::path::PathBuf, bool)> {
    let mut d = Some(dir);
    while let Some(p) = d {
        if let Ok(m) = std::fs::metadata(p) {
            if m.is_dir() {
                return Some((p.to_path_buf(), m.permissions().readonly()));
            }
        }
        d = p.parent();
    }
    None
}

/// PF-03: the memory store's directory is usable and its SQLite index agrees with the
/// markdown truth. Read-only: nothing is created, probed by writing, or rebuilt (the MCP
/// `preflight` tool is declared read-only). A stale index is amber with the repair command;
/// the rebuild itself is `shadowrunr memory reindex`.
pub fn pf03_memory(cfg: &Config) -> HealthReport {
    pf03(cfg).report
}

fn pf03(cfg: &Config) -> Checked {
    let dir = cfg.paths.memory();
    if !dir.is_dir() {
        return match nearest_dir_readonly(&dir) {
            Some((at, false)) => rep(
                "PF-03",
                HealthState::Green,
                format!(
                    "{} not created yet (the store creates it on first open under {}); nothing to index",
                    dir.display(),
                    at.display()
                ),
            ),
            Some((at, true)) => rep(
                "PF-03",
                HealthState::Red,
                format!("{} cannot be created: {} is read-only", dir.display(), at.display()),
            )
            .fix(Remedy::external(format!("make {} writable", at.display()))),
            None => rep(
                "PF-03",
                HealthState::Red,
                format!("{}: no existing parent directory", dir.display()),
            )
            .fix(Remedy::guided(
                None,
                None,
                "point [paths] data at an existing directory",
            )),
        };
    }
    if nearest_dir_readonly(&dir).is_some_and(|(_, ro)| ro) {
        return rep(
            "PF-03",
            HealthState::Red,
            format!("{} is read-only", dir.display()),
        )
        .fix(Remedy::external(format!("make {} writable", dir.display())));
    }
    match shadowrunr_brain::memory::verify_index(&dir) {
        Ok(c) if c.in_step() => rep(
            "PF-03",
            HealthState::Green,
            format!(
                "{}: index in step with the markdown ({} episodes, {} facts, {} tombstones)",
                dir.display(),
                c.files.episodes,
                c.files.facts,
                c.files.tombstones
            ),
        ),
        Ok(c) if !c.index_present && c.files == Default::default() => rep(
            "PF-03",
            HealthState::Green,
            format!("{}: empty store, no index yet", dir.display()),
        ),
        Ok(c) => rep(
            "PF-03",
            HealthState::Amber,
            format!(
                "{REINDEX_HINT} ({}: {})",
                dir.display(),
                c.problems.join("; ")
            ),
        )
        .fix(Remedy::Applied {
            fix: Fix::Reindex,
            command: "shadowrunr memory reindex".into(),
        }),
        Err(e) => rep(
            "PF-03",
            HealthState::Red,
            format!("memory markdown unreadable at {}: {e}", dir.display()),
        )
        .fix(Remedy::guided(
            Some("shadowrunr memory reindex".into()),
            Some(&dir),
            "repair the unreadable markdown, then reindex",
        )),
    }
}

/// PF-04: segment tables parse, active segments start, descoped segments are refused.
pub fn pf04_segments(cfg: &Config) -> HealthReport {
    pf04(cfg).report
}

fn pf04(cfg: &Config) -> Checked {
    let c = pf04_tables(cfg);
    if c.report.state == HealthState::Green {
        c
    } else if !cfg.paths.segments.is_dir() {
        c.fix(Remedy::guided(
            None,
            Some(&cfg.paths.segments),
            "point [paths] segments at the segment data directory",
        ))
    } else {
        c.fix(Remedy::external(
            "the segment tables shipped with this build are broken: rebuild from a good tree",
        ))
    }
}

fn pf04_tables(cfg: &Config) -> Checked {
    let tables = match shadowrunr_segments::table::tables() {
        Ok(t) => t.len(),
        Err(e) => return rep("PF-04", HealthState::Red, format!("segment tables: {e}")),
    };
    let (active, descoped) = (
        shadowrunr_segments::active_segments(),
        shadowrunr_segments::descoped_segments(),
    );
    // The configured character's config (its persona, tuning, flavour and rank overrides):
    // what its streams start with. A package that does not load is PF-02's red; the engine
    // defaults stand in here.
    let config = match shadowrunr_brain::load_character(
        &cfg.paths.characters.join(&cfg.sim.character),
        "",
    ) {
        Ok(pkg) => match crate::character::segment_config(cfg, &pkg) {
            Ok(c) => c,
            Err(e) => return rep("PF-04", HealthState::Red, e),
        },
        Err(_) => SegmentConfig::default(),
    };
    for &seg in &active {
        if let Err(e) = SegmentRuntime::with_config(seg, config.clone()) {
            return rep(
                "PF-04",
                HealthState::Red,
                format!("{} does not start: {e}", shadowrunr_sim::wire(&seg)),
            );
        }
    }
    for &seg in &descoped {
        match SegmentRuntime::with_config(seg, SegmentConfig::default()) {
            Err(SegmentError::Descoped { .. }) => {}
            Ok(_) => {
                return rep(
                    "PF-04",
                    HealthState::Red,
                    format!(
                        "descoped segment {} was not refused",
                        shadowrunr_sim::wire(&seg)
                    ),
                )
            }
            Err(e) => {
                return rep(
                    "PF-04",
                    HealthState::Red,
                    format!("{}: unexpected error {e}", shadowrunr_sim::wire(&seg)),
                )
            }
        }
    }
    if !cfg.paths.segments.is_dir() {
        return rep(
            "PF-04",
            HealthState::Red,
            format!(
                "segment data dir {} is missing (the sim instrument hashes it)",
                cfg.paths.segments.display()
            ),
        );
    }
    let names = |ids: &[SegmentId]| {
        ids.iter()
            .map(shadowrunr_sim::wire)
            .collect::<Vec<_>>()
            .join(", ")
    };
    rep(
        "PF-04",
        HealthState::Green,
        format!(
            "{tables} tables valid; {} start; {} refused",
            names(&active),
            names(&descoped)
        ),
    )
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// PF-05: `policy/platform-rules.json` is present and parses (amber when absent), and the
/// operator-installed lists its placeholders name: green with "installed list <id> (<n>
/// terms)", amber with "placeholder only".
pub fn pf05_policy(cfg: &Config) -> HealthReport {
    pf05(cfg).report
}

fn pf05(cfg: &Config) -> Checked {
    let path = cfg.paths.policy.join("platform-rules.json");
    if !path.is_file() {
        return rep(
            "PF-05",
            HealthState::Amber,
            format!(
                "{} absent: the brain uses its built-in term rules",
                path.display()
            ),
        )
        .fix(Remedy::guided(
            None,
            Some(&path),
            "write the platform rules file (the repo ships policy/platform-rules.json)",
        ));
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            return rep(
                "PF-05",
                HealthState::Red,
                format!("{}: {e}", path.display()),
            )
            .fix(Remedy::guided(
                None,
                Some(&path),
                "repair the platform rules file",
            ))
        }
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            return rep(
                "PF-05",
                HealthState::Red,
                format!("{}: {e}", path.display()),
            )
            .fix(Remedy::guided(
                None,
                Some(&path),
                "repair the platform rules file",
            ))
        }
    };
    let platforms: Vec<String> = v
        .get("platforms")
        .and_then(|p| p.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let rules = v
        .get("term_rules")
        .and_then(|r| r.as_array())
        .map_or(0, Vec::len);
    let policy = match Policy::from_file_with_local(&path, &cfg.paths.policy.join("local")) {
        Ok(p) => p,
        Err(e) => {
            return rep("PF-05", HealthState::Red, e.to_string()).fix(Remedy::guided(
                None,
                Some(&path),
                "repair the platform rules file",
            ))
        }
    };
    // The brain's PolicySource::Auto reads <characters>/../policy; say so when the
    // configured file is a different one.
    let auto = cfg
        .paths
        .characters
        .join("..")
        .join("policy")
        .join("platform-rules.json");
    let mut detail = format!(
        "{rules} term rules; platforms: {}",
        if platforms.is_empty() {
            "none".to_string()
        } else {
            platforms.join(", ")
        }
    );
    let (lists_state, lists_detail) = installed_lists(&policy.lists);
    if !lists_detail.is_empty() {
        detail.push_str("; ");
        detail.push_str(&lists_detail);
    }
    let mut state = lists_state;
    let mut remedy = list_remedy(&policy.lists);
    if !same_file(&path, &auto) {
        state = HealthState::Amber;
        detail.push_str(&format!(
            "; the brain reads {} (PolicySource::Auto), not this file",
            auto.display()
        ));
        remedy = remedy.or_else(|| {
            Some(Remedy::guided(
                None,
                Some(&auto),
                "keep the platform rules where the brain reads them, and point [paths] policy there",
            ))
        });
    }
    let c = rep("PF-05", state, detail);
    match remedy {
        Some(r) => c.fix(r),
        None => c,
    }
}

/// The remedy for unserved placeholder categories: install, per category, a list with a
/// known source (applied, network-gated); guided when a category has none.
fn list_remedy(lists: &[ListStatus]) -> Option<Remedy> {
    let mut chosen: Vec<ListSource> = Vec::new();
    let mut manual: Vec<&str> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for l in lists {
        let category = l.category.as_str();
        if seen.contains(&category) {
            continue;
        }
        seen.push(category);
        let in_category = || lists.iter().filter(move |x| x.category == category);
        if in_category().any(|x| x.installed_terms.is_some()) {
            continue;
        }
        // Only a list with no installed file yet: a fix never overwrites an operator's copy.
        let known = in_category().find_map(|x| {
            let k = crate::policy_install::known(&x.list_id)?;
            (x.error.is_none()).then(|| ListSource {
                list_id: x.list_id.clone(),
                source: k.url.to_string(),
            })
        });
        match known {
            Some(src) => chosen.push(src),
            None => manual.push(&l.list_id),
        }
    }
    if !manual.is_empty() {
        return Some(Remedy::Guided {
            command: Some(format!(
                "shadowrunr policy install-list {} --source <url|path>",
                manual[0]
            )),
            file: None,
            text: format!("no known source for {}", manual.join(", ")),
        });
    }
    if chosen.is_empty() {
        return None;
    }
    let command = chosen
        .iter()
        .map(|l| format!("shadowrunr policy install-list {}", l.list_id))
        .collect::<Vec<_>>()
        .join(" && ");
    Some(Remedy::Applied {
        fix: Fix::InstallLists { lists: chosen },
        command,
    })
}

/// The installed-list part of PF-05: green when every placeholder category is served by an
/// installed list ("installed list <id> (<n> terms)"), amber otherwise ("placeholder only").
pub fn installed_lists(lists: &[ListStatus]) -> (HealthState, String) {
    let mut parts = Vec::new();
    let mut unserved: Vec<&str> = Vec::new();
    for l in lists {
        match (l.installed_terms, &l.error) {
            (Some(n), _) => parts.push(format!("installed list {} ({n} terms)", l.list_id)),
            (None, Some(e)) => parts.push(format!(
                "list {} unusable ({e}); built-in stand-in",
                l.list_id
            )),
            (None, None) => parts.push(format!("list {}: placeholder only", l.list_id)),
        }
    }
    for l in lists {
        let served = lists
            .iter()
            .any(|x| x.category == l.category && x.installed_terms.is_some());
        if !served && !unserved.contains(&l.category.as_str()) {
            unserved.push(&l.category);
        }
    }
    let state = if unserved.is_empty() {
        HealthState::Green
    } else {
        HealthState::Amber
    };
    let mut detail = parts.join(", ");
    if !unserved.is_empty() {
        detail.push_str(&format!(
            " (placeholder only for {}: run `shadowrunr policy install-list <id>`)",
            unserved.join(", ")
        ));
    }
    (state, detail)
}

/// PF-06: `sim/calibration.json` is present (amber when only the built-in defaults exist).
pub fn pf06_calibration(cfg: &Config) -> HealthReport {
    pf06(cfg).report
}

fn pf06(cfg: &Config) -> Checked {
    let file = cfg.paths.sim.join("calibration.json");
    match calibration::load(&cfg.paths.sim) {
        Ok((cal, CalibrationOrigin::File { path }, _)) => {
            let assumptions = cal.assumptions().len();
            rep(
                "PF-06",
                HealthState::Green,
                format!(
                    "{path}: {} sources, {} archetypes, {} populations, {assumptions} figures are assumptions",
                    cal.sources.len(),
                    cal.archetypes.len(),
                    cal.populations.len()
                ),
            )
        }
        Ok((_, CalibrationOrigin::DefaultAssumption, _)) => rep(
            "PF-06",
            HealthState::Amber,
            format!(
                "{} absent: only the built-in defaults exist (every figure is an assumption)",
                file.display()
            ),
        )
        .fix(Remedy::guided(
            None,
            Some(&file),
            "write the calibration file (the repo ships sim/calibration.json)",
        )),
        Err(e) => rep("PF-06", HealthState::Red, e.to_string()).fix(Remedy::guided(
            None,
            Some(&file),
            "repair the calibration file",
        )),
    }
}

/// PF-07: the runs directory is usable and the disk has room. Read-only like PF-03: the
/// directory is not created and no probe file is written (a missing directory is created by
/// the first run).
pub fn pf07_disk(cfg: &Config) -> HealthReport {
    pf07(cfg).report
}

fn pf07(cfg: &Config) -> Checked {
    let dir = cfg.paths.sim_runs();
    let (at, readonly) = match nearest_dir_readonly(&dir) {
        Some(x) => x,
        None => {
            return rep(
                "PF-07",
                HealthState::Red,
                format!("{}: no existing parent directory", dir.display()),
            )
            .fix(Remedy::guided(
                None,
                None,
                "point [paths] runs at an existing directory",
            ))
        }
    };
    if readonly {
        return rep(
            "PF-07",
            HealthState::Red,
            format!(
                "{} not writable: {} is read-only",
                dir.display(),
                at.display()
            ),
        )
        .fix(Remedy::external(format!("make {} writable", at.display())));
    }
    match fs2::available_space(&at) {
        Ok(free) => {
            let gib = free as f64 / f64::from(1u32 << 30);
            let state = if free < RED_FREE_BYTES {
                HealthState::Red
            } else if free < AMBER_FREE_BYTES {
                HealthState::Amber
            } else {
                HealthState::Green
            };
            let c = rep(
                "PF-07",
                state,
                format!("{} writable; {gib:.1} GiB free", dir.display()),
            );
            if state == HealthState::Green {
                c
            } else {
                c.fix(Remedy::external(format!(
                    "free disk space on {}",
                    at.display()
                )))
            }
        }
        Err(e) => rep(
            "PF-07",
            HealthState::Amber,
            format!("{} writable; free space unknown: {e}", dir.display()),
        )
        .fix(Remedy::external(format!(
            "check the disk under {}",
            at.display()
        ))),
    }
}

/// PF-08: the media stack is out of scope in the media-less phase.
pub fn pf08_media() -> HealthReport {
    pf08().report
}

fn pf08() -> Checked {
    rep(
        "PF-08",
        HealthState::Amber,
        "not in media-less phase (TTS, avatar, OBS, chat connectors)",
    )
    .fix(Remedy::external(
        "the media stack is outside the media-less phase",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_state_wins() {
        assert_eq!(
            worst(HealthState::Green, HealthState::Amber),
            HealthState::Amber
        );
        assert_eq!(
            worst(HealthState::Amber, HealthState::Red),
            HealthState::Red
        );
        assert_eq!(
            worst(HealthState::Green, HealthState::Green),
            HealthState::Green
        );
    }

    fn status(list: &str, cat: &str, terms: Option<usize>) -> ListStatus {
        ListStatus {
            rule_id: format!("r-{list}"),
            list_id: list.into(),
            category: cat.into(),
            installed_terms: terms,
            error: None,
        }
    }

    #[test]
    fn installed_lists_green_when_every_category_is_served() {
        let (s, d) = installed_lists(&[
            status("awdev1-better-profane-words", "slur", Some(528)),
            status("surge-ai-profanity-en", "slur", None),
        ]);
        assert_eq!(s, HealthState::Green);
        assert!(d.contains("installed list awdev1-better-profane-words (528 terms)"));
        let (s, d) = installed_lists(&[
            status("awdev1-better-profane-words", "slur", None),
            status("surge-ai-profanity-en", "slur", None),
        ]);
        assert_eq!(s, HealthState::Amber);
        assert!(d.contains("placeholder only"));
        assert_eq!(installed_lists(&[]).0, HealthState::Green);
    }

    fn stamp(p: &Path) -> (Vec<u8>, std::time::SystemTime) {
        (
            std::fs::read(p).unwrap(),
            std::fs::metadata(p).unwrap().modified().unwrap(),
        )
    }

    fn seed_memory(dir: &Path) {
        let store = shadowrunr_brain::MemoryStore::open(dir).unwrap();
        store
            .write_episode(&shadowrunr_types::Episode {
                id: "ep_1".into(),
                ts_ms: 1_000,
                stream_id: "s1".into(),
                kind: shadowrunr_types::EpisodeKind::Stream,
                text: "Gorbo begged for a title.".into(),
                importance: 3,
            })
            .unwrap();
    }

    #[test]
    fn pf03_verifies_without_writing_and_names_the_repair() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = crate::e2e::test_config(tmp.path(), crate::e2e::repo().join("characters"));
        let mem = cfg.paths.memory();

        // No store yet: green, and the check does not create it.
        let r = pf03_memory(&cfg);
        assert_eq!(r.state, HealthState::Green, "{}", r.detail);
        assert!(!mem.exists(), "a check never creates the directory");

        seed_memory(&mem);
        let index = mem.join(shadowrunr_brain::memory::store::INDEX_FILE);
        let before = stamp(&index);
        let listing = |d: &Path| {
            let mut v: Vec<_> = std::fs::read_dir(d)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            v.sort();
            v
        };
        let files_before = listing(&mem);
        for _ in 0..2 {
            let r = pf03_memory(&cfg);
            assert_eq!(r.state, HealthState::Green, "{}", r.detail);
            assert!(r.detail.contains("index in step"), "{}", r.detail);
        }
        assert_eq!(stamp(&index), before, "index bytes and mtime unchanged");
        assert_eq!(listing(&mem), files_before, "no probe, journal or new file");

        // The index misses a markdown file: amber with the repair command, still no write.
        let ep_dir = mem.join("episodes").join("1970").join("01");
        let text = std::fs::read_to_string(ep_dir.join("ep_1.md")).unwrap();
        std::fs::write(ep_dir.join("ep_2.md"), text.replace("ep_1", "ep_2")).unwrap();
        let r = pf03_memory(&cfg);
        assert_eq!(r.state, HealthState::Amber, "{}", r.detail);
        assert!(r.detail.starts_with(REINDEX_HINT), "{}", r.detail);
        assert_eq!(stamp(&index), before);

        // The explicit repair brings it back to green.
        shadowrunr_brain::rebuild_index(&mem).unwrap();
        assert_eq!(pf03_memory(&cfg).state, HealthState::Green);
    }

    #[test]
    fn pf07_writes_no_probe() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = crate::e2e::test_config(tmp.path(), crate::e2e::repo().join("characters"));
        let dir = cfg.paths.sim_runs();
        let mtime = std::fs::metadata(&dir).unwrap().modified().unwrap();
        let r = pf07_disk(&cfg);
        assert_ne!(r.state, HealthState::Red, "{}", r.detail);
        assert!(!dir.join(".preflight-probe").exists());
        assert_eq!(std::fs::metadata(&dir).unwrap().modified().unwrap(), mtime);
        // A runs directory that does not exist yet is not created by the check.
        let mut cfg2 = cfg.clone();
        cfg2.paths.runs = tmp.path().join("fresh-runs");
        assert_ne!(pf07_disk(&cfg2).state, HealthState::Red);
        assert!(!tmp.path().join("fresh-runs").exists());
    }

    /// A characters root holding a malakor copy whose `situations.json` went through `edit`.
    fn characters_with_situations(
        tmp: &Path,
        edit: impl FnOnce(&mut Vec<serde_json::Value>),
    ) -> std::path::PathBuf {
        let root = tmp.join("characters");
        let dir = root.join("malakor");
        crate::e2e::copy_dir(&crate::e2e::repo().join("characters/malakor"), &dir);
        let path = dir.join("situations.json");
        let mut bank: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        edit(bank["situations"].as_array_mut().unwrap());
        std::fs::write(&path, serde_json::to_string_pretty(&bank).unwrap()).unwrap();
        root
    }

    #[test]
    fn pf02_is_red_for_a_defective_situation_bank() {
        type Edit = fn(&mut Vec<serde_json::Value>);
        let cases: [(Edit, &str); 2] = [
            (
                |s| {
                    let mut a = s[0].clone();
                    a["id"] = "x".into();
                    s.push(a.clone());
                    s.push(a);
                },
                "duplicate id x",
            ),
            (
                |s| s[0]["segments"] = serde_json::json!(["SEG-09"]),
                "SEG-09",
            ),
        ];
        for (edit, reason) in cases {
            let tmp = tempfile::tempdir().unwrap();
            let chars = characters_with_situations(tmp.path(), edit);
            let cfg = crate::e2e::test_config(tmp.path(), chars);
            let r = pf02_character(&cfg);
            assert_eq!(r.state, HealthState::Red, "{}", r.detail);
            assert!(r.detail.contains("situations.json"), "{}", r.detail);
            assert!(r.detail.contains(reason), "{}", r.detail);
        }
        // The shipped bank stays green.
        let tmp = tempfile::tempdir().unwrap();
        let cfg = crate::e2e::test_config(tmp.path(), crate::e2e::repo().join("characters"));
        assert_eq!(pf02_character(&cfg).state, HealthState::Green);
    }

    #[test]
    fn media_is_amber() {
        let r = pf08_media();
        assert_eq!(r.check, "PF-08");
        assert_eq!(r.state, HealthState::Amber);
        assert!(r.detail.contains("not in media-less phase"));
    }
}
