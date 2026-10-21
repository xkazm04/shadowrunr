//! The weighted rank ladder as a data table (`data/ranks.toml`).
//!
//! The table is loaded at runtime ([`RankTable::load`]) and falls back to a compiled-in copy
//! of the same file ([`RankTable::builtin`]): the engine default. A character package may
//! override weights, thresholds and the `!propose` gate (`[ranks]` in `character.toml`,
//! [`RankTable::with_overrides`]). Every threshold in it is a labelled assumption.

use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;
use shadowrunr_types::character::CharacterRanks;
use shadowrunr_types::Rank;

use crate::SegmentError;

/// The compiled-in default table: the exact bytes of `data/ranks.toml`.
pub const BUILTIN_RANKS_TOML: &str = include_str!("../data/ranks.toml");

/// Every rank, lowest first. The table must list exactly these, in this order.
pub const LADDER: [Rank; 6] = [
    Rank::Thrall,
    Rank::Acolyte,
    Rank::Cultist,
    Rank::Warlock,
    Rank::Lich,
    Rank::DreadCouncil,
];

/// One way to qualify for a rank through a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TierRule {
    /// Current subscription tier: 1, 2 or 3.
    pub tier: u8,
    /// Minimum cumulative months at that tier.
    #[serde(default)]
    pub min_months: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankRule {
    pub rank: Rank,
    pub weight: f64,
    /// Gifting at least this many subscriptions qualifies on its own.
    #[serde(default)]
    pub min_gifted: Option<u32>,
    #[serde(default)]
    pub tiers: Vec<TierRule>,
}

impl RankRule {
    fn matches(&self, sub_tier: Option<u8>, months: u32, gifted: u32) -> bool {
        let by_gift = self.min_gifted.is_some_and(|min| gifted >= min);
        let by_sub = sub_tier.is_some_and(|t| {
            self.tiers
                .iter()
                .any(|r| r.tier == t && months >= r.min_months)
        });
        by_gift || by_sub
    }
}

/// The rank ladder plus the `!propose` gate.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankTable {
    /// Lowest rank allowed to use `!propose`.
    pub propose_min_rank: Rank,
    pub ranks: Vec<RankRule>,
}

impl RankTable {
    /// Parse and validate a table from TOML text.
    pub fn from_toml_str(text: &str) -> Result<Self, SegmentError> {
        let table: RankTable = toml::from_str(text).map_err(|e| SegmentError::Data {
            file: "ranks.toml".into(),
            detail: e.to_string(),
        })?;
        table.validate()?;
        Ok(table)
    }

