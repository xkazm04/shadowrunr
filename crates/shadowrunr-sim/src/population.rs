//! The simulated audience: who is in the room, when they arrive and leave, what archetype they
//! play and what rank their (generated) subscription facts earn them.

use serde::{Deserialize, Serialize};
use shadowrunr_types::{Platform, Rank, Viewer};

use crate::calibration::Calibration;
use crate::rng::Rng;
use crate::SimError;

/// Rank derivation hook; in production `shadowrunr_segments::rank_for`.
pub type RankFn = fn(Option<u8>, u32, u32) -> Rank;

/// Behaviour family of an archetype id. Calibration ids are free-form, so the family is read
/// from the id; anything unrecognised is a generic chatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchetypeKind {
    Lurker,
    Regular,
    Newcomer,
    SubVoter,
    Baiter,
    RaiderWave,
    CopypastaWave,
    Mod,
    Other,
}

impl ArchetypeKind {
    pub fn of(id: &str) -> Self {
        let s = id.to_ascii_lowercase();
        if s.contains("lurk") {
            Self::Lurker
        } else if s.contains("raid") {
            Self::RaiderWave
        } else if s.contains("pasta") || s.contains("emote") {
            Self::CopypastaWave
        } else if s.contains("bait") || s.contains("troll") {
            Self::Baiter
        } else if s.contains("sub") || s.contains("vot") {
            Self::SubVoter
        } else if s.contains("regular") {
            Self::Regular
        } else if s.contains("new") {
            Self::Newcomer
        } else if s == "mod" || s.contains("moderator") || s.starts_with("mod") {
            Self::Mod
        } else {
            Self::Other
        }
    }

    pub fn is_wave(self) -> bool {
        matches!(self, Self::RaiderWave | Self::CopypastaWave)
    }
}

/// "Never leaves" marker for `depart_s`.
pub const STAYS: u32 = u32::MAX;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimViewer {
    pub viewer: Viewer,
    pub archetype: String,
    pub kind: ArchetypeKind,
    /// Whether this viewer ever chats on their own (lurkers mostly do not).
    pub chatty: bool,
    pub arrive_s: u32,
    pub depart_s: u32,
    /// Role-played by the `Chatter` LLM instead of the statistical generator.
    #[serde(default)]
    pub featured: bool,
}

