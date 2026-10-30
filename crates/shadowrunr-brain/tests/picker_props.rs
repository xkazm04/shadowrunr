//! Property tests for the chat picker's public API ([`shadowrunr_brain::picker`]). Each property
//! names the invariant it holds the code to, taken from the code and its doc comments. Every
//! message here is generated and simulated; no real chat.
//!
//! Proven red on a seeded fault (each fault seeded locally in `src/picker.rs`, the one test run,
//! the source restored; property -> function: fault -> observed failure):
//! - `equal_pickers_fed_equal_beats_pick_alike` -> `Picker::ranked`: the ranking reversed on
//!   every other call (a static counter; the clock was too coarse on Windows to differ) -> the
//!   two pickers' ids differed.
//! - `picks_are_distinct_candidates` -> `constrain`: the `chosen.contains` and viewer skips of
//!   the score loop dropped -> a message picked twice.
//! - `no_candidates_pick_nothing` -> `Picker::owed`: `filter_map(position)` became
//!   `map(position.unwrap_or(0))` -> an index-out-of-bounds panic at `picker.rs:430` on an
//!   owed id with no candidates.
//! - `every_safety_candidate_is_picked_first_in_inbox_order` -> `constrain`: the safety loop ran
//!   `.rev()` -> the safety ids were out of inbox order.
//! - `the_beat_cap_bounds_every_non_safety_pick` -> `constrain`: `chosen.len() >= max` became
//!   `> max` in the score loop -> `1 > 0`.
//! - `a_vote_is_never_picked` -> `constrain`: `is_vote(c)` dropped from the score-loop skip ->
//!   a non-safety vote was picked.
//! - `one_pick_per_viewer_per_beat` -> `constrain`: the `viewers.contains` skip dropped from the
//!   score loop -> a viewer picked twice.
//! - `at_most_one_injection_per_beat` -> `constrain`: the `injection && injection_taken` skip
//!   dropped from the score loop -> two injections in a beat.
//! - `the_raid_greeting_is_picked_when_it_can_be` -> `constrain`: the greeting's slot test
//!   `len < max` became `len + 1 < max` -> the greeting was not in the first free slot.
//! - `the_oldest_owed_request_goes_first` -> `constrain`: `for &i in owed` became
//!   `owed.iter().rev()` -> the last owed request went first. NOT red at first: with
//!   `OWED_PER_BEAT` = 1 only a beat with two real owed candidates tells, and the generated
//!   room rarely had them, so the property now draws its owed ids from the candidates.
//! - `pick_ids_are_the_chosen_ids` -> `Pick::ids`: `.rev()` added -> ids out of pick order.
//! - `a_question_mark_makes_a_question` -> `is_question`: `contains('?')` became
//!   `ends_with('?')` -> head = "", tail = " " was no question.
//! - `an_asking_first_word_makes_a_question` -> `is_question`: first word became last word ->
//!   lead = "", word = "who", rest = " a" was no question.
//! - `a_name_in_the_text_is_a_mention` -> `mentions`: the `!n.is_empty()` guard dropped ->
//!   `!mentions(&text, &[String::new()])` failed.
//! - `a_line_without_letters_or_digits_is_a_bare_emote` -> `is_bare_emote`: the empty-line
//!   `return true` became `false` -> text = "" was no bare emote.
//! - `up_to_three_emote_words_are_a_bare_emote` -> `is_bare_emote`: `words.len() <= 3` became
//!   `<= 2` -> three emote words were content.
//! - `content_is_never_a_bare_emote` -> `is_bare_emote`: `all` became `any` -> an emote run
//!   with one content word was a bare emote.
//! - `is_vote_is_exactly_a_vote_command` -> `is_vote`: the `Vote` match became
//!   `command.is_some()` -> a non-vote command counted as a vote.

use std::collections::{HashMap, HashSet};

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use shadowrunr_brain::moderation::Candidate;
use shadowrunr_brain::picker::{
    is_bare_emote, is_question, is_vote, mentions, PickContext, Picker, PickerConfig,
};
use shadowrunr_types::chat::{ChatMsg, Command, Platform, Rank, Viewer};

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource(
            "proptest-regressions",
        ))),
        ..ProptestConfig::default()
    }
}