    /// Load a table from a file at runtime.
    pub fn load(path: &Path) -> Result<Self, SegmentError> {
        let text = std::fs::read_to_string(path).map_err(|e| SegmentError::Io {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
        Self::from_toml_str(&text)
    }

    /// Load from `path` when it exists, else use the compiled-in default.
    pub fn load_or_builtin(path: &Path) -> Result<Self, SegmentError> {
        if path.exists() {
            Self::load(path)
        } else {
            Ok(Self::builtin().clone())
        }
    }

    /// This table with a character package's `[ranks]` overrides applied: a weight, a
    /// threshold (gifts and tiers together) or the `!propose` gate replaces only what it
    /// names. The merged table is validated like a file.
    pub fn with_overrides(&self, o: &CharacterRanks) -> Result<RankTable, SegmentError> {
        let mut t = self.clone();
        for r in &mut t.ranks {
            if let Some(w) = o.weights.get(&r.rank) {
                r.weight = *w;
            }
            if let Some(th) = o.thresholds.get(&r.rank) {
                r.min_gifted = th.min_gifted;
                r.tiers = th
                    .tiers
                    .iter()
                    .map(|x| TierRule {
                        tier: x.tier,
                        min_months: x.min_months,
                    })
                    .collect();
            }
        }
        if let Some(p) = o.propose_min_rank {
            t.propose_min_rank = p;
        }
        t.validate().map_err(|e| match e {
            SegmentError::Data { detail, .. } => SegmentError::Data {
                file: "character.toml [ranks]".into(),
                detail,
            },
            other => other,
        })?;
        Ok(t)
    }

    /// The compiled-in default (`data/ranks.toml`).
    pub fn builtin() -> &'static RankTable {
        static TABLE: OnceLock<RankTable> = OnceLock::new();
        TABLE.get_or_init(|| {
            RankTable::from_toml_str(BUILTIN_RANKS_TOML)
                .expect("built-in data/ranks.toml is valid (checked by tests)")
        })
    }

    fn validate(&self) -> Result<(), SegmentError> {
        let bad = |detail: String| SegmentError::Data {
            file: "ranks.toml".into(),
            detail,
        };
        let listed: Vec<Rank> = self.ranks.iter().map(|r| r.rank).collect();
        if listed != LADDER {
            return Err(bad(format!(
                "ranks must list the whole ladder lowest first {LADDER:?}, got {listed:?}"
            )));
        }
        let base = &self.ranks[0];
        if base.min_gifted.is_some() || !base.tiers.is_empty() {
            return Err(bad("the base rank (thrall) must have no conditions".into()));
        }
        let mut prev = 0.0_f64;
        for r in &self.ranks {
            if !r.weight.is_finite() || r.weight <= 0.0 || r.weight < prev {
                return Err(bad(format!(
                    "weights must be positive and non-decreasing; {:?} has {}",
                    r.rank, r.weight
                )));
            }
            prev = r.weight;
            if let Some(t) = r.tiers.iter().find(|t| !(1..=3).contains(&t.tier)) {
                return Err(bad(format!("{:?}: tier {} is not 1..=3", r.rank, t.tier)));
            }
        }
        Ok(())
    }

    /// Highest rank whose conditions match. Non-subscribers without gifts are `Thrall`.
    pub fn rank_for(&self, sub_tier: Option<u8>, months: u32, gifted: u32) -> Rank {
        self.ranks
            .iter()
            .rev()
            .find(|r| r.matches(sub_tier, months, gifted))
            .map_or(Rank::Thrall, |r| r.rank)
    }

    pub fn vote_weight(&self, rank: Rank) -> f64 {
        self.ranks
            .iter()
            .find(|r| r.rank == rank)
            .map_or(1.0, |r| r.weight)
    }

    pub fn may_propose(&self, rank: Rank) -> bool {
        rank >= self.propose_min_rank
    }
}

impl Default for RankTable {
    fn default() -> Self {
        Self::builtin().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> &'static RankTable {
        RankTable::builtin()
    }

    #[test]
    fn builtin_table_parses() {
        assert_eq!(t().ranks.len(), 6);
        assert_eq!(t().propose_min_rank, Rank::Warlock);
    }

    #[test]
    fn rank_boundaries() {
        use Rank::*;
        // (tier, months, gifted) -> rank; every boundary from the brief, both sides.
        let cases: &[(Option<u8>, u32, u32, Rank)] = &[
            (None, 0, 0, Thrall),
            (None, 100, 0, Thrall),
            (None, 0, 4, Thrall),
            (None, 0, 5, Warlock),
            (None, 0, 19, Warlock),
            (None, 0, 20, Lich),
            (None, 0, 49, Lich),
            (None, 0, 50, DreadCouncil),
            // Tier 1
            (Some(1), 0, 0, Acolyte),
            (Some(1), 2, 0, Acolyte),
            (Some(1), 3, 0, Cultist),
            (Some(1), 11, 0, Cultist),
            (Some(1), 12, 0, Warlock),
            (Some(1), 120, 0, Warlock),
            (Some(1), 0, 5, Warlock),
            (Some(1), 0, 20, Lich),
            (Some(1), 0, 50, DreadCouncil),
            // Tier 2
            (Some(2), 0, 0, Cultist),
            (Some(2), 5, 0, Cultist),
            (Some(2), 6, 0, Warlock),
            (Some(2), 11, 0, Warlock),
            (Some(2), 12, 0, Lich),
            (Some(2), 120, 0, Lich),
            // Tier 3
            (Some(3), 0, 0, Warlock),
            (Some(3), 5, 0, Warlock),
            (Some(3), 6, 0, Lich),
            (Some(3), 11, 0, Lich),
            (Some(3), 12, 0, DreadCouncil),
            // Unknown tier counts as no subscription.
            (Some(4), 50, 0, Thrall),
        ];
        for &(tier, months, gifted, want) in cases {
            assert_eq!(
                t().rank_for(tier, months, gifted),
                want,
                "tier={tier:?} months={months} gifted={gifted}"
            );
        }
    }