impl SimViewer {
    pub fn present_at(&self, t_s: u32) -> bool {
        self.arrive_s <= t_s && t_s < self.depart_s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Population {
    pub viewers: Vec<SimViewer>,
    /// Population mix actually used (normalised shares).
    pub mix: Vec<(String, f64)>,
    /// Calibration audience-size row used.
    pub size_key: String,
    /// Room-level messages per minute drawn for this run from the calibration range.
    pub msgs_per_min: f64,
    pub raid_at_s: Option<u32>,
    pub copypasta_at_s: Vec<u32>,
}

impl Population {
    pub fn ccv_at(&self, t_s: u32) -> u32 {
        self.viewers.iter().filter(|v| v.present_at(t_s)).count() as u32
    }

    pub fn get(&self, id: &str) -> Option<&SimViewer> {
        self.viewers.iter().find(|v| v.viewer.id == id)
    }

    pub fn count_by_archetype(&self) -> Vec<(String, u32)> {
        let mut out: Vec<(String, u32)> = Vec::new();
        for v in &self.viewers {
            match out.iter_mut().find(|(k, _)| *k == v.archetype) {
                Some((_, n)) => *n += 1,
                None => out.push((v.archetype.clone(), 1)),
            }
        }
        out
    }
}

/// Largest-remainder apportionment: integer counts summing to exactly `n` that follow the
/// shares as closely as integers can. Ties go to the earlier entry (deterministic).
pub fn apportion(shares: &[(String, f64)], n: u32) -> Vec<(String, u32)> {
    let total: f64 = shares.iter().map(|(_, s)| s.max(0.0)).sum();
    if total <= 0.0 || shares.is_empty() {
        return Vec::new();
    }
    let quotas: Vec<f64> = shares
        .iter()
        .map(|(_, s)| s.max(0.0) / total * f64::from(n))
        .collect();
    let mut counts: Vec<u32> = quotas.iter().map(|q| q.floor() as u32).collect();
    let assigned: u32 = counts.iter().sum();
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by(|&a, &b| {
        let ra = quotas[a] - quotas[a].floor();
        let rb = quotas[b] - quotas[b].floor();
        rb.total_cmp(&ra).then(a.cmp(&b))
    });
    for &i in order.iter().take((n - assigned) as usize) {
        counts[i] += 1;
    }
    shares
        .iter()
        .zip(counts)
        .map(|((k, _), c)| (k.clone(), c))
        .collect()
}

const NAME_A: &[&str] = &[
    "Gloom", "Crypt", "Ashen", "Dusk", "Hollow", "Murk", "Rune", "Bone", "Grim", "Mire", "Soot",
    "Wraith", "Cinder", "Fog", "Gloam", "Thorn", "Vex", "Moth", "Tomb", "Rust",
];
const NAME_B: &[&str] = &[
    "Potato", "Goblin", "Ledger", "Toad", "Candle", "Badger", "Pigeon", "Kettle", "Turnip",
    "Moose", "Lantern", "Biscuit", "Otter", "Spoon", "Raven", "Beetle", "Pickle", "Walrus",
];

/// Fictional handles (`fictional: true` in spirit: no real person).
pub(crate) fn handle(rng: &mut Rng) -> String {
    let a = rng.pick(NAME_A).copied().unwrap_or("Gloom");
    let b = rng.pick(NAME_B).copied().unwrap_or("Toad");
    match rng.below(3) {
        0 => format!("{a}{b}"),
        1 => format!("{a}{b}{}", rng.range_u32(1, 99)),
        _ => format!("{}_{}", a.to_ascii_lowercase(), b.to_ascii_lowercase()),
    }
}

/// Subscription facts per family. ASSUMPTIONS (not calibrated): sub likelihood, tier split
/// 80/12/8, months log-uniform up to 36, rare gifters.
fn sub_facts(kind: ArchetypeKind, rng: &mut Rng) -> (Option<u8>, u32, u32) {
    let p_sub = match kind {
        ArchetypeKind::SubVoter => 0.85,
        ArchetypeKind::Regular => 0.5,
        ArchetypeKind::Mod => 0.6,
        ArchetypeKind::Newcomer | ArchetypeKind::RaiderWave => 0.03,
        _ => 0.15,
    };
    if !rng.chance(p_sub) {
        return (None, 0, 0);
    }
    let x = rng.f64();
    let tier = if x < 0.80 {
        1
    } else if x < 0.92 {
        2
    } else {
        3
    };
    let months = (36f64.ln() * rng.f64()).exp().floor() as u32;
    let gifted = if rng.chance(0.02) {
        rng.range_u32(20, 60)
    } else if rng.chance(0.10) {
        rng.range_u32(1, 10)
    } else {
        0
    };
    (Some(tier), months.max(1), gifted)
}

/// Generate the room for one run. Deterministic in `seed`.
pub fn generate(
    cal: &Calibration,
    population_id: &str,
    audience_size: u32,
    duration_s: u32,
    seed: u64,
    rank_fn: RankFn,
) -> Result<Population, SimError> {
    if audience_size == 0 {
        return Err(SimError::Spec("audience_size must be at least 1".into()));
    }
    let mix = cal.population(population_id)?;
    let (size_key, size_cal) = cal.size_cal(audience_size);
    let mut rng = Rng::derive(seed, "population");

    let counts = apportion(&mix, audience_size);
    let mut slots: Vec<String> = Vec::with_capacity(audience_size as usize);
    for (arch, n) in &counts {
        for _ in 0..*n {
            slots.push(arch.clone());
        }
    }
    rng.shuffle(&mut slots);

    let raid_at_s = counts
        .iter()
        .any(|(a, n)| *n > 0 && ArchetypeKind::of(a) == ArchetypeKind::RaiderWave)
        .then(|| (f64::from(duration_s) * rng.range_f64(0.35, 0.6)) as u32);
    let has_pasta = counts
        .iter()
        .any(|(a, n)| *n > 0 && ArchetypeKind::of(a) == ArchetypeKind::CopypastaWave);
    let copypasta_at_s = if has_pasta {
        let waves = if duration_s >= 900 { 2 } else { 1 };
        (0..waves)
            .map(|i| {
                let lo = f64::from(duration_s) * (0.2 + 0.4 * f64::from(i));
                (lo + f64::from(duration_s) * rng.range_f64(0.0, 0.25)) as u32
            })
            .collect()
    } else {
        Vec::new()
    };

    let small_room = audience_size <= 5;
    let mut names: Vec<String> = Vec::new();
    let mut viewers = Vec::with_capacity(slots.len());
    for (i, arch) in slots.iter().enumerate() {
        let kind = ArchetypeKind::of(arch);
        let mut name = handle(&mut rng);
        if names.contains(&name) {
            name = format!("{name}_{i}");
        }
        names.push(name.clone());
        let (sub_tier, months, gifted) = sub_facts(kind, &mut rng);
        let rank = rank_fn(sub_tier, months, gifted);
        let (arrive_s, depart_s) = match kind {
            ArchetypeKind::RaiderWave => {
                let at = raid_at_s.unwrap_or(0);
                let leave = if rng.chance(0.6) {
                    at.saturating_add(rng.range_u32(60, 300))
                } else {
                    STAYS
                };
                (at, leave)
            }
            ArchetypeKind::Mod => (0, STAYS),
            _ if small_room => (0, STAYS),
            _ => {
                let arrive = if rng.chance(0.7) {
                    0
                } else {
                    (f64::from(duration_s) * rng.range_f64(0.0, 0.4)) as u32
                };
                let depart = if rng.chance(0.15) {
                    (f64::from(duration_s) * rng.range_f64(0.5, 1.0)) as u32
                } else {
                    STAYS
                };
                (arrive, depart.max(arrive + 1))
            }
        };
        viewers.push(SimViewer {
            viewer: Viewer {
                id: format!("sim-v{:04}", i + 1),
                name,
                platform: Platform::Sim,
                rank,
                sub_tier,
                months,
                gifted,
                is_mod: kind == ArchetypeKind::Mod,
            },
            archetype: arch.clone(),
            kind,
            chatty: false,
            arrive_s,
            depart_s,
            featured: false,
        });
    }

    // Chatting share: draw exactly round(share * n) chatters, weighted by archetype rate.
    let want = ((size_cal.chatting_share * f64::from(audience_size)).round() as usize)
        .max(usize::from(size_cal.chatting_share > 0.0))
        .min(viewers.len());
    let mut weights: Vec<f64> = viewers
        .iter()
        .map(|v| {
            cal.archetype(&v.archetype)
                .map(|a| a.rate_rel)
                .unwrap_or(1.0)
                .max(0.001)
        })
        .collect();
    for _ in 0..want {
        if let Some(i) = rng.weighted(&weights) {
            viewers[i].chatty = true;
            weights[i] = 0.0;
        }
    }

    let msgs_per_min = rng.range_f64(size_cal.msgs_per_min[0], size_cal.msgs_per_min[1]);
    Ok(Population {
        viewers,
        mix,
        size_key: size_key.to_string(),
        msgs_per_min,
        raid_at_s,
        copypasta_at_s,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Local stand-in for `shadowrunr_segments::rank_for` (still `todo!()` in the skeleton).
    pub(crate) fn fake_rank(sub: Option<u8>, months: u32, gifted: u32) -> Rank {
        match (sub, months, gifted) {
            (None, _, g) if g >= 5 => Rank::Warlock,
            (None, _, _) => Rank::Thrall,
            (Some(3), m, _) if m >= 12 => Rank::DreadCouncil,
            (Some(t), m, _) if t >= 2 || m >= 3 => Rank::Cultist,
            _ => Rank::Acolyte,
        }
    }

    #[test]
    fn apportion_sums_and_follows_shares() {
        let shares = vec![
            ("a".to_string(), 0.5),
            ("b".to_string(), 0.3),
            ("c".to_string(), 0.2),
        ];
        for n in [3u32, 7, 30, 300] {
            let c = apportion(&shares, n);
            assert_eq!(c.iter().map(|(_, k)| k).sum::<u32>(), n);
            for ((_, share), (_, k)) in shares.iter().zip(&c) {
                assert!((f64::from(*k) - share * f64::from(n)).abs() < 1.0);
            }
        }
    }

    #[test]
    fn generator_respects_mix_and_size_deterministically() {
        let cal = Calibration::default_assumption();
        for (pop, size) in [
            ("small_new_channel", 3u32),
            ("growing_channel", 30),
            ("raid_moment", 300),
        ] {
            let p = generate(&cal, pop, size, 1200, 42, fake_rank).unwrap();
            assert_eq!(p.viewers.len() as u32, size, "{pop}");
            let mix = cal.population(pop).unwrap();
            let want = apportion(&mix, size);
            let have = p.count_by_archetype();
            for (arch, n) in &want {
                let got = have
                    .iter()
                    .find(|(a, _)| a == arch)
                    .map(|(_, k)| *k)
                    .unwrap_or(0);
                assert_eq!(got, *n, "{pop}/{arch}");
                assert!(
                    (f64::from(got)
                        - mix.iter().find(|(a, _)| a == arch).unwrap().1 * f64::from(size))
                    .abs()
                        < 1.0
                );
            }
            // ccv never exceeds the audience and the room is never empty at the start.
            for t in (0..1200).step_by(30) {
                assert!(p.ccv_at(t) <= size);
            }
            assert!(p.ccv_at(0) >= 1);
            // Chatting share is exact (rounded).
            let (_, sc) = cal.size_cal(size);
            let chatty = p.viewers.iter().filter(|v| v.chatty).count();
            assert_eq!(
                chatty,
                ((sc.chatting_share * f64::from(size)).round() as usize).max(1)
            );
            // Ids are unique and names are unique.
            let mut names: Vec<_> = p.viewers.iter().map(|v| v.viewer.name.clone()).collect();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), p.viewers.len());

            let again = generate(&cal, pop, size, 1200, 42, fake_rank).unwrap();
            let a: Vec<_> = p
                .viewers
                .iter()
                .map(|v| (&v.viewer.name, v.arrive_s, v.depart_s, v.chatty))
                .collect();
            let b: Vec<_> = again
                .viewers
                .iter()
                .map(|v| (&v.viewer.name, v.arrive_s, v.depart_s, v.chatty))
                .collect();
            assert_eq!(a, b, "same seed, same room");
        }
        let a = generate(&cal, "growing_channel", 30, 1200, 1, fake_rank).unwrap();
        let b = generate(&cal, "growing_channel", 30, 1200, 2, fake_rank).unwrap();
        let na: Vec<_> = a.viewers.iter().map(|v| v.viewer.name.clone()).collect();
        let nb: Vec<_> = b.viewers.iter().map(|v| v.viewer.name.clone()).collect();
        assert_ne!(na, nb, "different seed, different room");
    }

    #[test]
    fn raid_wave_arrives_together() {
        let cal = Calibration::default_assumption();
        let p = generate(&cal, "raid_moment", 300, 1200, 9, fake_rank).unwrap();
        let at = p.raid_at_s.expect("raid scheduled");
        let raiders: Vec<_> = p
            .viewers
            .iter()
            .filter(|v| v.kind == ArchetypeKind::RaiderWave)
            .collect();
        assert!(!raiders.is_empty());
        assert!(raiders.iter().all(|v| v.arrive_s == at));
        assert!(p.ccv_at(at) > p.ccv_at(at.saturating_sub(1)));
    }

    #[test]
    fn ranks_come_from_the_hook() {
        let cal = Calibration::default_assumption();
        let p = generate(&cal, "growing_channel", 300, 600, 3, fake_rank).unwrap();
        for v in &p.viewers {
            assert_eq!(
                v.viewer.rank,
                fake_rank(v.viewer.sub_tier, v.viewer.months, v.viewer.gifted)
            );
        }
        assert!(p.viewers.iter().any(|v| v.viewer.rank > Rank::Thrall));
    }

    #[test]
    fn archetype_kinds() {
        assert_eq!(ArchetypeKind::of("raider_wave"), ArchetypeKind::RaiderWave);
        assert_eq!(
            ArchetypeKind::of("copypasta_wave"),
            ArchetypeKind::CopypastaWave
        );
        assert_eq!(ArchetypeKind::of("sub_voter"), ArchetypeKind::SubVoter);
        assert_eq!(ArchetypeKind::of("mod"), ArchetypeKind::Mod);
        assert_eq!(ArchetypeKind::of("newcomer"), ArchetypeKind::Newcomer);
        assert_eq!(ArchetypeKind::of("troll"), ArchetypeKind::Baiter);
        assert_eq!(ArchetypeKind::of("lurker"), ArchetypeKind::Lurker);
        assert_eq!(ArchetypeKind::of("whatever"), ArchetypeKind::Other);
    }
}