const VIEWERS: &[&str] = &["sim-ash", "sim-bly", "sim-cor", "sim-dun", "sim-eft"];

/// Simulated chat lines: questions, a mention, topic and idea talk, emotes, plain remarks.
const TEXTS: &[&str] = &[
    "what is the hex doing tonight?",
    "malakor you fraud",
    "the ledger is cooked",
    "what if the ledger burns",
    "we should summon the foil",
    "LUL",
    "W W",
    "gg",
    "nice",
    "ok",
    "why is the castle cold",
    "the castle ledger again",
];

fn rank() -> impl Strategy<Value = Rank> {
    prop_oneof![
        Just(Rank::Thrall),
        Just(Rank::Acolyte),
        Just(Rank::Cultist),
        Just(Rank::Warlock),
        Just(Rank::Lich),
        Just(Rank::DreadCouncil),
    ]
}

fn command() -> impl Strategy<Value = Option<Command>> {
    prop_oneof![
        6 => Just(None),
        1 => Just(Some(Command::Vote { option: "ledger".into() })),
        1 => Just(Some(Command::Propose { text: "burn the ledger".into() })),
        1 => Just(Some(Command::Summon { foil: "sim-foil".into() })),
    ]
}

/// One candidate before its id is assigned: viewer index, rank, text, command, injection,
/// safety, cluster count, timestamp.
type Spec = (usize, Rank, String, Option<Command>, bool, bool, u32, i64);

fn spec() -> impl Strategy<Value = Spec> {
    (
        0..VIEWERS.len(),
        rank(),
        prop_oneof![
            3 => prop::sample::select(TEXTS).prop_map(String::from),
            1 => "[a-zA-Z ?!]{0,30}",
        ],
        command(),
        prop::bool::weighted(0.15),
        prop::bool::weighted(0.1),
        1u32..5,
        0i64..100_000,
    )
}

/// Candidates with unique ids `m0`, `m1`, ... in inbox order.
fn candidates(max: usize) -> impl Strategy<Value = Vec<Candidate>> {
    prop::collection::vec(spec(), 0..max).prop_map(|specs| {
        specs
            .into_iter()
            .enumerate()
            .map(
                |(i, (v, rank, text, command, injection, safety, count, ts))| Candidate {
                    msg: ChatMsg {
                        id: format!("m{i}"),
                        ts_ms: ts,
                        viewer: Viewer {
                            id: VIEWERS[v].into(),
                            name: VIEWERS[v].into(),
                            platform: Platform::Sim,
                            rank,
                            sub_tier: None,
                            months: 0,
                            gifted: 0,
                            is_mod: false,
                        },
                        text,
                        command,
                    },
                    count,
                    viewers: 1,
                    injection,
                    safety: safety.then(|| "sim-safety-rule".to_string()),
                },
            )
            .collect()
    })
}

/// The room, as plain data (a [`PickContext`] is not `Clone`): message ids are drawn as indices
/// and resolved against the beat's candidates by [`room`].
#[derive(Debug, Clone)]
struct Room {
    regulars: Vec<usize>,
    first_contacts: Vec<usize>,
    raid: bool,
    raid_wave: Option<usize>,
    carried: Vec<(usize, i64)>,
    owed: Vec<usize>,
    greet: Option<usize>,
    topic: bool,
    poll: bool,
}

fn room_spec() -> impl Strategy<Value = Room> {
    let idx = 0usize..16;
    (
        prop::collection::vec(0..VIEWERS.len(), 0..3),
        prop::collection::vec(idx.clone(), 0..4),
        any::<bool>(),
        prop::option::of(idx.clone()),
        prop::collection::vec((idx.clone(), 0i64..120_000), 0..4),
        prop::collection::vec(idx.clone(), 0..3),
        prop::option::of(idx),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(regulars, first_contacts, raid, raid_wave, carried, owed, greet, topic, poll)| Room {
                regulars,
                first_contacts,
                raid,
                raid_wave,
                carried,
                owed,
                greet,
                topic,
                poll,
            },
        )
}

fn id(i: usize) -> String {
    format!("m{i}")
}

