//! Scene rounds (SEG-03 Table Read, contract appendix "SEG-03 Table Read"): a segment whose
//! table has the [`SCENE_STATE`] plays the stream's [`TableRead`] in rounds. A round is a
//! ballot (while two or more scenes are left), the scene, then free talk; the last scene plays
//! without a ballot. The runtime moves between the round states itself, a clock rule like the
//! cold open's, so a table that plays scenes names the five states and the rules below run
//! them; no code names a segment.
//!
//! - [`INTRO_STATE`]: the host brings the guest on. After `intro_s` the first round starts.
//! - [`BALLOT_STATE`]: the runtime's own poll over the scenes left (their titles, in ballot
//!   order), emitted as an ordinary `start_poll` / `vote` / closed `vote`. A tie is never
//!   settled silently: the first tied scene in ballot order plays, and the state summary tells
//!   the host it is his tie to break, naming that scene so his words match.
//! - [`SCENE_STATE`]: the brain performs the scene up (the state summary's [`SCENE_UP_PREFIX`]
//!   line) and reports its `scene_end`; [`crate::SegmentRuntime::scene_ended`] moves on. A scene
//!   nobody performs ends itself after `scene_max_s`.
//! - [`TALK_STATE`]: free talk about the scene for `talk_s`; the last one lasts until the floor.
//! - [`CLOSING_STATE`]: the host wraps up and signs off.
//!
//! The floor (`floor_s`) keeps the segment from being abandoned early: `switch_segment` is
//! refused before it (`segment_floor`), and so is `sign_off`, unless the host says the stream
//! itself is at its planned end ([`crate::SegmentRuntime::set_stream_ending`], the goodbye
//! window): a stream scheduled to end before the floor still signs off on time
//! (sbx-20261024-194036-10m: SEG-03 began at 04:03 of a 10-minute show, and three sign-offs in
//! the goodbye were refused). At the ceiling the round in progress ends after its scene and
//! the segment closes.
//!
//! The timings are ASSUMPTIONS, uncalibrated, and the config may override them
//! ([`RoundTimes`]). Free talk is 5 minutes, the middle of the format's 4-6: three rounds of a
//! 1-minute ballot, a 2-4 minute scene and 5 minutes of talk, after a 90-second intro, come to
//! about 26-32 minutes, inside the 20-minute floor and the 40-minute ceiling with room for a
//! slow scene.

use shadowrunr_types::scene::Scene;
use shadowrunr_types::TsMs;

use crate::runtime::PollOutcome;
use crate::scenes::TableRead;

/// The host brings the guest on.
pub const INTRO_STATE: &str = "intro";
/// The audience votes on the next scene.
pub const BALLOT_STATE: &str = "ballot";
/// A scene is being performed.
pub const SCENE_STATE: &str = "scene";
/// Free talk after a scene.
pub const TALK_STATE: &str = "talk";
/// The wrap-up and the sign-off.
pub const CLOSING_STATE: &str = "closing";
/// Every state a table that plays scenes must have.
pub const ROUND_STATES: [&str; 5] = [
    INTRO_STATE,
    BALLOT_STATE,
    SCENE_STATE,
    TALK_STATE,
    CLOSING_STATE,
];

/// The state summary line that names the scene to perform: `SCENE UP: <id> "<title>" ...`.
/// The brain reads the id after it (`shadowrunr_brain::scene`).
pub const SCENE_UP_PREFIX: &str = "SCENE UP: ";

/// The output record of a scene the runtime ended itself (nobody performed it).
pub const SCENE_TIMEOUT_RULE: &str = "scene_timeout";

/// The round clock. Every value is an ASSUMPTION (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundTimes {
    /// The intro before the first round.
    pub intro_s: u32,
    /// How long a scene ballot stays open.
    pub ballot_secs: u32,
    /// Free talk after each scene.
    pub talk_s: u32,
    /// `switch_segment` is refused before this, and `sign_off` too unless the stream is
    /// ending; the last talk lasts until it.
    pub floor_s: u32,
    /// From here no new round starts: the segment closes after the scene in progress.
    pub ceiling_s: u32,
    /// A scene still up this long after its cue is ended by the runtime.
    pub scene_max_s: u32,
}

impl RoundTimes {
    pub const DEFAULT: RoundTimes = RoundTimes {
        intro_s: 90,
        ballot_secs: 60,
        talk_s: 300,
        floor_s: 1_200,
        ceiling_s: 2_400,
        scene_max_s: 600,
    };

