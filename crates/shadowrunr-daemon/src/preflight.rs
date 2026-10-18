//! Pre-flight checks PF-01..PF-08: the media-less mapping of the design's pre-flight board.
//! Every check returns exactly one [`HealthReport`]; a red check blocks `sim.start` and makes
//! `shadowrunr preflight` exit non-zero. Nothing here spends quota.

use std::path::Path;

use shadowrunr_brain::moderation::{ListStatus, Policy};
use shadowrunr_segments::{SegmentConfig, SegmentError, SegmentRuntime};
use shadowrunr_sim::calibration::{self, CalibrationOrigin};
use shadowrunr_types::{HealthReport, HealthState, SegmentId};

use crate::config::Config;
use crate::engine::EngineHandle;

/// Free space below this is red; below `AMBER_FREE_BYTES` amber.
pub const RED_FREE_BYTES: u64 = 1 << 30;
pub const AMBER_FREE_BYTES: u64 = 5 << 30;

fn rep(check: &str, state: HealthState, detail: impl Into<String>) -> HealthReport {
    HealthReport {
        check: check.into(),
        state,
        detail: detail.into(),
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
    vec![
        pf01_engine(engine).await,
        pf02_character(cfg),
        pf03_memory(cfg),
        pf04_segments(cfg),
        pf05_policy(cfg),
        pf06_calibration(cfg),
        pf07_disk(cfg),
        pf08_media(),
    ]
}

/// PF-01: the engine's own health (WP1), folded into one report.
pub async fn pf01_engine(engine: &EngineHandle) -> HealthReport {
    let reports = engine.engine.health().await;
    if reports.is_empty() {
        return rep("PF-01", HealthState::Amber, "engine reported no health");
    }
    let state = reports
        .iter()
        .fold(HealthState::Green, |s, r| worst(s, r.state));
    let detail = reports
        .iter()
        .map(|r| r.detail.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    rep("PF-01", state, detail)
}

/// PF-02: the default character package loads and its constitution is present.
pub fn pf02_character(cfg: &Config) -> HealthReport {
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
        ),
    }
}

/// PF-03: the memory store is writable and its index rebuilds from the markdown truth.
pub fn pf03_memory(cfg: &Config) -> HealthReport {
    let dir = cfg.paths.memory();
    let probe = dir.join(".preflight-probe");
    let writable = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&probe, b"ok"))
        .and_then(|_| std::fs::remove_file(&probe));
    if let Err(e) = writable {
        return rep(
            "PF-03",
            HealthState::Red,
            format!("{} not writable: {e}", dir.display()),
        );
    }
    match shadowrunr_brain::rebuild_index(&dir) {
        Ok(r) => rep(
            "PF-03",
            HealthState::Green,
            format!(
                "{} writable; index rebuilt: {} episodes, {} facts, {} tombstones",
                dir.display(),
                r.episodes,
                r.facts,
                r.tombstones
            ),
        ),
        Err(e) => rep(
            "PF-03",
            HealthState::Red,
            format!("index rebuild failed at {}: {e}", dir.display()),
        ),
    }
}

/// PF-04: segment tables parse, active segments start, descoped segments are refused.
pub fn pf04_segments(cfg: &Config) -> HealthReport {
    let tables = match shadowrunr_segments::table::tables() {
        Ok(t) => t.len(),
        Err(e) => return rep("PF-04", HealthState::Red, format!("segment tables: {e}")),
    };
    for seg in [SegmentId::Seg01ImprovisedRoom, SegmentId::Seg02aHexmap] {
        if let Err(e) = SegmentRuntime::with_config(seg, SegmentConfig::default()) {
            return rep(
                "PF-04",
                HealthState::Red,
                format!("{} does not start: {e}", shadowrunr_sim::wire(&seg)),
            );
        }
    }
    for seg in [SegmentId::Seg02bCartography, SegmentId::Seg02bDependency] {
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
    rep(
        "PF-04",
        HealthState::Green,
        format!("{tables} tables valid; SEG-01, SEG-02A start; SEG-02B-I, SEG-02B-II refused"),
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
    let path = cfg.paths.policy.join("platform-rules.json");
    if !path.is_file() {
        return rep(
            "PF-05",
            HealthState::Amber,
            format!(
                "{} absent: the brain uses its built-in term rules",
                path.display()
            ),
        );
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            return rep(
                "PF-05",
                HealthState::Red,
                format!("{}: {e}", path.display()),
            )
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
        Err(e) => return rep("PF-05", HealthState::Red, e.to_string()),
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
    if !same_file(&path, &auto) {
        state = HealthState::Amber;
        detail.push_str(&format!(
            "; the brain reads {} (PolicySource::Auto), not this file",
            auto.display()
        ));
    }
    rep("PF-05", state, detail)
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
                cfg.paths.sim.join("calibration.json").display()
            ),
        ),
        Err(e) => rep("PF-06", HealthState::Red, e.to_string()),
    }
}

/// PF-07: the runs directory is writable and the disk has room.
pub fn pf07_disk(cfg: &Config) -> HealthReport {
    let dir = cfg.paths.sim_runs();
    let probe = dir.join(".preflight-probe");
    let writable = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&probe, b"ok"))
        .and_then(|_| std::fs::remove_file(&probe));
    if let Err(e) = writable {
        return rep(
            "PF-07",
            HealthState::Red,
            format!("{} not writable: {e}", dir.display()),
        );
    }
    match fs2::available_space(&dir) {
        Ok(free) => {
            let gib = free as f64 / f64::from(1u32 << 30);
            let state = if free < RED_FREE_BYTES {
                HealthState::Red
            } else if free < AMBER_FREE_BYTES {
                HealthState::Amber
            } else {
                HealthState::Green
            };
            rep(
                "PF-07",
                state,
                format!("{} writable; {gib:.1} GiB free", dir.display()),
            )
        }
        Err(e) => rep(
            "PF-07",
            HealthState::Amber,
            format!("{} writable; free space unknown: {e}", dir.display()),
        ),
    }
}

/// PF-08: the media stack is out of scope in the media-less phase.
pub fn pf08_media() -> HealthReport {
    rep(
        "PF-08",
        HealthState::Amber,
        "not in media-less phase (TTS, avatar, OBS, chat connectors)",
    )
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

    #[test]
    fn media_is_amber() {
        let r = pf08_media();
        assert_eq!(r.check, "PF-08");
        assert_eq!(r.state, HealthState::Amber);
        assert!(r.detail.contains("not in media-less phase"));
    }
}