    #[test]
    fn vote_weights_sub_linear() {
        let want = [1.0, 1.4, 1.7, 2.0, 2.2, 2.4];
        for (rank, w) in LADDER.iter().zip(want) {
            assert_eq!(t().vote_weight(*rank), w, "{rank:?}");
        }
        // Increments shrink or hold: sub-linear growth.
        let ws: Vec<f64> = LADDER.iter().map(|r| t().vote_weight(*r)).collect();
        for pair in ws.windows(3) {
            assert!(pair[2] - pair[1] <= pair[1] - pair[0] + 1e-9, "{pair:?}");
        }
    }

    #[test]
    fn propose_gate() {
        assert!(!t().may_propose(Rank::Cultist));
        assert!(t().may_propose(Rank::Warlock));
        assert!(t().may_propose(Rank::DreadCouncil));
    }

    #[test]
    fn invalid_tables_are_rejected() {
        let out_of_order = BUILTIN_RANKS_TOML.replace("rank = \"acolyte\"", "rank = \"cultist\"");
        assert!(RankTable::from_toml_str(&out_of_order).is_err());
        let decreasing = BUILTIN_RANKS_TOML.replace("weight = 2.4", "weight = 0.5");
        assert!(RankTable::from_toml_str(&decreasing).is_err());
        let unknown = format!("{BUILTIN_RANKS_TOML}\nbogus = 1\n");
        assert!(RankTable::from_toml_str(&unknown).is_err());
    }

    #[test]
    fn configurable_propose_rank() {
        let text = BUILTIN_RANKS_TOML.replace(
            "propose_min_rank = \"warlock\"",
            "propose_min_rank = \"acolyte\"",
        );
        let table = RankTable::from_toml_str(&text).unwrap();
        assert!(table.may_propose(Rank::Acolyte));
        assert!(!table.may_propose(Rank::Thrall));
    }

    #[test]
    fn load_or_builtin_falls_back() {
        let table = RankTable::load_or_builtin(Path::new("definitely/not/here.toml")).unwrap();
        assert_eq!(&table, t());
    }

    #[test]
    fn package_overrides_replace_only_what_they_name() {
        use shadowrunr_types::character::{RankThreshold, TierThreshold};
        assert_eq!(
            &t().with_overrides(&CharacterRanks::default()).unwrap(),
            t()
        );
        let mut o = CharacterRanks::default();
        o.weights.insert(Rank::Warlock, 2.1);
        o.thresholds.insert(
            Rank::Acolyte,
            RankThreshold {
                min_gifted: Some(1),
                tiers: vec![TierThreshold {
                    tier: 1,
                    min_months: 2,
                }],
            },
        );
        o.propose_min_rank = Some(Rank::Acolyte);
        let m = t().with_overrides(&o).unwrap();
        assert_eq!(m.vote_weight(Rank::Warlock), 2.1);
        assert_eq!(m.vote_weight(Rank::Lich), t().vote_weight(Rank::Lich));
        assert_eq!(m.rank_for(None, 0, 1), Rank::Acolyte);
        assert_eq!(m.rank_for(Some(1), 0, 0), Rank::Thrall);
        assert!(m.may_propose(Rank::Acolyte));
        // A merged ladder that breaks a rule is refused, naming the package.
        o.weights.insert(Rank::DreadCouncil, 0.5);
        let e = t().with_overrides(&o).unwrap_err().to_string();
        assert!(
            e.contains("character.toml [ranks]") && e.contains("non-decreasing"),
            "{e}"
        );
    }
}
