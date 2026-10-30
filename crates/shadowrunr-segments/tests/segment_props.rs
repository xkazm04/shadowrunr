//! Property tests for the segments crate's public API: the poll rules of [`SegmentRuntime`]
//! (driven through `parse_command`, as `ballot_vectors.rs` does), the rank ladder
//! ([`RankTable`]) and the round clock ([`RoundTimes`]). Each property names the invariant it
//! holds the code to, taken from the code and its doc comments. Inputs are generated and
//! simulated; no real chat.
//!
//! The scene rounds (`rounds::Rounds`: `choose`, `cue`, `ended`, `round`) are `pub(crate)`, so
//! no integration test can reach them; they are not covered here (see the commit message).
//!
//! Proven red on a seeded fault (each fault seeded locally in `src/`, the one test run, the
//! source restored; property -> function: fault -> observed failure):
//! - `a_viewer_counts_once_and_the_last_vote_moves_it` -> `ingest_chat`: the vote's
//!   `p.votes.insert(..)` became `entry(viewer).or_insert(..)` (first vote wins) -> the tallies
//!   differed from the last-vote model.
//! - `a_tally_is_the_sum_of_the_voters_weights` -> `Poll::tallies`: `+= w` became `+= 1.0` ->
//!   `alpha: 1 vs 1.4`.
//! - `a_vote_below_the_poll_rank_never_counts` -> `ingest_chat`: `rank < p.min_rank` became
//!   `rank > p.min_rank` -> a below-rank vote was counted, min_rank = Thrall.
//! - `equal_weight_goes_to_more_voters` -> `outcome`: `t.voters > b.voters` became `<` -> the
//!   one warlock beat the two thralls. NOT red at first in
//!   `the_outcome_is_the_same_in_any_arrival_order_and_follows_the_rule` (random ranks rarely
//!   level on weight with different voter counts), hence this focused property.
//! - `a_tie_lists_its_options_in_option_order` -> `outcome`: the tie arm `top.push(t)` became
//!   `top.insert(0, t)` -> the tied options came back reversed (n = 2).
//! - `a_rank_is_the_highest_that_matches_and_never_falls_with_more_standing` -> `rank_for`:
//!   `.rev()` dropped (lowest match) -> `Warlock below Lich by gifts`.
//! - `vote_weight_never_falls_up_the_ladder` -> `vote_weight`: `r.weight` became
//!   `3.0 - r.weight` -> `vote_weight(Thrall) <= vote_weight(Acolyte)` failed.
//! - `may_propose_agrees_with_the_rank_a_viewer_resolves_to` -> `may_propose`: `>=` became `>`
//!   -> `may_propose(r) == (r >= propose_min_rank)` failed.
//! - `a_propose_is_kept_only_from_a_rank_that_may` -> `ingest_chat`: `!may_propose(rank)`
//!   became `may_propose(rank)` -> the kept proposals differed (gate = Thrall).
//! - `overrides_replace_only_what_they_name` -> `with_overrides`: `weights.get(&r.rank)` became
//!   `weights.values().next()` -> an unnamed rank took the override (named = Thrall).
//! - `round_times_check_is_the_documented_rule` -> `RoundTimes::check`: `ceiling_s < floor_s`
//!   became `<=` -> a ceiling equal to the floor was refused (all four limits 1).
//! - `the_outcome_is_the_same_in_any_arrival_order_and_follows_the_rule`: seeded with the
//!   `outcome` fault above it stayed green; it still holds the order-independence and the
//!   no-votes / sole-winner rule, and `equal_weight_goes_to_more_voters` holds the level case.

use std::collections::BTreeMap;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use shadowrunr_segments::table::ColdOpenRule;
use shadowrunr_segments::{
    parse_command, PollOutcome, RankTable, RoundTimes, SegmentConfig, SegmentRuntime,
};
use shadowrunr_types::character::CharacterRanks;
use shadowrunr_types::{ChatMsg, Op, Platform, Rank, SegmentId, Viewer, VoteTally};

