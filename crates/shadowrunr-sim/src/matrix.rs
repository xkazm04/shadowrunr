//! Tournament matrices (`sim/matrices/*.toml`): tones x core tiers x segments x audience
//! sizes x seeds, with a run duration.
//!
//! WP15: `brief = "current"` runs every spec with the brief the next live stream would
//! receive (`<data>`), `brief = "<path>"` with a `StreamBrief` JSON file (relative to the
//! working directory, like every sim path). Absent: no brief, and the instrument hash is
//! what it always was.
//!
//! Reconcile opt-5: `rate_cap_exemptions = true` runs every spec with the rate-cap exemption
//! experiment on (see `shadowrunr_brain::FilterConfig`). Absent: off, same hash as before.
//!
//! `case = "<id>"` audits a staged real case from `<data>/cases/<id>/` in every SEG-02A run
//! instead of the fictional demo case (its content hash joins the instrument). Absent: the demo
//! case, same hash as before.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use shadowrunr_types::{Effort, ModelTier, SegmentId, SimRunSpec};

use crate::{wire, BriefSource, SimError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Axes {
    pub tones: Vec<String>,
    pub core_tiers: Vec<ModelTier>,
    pub segments: Vec<SegmentId>,
    pub audience_sizes: Vec<u32>,
    pub seeds: Vec<u64>,
}

fn default_tick() -> u32 {
    5
}
fn default_effort() -> Effort {
    Effort::Low
}
fn default_population() -> String {
    "growing_channel".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Matrix {
    pub id: String,
    pub character: String,
    /// Simulated stream length per run, seconds.
    pub duration_s: u32,
    #[serde(default = "default_tick")]
    pub tick_s: u32,
    #[serde(default = "default_effort")]
    pub core_effort: Effort,
    /// Max runs in flight (each run makes its own engine calls).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<usize>,
    /// Judge preference: the first tier that differs from a run's core tier judges it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub judge_tiers: Vec<ModelTier>,
    /// Featured-chatter call cadence, in ticks (1 = every tick).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub featured_every_ticks: Option<u32>,
    /// Max featured chatters per run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_featured: Option<usize>,
    /// Default population id for every size.
    #[serde(default = "default_population")]
    pub population: String,
    /// Per-size population override: `"300" = "raid-moment"`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub populations: BTreeMap<String, String>,
    /// The Lord's brief for every run: `"current"` or a path (WP15).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<String>,
    /// Reconcile opt-5 experiment for every run (`SimOptions::rate_cap_exemptions`). Absent
    /// or false: off, and the instrument hash is what it always was.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rate_cap_exemptions: bool,
    /// A staged real case for every SEG-02A run (`SimOptions::case`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case: Option<String>,
    pub axes: Axes,
}

impl Matrix {
    pub fn load(path: &Path) -> Result<Self, SimError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| SimError::Io(format!("{}: {e}", path.display())))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, SimError> {
        let m: Matrix = toml::from_str(text).map_err(|e| SimError::Spec(format!("matrix: {e}")))?;
        let a = &m.axes;
        if a.tones.is_empty()
            || a.core_tiers.is_empty()
            || a.segments.is_empty()
            || a.audience_sizes.is_empty()
            || a.seeds.is_empty()
        {
            return Err(SimError::Spec(format!(
                "matrix {}: every axis needs at least one value",
                m.id
            )));
        }
        if m.duration_s == 0 || m.tick_s == 0 {
            return Err(SimError::Spec(
                "duration_s and tick_s must be positive".into(),
            ));
        }
        if let Some(case) = &m.case {
            shadowrunr_segments::cases::check_case_id(case)
                .map_err(|e| SimError::Spec(format!("matrix {}: {e}", m.id)))?;
            if a.segments.iter().any(|s| *s != SegmentId::Seg02aHexmap) {
                return Err(SimError::Spec(format!(
                    "matrix {}: a case is audited in SEG-02A runs only",
                    m.id
                )));
            }
        }
        if !m
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(SimError::Spec(format!(
                "matrix id {:?}: use [A-Za-z0-9_-]",
                m.id
            )));
        }
        Ok(m)
    }

    /// The brief source the matrix declares (`BriefSource::None` when it declares none).
    pub fn brief_source(&self) -> BriefSource {
        self.brief
            .as_deref()
            .map(BriefSource::parse)
            .unwrap_or_default()
    }

    pub fn population_for(&self, size: u32) -> String {
        self.populations
            .get(&size.to_string())
            .cloned()
            .unwrap_or_else(|| self.population.clone())
    }

    pub fn cells(&self) -> usize {
        let a = &self.axes;
        a.tones.len() * a.core_tiers.len() * a.segments.len() * a.audience_sizes.len()
    }

    /// Every run of the matrix, in a stable order.
    pub fn specs(&self) -> Vec<SimRunSpec> {
        let a = &self.axes;
        let mut out = Vec::new();
        for tone in &a.tones {
            for tier in &a.core_tiers {
                for seg in &a.segments {
                    for size in &a.audience_sizes {
                        for seed in &a.seeds {
                            out.push(SimRunSpec {
                                run_id: run_id(tone, *tier, *seg, *size, *seed),
                                character: self.character.clone(),
                                tone_variant: tone.clone(),
                                core_tier: *tier,
                                core_effort: self.core_effort,
                                segment: *seg,
                                audience_size: *size,
                                seed: *seed,
                                duration_s: self.duration_s,
                                population: self.population_for(*size),
                            });
                        }
                    }
                }
            }
        }
        out
    }
}

