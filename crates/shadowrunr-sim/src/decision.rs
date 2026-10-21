//! Which core tier goes live: the tournament's answer as data (`results.json`
//! `tier_decisions`), decided here and only rendered by `report.md`.
//!
//! The rule is lexicographic and declared before any run: hard rules gate, coverage gates,
//! quality ranks, cost decides within the noise.
//!
//! 1. A cell with a hard-rule fail in any judged run is ineligible (`hard_fail`), counted
//!    per run, never pooled.
//! 2. A cell whose evaluated hard checks (pass + fail) are under [`MIN_EVALUATED_SHARE`] of
//!    pass + fail + not_evaluable, or that has no judged run or no quality fact, is
//!    `unmeasured`: not a pass, so it cannot win.
//! 3. The eligible rank by mean quality. Every cell within the per-run noise of the best
//!    (the standard error of the difference, from per-run quality shares) is in the tie set.
//! 4. The tie set is settled by the lowest cost per usable stream (cell cost / judged runs
//!    with no hard fail), then core p50 latency.
//!
//! Only cells judged by the group's reference judge are ranked: the judge tier that judged
//! the most cells, ties to the higher tier. A cell judged by another tier, or by more than
//! one, is a different instrument slice and goes to `beside` with its own n.
//! A winner with fewer than [`THIN_N`] judged runs is provisional. No eligible cell: no
//! winner, and `ineligible` is the list of failures to fix.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use shadowrunr_types::results::{
    BesideCell, BesideReason, CellKey, CellResult, DecidedBy, DecisionGroup, IneligibleCell,
    IneligibleReason, RankedCell, TierDecision,
};

use crate::summary::{CellResultExt, THIN_N};

/// The declared coverage bar: a cell must have evaluated at least this share of its hard
/// checks (pass + fail over pass + fail + not_evaluable) to be eligible.
pub const MIN_EVALUATED_SHARE: f64 = 0.5;

/// The group a cell is decided in.
pub fn group_of(k: &CellKey) -> DecisionGroup {
    DecisionGroup {
        instrument_hash: k.instrument_hash.clone(),
        tone: k.tone.clone(),
        segment: k.segment.clone(),
        audience_size: k.audience_size,
    }
}

fn tier_rank(t: &str) -> u8 {
    match t {
        "haiku" => 1,
        "sonnet" => 2,
        "opus" => 3,
        _ => 0,
    }
}

/// The judge tier that judged the most cells in the group; ties go to the higher tier.
pub fn reference_judge(cells: &[&CellResult]) -> Option<String> {
    let mut n: BTreeMap<&str, u32> = BTreeMap::new();
    for c in cells {
        for t in &c.judge_tiers {
            *n.entry(t.as_str()).or_default() += 1;
        }
    }
    n.into_iter()
        .max_by(|a, b| {
            a.1.cmp(&b.1)
                .then(tier_rank(a.0).cmp(&tier_rank(b.0)))
                .then(b.0.cmp(a.0))
        })
        .map(|(t, _)| t.to_string())
}

/// Evaluated hard checks over pass + fail + not_evaluable; None when there are none.
pub fn evaluated_share(c: &CellResult) -> Option<f64> {
    let (mut ev, mut all) = (0u32, 0u32);
    for t in c.rules.values() {
        ev += t.pass + t.fail;
        all += t.pass + t.fail + t.not_evaluable;
    }
    (all > 0).then(|| f64::from(ev) / f64::from(all))
}

/// Standard error of the mean of per-run values; None below two runs.
pub fn standard_error(xs: &[f64]) -> Option<f64> {
    if xs.len() < 2 {
        return None;
    }
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Some((var / n).sqrt())
}

fn runs_with_fail(c: &CellResult) -> u32 {
    // A results.json from before per-run counts: a pooled fail still means some run failed.
    c.runs_with_fail
        .unwrap_or_else(|| u32::from(c.rule_fails() > 0))
}

fn ineligible(c: &CellResult, reason: IneligibleReason) -> IneligibleCell {
    IneligibleCell {
        core_tier: c.cell.core_tier.clone(),
        reason,
        judged: c.judged,
        runs_with_fail: runs_with_fail(c),
        evaluated_share: evaluated_share(c),
        failing_rules: match reason {
            IneligibleReason::HardFail => c
                .rules
                .iter()
                .filter(|(_, t)| t.fail > 0)
                .map(|(k, _)| k.clone())
                .collect(),
            IneligibleReason::Unmeasured => Vec::new(),
        },
    }
}

