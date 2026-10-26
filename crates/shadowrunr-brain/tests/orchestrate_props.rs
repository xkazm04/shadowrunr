//! Property tests for the orchestrator's public API ([`shadowrunr_brain::orchestrate`]). Each
//! property names the invariant it holds the code to, taken from the code and its doc comments.
//! Every viewer, message and beat here is generated and simulated; no real chat.

use std::collections::HashMap;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use shadowrunr_brain::moderation::Candidate;
use shadowrunr_brain::orchestrate::requests::Ask;
use shadowrunr_brain::orchestrate::{BeatInputs, Orchestrator, Plan, COLD_OPEN_STATE};
use shadowrunr_brain::picker::Pick;
use shadowrunr_types::chat::{ChatMsg, Command, Platform, Rank, Viewer};
use shadowrunr_types::prep::StreamPrep;
use shadowrunr_types::{SegmentContext, SegmentId, StreamEvent};

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 48,
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource(
            "proptest-regressions",
        ))),
        ..ProptestConfig::default()
    }
}

const VIEWERS: &[&str] = &[
    "sim-ash", "sim-bly", "sim-cor", "sim-dun", "sim-eft", "sim-fen", "sim-gil",
];

/// Simulated chat lines: questions, a mention, requests, asks, emotes, votes, plain remarks.
const TEXTS: &[&str] = &[
    "what is the hex doing tonight?",
    "malakor you fraud",
    "are you a real person or an AI?",
    "roast me hard please",
    "i feel really sad and alone tonight",
    "!vote a",
    "LUL",
    "the moat looks cold",
    "what is this place?",
    "gg",
    "someone just subscribed",
];

const STATES: &[&str] = &[COLD_OPEN_STATE, "improv", "wrap", "CLOSING NOW"];

const ASKS: &[Ask] = &[
    Ask::MinorFlirt,
    Ask::Intrusion,
    Ask::RoastMe,
    Ask::Impression,
    Ask::CoinShill,
    Ask::Address,
    Ask::Explicit,
    Ask::AreYouAi,
    Ask::Sad,
];

/// One simulated beat: where it sits in time (a step that may be negative, so timestamps need
/// not be monotonic), the speech on air, and the simulated messages that arrive.
#[derive(Debug, Clone)]
struct Beat {
    step_ms: i64,
    state: usize,
    with_prep: bool,
    speech_ahead_ms: Option<i64>,
    closing: bool,
    msgs: Vec<(usize, usize, bool)>,
}

fn beat(max_msgs: usize) -> impl Strategy<Value = Beat> {
    (
        -30_000i64..60_000,
        0..STATES.len(),
        any::<bool>(),
        proptest::option::of(-5_000i64..30_000),
        any::<bool>(),
        proptest::collection::vec(
            (0..VIEWERS.len(), 0..TEXTS.len(), any::<bool>()),
            0..max_msgs,
        ),
    )
        .prop_map(
            |(step_ms, state, with_prep, speech_ahead_ms, closing, msgs)| Beat {
                step_ms,
                state,
                with_prep,
                speech_ahead_ms,
                closing,
                msgs,
            },
        )
}

/// A stream of a few beats, with an ordinary inbox.
fn beats() -> impl Strategy<Value = Vec<Beat>> {
    proptest::collection::vec(beat(8), 0..8)
}

fn chat_msg(id: String, ts_ms: i64, viewer: usize, text: usize, vote: bool) -> ChatMsg {
    ChatMsg {
        id,
        ts_ms,
        viewer: Viewer {
            id: VIEWERS[viewer].into(),
            name: VIEWERS[viewer].into(),
            platform: Platform::Sim,
            rank: Rank::Thrall,
            sub_tier: None,
            months: 0,
            gifted: 0,
            is_mod: false,
        },
        text: TEXTS[text].into(),
        command: vote.then(|| Command::Vote { option: "a".into() }),
    }
}

fn cand(m: ChatMsg) -> Candidate {
    Candidate {
        msg: m,
        count: 1,
        viewers: 1,
        injection: false,
        safety: None,
    }
}

fn context(state: &str) -> SegmentContext {
    SegmentContext {
        segment: SegmentId::Seg01ImprovisedRoom,
        rules_prompt: "R".into(),
        state_summary: "S".into(),
        state: state.into(),
        allowed_ops: Vec::new(),
        ccv: 5,
        elapsed_s: 10,
        hexes: Vec::new(),
    }
}

