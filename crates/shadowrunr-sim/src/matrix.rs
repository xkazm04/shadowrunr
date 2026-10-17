//! Tournament matrices (`sim/matrices/*.toml`): tones x core tiers x segments x audience
//! sizes x seeds, with a run duration.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use shadowrunr_types::{Effort, ModelTier, SegmentId, SimRunSpec};

use crate::{wire, SimError};

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
        assert!(Matrix::parse("id = \"x\"\ncharacter = \"m\"\nduration_s = 1\n[axes]\ntones = []\ncore_tiers = []\nsegments = []\naudience_sizes = []\nseeds = []\n").is_err());
    }
}