fn cmp_opt<T: PartialOrd>(a: Option<T>, b: Option<T>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Decide one group. `run_quality` holds each cell's per-run quality shares (the share of a
/// run's quality facts that were true); a cell without them has no noise estimate.
pub fn decide_group(
    group: DecisionGroup,
    cells: &[&CellResult],
    run_quality: &BTreeMap<CellKey, Vec<f64>>,
) -> TierDecision {
    let reference = reference_judge(cells);
    let mut ranked = Vec::new();
    let mut ineligible_cells = Vec::new();
    let mut beside = Vec::new();
    for c in cells {
        let off_reference = match c.judge_tiers.as_slice() {
            [] => false,
            [t] => Some(t) != reference.as_ref(),
            _ => true,
        };
        if off_reference {
            beside.push(BesideCell {
                core_tier: c.cell.core_tier.clone(),
                reason: BesideReason::JudgeMismatch,
                judge_tiers: c.judge_tiers.clone(),
                judged: c.judged,
            });
            continue;
        }
        if runs_with_fail(c) > 0 {
            ineligible_cells.push(ineligible(c, IneligibleReason::HardFail));
            continue;
        }
        let quality = c.mean_quality();
        let covered = evaluated_share(c).is_some_and(|s| s >= MIN_EVALUATED_SHARE);
        let Some(mean_quality) = quality.filter(|_| c.judged > 0 && covered) else {
            ineligible_cells.push(ineligible(c, IneligibleReason::Unmeasured));
            continue;
        };
        let usable_runs = c.usable_runs.unwrap_or(c.judged);
        ranked.push(RankedCell {
            core_tier: c.cell.core_tier.clone(),
            judged: c.judged,
            usable_runs,
            mean_quality,
            quality_se: run_quality.get(&c.cell).and_then(|xs| standard_error(xs)),
            cost_per_usable_stream: (usable_runs > 0)
                .then(|| c.usage.cost_usd / f64::from(usable_runs)),
            core_p50_ms: c.core_latency.p50_ms,
        });
    }
    ranked.sort_by(|a, b| {
        b.mean_quality
            .total_cmp(&a.mean_quality)
            .then(a.core_tier.cmp(&b.core_tier))
    });

    let mut tie: Vec<&RankedCell> = match ranked.first() {
        Some(best) => ranked
            .iter()
            .filter(|c| {
                let noise = (best.quality_se.unwrap_or(0.0).powi(2)
                    + c.quality_se.unwrap_or(0.0).powi(2))
                .sqrt();
                best.mean_quality - c.mean_quality <= noise
            })
            .collect(),
        None => Vec::new(),
    };
    tie.sort_by(|a, b| {
        cmp_opt(a.cost_per_usable_stream, b.cost_per_usable_stream)
            .then(cmp_opt(a.core_p50_ms, b.core_p50_ms))
            .then(b.mean_quality.total_cmp(&a.mean_quality))
            .then(a.core_tier.cmp(&b.core_tier))
    });
    let winner = tie.first().map(|w| (*w).clone());
    let decided_by = winner.as_ref().map(|_| {
        if ranked.len() == 1 {
            DecidedBy::OnlyEligible
        } else if tie.len() > 1 {
            DecidedBy::CostPerUsableStream
        } else {
            DecidedBy::Quality
        }
    });
    TierDecision {
        group,
        reference_judge: reference,
        tie_set: tie.iter().map(|c| c.core_tier.clone()).collect(),
        provisional: winner.as_ref().is_some_and(|w| w.judged < THIN_N),
        winner: winner.map(|w| w.core_tier),
        decided_by,
        ranked,
        ineligible: ineligible_cells,
        beside,
    }
}

/// Decide every group (instrument, tone, segment, audience size) of a tournament's cells.
pub fn decide(
    cells: &[CellResult],
    run_quality: &BTreeMap<CellKey, Vec<f64>>,
) -> Vec<TierDecision> {
    let mut groups: BTreeMap<DecisionGroup, Vec<&CellResult>> = BTreeMap::new();
    for c in cells {
        groups.entry(group_of(&c.cell)).or_default().push(c);
    }
    groups
        .into_iter()
        .map(|(g, cs)| decide_group(g, &cs, run_quality))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::results::{Latency, QualityTally, RuleTally, UsageTotals};

    /// A hand-built cell: `judged` runs by `judge`, one hard rule with `fails` / `ne`
    /// not-evaluable checks (the rest pass), one quality dimension at `quality`, and
    /// `cost_per_run` dollars per run.
    fn cell(
        tier: &str,
        judge: &str,
        judged: u32,
        quality: f64,
        cost_per_run: f64,
        runs_failing: u32,
        ne: u32,
    ) -> CellResult {
        let mut rules = BTreeMap::new();
        let mut t = RuleTally {
            fail: runs_failing,
            not_evaluable: ne,
            ..Default::default()
        };
        t.pass = judged.saturating_sub(runs_failing + ne);
        rules.insert("red-line:doxxing".to_string(), t);
        let mut q = BTreeMap::new();
        q.insert(
            "humor_landed".to_string(),
            QualityTally {
                rate: Some(quality),
                n: judged,
                ..Default::default()
            },
        );
        CellResult {
            cell: CellKey {
                instrument_hash: "h".repeat(8),
                tone: "theatrical".into(),
                core_tier: tier.into(),
                segment: "SEG-01".into(),
                audience_size: 3,
            },
            runs: judged,
            judged,
            void: 0,
            failed: 0,
            judge_tiers: if judge.is_empty() {
                vec![]
            } else {
                judge.split(',').map(String::from).collect()
            },
            rules,
            quality: q,
            core_latency: Latency::default(),
            usage: UsageTotals {
                cost_usd: cost_per_run * f64::from(judged),
                ..Default::default()
            },
            usage_by_role: BTreeMap::new(),
            max_say_gap_s: None,
            runs_with_fail: Some(runs_failing),
            usable_runs: Some(judged - runs_failing),
        }
    }

    fn one(cells: &[CellResult], rq: &BTreeMap<CellKey, Vec<f64>>) -> TierDecision {
        let d = decide(cells, rq);
        assert_eq!(d.len(), 1, "one group");
        d.into_iter().next().unwrap()
    }

    /// Case 1: a quality gap inside the per-run noise is a tie; cost per usable stream
    /// decides, and the cheap tier wins.
    #[test]
    fn a_tie_within_the_noise_goes_to_the_cheaper_tier() {
        let haiku = cell("haiku", "opus", 6, 0.80, 0.75, 0, 0);
        let sonnet = cell("sonnet", "opus", 6, 0.81, 15.37, 0, 0);
        let mut rq = BTreeMap::new();
        rq.insert(haiku.cell.clone(), vec![0.6, 1.0, 0.7, 0.9, 0.8, 0.8]);
        rq.insert(sonnet.cell.clone(), vec![0.7, 0.9, 0.6, 1.0, 0.86, 0.8]);
        let d = one(&[haiku.clone(), sonnet.clone()], &rq);
        assert_eq!(d.winner.as_deref(), Some("haiku"));
        assert_eq!(d.tie_set, vec!["haiku".to_string(), "sonnet".to_string()]);
        assert_eq!(d.decided_by, Some(DecidedBy::CostPerUsableStream));
        assert!(!d.provisional);
        let h = d.ranked.iter().find(|c| c.core_tier == "haiku").unwrap();
        assert!((h.cost_per_usable_stream.unwrap() - 0.75).abs() < 1e-9);

        // Falsifier: a gap well outside the noise is earned, and quality decides.
        let mut tight = BTreeMap::new();
        tight.insert(haiku.cell.clone(), vec![0.79, 0.80, 0.81, 0.80, 0.80, 0.80]);
        tight.insert(
            sonnet.cell.clone(),
            vec![0.95, 0.96, 0.94, 0.95, 0.95, 0.95],
        );
        let mut s2 = sonnet;
        s2.quality.get_mut("humor_landed").unwrap().rate = Some(0.95);
        let d = one(&[haiku, s2], &tight);
        assert_eq!(d.winner.as_deref(), Some("sonnet"));
        assert_eq!(d.tie_set, vec!["sonnet".to_string()]);
        assert_eq!(d.decided_by, Some(DecidedBy::Quality));
    }

    /// Case 3: zero fails with 70% of the hard checks not evaluable is unmeasured, and it
    /// cannot win even with the best quality.
    #[test]
    fn an_unmeasured_cell_cannot_win() {
        let mut thin = cell("sonnet", "opus", 10, 0.99, 1.0, 0, 7);
        thin.runs = 10;
        let clean = cell("haiku", "opus", 10, 0.60, 1.0, 0, 0);
        let d = one(&[thin, clean], &BTreeMap::new());
        assert_eq!(d.winner.as_deref(), Some("haiku"));
        let i = &d.ineligible[0];
        assert_eq!(
            (i.core_tier.as_str(), i.reason),
            ("sonnet", IneligibleReason::Unmeasured)
        );
        assert!((i.evaluated_share.unwrap() - 0.30).abs() < 1e-9);
        assert_eq!(d.decided_by, Some(DecidedBy::OnlyEligible));
    }

    /// Case 4: the only eligible cell has 3 judged runs: it wins, provisionally.
    #[test]
    fn a_thin_winner_is_provisional_in_the_data() {
        let thin = cell("haiku", "opus", 3, 0.7, 1.0, 0, 0);
        let failing = cell("sonnet", "opus", 6, 0.9, 1.0, 1, 0);
        let d = one(&[thin, failing], &BTreeMap::new());
        assert_eq!(d.winner.as_deref(), Some("haiku"));
        assert!(d.provisional);
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["provisional"], serde_json::json!(true));
    }

    /// Case 5: the reference judge is the tier that judged the most cells (ties to the
    /// higher tier); a cell judged by another tier, or by two, is beside and never ranked.
    #[test]
    fn cells_judged_off_the_reference_are_beside_never_ranked() {
        let a = cell("haiku", "opus", 6, 0.70, 1.0, 0, 0);
        let b = cell("sonnet", "opus", 6, 0.75, 2.0, 0, 0);
        let c = cell("opus", "sonnet", 6, 0.99, 0.1, 0, 0);
        let d = one(&[a.clone(), b.clone(), c.clone()], &BTreeMap::new());
        assert_eq!(d.reference_judge.as_deref(), Some("opus"));
        assert_eq!(d.beside.len(), 1);
        assert_eq!(d.beside[0].core_tier, "opus");
        assert_eq!(d.beside[0].reason, BesideReason::JudgeMismatch);
        assert!(d.ranked.iter().all(|r| r.core_tier != "opus"));
        assert_eq!(d.winner.as_deref(), Some("sonnet"));

        // A cell with two judge tiers is a mixed slice: beside, whatever the reference.
        let mut mixed = b.clone();
        mixed.judge_tiers = vec!["opus".into(), "sonnet".into()];
        let d = one(&[a.clone(), mixed, c.clone()], &BTreeMap::new());
        assert!(d.beside.iter().any(|x| x.core_tier == "sonnet"));
        assert_eq!(d.winner.as_deref(), Some("haiku"));

        // One cell each: the tie goes to the higher judge tier.
        let d = one(&[b.clone(), c.clone()], &BTreeMap::new());
        assert_eq!(d.reference_judge.as_deref(), Some("opus"));
        assert_eq!(d.beside[0].core_tier, "opus");

        // A group with a single judge tier ranks every cell.
        let c1 = cell("opus", "opus", 6, 0.99, 0.1, 0, 0);
        let d = one(&[a, b, c1], &BTreeMap::new());
        assert!(d.beside.is_empty());
        assert_eq!(d.ranked.len(), 3);
    }

    /// Case 6 (decision half): every cell has a hard fail -> no winner; the failing cells
    /// and rules are listed.
    #[test]
    fn nothing_passes_nothing_goes_live() {
        let a = cell("haiku", "opus", 6, 0.9, 1.0, 2, 0);
        let b = cell("sonnet", "opus", 6, 0.9, 1.0, 1, 0);
        let d = one(&[a, b], &BTreeMap::new());
        assert_eq!(d.winner, None);
        assert_eq!(d.decided_by, None);
        assert!(d.tie_set.is_empty() && d.ranked.is_empty());
        assert_eq!(d.ineligible.len(), 2);
        for i in &d.ineligible {
            assert_eq!(i.reason, IneligibleReason::HardFail);
            assert_eq!(i.failing_rules, vec!["red-line:doxxing".to_string()]);
        }
    }

    #[test]
    fn standard_error_needs_two_runs() {
        assert_eq!(standard_error(&[0.5]), None);
        let se = standard_error(&[0.0, 1.0]).unwrap();
        assert!((se - 0.5).abs() < 1e-9);
    }
}