const LADDER: [Rank; 6] = [
    Rank::Thrall,
    Rank::Acolyte,
    Rank::Cultist,
    Rank::Warlock,
    Rank::Lich,
    Rank::DreadCouncil,
];
const OPTIONS: [&str; 3] = ["alpha", "beta", "gamma"];
const EPS: f64 = 1e-9;

type Ballot = (usize, usize, bool);

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 96,
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource(
            "proptest-regressions",
        ))),
        ..ProptestConfig::default()
    }
}

fn rank() -> impl Strategy<Value = Rank> {
    (0usize..6).prop_map(|i| LADDER[i])
}

/// One simulated vote: who (0..6), the option (0..3), and whether the line names the option by
/// number or by its name in capitals.
fn ballot() -> impl Strategy<Value = Ballot> {
    (0usize..6, 0usize..3, any::<bool>())
}

fn viewer(id: &str, rank: Rank) -> Viewer {
    Viewer {
        id: id.into(),
        name: id.into(),
        platform: Platform::Sim,
        rank,
        sub_tier: None,
        months: 0,
        gifted: 0,
        is_mod: false,
    }
}

fn msg(i: usize, who: &str, rank: Rank, text: &str) -> ChatMsg {
    ChatMsg {
        id: format!("m{i}"),
        ts_ms: 2_000 + 10 * i as i64,
        viewer: viewer(who, rank),
        text: text.into(),
        command: parse_command(text),
    }
}

fn past_cold_open() -> SegmentConfig {
    SegmentConfig {
        foils: vec!["the-disruptor".into(), "the-life-coach".into()],
        cold_open: Some(ColdOpenRule {
            to: "opening".into(),
            min_s: 0,
            max_s: 0,
            active_chatters: 1,
            window_s: 60,
            sustain_s: 0,
        }),
        ..Default::default()
    }
}

/// A fresh SEG-01 runtime with a crowd poll `p1` open at `min_rank`.
fn runtime_with_poll(min_rank: Rank) -> SegmentRuntime {
    let mut rt =
        SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, past_cold_open()).unwrap();
    rt.start(0);
    rt.apply_ops(
        1_000,
        &[Op::StartPoll {
            poll_id: "p1".into(),
            question: "Which way?".into(),
            options: OPTIONS.iter().map(|s| s.to_string()).collect(),
            min_rank,
            secs: 60,
        }],
    );
    assert_eq!(rt.open_poll_id(), Some("p1"));
    rt
}

fn vote_text(option: usize, by_number: bool) -> String {
    if by_number {
        format!("!vote {}", option + 1)
    } else {
        format!("!vote {}", OPTIONS[option].to_uppercase())
    }
}

/// Cast `ballots` one line at a time, in order. `ranks[v]` is viewer `v`'s rank.
fn cast(rt: &mut SegmentRuntime, ranks: &[Rank], ballots: &[Ballot]) {
    for (i, &(v, option, by_number)) in ballots.iter().enumerate() {
        let m = msg(i, &format!("v{v}"), ranks[v], &vote_text(option, by_number));
        rt.ingest_chat(m.ts_ms, &[m]);
    }
}

/// What the poll rules say the tallies are: each viewer's last vote that met `min_rank`,
/// summed in viewer-id order (the order the runtime sums in): (voters, weight) per option.
fn model(table: &RankTable, min_rank: Rank, ranks: &[Rank], ballots: &[Ballot]) -> Vec<(u32, f64)> {
    let mut last: BTreeMap<usize, usize> = BTreeMap::new();
    for &(v, option, _) in ballots {
        if ranks[v] >= min_rank {
            last.insert(v, option);
        }
    }
    let mut out = vec![(0u32, 0.0f64); OPTIONS.len()];
    for (v, option) in last {
        out[option].0 += 1;
        out[option].1 += table.vote_weight(ranks[v]);
    }
    out
}

