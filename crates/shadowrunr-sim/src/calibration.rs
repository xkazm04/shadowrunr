//! The audience calibration (`sim/calibration.json`, schema in docs/spec/contract.md).
//!
//! WPR writes the real file from sourced research. When it is absent the simulator falls back
//! to `data/default-calibration.json`, whose every figure is an assumption, and every report
//! built on it says so.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::SimError;

/// The compiled-in fallback. Labelled `"assumption"` throughout.
pub const DEFAULT_CALIBRATION: &str = include_str!("../data/default-calibration.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeCal {
    /// Share of present viewers who ever chat.
    pub chatting_share: f64,
    /// Room-level messages per minute, [low, high].
    pub msgs_per_min: [f64; 2],
    #[serde(default)]
    pub source_ids: Vec<String>,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchetypeCal {
    #[serde(default)]
    pub description: String,
    /// Relative message rate (1.0 = an average chatter).
    #[serde(default = "one")]
    pub rate_rel: f64,
    #[serde(default)]
    pub behaviours: Vec<String>,
    #[serde(default)]
    pub example_lines: Vec<String>,
    #[serde(default)]
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaitTactic {
    pub id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub example: String,
    #[serde(default)]
    pub source_ids: Vec<String>,
    /// The situations that perform this tactic, per character bank:
    /// `{ "malakor": ["prompt-injection"] }`. The binding lives here, beside the sourced
    /// threat model, so the bank's schema does not change; `situations::tactic_reach` checks
    /// every id against the bank and reports the tactics no situation performs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub situations: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calibration {
    pub version: u32,
    #[serde(default)]
    pub sources: Vec<Source>,
    pub audience_sizes: BTreeMap<String, SizeCal>,
    pub archetypes: BTreeMap<String, ArchetypeCal>,
    pub populations: BTreeMap<String, BTreeMap<String, f64>>,
    #[serde(default)]
    pub bait_tactics: Vec<BaitTactic>,
}

/// Where the calibration in use came from (a wire type: `results.json` lists it).
pub use shadowrunr_types::results::CalibrationOrigin;

/// A figure is an assumption when it cites no source (or cites "assumption" explicitly).
pub fn is_assumption(source_ids: &[String]) -> bool {
    source_ids.is_empty()
        || source_ids
            .iter()
            .all(|s| s.trim().eq_ignore_ascii_case("assumption"))
}

/// Sourced, but the source list also says "assumption" (a figure extrapolated from sources).
pub fn is_partly_assumption(source_ids: &[String]) -> bool {
    source_ids
        .iter()
        .any(|s| s.trim().eq_ignore_ascii_case("assumption"))
}

fn norm_id(s: &str) -> String {
    s.trim().to_ascii_lowercase().replace(['_', ' '], "-")
}

impl Calibration {
    pub fn parse(text: &str) -> Result<Self, SimError> {
        let cal: Calibration = serde_json::from_str(text)
            .map_err(|e| SimError::Spec(format!("calibration does not match the contract: {e}")))?;
        if cal.audience_sizes.is_empty() {
            return Err(SimError::Spec("calibration has no audience_sizes".into()));
        }
        for (k, v) in &cal.audience_sizes {
            if k.parse::<u32>().is_err() {
                return Err(SimError::Spec(format!(
                    "calibration audience size key {k:?} is not a number"
                )));
            }
            if !(0.0..=1.0).contains(&v.chatting_share) || v.msgs_per_min[0] > v.msgs_per_min[1] {
                return Err(SimError::Spec(format!(
                    "calibration audience size {k}: chatting_share must be 0..1 and msgs_per_min [low, high]"
                )));
            }
        }
        cal.check_references()?;
        Ok(cal)
    }

    /// Cross-references a typo would otherwise turn into a silently skewed mix or an unsourced
    /// figure that claims a source: population archetypes must be archetypes, every cited
    /// source id must be a source (or the literal `assumption`), tactic ids must be unique.
    fn check_references(&self) -> Result<(), SimError> {
        for (pop, mix) in &self.populations {
            if let Some(a) = mix.keys().find(|a| !self.archetypes.contains_key(*a)) {
                return Err(SimError::Spec(format!(
                    "calibration populations.{pop} names archetype {a:?}, which is not in archetypes (have: {})",
                    self.archetypes.keys().cloned().collect::<Vec<_>>().join(", ")
                )));
            }
        }
        let known = |id: &str| {
            id.trim().eq_ignore_ascii_case("assumption") || self.sources.iter().any(|s| s.id == id)
        };
        let cited = self
            .audience_sizes
            .iter()
            .map(|(k, v)| (format!("audience_sizes.{k}"), &v.source_ids))
            .chain(
                self.archetypes
                    .iter()
                    .map(|(k, v)| (format!("archetypes.{k}"), &v.source_ids)),
            )
            .chain(
                self.bait_tactics
                    .iter()
                    .map(|t| (format!("bait_tactics.{}", t.id), &t.source_ids)),
            );
        for (path, ids) in cited {
            if let Some(id) = ids.iter().find(|id| !known(id)) {
                return Err(SimError::Spec(format!(
                    "calibration {path} cites source {id:?}, which is not in sources"
                )));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        if let Some(t) = self
            .bait_tactics
            .iter()
            .find(|t| !seen.insert(t.id.as_str()))
        {
            return Err(SimError::Spec(format!(
                "calibration bait_tactics has duplicate id {:?}",
                t.id
            )));
        }
        Ok(())
    }

    pub fn default_assumption() -> Self {
        Self::parse(DEFAULT_CALIBRATION).expect("bundled default calibration parses")
    }

    /// The calibration row for an audience size: the configured size nearest on a log scale.
    pub fn size_cal(&self, audience: u32) -> (&str, &SizeCal) {
        let a = f64::from(audience.max(1)).ln();
        self.audience_sizes
            .iter()
            .min_by(|(ka, _), (kb, _)| {
                let da = (f64::from(ka.parse::<u32>().unwrap_or(1).max(1)).ln() - a).abs();
                let db = (f64::from(kb.parse::<u32>().unwrap_or(1).max(1)).ln() - a).abs();
                da.total_cmp(&db)
            })
            .map(|(k, v)| (k.as_str(), v))
            .expect("audience_sizes is non-empty (checked in parse)")
    }

    /// Population mix by id, tolerant of `_`/`-`/case differences. Shares are normalised.
    pub fn population(&self, id: &str) -> Result<Vec<(String, f64)>, SimError> {
        let want = norm_id(id);
        let mix = self
            .populations
            .iter()
            .find(|(k, _)| norm_id(k) == want)
            .map(|(_, v)| v)
            .ok_or_else(|| {
                SimError::Spec(format!(
                    "population {id:?} not in calibration (have: {})",
                    self.populations
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
        let total: f64 = mix.values().filter(|v| **v > 0.0).sum();
        if total <= 0.0 {
            return Err(SimError::Spec(format!(
                "population {id:?} has no positive share"
            )));
        }
        Ok(mix
            .iter()
            .filter(|(_, v)| **v > 0.0)
            .map(|(k, v)| (k.clone(), v / total))
            .collect())
    }

    pub fn archetype(&self, id: &str) -> Option<&ArchetypeCal> {
        self.archetypes.get(id)
    }

    /// Figures in use that cite no source (`<path>`) or are partly assumed (`<path> (partly)`),
    /// for the report's assumptions section.
    pub fn assumptions(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut note = |path: String, ids: &[String]| {
            if is_assumption(ids) {
                out.push(path);
            } else if is_partly_assumption(ids) {
                out.push(format!("{path} (partly)"));
            }
        };
        for (k, v) in &self.audience_sizes {
            note(format!("audience_sizes.{k}"), &v.source_ids);
        }
        for (k, v) in &self.archetypes {
            note(format!("archetypes.{k}"), &v.source_ids);
        }
        for t in &self.bait_tactics {
            note(format!("bait_tactics.{}", t.id), &t.source_ids);
        }
        out
    }
}

/// Load `<sim_root>/calibration.json`, or the default assumption when it does not exist.
/// A file that exists but does not parse is an error: a silent fallback would hide it.
pub fn load(sim_root: &Path) -> Result<(Calibration, CalibrationOrigin, String), SimError> {
    let path: PathBuf = sim_root.join("calibration.json");
    if path.is_file() {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| SimError::Io(format!("{}: {e}", path.display())))?;
        let cal = Calibration::parse(&text)?;
        Ok((
            cal,
            CalibrationOrigin::File {
                path: path.display().to_string(),
            },
            text,
        ))
    } else {
        Ok((
            Calibration::default_assumption(),
            CalibrationOrigin::DefaultAssumption,
            DEFAULT_CALIBRATION.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parses_and_is_all_assumption() {
        let cal = Calibration::default_assumption();
        assert!(cal.sources.is_empty());
        for size in ["3", "30", "300"] {
            assert!(cal.audience_sizes.contains_key(size));
        }
        for id in [
            "lurker",
            "regular",
            "newcomer",
            "sub_voter",
            "baiter",
            "raider_wave",
            "copypasta_wave",
            "mod",
        ] {
            let a = cal.archetype(id).expect(id);
            assert!(is_assumption(&a.source_ids));
            assert!(!a.example_lines.is_empty(), "{id} has example lines");
        }
        assert_eq!(
            cal.assumptions().len(),
            3 + cal.archetypes.len() + cal.bait_tactics.len()
        );
        assert!(DEFAULT_CALIBRATION.contains("\"assumption\""));
    }

    #[test]
    fn nearest_size_on_log_scale() {
        let cal = Calibration::default_assumption();
        assert_eq!(cal.size_cal(3).0, "3");
        assert_eq!(cal.size_cal(8).0, "3");
        assert_eq!(cal.size_cal(12).0, "30");
        assert_eq!(cal.size_cal(1000).0, "300");
    }

    #[test]
    fn population_lookup_is_tolerant_and_normalised() {
        let cal = Calibration::default_assumption();
        let mix = cal.population("Small-New-Channel").unwrap();
        let total: f64 = mix.iter().map(|(_, v)| v).sum();
        assert!((total - 1.0).abs() < 1e-9);
        assert!(cal.population("nope").is_err());
    }

    #[test]
    fn researched_file_parses_when_present() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sim");
        if !p.join("calibration.json").is_file() {
            return;
        }
        let (cal, origin, _) = load(&p).unwrap();
        assert!(matches!(origin, CalibrationOrigin::File { .. }));
        for size in [3, 30, 300] {
            let (_, sc) = cal.size_cal(size);
            assert!(sc.msgs_per_min[0] <= sc.msgs_per_min[1]);
        }
        for pop in ["small_new_channel", "growing_channel", "raid_moment"] {
            assert!(cal.population(pop).is_ok(), "{pop}");
        }
    }

    fn edited(f: impl FnOnce(&mut serde_json::Value)) -> Result<Calibration, SimError> {
        let mut v: serde_json::Value = serde_json::from_str(DEFAULT_CALIBRATION).unwrap();
        v["sources"] = serde_json::json!([{ "id": "c01", "title": "a source" }]);
        f(&mut v);
        Calibration::parse(&v.to_string())
    }

    #[test]
    fn a_population_naming_an_unknown_archetype_does_not_parse() {
        let e = edited(|v| v["populations"]["small_new_channel"]["trol"] = 0.1.into()).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("small_new_channel"), "{msg}");
        assert!(msg.contains("trol"), "{msg}");
    }

    #[test]
    fn a_dangling_source_id_does_not_parse() {
        let e = edited(|v| v["archetypes"]["baiter"]["source_ids"] = serde_json::json!(["c99"]))
            .unwrap_err();
        assert!(e.to_string().contains("archetypes.baiter"), "{e}");
        assert!(e.to_string().contains("c99"), "{e}");
        let e = edited(|v| v["bait_tactics"][0]["source_ids"] = serde_json::json!(["c01", "c99"]))
            .unwrap_err();
        assert!(
            e.to_string().contains("bait_tactics.spell-word-slur"),
            "{e}"
        );
        assert!(e.to_string().contains("c99"), "{e}");
        let e = edited(|v| v["audience_sizes"]["3"]["source_ids"] = serde_json::json!(["c99"]))
            .unwrap_err();
        assert!(e.to_string().contains("audience_sizes.3"), "{e}");
        // Known ids and the literal "assumption" are fine.
        edited(|v| {
            v["archetypes"]["baiter"]["source_ids"] = serde_json::json!(["c01", "assumption"])
        })
        .unwrap();
    }

    #[test]
    fn unsourced_bait_tactics_are_assumptions() {
        let cal =
            edited(|v| v["bait_tactics"][1]["source_ids"] = serde_json::json!(["c01"])).unwrap();
        let a = cal.assumptions();
        assert!(
            a.contains(&"bait_tactics.spell-word-slur".to_string()),
            "{a:?}"
        );
        assert!(
            !a.iter()
                .any(|x| x.starts_with("bait_tactics.chat-injection")),
            "{a:?}"
        );
    }

    #[test]
    fn the_shipped_calibration_has_no_dangling_reference() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sim/calibration.json");
        let text = std::fs::read_to_string(p).unwrap();
        let cal = Calibration::parse(&text).unwrap();
        assert_eq!(cal.bait_tactics.len(), 14);
    }

    #[test]
    fn missing_file_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let (_, origin, _) = load(dir.path()).unwrap();
        assert_eq!(origin, CalibrationOrigin::DefaultAssumption);
        std::fs::write(dir.path().join("calibration.json"), "{ not json").unwrap();
        assert!(load(dir.path()).is_err());
    }
}