    /// Consistent numbers: a positive ballot, talk and scene limit, and a ceiling at or above
    /// the floor.
    pub fn check(&self) -> Result<(), String> {
        if self.ballot_secs == 0
            || self.talk_s == 0
            || self.scene_max_s == 0
            || self.ceiling_s < self.floor_s
        {
            return Err(
                "rounds: a positive ballot, talk and scene limit, and a ceiling at or above the \
                 floor"
                    .into(),
            );
        }
        Ok(())
    }
}

impl Default for RoundTimes {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How the scene up was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cue {
    /// The last scene left: no ballot.
    Unvoted,
    /// A ballot chose it (or tied, or drew no votes: the first in ballot order then).
    Ballot {
        poll_id: String,
        outcome: PollOutcome,
    },
}

/// The rounds of one Table Read.
#[derive(Debug, Clone)]
pub(crate) struct Rounds {
    pub guest: String,
    pub scenes: Vec<Scene>,
    /// Indices into `scenes` not played yet, in ballot order.
    pub left: Vec<usize>,
    /// The scene up (cued or being performed), when it was cued and how it was chosen.
    pub up: Option<(usize, TsMs, Cue)>,
    /// Played scenes, in order.
    pub played: Vec<usize>,
    /// When the current free talk began.
    pub talk_since: Option<TsMs>,
    /// Scene ballots opened.
    pub ballots: u32,
    /// The last ballot's id, outcome and the scene it put up.
    pub last_ballot: Option<(String, PollOutcome, usize)>,
    pub times: RoundTimes,
}

impl Rounds {
    pub fn new(tr: &TableRead, times: RoundTimes) -> Self {
        Self {
            guest: tr.guest.clone(),
            left: (0..tr.scenes.len()).collect(),
            scenes: tr.scenes.clone(),
            up: None,
            played: Vec::new(),
            talk_since: None,
            ballots: 0,
            last_ballot: None,
            times,
        }
    }

    pub fn title(&self, i: usize) -> &str {
        &self.scenes[i].title
    }

    /// The titles of the scenes left, in ballot order (a ballot's options).
    pub fn left_titles(&self) -> Vec<String> {
        self.left
            .iter()
            .map(|&i| self.scenes[i].title.clone())
            .collect()
    }

    /// The scene a closed ballot puts up: the winner; on a tie the tied scene first in ballot
    /// order; with no votes the first scene left.
    pub fn choose(&self, outcome: &PollOutcome) -> Option<usize> {
        let by_title = |t: &str| {
            self.left
                .iter()
                .copied()
                .find(|&i| self.scenes[i].title == t)
        };
        match outcome {
            PollOutcome::Winner(w) => by_title(w).or_else(|| self.left.first().copied()),
            PollOutcome::Tie { options, .. } => self
                .left
                .iter()
                .copied()
                .find(|&i| options.iter().any(|o| o == &self.scenes[i].title)),
            PollOutcome::NoVotes => self.left.first().copied(),
        }
    }

    pub fn cue(&mut self, i: usize, now_ms: TsMs, how: Cue) {
        self.up = Some((i, now_ms, how));
        self.talk_since = None;
    }

    /// The scene up ended: it leaves the ballot and free talk starts. False when `scene_id`
    /// is not the scene up.
    pub fn ended(&mut self, scene_id: &str, now_ms: TsMs) -> bool {
        let Some((i, _, _)) = &self.up else {
            return false;
        };
        let i = *i;
        if self.scenes[i].id != scene_id {
            return false;
        }
        self.up = None;
        self.left.retain(|&k| k != i);
        self.played.push(i);
        self.talk_since = Some(now_ms);
        true
    }

    /// The round in progress (1-based) and the number of rounds.
    pub fn round(&self) -> (usize, usize) {
        let open = usize::from(self.up.is_some());
        (
            (self.played.len() + open).clamp(1, self.scenes.len()),
            self.scenes.len(),
        )
    }