/// What one beat produced, in a form two runs can be compared on.
#[derive(Debug, PartialEq)]
struct Outcome {
    mode: String,
    ceiling: usize,
    reason: String,
    note: Option<String>,
    offered: Vec<String>,
    event: String,
}

fn outcome(plan: &Plan, offered: &[Candidate], event: &StreamEvent) -> Outcome {
    Outcome {
        mode: format!("{:?}", plan.mode),
        ceiling: plan.ceiling,
        reason: plan.reason.clone(),
        note: plan.note.clone(),
        offered: offered.iter().map(|c| c.msg.id.clone()).collect(),
        event: format!("{event:?}"),
    }
}

/// The state a failed beat must leave as it found it.
#[derive(Debug, PartialEq)]
struct Observable {
    owed: Vec<String>,
    asked: Vec<(String, usize)>,
    ages: Vec<(String, i64)>,
    agenda: String,
}

/// A driver that plays generated beats through the orchestrator the way the engine does:
/// observe, screen, plan, offer, pick what is offered up to the ceiling, commit.
struct Stream {
    o: Orchestrator,
    prep: StreamPrep,
    now: i64,
    next_id: u64,
    names: Vec<String>,
    /// Which viewer sent each message id.
    owner: HashMap<String, String>,
}

impl Stream {
    fn new(seed: u64) -> Self {
        Self {
            o: Orchestrator::new(seed),
            prep: serde_json::from_str(include_str!("fixtures/prep-sample.json"))
                .expect("the sample prep parses"),
            now: 100_000,
            next_id: 0,
            names: vec!["malakor".into()],
            owner: HashMap::new(),
        }
    }

    fn inbox(&mut self, b: &Beat) -> Vec<ChatMsg> {
        let mut out = Vec::new();
        for &(v, t, vote) in &b.msgs {
            self.next_id += 1;
            let m = chat_msg(format!("sim-m{}", self.next_id), self.now, v, t, vote);
            self.owner.insert(m.id.clone(), m.viewer.id.clone());
            out.push(m);
        }
        out
    }

    /// Everything the rollback test looks at, taken at the stream's current time.
    fn observe(&self, cands: &[Candidate]) -> Observable {
        let mut asked = Vec::new();
        for v in VIEWERS {
            for (i, a) in ASKS.iter().enumerate() {
                if self.o.has_asked(v, *a) {
                    asked.push((v.to_string(), i));
                }
            }
        }
        let mut ages: Vec<(String, i64)> = self.o.carried_ages(self.now).into_iter().collect();
        ages.sort();
        Observable {
            owed: self.o.owed(cands, self.now),
            asked,
            ages,
            agenda: self.o.agenda_summary(),
        }
    }

    fn inputs<'a>(
        b: &Beat,
        ctx: &'a SegmentContext,
        cands: &'a [Candidate],
        now: i64,
        prep: &'a StreamPrep,
        names: &'a [String],
    ) -> BeatInputs<'a> {
        BeatInputs {
            now_ms: now,
            ctx,
            cands,
            prep: b.with_prep.then_some(prep),
            speech_until_ms: b.speech_ahead_ms.map(|d| now + d),
            speech_lead_ms: 2_000,
            expected_latency_ms: 0,
            closing: b.closing,
            names,
            max_picks: 2,
            topic_words: &[],
            scene: None,
        }
    }

    /// Play one beat. With `roll_back` it ends as a failed core call instead of a delivered
    /// one, and the state before the plan and after the rollback comes back too.
    fn play(&mut self, b: &Beat, roll_back: bool) -> (Outcome, Option<(Observable, Observable)>) {
        self.now += b.step_ms;
        let inbox = self.inbox(b);
        let cands: Vec<Candidate> = inbox.iter().cloned().map(cand).collect();
        self.o.observe_inbox(&inbox);
        let mut ev = Vec::new();
        let cands = self.o.screen(self.now, cands, &mut ev);
        let txn = self.o.begin();
        let before = self.observe(&cands);
        let ctx = context(STATES[b.state]);
        let inputs = Self::inputs(b, &ctx, &cands, self.now, &self.prep, &self.names);
        let mut plan = self.o.plan(&inputs);
        let offered = self.o.offered(&plan, &cands);
        let chosen: Vec<Candidate> = offered.iter().take(plan.ceiling).cloned().collect();
        let pick = Pick {
            chosen,
            reason: String::new(),
        };
        let event = self.o.commit(&mut plan, &pick, self.now);
        let out = outcome(&plan, &offered, &event);
        if roll_back {
            self.o.rollback(txn, &pick, self.now);
            let after = self.observe(&cands);
            (out, Some((before, after)))
        } else {
            (out, None)
        }
    }
}