/// A fresh [`PickContext`] for a [`Room`]; ids past the candidate list name nobody, as an id
/// the orchestrator still holds after its message left would.
fn room(r: &Room) -> PickContext {
    PickContext {
        regulars: r.regulars.iter().map(|&v| VIEWERS[v].to_string()).collect(),
        poll_options: if r.poll {
            vec!["ledger".into(), "castle".into()]
        } else {
            Vec::new()
        },
        names: vec!["malakor".into()],
        first_contacts: r.first_contacts.iter().map(|&i| id(i)).collect(),
        raid: r.raid,
        raid_wave: r.raid_wave.map(id),
        carried: r.carried.iter().map(|&(i, age)| (id(i), age)).collect(),
        owed: r.owed.iter().map(|&i| id(i)).collect(),
        topic_words: if r.topic {
            vec!["ledger".into(), "castle".into()]
        } else {
            Vec::new()
        },
        greet: r.greet.map(id),
    }
}

fn picker_config() -> impl Strategy<Value = PickerConfig> {
    (0usize..5, 1usize..6, 0usize..4, 0.5f64..3.0, 1usize..20).prop_map(
        |(max_picks, share_window, share_max, whale_factor, whale_window)| PickerConfig {
            max_picks,
            share_window,
            share_max,
            whale_factor,
            whale_window,
            ..PickerConfig::default()
        },
    )
}

/// A few beats in a row, so the share, whale, novelty and stale state carries between them.
fn beats() -> impl Strategy<Value = Vec<(Vec<Candidate>, Room)>> {
    prop::collection::vec((candidates(12), room_spec()), 1..6)
}