/// `<tone>.<tier>.<segment>.a<size>.s<seed>`, safe as a directory name on Windows.
pub fn run_id(tone: &str, tier: ModelTier, seg: SegmentId, size: u32, seed: u64) -> String {
    let tone: String = tone
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{tone}.{}.{}.a{size}.s{seed}", wire(&tier), wire(&seg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped(name: &str) -> Matrix {
        Matrix::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../sim/matrices")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn shipped_matrices_parse() {
        let pilot = shipped("pilot.toml");
        assert!(pilot.specs().len() <= 12, "pilot stays small");
        let heavy = shipped("heavy.toml");
        assert_eq!(heavy.axes.tones.len(), 5);
        assert_eq!(
            heavy.axes.core_tiers,
            vec![ModelTier::Haiku, ModelTier::Sonnet, ModelTier::Opus]
        );
        assert_eq!(
            heavy.axes.segments,
            vec![SegmentId::Seg01ImprovisedRoom, SegmentId::Seg02aHexmap]
        );
        assert_eq!(heavy.axes.audience_sizes, vec![3, 30, 300]);
        assert_eq!(heavy.axes.seeds.len(), 5);
        assert_eq!(heavy.specs().len(), 5 * 3 * 2 * 3 * 5);
        assert_eq!(heavy.cells(), 90);
        let ids: std::collections::HashSet<_> =
            heavy.specs().into_iter().map(|s| s.run_id).collect();
        assert_eq!(ids.len(), 450, "run ids are unique");
    }

    #[test]
    fn run_ids_and_populations() {
        assert_eq!(
            run_id(
                "theatrical",
                ModelTier::Haiku,
                SegmentId::Seg02aHexmap,
                30,
                2
            ),
            "theatrical.haiku.SEG-02A.a30.s2"
        );
        assert_eq!(
            run_id(
                "a/b c",
                ModelTier::Opus,
                SegmentId::Seg01ImprovisedRoom,
                3,
                1
            ),
            "a_b_c.opus.SEG-01.a3.s1"
        );
        let m = Matrix::parse(
            r#"
id = "t"
character = "malakor"
duration_s = 60
population = "growing"
populations = { "3" = "small-new" }
[axes]
tones = ["theatrical"]
core_tiers = ["haiku"]
segments = ["SEG-01"]
audience_sizes = [3, 30]
seeds = [1]
"#,
        )
        .unwrap();
        let specs = m.specs();
        assert_eq!(specs[0].population, "small-new");
        assert_eq!(specs[1].population, "growing");
        assert_eq!(m.brief_source(), BriefSource::None);
        assert!(Matrix::parse("id = \"x\"\ncharacter = \"m\"\nduration_s = 1\n[axes]\ntones = []\ncore_tiers = []\nsegments = []\naudience_sizes = []\nseeds = []\n").is_err());
    }

    #[test]
    fn a_matrix_may_name_a_brief() {
        let base = "id = \"t\"\ncharacter = \"malakor\"\nduration_s = 60\n";
        let axes = "[axes]\ntones = [\"theatrical\"]\ncore_tiers = [\"haiku\"]\nsegments = [\"SEG-01\"]\naudience_sizes = [3]\nseeds = [1]\n";
        let current = Matrix::parse(&format!("{base}brief = \"current\"\n{axes}")).unwrap();
        assert_eq!(current.brief_source(), BriefSource::Current);
        let file = Matrix::parse(&format!("{base}brief = \"sim/briefs/a.json\"\n{axes}")).unwrap();
        assert_eq!(
            file.brief_source(),
            BriefSource::File(std::path::PathBuf::from("sim/briefs/a.json"))
        );
        // The shipped matrices declare none: their tournaments keep their hashes.
        assert_eq!(shipped("pilot.toml").brief_source(), BriefSource::None);
        assert_eq!(shipped("heavy.toml").brief_source(), BriefSource::None);
    }

    #[test]
    fn the_three_hour_case_matrix_is_one_long_real_case_run() {
        let m = shipped("case-3h.toml");
        let specs = m.specs();
        assert_eq!(specs.len(), 1, "one run");
        let s = &specs[0];
        assert_eq!(s.duration_s, 10_800);
        assert_eq!(s.tone_variant, "unhinged-roastmaster");
        assert_eq!(s.core_tier, ModelTier::Sonnet);
        assert_eq!(s.segment, SegmentId::Seg02aHexmap);
        assert_eq!(s.audience_size, 30);
        assert_eq!(s.seed, 1);
        assert_eq!(m.tick_s, 5, "2,160 ticks");
        assert_eq!(m.judge_tiers, vec![ModelTier::Opus]);
        let case = m.case.as_deref().expect("a real case");
        shadowrunr_segments::cases::check_case_id(case).unwrap();
        // No other shipped matrix names a case: their hashes are unchanged.
        for name in ["pilot.toml", "heavy.toml", "night.toml"] {
            assert!(shipped(name).case.is_none(), "{name}");
        }
        // A case is for SEG-02A runs only.
        let bad = "id = \"x\"\ncharacter = \"malakor\"\nduration_s = 60\ncase = \"ABC-1\"\n[axes]\ntones = [\"theatrical\"]\ncore_tiers = [\"haiku\"]\nsegments = [\"SEG-01\"]\naudience_sizes = [3]\nseeds = [1]\n";
        assert!(Matrix::parse(bad).is_err());
        assert!(Matrix::parse(&bad.replace("SEG-01", "SEG-03")).is_err());
        assert!(Matrix::parse(&bad.replace("SEG-01", "SEG-02A")).is_ok());
        // A Table Read matrix: its runs are SEG-03 runs.
        let seg03 = Matrix::parse(
            &bad.replace("case = \"ABC-1\"\n", "")
                .replace("[\"SEG-01\"]", "[\"SEG-03\", \"SEG-01\"]"),
        )
        .unwrap();
        let segs: Vec<SegmentId> = seg03.specs().iter().map(|s| s.segment).collect();
        assert_eq!(
            segs,
            [SegmentId::Seg03TableRead, SegmentId::Seg01ImprovisedRoom]
        );
        assert!(seg03.specs()[0].run_id.contains("SEG-03"));
        assert!(
            Matrix::parse(&bad.replace("SEG-01", "SEG-02A").replace("ABC-1", "bad id")).is_err()
        );
    }

    #[test]
    fn opt5_the_rate_cap_experiment_is_a_matrix_switch_off_by_default() {
        for name in [
            "pilot.toml",
            "heavy.toml",
            "night.toml",
            "rate-cap-exemptions-control.toml",
        ] {
            assert!(!shipped(name).rate_cap_exemptions, "{name}");
        }
        let control = shipped("rate-cap-exemptions-control.toml");
        let treatment = shipped("rate-cap-exemptions.toml");
        assert!(treatment.rate_cap_exemptions);
        assert_ne!(control.id, treatment.id, "separate tournament directories");
        // The same 12 runs in both arms, so they compare run for run.
        let ids = |m: &Matrix| m.specs().into_iter().map(|s| s.run_id).collect::<Vec<_>>();
        assert_eq!(ids(&control), ids(&treatment));
        assert_eq!(ids(&treatment).len(), 2 * 3 * 2);
        assert_eq!(treatment.axes.core_tiers, vec![ModelTier::Sonnet]);
        assert_eq!(treatment.axes.tones, vec!["theatrical".to_string()]);
        assert_eq!(treatment.axes.audience_sizes, vec![3, 30, 300]);
        assert_eq!(treatment.judge_tiers, shipped("night.toml").judge_tiers);
        // The switch reaches the run options; a matrix cannot turn it off for a caller.
        let base = crate::SimOptions::default();
        assert!(!crate::tournament::effective_options(&control, &base).rate_cap_exemptions);
        let on = crate::tournament::effective_options(&treatment, &base);
        assert!(on.rate_cap_exemptions);
        assert_eq!(on.experiments(), vec![crate::RATE_CAP_EXEMPTIONS]);
        assert!(base.experiments().is_empty());
    }
}