proptest! {
    #![proptest_config(config())]

    /// Determinism: two orchestrators built with the same seed and fed the same beats make the
    /// same plans, offer the same candidates and commit the same records.
    #[test]
    fn same_seed_same_inputs_give_identical_plans_and_commits(
        seed in any::<u64>(),
        beats in beats(),
    ) {
        let mut a = Stream::new(seed);
        let mut b = Stream::new(seed);
        for beat in &beats {
            prop_assert_eq!(a.play(beat, false).0, b.play(beat, false).0);
        }
    }

    /// Transaction rollback: `begin` then `rollback` leaves the observable state (what is owed,
    /// who was asked what, how long carried messages have waited, the agenda) as it was before
    /// the beat, whatever the beat planned and committed in between.
    #[test]
    fn rollback_restores_the_state_from_before_the_beat(
        seed in any::<u64>(),
        warmup in beats(),
        failed in beat(8),
    ) {
        let mut s = Stream::new(seed);
        for beat in &warmup {
            s.play(beat, false);
        }
        let (_, states) = s.play(&failed, true);
        let (before, after) = states.expect("a rolled-back beat reports both states");
        prop_assert_eq!(before.asked, after.asked);
        prop_assert_eq!(before.ages, after.ages);
        prop_assert_eq!(before.agenda, after.agenda);
        prop_assert_eq!(before.owed, after.owed);
    }

    /// Offer is a filter: `offered` returns only candidates drawn from the ones it was given
    /// (same ids, none invented, none repeated), in their order.
    #[test]
    fn offered_is_a_subsequence_of_the_candidates(
        seed in any::<u64>(),
        warmup in beats(),
        last in beat(12),
    ) {
        let mut s = Stream::new(seed);
        for beat in &warmup {
            s.play(beat, false);
        }
        s.now += last.step_ms;
        let inbox = s.inbox(&last);
        let cands: Vec<Candidate> = inbox.iter().cloned().map(cand).collect();
        s.o.observe_inbox(&inbox);
        let mut ev = Vec::new();
        let cands = s.o.screen(s.now, cands, &mut ev);
        let ctx = context(STATES[last.state]);
        let inputs = Stream::inputs(&last, &ctx, &cands, s.now, &s.prep, &s.names);
        let plan = s.o.plan(&inputs);
        let offered = s.o.offered(&plan, &cands);
        let mut rest = cands.iter();
        for o in &offered {
            prop_assert!(
                rest.any(|c| c.msg.id == o.msg.id),
                "offered {} is not among the candidates, in order",
                o.msg.id
            );
        }
    }

    /// Forgetting a viewer: once `forget_viewer` ran and the next beat was screened, none of
    /// that viewer's messages is carried any more (`carried_ages` names none of them), and
    /// the viewer has been asked nothing (`has_asked` is false for every ask).
    #[test]
    fn a_forgotten_viewer_is_not_carried_into_the_next_beat(
        seed in any::<u64>(),
        warmup in beats(),
        who in 0..VIEWERS.len(),
    ) {
        let mut s = Stream::new(seed);
        for beat in &warmup {
            s.play(beat, false);
        }
        s.o.forget_viewer(VIEWERS[who]);
        for ask in ASKS {
            prop_assert!(!s.o.has_asked(VIEWERS[who], *ask), "{ask:?} still held");
        }
        let mut ev = Vec::new();
        s.o.screen(s.now, Vec::new(), &mut ev);
        for id in s.o.carried_ages(s.now).keys() {
            prop_assert_ne!(s.owner.get(id).map(String::as_str), Some(VIEWERS[who]));
        }
    }

    /// No panic: `plan` and `commit` hold for any well-formed inputs, including an empty
    /// inbox, a raid-sized inbox and timestamps that go backwards.
    #[test]
    fn plan_and_commit_never_panic(
        seed in any::<u64>(),
        raid in proptest::collection::vec(beat(40), 1..4),
        mixed in beats(),
    ) {
        let mut s = Stream::new(seed);
        let quiet = Beat {
            step_ms: 0,
            state: 1,
            with_prep: false,
            speech_ahead_ms: None,
            closing: false,
            msgs: Vec::new(),
        };
        s.play(&quiet, false);
        for beat in raid.iter().chain(&mixed) {
            s.play(beat, false);
        }
    }
}