/// The documented outcome: the highest weight wins; equal weight goes to more voters; options
/// level on both are a tie, in option order.
fn expected_outcome(tallies: &[(u32, f64)]) -> PollOutcome {
    let live: Vec<usize> = (0..tallies.len()).filter(|&i| tallies[i].0 > 0).collect();
    let Some(top_w) = live.iter().map(|&i| tallies[i].1).reduce(f64::max) else {
        return PollOutcome::NoVotes;
    };
    let level: Vec<usize> = live
        .into_iter()
        .filter(|&i| (tallies[i].1 - top_w).abs() <= EPS)
        .collect();
    let top_v = level.iter().map(|&i| tallies[i].0).max().unwrap();
    let best: Vec<usize> = level
        .into_iter()
        .filter(|&i| tallies[i].0 == top_v)
        .collect();
    match best.as_slice() {
        [one] => PollOutcome::Winner(OPTIONS[*one].into()),
        many => PollOutcome::Tie {
            options: many.iter().map(|&i| OPTIONS[i].into()).collect(),
            voters: top_v,
        },
    }
}

fn close(rt: &mut SegmentRuntime) -> PollOutcome {
    rt.apply_ops(
        9_000,
        &[Op::ClosePoll {
            poll_id: "p1".into(),
        }],
    );
    rt.last_crowd().expect("the poll closed").1.clone()
}