    /// `"A", "B"` (or `none`).
    pub fn quoted(&self, idx: &[usize]) -> String {
        if idx.is_empty() {
            return "none".into();
        }
        idx.iter()
            .map(|&i| format!("\"{}\"", self.scenes[i].title))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// A foil id as a name: `the-painter` -> `the painter`.
pub fn foil_name(id: &str) -> String {
    id.replace(['-', '_'], " ")
}

/// Property tests for the scene rounds. Each property names the invariant it holds the code
/// to, taken from the code and its doc comments. Inputs are generated and simulated (made-up
/// scene titles); no real chat. `Rounds` is `pub(crate)`, so these live in the crate, which is
/// why `tests/segment_props.rs` does not cover it.
///
/// Reshaped from the brief, because the code does not promise it: `cue` does NOT take the
/// scene out of `left` (`ended` does: "it leaves the ballot"), so the property is that `up`
/// names the scene and `left` is untouched, and the accounting is `played` plus `left`, the
/// scene up still being in `left`. `choose` on a tie whose options name no scene left returns
/// `None`; the runtime only ever offers titles from `left`, so that arm is generated from
/// titles of the scenes (left or played), and only the "never outside `left`" half is held
/// for it.
///
/// Proven red on a seeded fault (each fault seeded locally in non-test code, the one test run,
/// the source restored; property -> function: fault -> observed failure):
/// - `choose_never_returns_a_scene_outside_left` -> `choose`: the `by_title` search ran over
///   `0..self.scenes.len()` instead of `self.left` -> `Winner("Scene 0") chose 0, left []` (a
///   played scene put up again).
/// - `choose_follows_the_documented_rule` -> `choose`: `.rev()` added before the tie arm's
///   `find` -> the last tied scene came back, not the first in ballot order.
/// - `cue_puts_the_scene_up_and_leaves_the_ballot_to_ended` -> `cue`: `up` took the last scene
///   left instead of `i` -> the test panicked (`up` named another scene, `ended` refused the
///   cued id).
/// - `ended_accepts_only_the_scene_up_and_only_once` -> `ended`: the `scene_id` comparison
///   dropped -> `id 0 ended while 1 is up`.
/// - `ended_accepts_only_the_scene_up_and_only_once` -> `ended`: `self.played.push(i)` doubled
///   -> the scene was in `played` twice (the exactly-once count failed).
/// - `played_plus_left_accounts_for_every_scene_once` -> `ended`: `self.left.retain(..)`
///   dropped -> `played [0, 0, 0, 0, 0] left [0]`.
/// - `the_round_counter_names_the_round_in_progress` -> `round`: `open` forced to 0 ->
///   `round 1/2, played 1 up Some((1, ..))`.
/// - NOT red: the upper bound of `round`'s `clamp(1, scenes.len())`. With `.max(1)` instead,
///   every property stayed green: `played` plus the one scene up never exceeds the scene count
///   (the one up is still unplayed), so the bound is unreachable through `cue` / `ended`; the
///   exact model in `the_round_counter_names_the_round_in_progress` holds the counter within
///   `1..=total` anyway.
#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;
    use shadowrunr_types::scene::SceneStatus;

    fn config() -> ProptestConfig {
        ProptestConfig {
            cases: 96,
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    /// A table read of `n` simulated scenes with distinct ids and titles.
    fn table(n: usize) -> TableRead {
        TableRead {
            guest: "simulated-guest".into(),
            scenes: (0..n)
                .map(|k| Scene {
                    id: format!("scene-{k}"),
                    title: format!("Scene {k}"),
                    pitch: format!("A simulated pitch {k}."),
                    guest: "simulated-guest".into(),
                    status: SceneStatus::Approved,
                    notes: Vec::new(),
                    lines: Vec::new(),
                    fictional: true,
                })
                .collect(),
        }
    }

    fn times() -> impl Strategy<Value = RoundTimes> {
        (
            1u32..600,
            1u32..600,
            1u32..600,
            0u32..3_000,
            0u32..3_000,
            1u32..900,
        )
            .prop_map(
                |(intro_s, ballot_secs, talk_s, floor_s, extra, scene_max_s)| RoundTimes {
                    intro_s,
                    ballot_secs,
                    talk_s,
                    floor_s,
                    ceiling_s: floor_s + extra,
                    scene_max_s,
                },
            )
    }

    /// A simulated rounds state: 1-5 scenes (a table read holds 1-3; more is a wider net).
    fn rounds() -> impl Strategy<Value = Rounds> {
        (1usize..=5, prop_oneof![Just(RoundTimes::DEFAULT), times()])
            .prop_map(|(n, t)| Rounds::new(&table(n), t))
    }

    /// A ballot outcome over the titles of `n` scenes (the winner may also be an unknown
    /// title), tied options being a non-empty list of the known titles.
    fn outcome(n: usize) -> impl Strategy<Value = PollOutcome> {
        prop_oneof![
            Just(PollOutcome::NoVotes),
            (0..=n).prop_map(|k| PollOutcome::Winner(format!("Scene {k}"))),
            (proptest::collection::vec(0..n, 1..=n), 1u32..20).prop_map(|(ix, voters)| {
                PollOutcome::Tie {
                    options: ix.iter().map(|k| format!("Scene {k}")).collect(),
                    voters,
                }
            }),
        ]
    }

    /// A rounds state and enough outcomes to play every scene.
    fn game() -> impl Strategy<Value = (Rounds, Vec<PollOutcome>)> {
        rounds().prop_flat_map(|r| {
            let n = r.scenes.len();
            (Just(r), proptest::collection::vec(outcome(n), 5))
        })
    }

    fn ballot() -> Cue {
        Cue::Ballot {
            poll_id: "simulated-poll".into(),
            outcome: PollOutcome::NoVotes,
        }
    }

    /// Plays the rounds out (ballot, cue, end), calling `each` on the state after every step.
    fn play(r: &mut Rounds, outcomes: &[PollOutcome], mut each: impl FnMut(&Rounds)) {
        each(r);
        for (step, o) in outcomes.iter().enumerate() {
            if r.left.is_empty() {
                break;
            }
            // A tie over only played scenes is never offered; fall back to the first scene left.
            let i = r
                .choose(o)
                .or_else(|| r.left.first().copied())
                .expect("scenes are left");
            let now = step as TsMs * 1_000;
            r.cue(i, now, ballot());
            each(r);
            let id = r.scenes[i].id.clone();
            assert!(r.ended(&id, now + 500));
            each(r);
        }
    }

    proptest! {
        #![proptest_config(config())]

        /// `choose` never returns a scene that is not in `left`, whatever the outcome names
        /// (a played scene, an unknown title, a tie over played scenes) and however many
        /// scenes were played; it is `None` only when no scene is left or a tie names none
        /// left.
        #[test]
        fn choose_never_returns_a_scene_outside_left((mut r, outcomes) in game(), probe in 0usize..5) {
            let n = r.scenes.len();
            let probes = [
                PollOutcome::NoVotes,
                PollOutcome::Winner(format!("Scene {probe}")),
                PollOutcome::Winner("not a scene".into()),
                PollOutcome::Tie { options: (0..n).map(|k| format!("Scene {k}")).collect(), voters: 1 },
            ];
            let mut bad = None;
            play(&mut r, &outcomes, |r| {
                for p in &probes {
                    match r.choose(p) {
                        Some(i) if !r.left.contains(&i) => {
                            bad = Some(format!("{p:?} chose {i}, left {:?}", r.left));
                        }
                        None if !r.left.is_empty() && !matches!(p, PollOutcome::Tie { .. }) => {
                            bad = Some(format!("{p:?} chose none, left {:?}", r.left));
                        }
                        _ => {}
                    }
                }
            });
            prop_assert!(bad.is_none(), "{:?}", bad);
        }

        /// The documented rule: the winner when it is left, else the first scene left; a tie
        /// puts up the tied scene first in ballot order; no votes the first scene left.
        #[test]
        fn choose_follows_the_documented_rule((mut r, outcomes) in game(), o in 0usize..5) {
            let n = r.scenes.len();
            // Play a prefix so `left` is a proper, ordered subset.
            play(&mut r, &outcomes[..o % n], |_| {});
            let first = r.left.first().copied();
            prop_assert_eq!(r.choose(&PollOutcome::NoVotes), first);
            for &i in &r.left {
                let title = r.scenes[i].title.clone();
                prop_assert_eq!(r.choose(&PollOutcome::Winner(title)), Some(i));
            }
            prop_assert_eq!(r.choose(&PollOutcome::Winner("not a scene".into())), first);
            // Every non-empty subset of the titles left, offered in reverse order.
            let titles = r.left_titles();
            for mask in 1u32..(1 << titles.len()) {
                let mut options: Vec<String> = titles
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| mask >> k & 1 == 1)
                    .map(|(_, t)| t.clone())
                    .collect();
                options.reverse();
                let tie = PollOutcome::Tie { options: options.clone(), voters: 1 };
                let want = r.left.iter().copied().find(|&i| options.contains(&r.scenes[i].title));
                prop_assert_eq!(r.choose(&tie), want);
            }
        }

        /// After `cue(i, ..)` the scene up is `i` with the cue time and how, free talk is not
        /// running, and `left` is untouched: the scene leaves the ballot only when it ends.
        #[test]
        fn cue_puts_the_scene_up_and_leaves_the_ballot_to_ended(
            (mut r, outcomes) in game(), pick in 0usize..5, now in 0i64..1_000_000,
        ) {
            play(&mut r, &outcomes[..pick % 3], |_| {});
            if r.left.is_empty() {
                return Ok(());
            }
            r.talk_since = Some(7);
            let i = r.left[pick % r.left.len()];
            let (left, played) = (r.left.clone(), r.played.clone());
            r.cue(i, now, Cue::Unvoted);
            prop_assert_eq!(r.up.clone(), Some((i, now, Cue::Unvoted)));
            prop_assert_eq!(r.talk_since, None);
            prop_assert_eq!(&r.left, &left);
            prop_assert_eq!(&r.played, &played);
        }

        /// `ended` is true only for the scene up (false with none up, false for any other
        /// id, state untouched), and moves that scene to `played` exactly once: a second call
        /// with the same id is false and changes nothing.
        #[test]
        fn ended_accepts_only_the_scene_up_and_only_once(
            (mut r, outcomes) in game(), pick in 0usize..5, now in 0i64..1_000_000,
        ) {
            play(&mut r, &outcomes[..pick % 3], |_| {});
            if r.left.is_empty() {
                return Ok(());
            }
            let mut ids: Vec<String> = r.scenes.iter().map(|s| s.id.clone()).collect();
            ids.push(String::new());
            ids.push("unknown".into());
            // Nothing up: no id ends anything.
            let before = (r.left.clone(), r.played.clone(), r.talk_since);
            for id in &ids {
                prop_assert!(!r.ended(id, now));
            }
            prop_assert_eq!(&(r.left.clone(), r.played.clone(), r.talk_since), &before);

            let i = r.left[pick % r.left.len()];
            r.cue(i, 1, Cue::Unvoted);
            for (k, id) in ids.iter().enumerate().filter(|(k, _)| *k != i) {
                prop_assert!(!r.ended(id, now), "id {k} ended while {i} is up");
            }
            prop_assert_eq!(r.up.as_ref().map(|u| u.0), Some(i));
            prop_assert_eq!(&r.played, &before.1);

            prop_assert!(r.ended(&ids[i], now));
            prop_assert!(r.up.is_none());
            prop_assert_eq!(r.talk_since, Some(now));
            prop_assert_eq!(r.played.iter().filter(|&&k| k == i).count(), 1);
            prop_assert!(!r.left.contains(&i));
            let after = (r.left.clone(), r.played.clone());
            prop_assert!(!r.ended(&ids[i], now + 1));
            prop_assert_eq!(&(r.left.clone(), r.played.clone()), &after);
            prop_assert_eq!(r.talk_since, Some(now));
        }

        /// Through any play-out, `played` plus `left` name every scene exactly once (the
        /// scene up is still in `left`), `up` is always a scene left, and when all are played
        /// nothing is left.
        #[test]
        fn played_plus_left_accounts_for_every_scene_once((mut r, outcomes) in game()) {
            let n = r.scenes.len();
            let mut bad = None;
            play(&mut r, &outcomes, |r| {
                let mut all: Vec<usize> = r.played.iter().chain(&r.left).copied().collect();
                all.sort_unstable();
                if all != (0..n).collect::<Vec<_>>() {
                    bad = Some(format!("played {:?} left {:?}", r.played, r.left));
                }
                if let Some((i, _, _)) = &r.up {
                    if !r.left.contains(i) {
                        bad = Some(format!("up {i} not in left {:?}", r.left));
                    }
                }
            });
            prop_assert!(bad.is_none(), "{:?}", bad);
            prop_assert!(r.left.is_empty() && r.played.len() == n);
        }

        /// `round()` reports the scene count as the total, and the round in progress: the
        /// number played plus the one up, at least 1, never past the total.
        #[test]
        fn the_round_counter_names_the_round_in_progress((mut r, outcomes) in game()) {
            let n = r.scenes.len();
            let mut bad = None;
            play(&mut r, &outcomes, |r| {
                let (round, total) = r.round();
                let want = if r.up.is_some() { r.played.len() + 1 } else { r.played.len().max(1) };
                if total != n || round != want || !(1..=total).contains(&round) {
                    bad = Some(format!("round {round}/{total}, played {} up {:?}", r.played.len(), r.up));
                }
            });
            prop_assert!(bad.is_none(), "{:?}", bad);
        }
    }
}