proptest! {
    #![proptest_config(config())]

    /// `pick_deterministic` is deterministic: two fresh pickers with equal config fed the same
    /// beats choose the same messages for the same reasons.
    #[test]
    fn equal_pickers_fed_equal_beats_pick_alike(cfg in picker_config(), beats in beats()) {
        let mut a = Picker::new(cfg.clone());
        let mut b = Picker::new(cfg);
        for (cands, r) in &beats {
            let pa = a.pick_deterministic(cands, &room(r));
            let pb = b.pick_deterministic(cands, &room(r));
            prop_assert_eq!(pa.ids(), pb.ids());
            prop_assert_eq!(pa.reason, pb.reason);
        }
    }

    /// Every picked id is a candidate's id, and no message is picked twice in a beat.
    #[test]
    fn picks_are_distinct_candidates(cfg in picker_config(), beats in beats()) {
        let mut p = Picker::new(cfg);
        for (cands, r) in &beats {
            let ids = p.pick_deterministic(cands, &room(r)).ids();
            let known: HashSet<&str> = cands.iter().map(|c| c.msg.id.as_str()).collect();
            prop_assert!(ids.iter().all(|i| known.contains(i.as_str())), "{ids:?}");
            let unique: HashSet<&String> = ids.iter().collect();
            prop_assert_eq!(unique.len(), ids.len(), "duplicate pick in {:?}", ids);
        }
    }

    /// An empty candidate list picks nothing, whatever came before and whatever the room says.
    #[test]
    fn no_candidates_pick_nothing(cfg in picker_config(), beats in beats(), r in room_spec()) {
        let mut p = Picker::new(cfg);
        for (cands, r) in &beats {
            p.pick_deterministic(cands, &room(r));
        }
        prop_assert!(p.pick_deterministic(&[], &room(&r)).chosen.is_empty());
    }

    /// Every safety-critical candidate is chosen, first and in inbox order, whatever the cap.
    #[test]
    fn every_safety_candidate_is_picked_first_in_inbox_order(
        cfg in picker_config(),
        beats in beats(),
    ) {
        let mut p = Picker::new(cfg);
        for (cands, r) in &beats {
            let ids = p.pick_deterministic(cands, &room(r)).ids();
            let safety: Vec<String> = cands
                .iter()
                .filter(|c| c.safety.is_some())
                .map(|c| c.msg.id.clone())
                .collect();
            prop_assert!(ids.len() >= safety.len());
            prop_assert_eq!(&ids[..safety.len()], &safety[..]);
        }
    }

    /// Past the safety picks the beat's cap holds: no more than `max_picks` messages in all
    /// unless safety alone went past it, and then nothing else.
    #[test]
    fn the_beat_cap_bounds_every_non_safety_pick(cfg in picker_config(), beats in beats()) {
        let max = cfg.max_picks;
        let mut p = Picker::new(cfg);
        for (cands, r) in &beats {
            let chosen = p.pick_deterministic(cands, &room(r)).chosen;
            let safety = chosen.iter().filter(|c| c.safety.is_some()).count();
            prop_assert!(chosen.len() <= max.max(safety), "{} > {max}", chosen.len());
        }
    }

    /// A vote is never answered (the segment counts it): no non-safety pick is a vote.
    #[test]
    fn a_vote_is_never_picked(cfg in picker_config(), beats in beats()) {
        let mut p = Picker::new(cfg);
        for (cands, r) in &beats {
            let chosen = p.pick_deterministic(cands, &room(r)).chosen;
            prop_assert!(!chosen.iter().any(|c| c.safety.is_none() && is_vote(c)));
        }
    }

    /// One pick per viewer per beat: no non-safety pick shares its viewer with another pick.
    #[test]
    fn one_pick_per_viewer_per_beat(cfg in picker_config(), beats in beats()) {
        let mut p = Picker::new(cfg);
        for (cands, r) in &beats {
            let chosen = p.pick_deterministic(cands, &room(r)).chosen;
            let mut seen: HashMap<&str, usize> = HashMap::new();
            for c in &chosen {
                *seen.entry(c.msg.viewer.id.as_str()).or_default() += 1;
            }
            for c in chosen.iter().filter(|c| c.safety.is_none()) {
                prop_assert_eq!(seen[c.msg.viewer.id.as_str()], 1, "{} twice", &c.msg.viewer.id);
            }
        }
    }

    /// At most one injection attempt a beat: a non-safety injection pick is the only injection
    /// among the picks.
    #[test]
    fn at_most_one_injection_per_beat(cfg in picker_config(), beats in beats()) {
        let mut p = Picker::new(cfg);
        for (cands, r) in &beats {
            let chosen = p.pick_deterministic(cands, &room(r)).chosen;
            let injections = chosen.iter().filter(|c| c.injection).count();
            if chosen.iter().any(|c| c.injection && c.safety.is_none()) {
                prop_assert_eq!(injections, 1);
            }
        }
    }

    /// A waiting raid's greeting takes the first slot after safety when it can: a candidate,
    /// not safety, not a vote, a slot left under the cap, its viewer not already picked and no
    /// second injection.
    #[test]
    fn the_raid_greeting_is_picked_when_it_can_be(
        cfg in picker_config(),
        cands in candidates(12),
        r in room_spec(),
    ) {
        let max = cfg.max_picks;
        let mut p = Picker::new(cfg);
        let ctx = room(&r);
        let ids = p.pick_deterministic(&cands, &ctx).ids();
        let Some(g) = ctx.greet.as_ref().and_then(|g| cands.iter().find(|c| &c.msg.id == g))
        else {
            return Ok(());
        };
        let safety: Vec<&Candidate> = cands.iter().filter(|c| c.safety.is_some()).collect();
        let free = g.safety.is_none()
            && !is_vote(g)
            && safety.len() < max
            && !safety.iter().any(|s| s.msg.viewer.id == g.msg.viewer.id)
            && !(g.injection && safety.iter().any(|s| s.injection));
        if free {
            prop_assert_eq!(&ids[safety.len()], &g.msg.id);
        }
    }

    /// With no safety pick and no greeting, the first owed request that is a candidate (not
    /// safety, not a vote) takes the beat's first slot whenever the beat has one.
    #[test]
    fn the_oldest_owed_request_goes_first(
        cfg in picker_config(),
        cands in candidates(12),
        mut r in room_spec(),
        owed_at in prop::collection::vec(any::<prop::sample::Index>(), 2..5),
    ) {
        r.greet = None;
        // Several owed requests that are really candidates, so which of them goes first matters.
        if !cands.is_empty() {
            r.owed = owed_at.iter().map(|ix| ix.index(cands.len())).collect();
        }
        let max = cfg.max_picks;
        let mut p = Picker::new(cfg);
        let ctx = room(&r);
        let ids = p.pick_deterministic(&cands, &ctx).ids();
        if max == 0 || cands.iter().any(|c| c.safety.is_some()) {
            return Ok(());
        }
        let owed = ctx.owed.iter().find_map(|o| {
            cands
                .iter()
                .find(|c| &c.msg.id == o && c.safety.is_none() && !is_vote(c))
        });
        if let Some(o) = owed {
            prop_assert_eq!(ids.first(), Some(&o.msg.id));
        }
    }

    /// `Pick::ids` lists the chosen messages' ids, in pick order.
    #[test]
    fn pick_ids_are_the_chosen_ids(cfg in picker_config(), cands in candidates(12), r in room_spec()) {
        let pick = Picker::new(cfg).pick_deterministic(&cands, &room(&r));
        let ids: Vec<String> = pick.chosen.iter().map(|c| c.msg.id.clone()).collect();
        prop_assert_eq!(pick.ids(), ids);
    }

    /// A message with a question mark anywhere is a question.
    #[test]
    fn a_question_mark_makes_a_question(head in "\\PC{0,30}", tail in "\\PC{0,30}") {
        let text = format!("{head}?{tail}");
        prop_assert!(is_question(&text));
    }

    /// A message whose first word asks (who, what, why, ..., in any case, after any leading
    /// punctuation) is a question without a question mark.
    #[test]
    fn an_asking_first_word_makes_a_question(
        lead in "[ .,!-]{0,3}",
        word in prop::sample::select(&["who", "what", "why", "how", "when", "where", "which",
            "can", "could", "would", "will", "do", "does", "did", "is", "are", "should"][..]),
        upper in any::<bool>(),
        rest in "( [a-z]{1,8}){0,5}",
    ) {
        let word = if upper { word.to_uppercase() } else { word.to_string() };
        let text = format!("{lead}{word}{rest}");
        prop_assert!(is_question(&text));
    }

    /// No names, or only empty ones, mention nobody; a name present in the text, in any case,
    /// is a mention.
    #[test]
    fn a_name_in_the_text_is_a_mention(
        name in "[a-z]{1,10}",
        head in "[a-zA-Z ]{0,20}",
        tail in "[a-zA-Z ]{0,20}",
        upper in any::<bool>(),
    ) {
        let shown = if upper { name.to_uppercase() } else { name.clone() };
        let text = format!("{head}{shown}{tail}");
        prop_assert!(mentions(&text, std::slice::from_ref(&name)));
        prop_assert!(!mentions(&text, &[]));
        prop_assert!(!mentions(&text, &[String::new()]));
    }

    /// A line with no letters or digits at all is a bare emote.
    #[test]
    fn a_line_without_letters_or_digits_is_a_bare_emote(text in "[ !?.,:;()*~<>^_-]{0,30}") {
        prop_assert!(is_bare_emote(&text));
    }

    /// One to three emote words, in any case and separated by any punctuation, are a bare emote.
    #[test]
    fn up_to_three_emote_words_are_a_bare_emote(
        words in prop::collection::vec(
            prop::sample::select(&["lul", "lol", "kekw", "pog", "w", "gg", "xd", "o7", "kappa"][..]),
            1..=3,
        ),
        sep in "[ !.]{1,3}",
        upper in any::<bool>(),
    ) {
        let line = words.join(&sep);
        let line = if upper { line.to_uppercase() } else { line };
        prop_assert!(is_bare_emote(&line));
    }

    /// More than three words, or any word that is not an emote, is not a bare emote.
    #[test]
    fn content_is_never_a_bare_emote(
        emotes in prop::collection::vec(prop::sample::select(&["lul", "w", "gg"][..]), 0..3),
        word in "q[a-z]{3,8}",
        extra in prop::collection::vec(prop::sample::select(&["lul", "w", "gg"][..]), 4..6),
    ) {
        let mut with_word = emotes.clone();
        with_word.push(&word);
        prop_assert!(!is_bare_emote(&with_word.join(" ")));
        prop_assert!(!is_bare_emote(&extra.join(" ")));
    }

    /// `is_vote` is exactly "the command is a vote".
    #[test]
    fn is_vote_is_exactly_a_vote_command(cands in candidates(6)) {
        for c in &cands {
            prop_assert_eq!(is_vote(c), matches!(c.msg.command, Some(Command::Vote { .. })));
        }
    }
}