fn assert_tallies(got: &[VoteTally], want: &[(u32, f64)]) -> Result<(), TestCaseError> {
    prop_assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        prop_assert_eq!(g.voters, w.0, "voters on {}", &g.option);
        prop_assert!(
            (g.weight - w.1).abs() <= EPS,
            "weight {} vs {}",
            g.weight,
            w.1
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    /// A viewer's ballot counts once per poll, a re-vote moves it ("One vote per viewer; the
    /// last one counts").
    #[test]
    fn a_viewer_counts_once_and_the_last_vote_moves_it(
        ranks in prop::collection::vec(rank(), 6),
        ballots in prop::collection::vec(ballot(), 0..24),
    ) {
        let mut rt = runtime_with_poll(Rank::Thrall);
        cast(&mut rt, &ranks, &ballots);
        let want = model(RankTable::builtin(), Rank::Thrall, &ranks, &ballots);
        let distinct: std::collections::BTreeSet<usize> = ballots.iter().map(|b| b.0).collect();
        let got = rt.open_poll_tallies().unwrap();
        prop_assert_eq!(got.iter().map(|t| t.voters as usize).sum::<usize>(), distinct.len());
        assert_tallies(&got, &want)?;
    }

    /// A tally is the sum of the vote weights of the ranks that voted.
    #[test]
    fn a_tally_is_the_sum_of_the_voters_weights(
        ranks in prop::collection::vec(rank(), 6),
        ballots in prop::collection::vec(ballot(), 1..24),
    ) {
        let mut rt = runtime_with_poll(Rank::Thrall);
        cast(&mut rt, &ranks, &ballots);
        let table = RankTable::builtin();
        let last: BTreeMap<usize, usize> = ballots.iter().map(|&(v, o, _)| (v, o)).collect();
        for (o, t) in rt.open_poll_tallies().unwrap().iter().enumerate() {
            let sum: f64 = last
                .iter()
                .filter(|(_, &opt)| opt == o)
                .map(|(&v, _)| table.vote_weight(ranks[v]))
                .sum();
            prop_assert!((t.weight - sum).abs() <= EPS, "{}: {} vs {}", t.option, t.weight, sum);
        }
    }

    /// A vote under the poll's `min_rank` is declined, never counted.
    #[test]
    fn a_vote_below_the_poll_rank_never_counts(
        min_rank in rank(),
        ranks in prop::collection::vec(rank(), 6),
        ballots in prop::collection::vec(ballot(), 0..24),
    ) {
        let mut rt = runtime_with_poll(min_rank);
        cast(&mut rt, &ranks, &ballots);
        let want = model(RankTable::builtin(), min_rank, &ranks, &ballots);
        assert_tallies(&rt.open_poll_tallies().unwrap(), &want)?;
    }

    /// The same ballots from distinct viewers close the same in any arrival order, and by the
    /// documented rule (weight, then voters, then a tie).
    #[test]
    fn the_outcome_is_the_same_in_any_arrival_order_and_follows_the_rule(
        voters in prop::collection::vec((rank(), 0usize..3, any::<u32>()), 0..6),
    ) {
        let mut ranks: Vec<Rank> = voters.iter().map(|v| v.0).collect();
        ranks.resize(6, Rank::Thrall);
        let in_index: Vec<Ballot> =
            voters.iter().enumerate().map(|(i, v)| (i, v.1, false)).collect();
        let mut keyed: Vec<(u32, Ballot)> =
            voters.iter().zip(&in_index).map(|(v, b)| (v.2, *b)).collect();
        keyed.sort_by_key(|x| x.0);
        let shuffled: Vec<Ballot> = keyed.into_iter().map(|x| x.1).collect();

        let mut a = runtime_with_poll(Rank::Thrall);
        cast(&mut a, &ranks, &in_index);
        let mut b = runtime_with_poll(Rank::Thrall);
        cast(&mut b, &ranks, &shuffled);
        let (oa, ob) = (close(&mut a), close(&mut b));
        prop_assert_eq!(&oa, &ob);
        let want = expected_outcome(&model(RankTable::builtin(), Rank::Thrall, &ranks, &in_index));
        prop_assert_eq!(oa, want);
    }

    /// Equal weight goes to more voters: two thralls (1.0 + 1.0) outweigh nobody against one
    /// warlock (2.0), so the option the thralls chose wins on voters, wherever the options sit.
    #[test]
    fn equal_weight_goes_to_more_voters(thralls_on in 0usize..3, warlock_on in 0usize..3) {
        prop_assume!(thralls_on != warlock_on);
        let mut rt = runtime_with_poll(Rank::Thrall);
        let ranks = [Rank::Thrall, Rank::Thrall, Rank::Warlock, Rank::Thrall, Rank::Thrall, Rank::Thrall];
        let ballots: Vec<Ballot> = vec![(2, warlock_on, true), (0, thralls_on, true), (1, thralls_on, false)];
        cast(&mut rt, &ranks, &ballots);
        prop_assert_eq!(close(&mut rt), PollOutcome::Winner(OPTIONS[thralls_on].into()));
    }

    /// A tie names its options in option order and the voter count they share.
    #[test]
    fn a_tie_lists_its_options_in_option_order(n in 2usize..=3, r in rank()) {
        let mut rt = runtime_with_poll(Rank::Thrall);
        // One viewer of the same rank on each of the first `n` options, cast last option first.
        let ranks = vec![r; 6];
        let ballots: Vec<Ballot> = (0..n).rev().map(|o| (o, o, true)).collect();
        cast(&mut rt, &ranks, &ballots);
        let want = PollOutcome::Tie {
            options: OPTIONS[..n].iter().map(|s| s.to_string()).collect(),
            voters: 1,
        };
        prop_assert_eq!(close(&mut rt), want);
    }
}

/// Subscriber standing: tier 1..=4 (4 is no tier the ladder knows), months, gifts.
fn standing() -> impl Strategy<Value = (Option<u8>, u32, u32)> {
    (prop::option::of(1u8..=4), 0u32..80, 0u32..80)
}

proptest! {
    #![proptest_config(config())]

    /// `rank_for` is the highest rank whose conditions match, so more standing never lowers it.
    #[test]
    fn a_rank_is_the_highest_that_matches_and_never_falls_with_more_standing(
        (tier, months, gifted) in standing(),
        more_months in 0u32..40,
        more_gifts in 0u32..40,
    ) {
        let t = RankTable::builtin();
        let r = t.rank_for(tier, months, gifted);
        prop_assert!(t.rank_for(tier, months + more_months, gifted + more_gifts) >= r);
        // Highest match: gifting enough for a rank on its own is never ranked below it.
        for rule in &t.ranks {
            if rule.min_gifted.is_some_and(|min| gifted >= min) {
                prop_assert!(r >= rule.rank, "{r:?} below {:?} by gifts", rule.rank);
            }
        }
        // No subscription and no gifts is the base rank.
        prop_assert_eq!(t.rank_for(None, months, 0), Rank::Thrall);
    }

    /// Vote weights are positive and never fall up the ladder (a table that breaks it is
    /// refused at load).
    #[test]
    fn vote_weight_never_falls_up_the_ladder(a in rank(), b in rank()) {
        let t = RankTable::builtin();
        prop_assert!(t.vote_weight(a) > 0.0);
        if a <= b {
            prop_assert!(t.vote_weight(a) <= t.vote_weight(b));
        }
    }

    /// `may_propose` agrees with the rank a viewer resolves to: a viewer may propose exactly
    /// when the rank they resolve to is at or above `propose_min_rank`, under the built-in
    /// table and under a package's overridden gate.
    #[test]
    fn may_propose_agrees_with_the_rank_a_viewer_resolves_to(
        (tier, months, gifted) in standing(),
        gate in prop::option::of(rank()),
    ) {
        let o = CharacterRanks { propose_min_rank: gate, ..CharacterRanks::default() };
        let t = RankTable::builtin().with_overrides(&o).unwrap();
        let r = t.rank_for(tier, months, gifted);
        prop_assert_eq!(t.may_propose(r), r >= t.propose_min_rank);
        prop_assert_eq!(t.propose_min_rank, gate.unwrap_or(RankTable::builtin().propose_min_rank));
        // The gate is monotone: a higher rank may propose whenever a lower one may.
        for w in LADDER.windows(2) {
            prop_assert!(!t.may_propose(w[0]) || t.may_propose(w[1]));
        }
    }

    /// `!propose` through the runtime is kept only from a rank the table lets propose.
    #[test]
    fn a_propose_is_kept_only_from_a_rank_that_may(
        gate in rank(),
        ranks in prop::collection::vec(rank(), 1..6),
    ) {
        let o = CharacterRanks { propose_min_rank: Some(gate), ..CharacterRanks::default() };
        let config = SegmentConfig {
            ranks: RankTable::builtin().with_overrides(&o).unwrap(),
            ..past_cold_open()
        };
        let mut rt = SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, config).unwrap();
        rt.start(0);
        for (i, r) in ranks.iter().enumerate() {
            let m = msg(i, &format!("v{i}"), *r, "!propose a goat");
            rt.ingest_chat(m.ts_ms, &[m]);
        }
        let kept: Vec<Rank> = rt.proposals().iter().map(|p| p.rank).collect();
        let want: Vec<Rank> = ranks.iter().copied().filter(|r| *r >= gate).collect();
        prop_assert_eq!(kept, want);
    }

    /// An override replaces only what it names; the rest of a valid merged ladder is kept.
    #[test]
    fn overrides_replace_only_what_they_name(named in rank()) {
        let base = RankTable::builtin();
        // Raising the top rung keeps the ladder valid; any other rung is set to its own weight
        // plus a hair, which stays within the next rung's weight.
        let w = base.vote_weight(named) + 0.01;
        let mut o = CharacterRanks::default();
        o.weights.insert(named, w);
        let t = base.with_overrides(&o).unwrap();
        prop_assert_eq!(t.vote_weight(named), w);
        for r in LADDER.iter().filter(|r| **r != named) {
            prop_assert_eq!(t.vote_weight(*r), base.vote_weight(*r));
        }
        prop_assert_eq!(t.propose_min_rank, base.propose_min_rank);
    }

    /// `RoundTimes::check` accepts exactly a positive ballot, talk and scene limit with the
    /// ceiling at or above the floor.
    #[test]
    fn round_times_check_is_the_documented_rule(
        ballot_secs in 0u32..4,
        talk_s in 0u32..4,
        scene_max_s in 0u32..4,
        floor_s in 0u32..6,
        ceiling_s in 0u32..6,
    ) {
        let t = RoundTimes { ballot_secs, talk_s, scene_max_s, floor_s, ceiling_s, ..RoundTimes::DEFAULT };
        let ok = ballot_secs > 0 && talk_s > 0 && scene_max_s > 0 && ceiling_s >= floor_s;
        prop_assert_eq!(t.check().is_ok(), ok);
    }
}
