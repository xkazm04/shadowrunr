//! Orchestrator decisions, beat by beat, on a fictional prep. No engine is involved.

use std::collections::HashSet;

use shadowrunr_types::prep::{BeatMode, StreamPrep};
use shadowrunr_types::{ChatMsg, Command, Platform, Rank, SegmentContext, SegmentId, Viewer};

use super::*;
use crate::picker::Pick;

pub(crate) fn sample_prep() -> StreamPrep {
    serde_json::from_str(include_str!("../../tests/fixtures/prep-sample.json"))
        .expect("the sample prep parses")
}

fn ctx(state: &str) -> SegmentContext {
    ctx_with(state, "S")
}

fn ctx_with(state: &str, summary: &str) -> SegmentContext {
    SegmentContext {
        segment: SegmentId::Seg01ImprovisedRoom,
        rules_prompt: "R".into(),
        state_summary: summary.into(),
        state: state.into(),
        allowed_ops: Vec::new(),
        ccv: 5,
        elapsed_s: 10,
        hexes: Vec::new(),
    }
}

fn msg(id: &str, viewer: &str, text: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        ts_ms: 0,
        viewer: Viewer {
            id: viewer.into(),
            name: viewer.into(),
            platform: Platform::Sim,
            rank: Rank::Thrall,
            sub_tier: None,
            months: 0,
            gifted: 0,
            is_mod: false,
        },
        text: text.into(),
        command: None,
    }
}

fn cand(m: &ChatMsg) -> Candidate {
    Candidate {
        msg: m.clone(),
        count: 1,
        viewers: 1,
        injection: false,
        safety: None,
    }
}

/// One beat: observe, screen, plan, offer, then "pick" everything offered up to the ceiling
/// (safety first), and commit. Returns the plan and the offered ids.
struct Room {
    o: Orchestrator,
    names: Vec<String>,
    last_say: Option<TsMs>,
    /// Speech queued on the speech clock until then (the brain's `aired` lines).
    speaking_until: Option<TsMs>,
    closing: bool,
    /// The segment's topic words this beat.
    topic: Vec<String>,
}

impl Room {
    fn new() -> Self {
        Self {
            o: Orchestrator::new(7),
            names: vec!["malakor".into()],
            last_say: None,
            speaking_until: None,
            closing: false,
            topic: Vec::new(),
        }
    }

    fn beat(
        &mut self,
        now: TsMs,
        state: &str,
        prep: Option<&StreamPrep>,
        inbox: Vec<ChatMsg>,
        cands: Vec<Candidate>,
    ) -> (Plan, Vec<String>, StreamEvent) {
        let (p, ids, e, _) = self.beat_in(now, &ctx(state), prep, inbox, cands);
        (p, ids, e)
    }

    /// A beat in a given context; also returns the screen's moderation records.
    fn beat_in(
        &mut self,
        now: TsMs,
        c: &SegmentContext,
        prep: Option<&StreamPrep>,
        inbox: Vec<ChatMsg>,
        cands: Vec<Candidate>,
    ) -> (Plan, Vec<String>, StreamEvent, Vec<StreamEvent>) {
        self.o.observe_inbox(&inbox);
        let mut ev = Vec::new();
        let cands = self.o.screen(now, cands, &mut ev);
        let mut plan = self.o.plan(&BeatInputs {
            now_ms: now,
            ctx: c,
            cands: &cands,
            prep,
            speech_until_ms: self.speaking_until.max(self.last_say),
            speech_lead_ms: crate::pace::PaceConfig::default().speech_lead_ms,
            expected_latency_ms: 0,
            closing: self.closing,
            names: &self.names,
            max_picks: crate::picker::PickerConfig::default().max_picks,
            topic_words: &self.topic,
        });
        let offered = self.o.offered(&plan, &cands);
        let mut chosen: Vec<Candidate> = offered
            .iter()
            .filter(|c| c.safety.is_some())
            .cloned()
            .collect();
        for c in offered.iter().filter(|c| c.safety.is_none()) {
            if chosen.len() < plan.ceiling {
                chosen.push(c.clone());
            }
        }
        let ids = chosen.iter().map(|c| c.msg.id.clone()).collect();
        let pick = Pick {
            chosen,
            reason: String::new(),
        };
        let e = self.o.commit(&mut plan, &pick, now);
        ev.extend(self.o.take_capped(now));
        if plan.mode != BeatMode::Hold {
            self.last_say = Some(now);
        }
        (plan, ids, e, ev)
    }
}

fn quiet(room: &mut Room, now: TsMs, state: &str, prep: Option<&StreamPrep>) -> Plan {
    room.beat(now, state, prep, Vec::new(), Vec::new()).0
}

#[test]
fn the_cold_open_tells_the_story_in_order_passes_only_first_contacts_and_safety_then_hands_off() {
    let prep = sample_prep();
    let mut room = Room::new();
    let mut voiced: Vec<String> = Vec::new();

    // Beat 1: a first contact and a regular's chatter (seen in an earlier beat) arrive.
    room.o.observe_inbox(&[msg("old", "regular", "hi")]);
    let a = msg("a", "newbie", "hello there, what is this place?");
    let b = msg("b", "regular", "malakor tell us about the moat please");
    let mut vote = msg("v", "voter", "!vote a");
    vote.command = Some(Command::Vote { option: "a".into() });
    let (p, ids, e) = room.beat(
        5_000,
        COLD_OPEN_STATE,
        Some(&prep),
        vec![a.clone(), b.clone(), vote.clone()],
        vec![cand(&a), cand(&b), cand(&vote)],
    );
    assert_eq!(p.mode, BeatMode::ColdOpen);
    assert_eq!(p.bit_id.as_deref(), Some("co-lantern-tax"));
    assert_eq!(
        ids,
        ["a"],
        "only the first contact, never the regular or the vote"
    );
    let StreamEvent::BeatPlan { reason, picks, .. } = e else {
        panic!("a beat_plan")
    };
    assert_eq!(picks, 1);
    assert!(
        reason.starts_with("room filling: cold open lines 1-2/6"),
        "{reason}"
    );
    voiced.push(p.material.clone().unwrap());

    // Two first contacts in one beat: at most one passes.
    let c1 = msg("c1", "n1", "first time here");
    let c2 = msg("c2", "n2", "hey hey");
    let (p, ids, _) = room.beat(
        10_000,
        COLD_OPEN_STATE,
        Some(&prep),
        vec![c1.clone(), c2.clone()],
        vec![cand(&c1), cand(&c2)],
    );
    assert_eq!(ids.len(), 1, "{ids:?}");
    voiced.push(p.material.clone().unwrap());

    // A disclosure: safety passes, and the story pauses (no prepared material).
    let mut s = cand(&msg("s", "regular", "i want to end it all"));
    s.safety = Some("builtin-self-harm-disclosure".into());
    let (p, ids, _) = room.beat(15_000, COLD_OPEN_STATE, Some(&prep), Vec::new(), vec![s]);
    assert_eq!(p.mode, BeatMode::React);
    assert!(p.material.is_none());
    assert_eq!(ids, ["s"]);

    let p = quiet(&mut room, 20_000, COLD_OPEN_STATE, Some(&prep));
    voiced.push(p.material.clone().unwrap());
    let all = voiced.join("\n");
    let mut at = 0;
    for line in &prep.cold_open.beats {
        let i = all[at..]
            .find(line.as_str())
            .unwrap_or_else(|| panic!("{line} in order:\n{all}"));
        at += i;
    }
    assert!(all.contains("land it"), "{all}");

    // The handoff, then hold or greet first contacts until the state changes.
    let p = quiet(&mut room, 25_000, COLD_OPEN_STATE, Some(&prep));
    assert_eq!(p.mode, BeatMode::Bridge);
    assert!(p.material.unwrap().contains(&prep.cold_open.handoff));
    let p = quiet(&mut room, 26_000, COLD_OPEN_STATE, Some(&prep));
    assert_eq!(p.mode, BeatMode::Hold, "{}", p.reason);
    let n = msg("n3", "n3", "evening, lich");
    let (p, ids, _) = room.beat(
        27_000,
        COLD_OPEN_STATE,
        Some(&prep),
        vec![n.clone()],
        vec![cand(&n)],
    );
    assert_eq!((p.mode, ids.len()), (BeatMode::React, 1));
    // After the handoff a quiet beat draws from the segment's deck, premises first.
    let p = quiet(&mut room, 40_000, COLD_OPEN_STATE, Some(&prep));
    assert_eq!(p.mode, BeatMode::Bit, "{}", p.reason);
    assert!(
        p.reason.starts_with("room filling: deck premise"),
        "{}",
        p.reason
    );
    assert!(p.note.is_none() && p.material.is_some());
}

#[test]
fn without_a_prep_the_cold_open_is_a_short_gathering_open_and_first_contacts_only() {
    let mut room = Room::new();
    let p = quiet(&mut room, 5_000, COLD_OPEN_STATE, None);
    assert_eq!(p.mode, BeatMode::ColdOpen);
    assert_eq!(p.note.as_deref(), Some(GATHERING_NOTE));
    assert!(p.material.is_none());
    room.o.observe_inbox(&[msg("x", "old", "hi")]);
    let old = msg("o", "old", "malakor what is the plan tonight?");
    let (p, ids, _) = room.beat(
        8_000,
        COLD_OPEN_STATE,
        None,
        vec![old.clone()],
        vec![cand(&old)],
    );
    assert!(ids.is_empty(), "a returning chatter waits: {}", p.reason);
    assert_eq!(p.mode, BeatMode::Hold);
}

#[test]
fn after_the_cold_open_quiet_brings_a_bit_never_repeated_and_the_room_answering_skips_a_line() {
    let prep = sample_prep();
    let mut room = Room::new();
    let mut bits: Vec<String> = Vec::new();
    let mut t = 0;
    let mut story_seen = false;
    for _ in 0..80 {
        t += 5_000;
        let p = quiet(&mut room, t, "opening", Some(&prep));
        if matches!(p.mode, BeatMode::Bit | BeatMode::Bridge) {
            if let Some(id) = &p.bit_id {
                if id == "s-01" {
                    story_seen = true;
                }
                if bits.last() != Some(id) || id != "s-01" {
                    bits.push(id.clone());
                }
            }
        }
    }
    let unique: HashSet<&String> = bits.iter().collect();
    assert_eq!(unique.len(), bits.len(), "no bit offered twice: {bits:?}");
    assert_eq!(unique.len(), 4, "the SEG-01 deck only: {bits:?}");
    assert!(story_seen);
    assert!(!bits.iter().any(|b| b == "x-01"), "another segment's bit");

    // Back-off: a story in progress, then the room answers: its next line is skipped.
    let mut room = Room::new();
    let mut prep2 = sample_prep();
    prep2.bits.retain(|b| b.id == "s-01");
    let mut t = 0;
    let first = loop {
        t += 5_000;
        let p = quiet(&mut room, t, "opening", Some(&prep2));
        if p.bit_id.as_deref() == Some("s-01") {
            break p;
        }
    };
    assert!(
        first.reason.contains("story s-01 lines 1-2/4"),
        "{}",
        first.reason
    );
    let m = msg("r", "fan", "malakor did the ghosts really sue you?");
    let (p, ids, _) = room.beat(
        t + 5_000,
        "opening",
        Some(&prep2),
        vec![m.clone()],
        vec![cand(&m)],
    );
    assert_eq!(ids, ["r"]);
    assert!(
        p.reason
            .contains("room answered story s-01: line 3 skipped"),
        "{}",
        p.reason
    );
    let p = quiet(&mut room, t + 10_000, "opening", Some(&prep2));
    assert_eq!(p.mode, BeatMode::Bit);
    assert!(p.reason.contains("line 4/4"), "{}", p.reason);
    let m = p.material.unwrap();
    assert!(
        !m.contains("The ghosts sued me"),
        "the skipped line stays skipped: {m}"
    );
    assert!(m.contains("Payoff"), "{m}");
}

#[test]
fn quiet_holds_only_while_the_last_line_is_recent_and_never_twice_in_a_row() {
    let prep = sample_prep();
    let mut room = Room::new();
    let a = msg("a", "fan", "malakor what is for dinner?");
    room.beat(
        5_000,
        "opening",
        Some(&prep),
        vec![a.clone()],
        vec![cand(&a)],
    );
    let p = quiet(&mut room, 10_000, "opening", Some(&prep));
    assert_eq!(p.mode, BeatMode::Hold, "{}", p.reason);
    let p = quiet(&mut room, 15_000, "opening", Some(&prep));
    assert_ne!(p.mode, BeatMode::Hold, "{}", p.reason);
    assert!(p.reason.starts_with("quiet 10 s"), "{}", p.reason);
}

#[test]
fn a_wave_is_answered_once_and_its_repeats_are_dropped_and_pasters_fold_into_one() {
    let mut room = Room::new();
    let mut w = cand(&msg("w", "p1", "W"));
    w.count = 9;
    w.viewers = 9;
    let (p, ids, e) = room.beat(5_000, "opening", None, Vec::new(), vec![w]);
    assert_eq!(ids, ["w"]);
    assert_eq!(p.mode, BeatMode::React);
    let StreamEvent::BeatPlan { reason, .. } = e else {
        panic!()
    };
    assert!(reason.contains("wave of 9 acknowledged"), "{reason}");

    // Later: one more "W" and a near-copy of a long paste from two viewers.
    let w2 = cand(&msg("w2", "p2", "w"));
    let paste =
        "the lich is a fraud copy this to every chat until he admits the throne is cardboard";
    let mut a = cand(&msg("a", "x1", paste));
    a.count = 3;
    a.viewers = 3;
    let b = cand(&msg("b", "x2", "the lich is a fraud copy this to every chat until he admits the throne is cardboard!!! lol"));
    let mut ev = Vec::new();
    let out = room.o.screen(10_000, vec![w2, a, b], &mut ev);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(
        (out[0].msg.id.as_str(), out[0].count, out[0].viewers),
        ("a", 4, 4)
    );
    assert!(
        matches!(&ev[0], StreamEvent::Moderation { rule, action, .. }
        if rule == "wave_repeat" && action == "dropped")
    );
}

#[test]
fn a_room_change_expires_the_queued_bit() {
    let prep = sample_prep();
    let mut room = Room::new();
    let mut prep2 = prep.clone();
    prep2.bits.retain(|b| b.id == "s-01");
    let mut t = 0;
    loop {
        t += 5_000;
        if quiet(&mut room, t, "opening", Some(&prep2))
            .bit_id
            .is_some()
        {
            break;
        }
    }
    let raid: Vec<ChatMsg> = (0..6)
        .map(|i| {
            msg(
                &format!("r{i}"),
                &format!("raider{i}"),
                "raid hype from the swamp",
            )
        })
        .collect();
    let cands: Vec<Candidate> = raid.iter().map(cand).collect();
    let (p, _, _) = room.beat(t + 5_000, "opening", Some(&prep2), raid, cands);
    assert!(
        p.reason
            .starts_with("room changed (raid): bit s-01 expired"),
        "{}",
        p.reason
    );
    // The expired bit never comes back.
    for k in 2..10 {
        let p = quiet(&mut room, t + 5_000 * k, "opening", Some(&prep2));
        assert_ne!(p.bit_id.as_deref(), Some("s-01"), "{}", p.reason);
    }
}

#[test]
fn chat_folds_into_the_segment_one_pick_a_beat_and_a_poll_state_a_bridge() {
    // Was: a thin room got a bit beside its one pick only after three reacts in a row. Now
    // every chat beat advances the segment: a premise beside the first pick, the back-to-chat
    // connector after it, and with nothing prepared due a react told to tie back.
    let prep = sample_prep();
    let mut room = Room::new();
    let mut modes = Vec::new();
    for i in 0..4 {
        let m = msg(
            &format!("m{i}"),
            "fan",
            &format!("malakor question number {i}?"),
        );
        let (p, ids, _) = room.beat(
            5_000 * (i + 1),
            "opening",
            Some(&prep),
            vec![m.clone()],
            vec![cand(&m)],
        );
        modes.push(p.mode);
        assert_eq!(ids.len(), 1, "{}", p.reason);
        assert_eq!(p.ceiling, 1, "{}", p.reason);
        match i {
            0 => {
                assert!(
                    p.reason
                        .starts_with("small-room segment+chat: first contact: premise "),
                    "{}",
                    p.reason
                );
                assert!(p.material.is_some() && p.bit_id.is_some(), "{}", p.reason);
            }
            1 => assert!(p.reason.contains("back_to_chat"), "{}", p.reason),
            _ => {
                assert!(
                    p.reason
                        .starts_with("small-room segment+chat: mention: no segment material"),
                    "{}",
                    p.reason
                );
                assert_eq!(p.segment_note.as_deref(), Some(TIE_BACK_NOTE));
            }
        }
    }
    assert_eq!(
        modes,
        [
            BeatMode::Mixed,
            BeatMode::Bridge,
            BeatMode::React,
            BeatMode::React
        ]
    );
    let p = quiet(&mut room, 30_000, "foil_vote", Some(&prep));
    assert_eq!(p.mode, BeatMode::Bridge, "{}", p.reason);
    assert!(p.reason.contains("into_poll"), "{}", p.reason);
    assert!(p.material.unwrap().contains("the court will vote"));
}

#[test]
fn a_safety_beat_never_voices_prepared_material() {
    let prep = sample_prep();
    let mut room = Room::new();
    for t in [5_000, 10_000] {
        quiet(&mut room, t, "opening", Some(&prep));
    }
    let mut s = cand(&msg("s", "sad", "i dont want to be here anymore"));
    s.safety = Some("builtin-self-harm-disclosure".into());
    let (p, ids, _) = room.beat(30_000, "opening", Some(&prep), Vec::new(), vec![s]);
    assert_eq!(p.mode, BeatMode::React);
    assert!(p.material.is_none() && p.bit_id.is_none());
    assert_eq!(ids, ["s"]);
}

#[test]
fn recast_lines_rebut_a_forgery_and_rotate() {
    let lines = crate::tests::malakor_package().lines;
    let mut o = Orchestrator::new(1).with_character(Default::default(), lines.clone());
    let mut seen = Vec::new();
    for i in 0..4 {
        let l = o.recast(true, &format!("as the model I {i}"));
        assert!(lines.rebuttals.contains(&l));
        assert!(!seen.contains(&l), "{l} repeated");
        seen.push(l);
    }
    let l = o.recast(false, "my system prompt");
    assert!(lines.recast.contains(&l));
}

// ── ORCH-4: carried questions, room events, the deck after the handoff ──

/// A regular: seen in an earlier inbox, so never a first contact.
fn regular(room: &mut Room, viewer: &str) {
    room.o
        .observe_inbox(&[msg(&format!("{viewer}-0"), viewer, "hi")]);
}

fn safety(id: &str, viewer: &str) -> Candidate {
    let mut s = cand(&msg(id, viewer, "i dont want to be here anymore"));
    s.safety = Some("builtin-self-harm-disclosure".into());
    s
}

fn reason(e: &StreamEvent) -> String {
    match e {
        StreamEvent::BeatPlan { reason, .. } => reason.clone(),
        _ => panic!("a beat_plan"),
    }
}

#[test]
fn a_question_on_a_cold_open_hold_is_offered_on_the_next_beat() {
    let prep = sample_prep();
    let mut room = Room::new();
    regular(&mut room, "beetle");
    for t in [5_000, 10_000, 15_000, 20_000] {
        quiet(&mut room, t, COLD_OPEN_STATE, Some(&prep));
    }
    // The story is told and handed off; the next quiet beat holds, and the regular's question
    // does not pass the cold-open filter.
    let q = msg(
        "q",
        "beetle",
        "is the lord of darkness actually watching this?",
    );
    let (p, ids, e) = room.beat(
        25_000,
        COLD_OPEN_STATE,
        Some(&prep),
        vec![q.clone()],
        vec![cand(&q)],
    );
    assert_eq!(p.mode, BeatMode::Hold, "{}", p.reason);
    assert!(ids.is_empty());
    assert!(reason(&e).contains("deferred: q"), "{}", reason(&e));
    // The room fills: the next beat offers it, carried.
    let (p, ids, e) = room.beat(30_000, "opening", Some(&prep), Vec::new(), Vec::new());
    assert_eq!(ids, ["q"], "{}", p.reason);
    assert!(
        reason(&e).contains("carried over: q (5 s)"),
        "{}",
        reason(&e)
    );
}

#[test]
fn a_question_on_a_safety_beat_is_offered_after_it_once_and_a_wave_never_waits() {
    let mut room = Room::new();
    regular(&mut room, "beetle");
    let q = msg(
        "q",
        "beetle",
        "wait is the lord of darkness actually watching right now?",
    );
    let mut wave = cand(&msg("w", "paster", "W"));
    wave.count = 7;
    wave.viewers = 7;
    let (p, ids, e) = room.beat(
        5_000,
        "opening",
        None,
        vec![q.clone()],
        vec![safety("s", "sad"), cand(&q), wave],
    );
    assert_eq!(ids, ["s"], "a safety beat answers the disclosure alone");
    assert_eq!(p.mode, BeatMode::React);
    assert!(reason(&e).ends_with("deferred: q"), "{}", reason(&e));
    let (_, ids, e) = room.beat(10_000, "opening", None, Vec::new(), Vec::new());
    assert_eq!(ids, ["q"], "the question, never the wave");
    assert!(
        reason(&e).contains("carried over: q (5 s)"),
        "{}",
        reason(&e)
    );
    // Answered: never offered again.
    for t in [15_000, 20_000, 25_000] {
        let (_, ids, _) = room.beat(t, "opening", None, Vec::new(), Vec::new());
        assert!(ids.is_empty(), "{ids:?}");
    }
}

#[test]
fn at_most_six_messages_are_carried() {
    let mut room = Room::new();
    let mut cands = vec![safety("s", "sad")];
    let mut inbox = Vec::new();
    for i in 0..8 {
        let m = msg(
            &format!("q{i}"),
            &format!("v{i}"),
            "malakor, who holds the moat?",
        );
        inbox.push(m.clone());
        cands.push(cand(&m));
    }
    let (_, _, e, ev) = room.beat_in(5_000, &ctx("opening"), None, inbox, cands);
    let r = reason(&e);
    let deferred = r.split("deferred: ").nth(1).unwrap_or_default();
    assert_eq!(deferred.split(", ").count(), CARRY_MAX, "{r}");
    // The two the cap dropped each leave a record.
    let capped: Vec<&str> = ev
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Moderation {
                rule,
                msg_id,
                action,
                ..
            } if rule == CARRY_RULE && action == CAPPED => msg_id.as_deref(),
            _ => None,
        })
        .collect();
    assert_eq!(capped.len(), 2, "{ev:?}");
    assert!(capped.iter().all(|id| !deferred.contains(id)), "{capped:?}");
}

/// Whether the screen dropped `id` as an expired carry.
fn expired(ev: &[StreamEvent], id: &str) -> bool {
    ev.iter().any(|e| {
        matches!(e, StreamEvent::Moderation { rule, msg_id, action, .. }
            if rule == CARRY_RULE && msg_id.as_deref() == Some(id) && action == "expired")
    })
}

/// A fresh first contact that fills a beat's ceiling (one pick, required chat first) ahead of
/// anything carried. (Two, before the ceiling became one: the second was carried and the carry
/// cap, not the clock, then dropped the oldest.)
fn fresh_pair(tag: &str) -> (Vec<ChatMsg>, Vec<Candidate>) {
    let inbox: Vec<ChatMsg> = (0..1)
        .map(|i| {
            msg(
                &format!("{tag}-{i}"),
                &format!("fan-{tag}-{i}"),
                &format!("malakor, is the {tag} moat {i} deep?"),
            )
        })
        .collect();
    let cands = inbox.iter().map(cand).collect();
    (inbox, cands)
}

#[test]
fn a_deferral_is_held_through_the_cold_open_and_expires_25_s_after_the_room_opens() {
    // sbx-20261021-011413-5m: four deferred messages expired during a 113 s cold open, before
    // the room ever opened to answer them.
    let mut room = Room::new();
    regular(&mut room, "beetle");
    quiet(&mut room, 0, COLD_OPEN_STATE, None);
    let q = msg("q", "beetle", "malakor what is the plan tonight?");
    room.beat(
        5_000,
        COLD_OPEN_STATE,
        None,
        vec![q.clone()],
        vec![cand(&q)],
    );
    for t in (10_000..=120_000).step_by(5_000) {
        let (_, ids, _, ev) = room.beat_in(t, &ctx(COLD_OPEN_STATE), None, Vec::new(), Vec::new());
        assert!(ids.is_empty(), "the cold open never passes a regular");
        assert!(
            !expired(&ev, "q"),
            "nothing expires while the room fills ({t})"
        );
    }
    // The room opens at 125 s, but fresher chat fills every beat: it is dropped 25 s after the
    // opening beat's drain, with a record.
    let mut expired_at = None;
    for t in (125_000..=160_000).step_by(5_000) {
        let (inbox, cands) = fresh_pair(&t.to_string());
        let (_, ids, _, ev) = room.beat_in(t, &ctx("opening"), None, inbox, cands);
        assert!(!ids.contains(&"q".to_string()), "{ids:?}");
        if expired(&ev, "q") {
            expired_at = Some(t);
        }
    }
    assert_eq!(
        expired_at,
        Some(150_000),
        "the room opened at 125 s, plus 25 s"
    );
    let (_, ids, _) = room.beat(170_000, "opening", None, Vec::new(), Vec::new());
    assert!(ids.is_empty(), "an expired deferral never comes back");
}

#[test]
fn a_gift_in_the_cold_open_is_carried_and_thanked_when_the_room_opens() {
    // sbx-20261021-004215-5m: "gifting 5 subs" from a regular arrived after the cold-open
    // handoff, the filter passed only first contacts, and the gift was never carried.
    let prep = sample_prep();
    let mut room = Room::new();
    regular(&mut room, "badger");
    for t in [5_000, 10_000, 15_000, 20_000] {
        quiet(&mut room, t, COLD_OPEN_STATE, Some(&prep));
    }
    let g = msg(
        "g",
        "badger",
        "gifting 5 subs to the minions for the deposit fund",
    );
    let (p, ids, e) = room.beat(
        25_000,
        COLD_OPEN_STATE,
        Some(&prep),
        vec![g.clone()],
        vec![cand(&g)],
    );
    assert!(
        ids.is_empty(),
        "the cold open passes first contacts only: {}",
        p.reason
    );
    assert!(reason(&e).contains("deferred: g"), "{}", reason(&e));
    // A long cold open: held, never expired.
    for t in (30_000..=90_000).step_by(5_000) {
        let (_, ids, _, ev) = room.beat_in(
            t,
            &ctx(COLD_OPEN_STATE),
            Some(&prep),
            Vec::new(),
            Vec::new(),
        );
        assert!(ids.is_empty());
        assert!(!expired(&ev, "g"), "{t}");
    }
    // The room opens: the first beat answers it.
    let (p, ids, e) = room.beat(95_000, "opening", Some(&prep), Vec::new(), Vec::new());
    assert_eq!(ids, ["g"], "{}", p.reason);
    assert!(reason(&e).contains("gift"), "{}", reason(&e));
    assert!(
        reason(&e).contains("carried over: g (70 s)"),
        "{}",
        reason(&e)
    );
}

#[test]
fn the_cold_open_cap_keeps_first_contacts_oldest_first() {
    let mut room = Room::new();
    for v in ["r1", "r2", "r3", "r4", "r5"] {
        regular(&mut room, v);
    }
    // A hold beat (speech queued) defers everything: two first contacts, then five regulars,
    // then two more first contacts, a beat apart.
    room.speaking_until = Some(200_000);
    let mut t = 5_000;
    let mut beat = |room: &mut Room, id: &str, viewer: &str| {
        let m = msg(id, viewer, "malakor, what is the plan tonight?");
        room.beat(t, COLD_OPEN_STATE, None, vec![m.clone()], vec![cand(&m)]);
        t += 5_000;
    };
    beat(&mut room, "f1", "new1");
    beat(&mut room, "f2", "new2");
    for v in ["r1", "r2", "r3", "r4", "r5"] {
        beat(&mut room, &format!("q-{v}"), v);
    }
    beat(&mut room, "f3", "new3");
    beat(&mut room, "f4", "new4");
    let kept: Vec<String> = room
        .o
        .carried
        .iter()
        .map(|k| k.cand.msg.id.clone())
        .collect();
    assert_eq!(
        kept,
        ["f1", "f2", "f3", "f4", "q-r5", "q-r4"],
        "first contacts first, the oldest first; then the newest of the rest"
    );
}

const OPEN_P1: &str = "open crowd poll p1: \"Moat or swamp?\" | moat = 2.0 (1 votes), \
swamp = 1.0 (1 votes) | closes in 4 s | min rank Thrall";
const OPEN_P2: &str = "open crowd poll p2: \"Soup or bread?\" | soup = 1.0 (1 votes), \
bread = 1.0 (1 votes) | closes in 4 s | min rank Thrall";

#[test]
fn a_closed_poll_is_announced_once_with_its_result_and_expires_the_queued_bit() {
    let mut prep = sample_prep();
    prep.bits.retain(|b| b.id == "s-01");
    let mut room = Room::new();
    let mut t = 0;
    // A story is being told while the poll runs.
    loop {
        t += 5_000;
        let c = ctx_with("opening", OPEN_P1);
        let (p, _, _, _) = room.beat_in(t, &c, Some(&prep), vec![], vec![]);
        if p.bit_id.as_deref() == Some("s-01") {
            break;
        }
    }
    let closed = ctx_with("opening", "no poll open\nlast poll p1 closed: winner moat");
    let (p, _, _, _) = room.beat_in(t + 5_000, &closed, Some(&prep), vec![], vec![]);
    assert_eq!(p.mode, BeatMode::Bridge, "{}", p.reason);
    assert!(p.reason.contains("bit s-01 expired"), "{}", p.reason);
    assert!(p.reason.contains("announce poll p1 closed"), "{}", p.reason);
    let note = p.room_note.unwrap();
    assert!(
        note.contains("\"Moat or swamp?\"") && note.contains("winner moat"),
        "{note}"
    );
    assert!(
        note.contains("Announce the result now and act on it"),
        "{note}"
    );
    assert_eq!(p.note.as_deref(), Some(ANNOUNCE_NOTE));
    // Once only; the expired story never resumes.
    for k in 2..6 {
        let (p, _, _, _) = room.beat_in(t + 5_000 * k, &closed, Some(&prep), vec![], vec![]);
        assert!(p.room_note.is_none(), "{}", p.reason);
        assert_ne!(p.bit_id.as_deref(), Some("s-01"), "{}", p.reason);
    }
}

#[test]
fn a_tied_poll_is_announced_as_a_tie_after_a_safety_beat_with_the_chat() {
    let mut room = Room::new();
    room.beat_in(5_000, &ctx_with("opening", OPEN_P2), None, vec![], vec![]);
    let tie = ctx_with(
        "opening",
        "last poll p2 closed: TIE between soup and bread (1 votes each): you break it, and \
         say you are breaking it",
    );
    // The poll closes on a safety beat: held back.
    let (p, _, _, _) = room.beat_in(10_000, &tie, None, vec![], vec![safety("s", "sad")]);
    assert!(p.room_note.is_none());
    assert!(p.reason.contains("room event held back"), "{}", p.reason);
    // The next beat announces it, with the chat that came in.
    let m = msg("m", "fan", "malakor so who won the soup vote?");
    let (p, ids, _, _) = room.beat_in(15_000, &tie, None, vec![m.clone()], vec![cand(&m)]);
    assert_eq!(p.mode, BeatMode::React);
    assert_eq!(ids, ["m"]);
    assert!(
        p.reason.contains("announce poll p2 closed (tie) with chat"),
        "{}",
        p.reason
    );
    let note = p.room_note.unwrap();
    assert!(note.contains("TIE between soup and bread"), "{note}");
    assert!(
        note.contains("as a tie (never call either option the winner)"),
        "{note}"
    );
    let (p, _, _, _) = room.beat_in(20_000, &tie, None, vec![], vec![]);
    assert!(p.room_note.is_none(), "announced once");
}

#[test]
fn a_foil_vote_won_and_a_handover_due_are_announced_once_each() {
    let prep = sample_prep();
    let mut room = Room::new();
    let open = ctx_with(
        "foil_vote",
        "open foil vote f1: \"Who joins?\" | the-inspector = 3.0 (2 votes) | closes in 2 s \
         | min rank Thrall",
    );
    room.beat_in(5_000, &open, Some(&prep), vec![], vec![]);
    let won = ctx_with("foil_active", "no poll open\nactive foil: the-inspector");
    let (p, _, _, _) = room.beat_in(10_000, &won, Some(&prep), vec![], vec![]);
    let note = p.room_note.unwrap_or_default();
    assert!(
        note.contains("\"Who joins?\"") && note.contains("the-inspector won"),
        "{note}"
    );

    let due = ctx_with(
        "handover_pending",
        "no poll open\nHANDOVER DUE: let the foil dare you, then emit switch_segment to SEG-02A",
    );
    let (p, _, _, _) = room.beat_in(15_000, &due, Some(&prep), vec![], vec![]);
    assert_eq!(p.mode, BeatMode::Bridge, "{}", p.reason);
    assert!(p.reason.contains("announce handover due"), "{}", p.reason);
    assert!(p.room_note.unwrap().contains("HANDOVER DUE"));
    assert!(
        p.material.unwrap().contains("another chamber calls"),
        "the into_handover connector"
    );
    let (p, _, _, _) = room.beat_in(20_000, &due, Some(&prep), vec![], vec![]);
    assert!(p.room_note.is_none(), "{}", p.reason);
}

#[test]
fn after_the_handoff_the_cold_open_voices_the_deck_and_one_gathering_line_at_most() {
    let prep = sample_prep();
    let mut room = Room::new();
    for t in [5_000, 10_000, 15_000, 20_000] {
        quiet(&mut room, t, COLD_OPEN_STATE, Some(&prep));
    }
    let mut voiced: Vec<(BeatMode, String)> = Vec::new();
    let mut gathering = 0;
    for k in 1..=16 {
        let p = quiet(&mut room, 20_000 + 5_000 * k, COLD_OPEN_STATE, Some(&prep));
        if p.note.as_deref() == Some(GATHERING_NOTE) {
            gathering += 1;
        }
        if p.mode != BeatMode::Hold {
            voiced.push((p.mode, p.reason.clone()));
        }
    }
    // "room filling: deck premise p-02" -> "premise".
    let kinds: Vec<&str> = voiced
        .iter()
        .map(|(_, r)| r.split(' ').nth(3).unwrap_or(""))
        .collect();
    assert!(
        kinds.starts_with(&["premise", "premise", "premise", "story", "story"]),
        "premises first, then the rest of the deck: {voiced:?}"
    );
    assert!(voiced[..5]
        .iter()
        .all(|v| v.0 == BeatMode::Bit && v.1.starts_with("room filling: deck ")));
    assert_eq!(gathering, 1, "{voiced:?}");
    assert!(
        voiced[5].1.contains("deck empty, one gathering line"),
        "{voiced:?}"
    );
    assert!(
        voiced[6..].iter().all(|v| v.0 == BeatMode::Improvise),
        "{voiced:?}"
    );
}

#[test]
fn without_a_prep_gathering_lines_are_20_s_apart_with_holds_and_asides_between() {
    let mut room = Room::new();
    let mut at: Vec<TsMs> = Vec::new();
    let mut spoke: Vec<TsMs> = Vec::new();
    for k in 1..=16 {
        let t = 5_000 * k;
        let p = quiet(&mut room, t, COLD_OPEN_STATE, None);
        match p.mode {
            BeatMode::ColdOpen => {
                assert_eq!(p.note.as_deref(), Some(GATHERING_NOTE));
                at.push(t);
            }
            BeatMode::Improvise => {
                assert_eq!(p.note.as_deref(), Some(ASIDE_NOTE), "{}", p.reason);
                assert!(p.reason.contains("no prep: aside"), "{}", p.reason);
            }
            BeatMode::Hold => assert!(p.reason.contains("no prep: hold"), "{}", p.reason),
            m => panic!("{m:?}: {}", p.reason),
        }
        if p.mode != BeatMode::Hold {
            spoke.push(t);
        }
    }
    assert!(at.len() >= 3, "{at:?}");
    assert!(
        at.windows(2).all(|w| w[1] - w[0] >= GATHERING_SPACING_MS),
        "{at:?}"
    );
    assert!(
        spoke.windows(2).all(|w| w[1] - w[0] <= 10_000),
        "never a long silence: {spoke:?}"
    );
}

// ----- speech pacing ----------------------------------------------------------------------

#[test]
fn queued_speech_holds_the_beat_but_never_a_safety_or_a_closing_beat() {
    let mut room = Room::new();
    regular(&mut room, "beetle");
    room.speaking_until = Some(12_400);
    let q = msg("q", "beetle", "malakor, who holds the moat?");
    let (p, ids, e) = room.beat(5_000, "opening", None, vec![q.clone()], vec![cand(&q)]);
    assert_eq!(p.mode, BeatMode::Hold);
    assert!(p.speaking);
    assert!(ids.is_empty());
    assert!(
        reason(&e).starts_with("speaking: 7.4 s queued"),
        "{}",
        reason(&e)
    );
    // Within the lead (1.5 s) the beat goes ahead.
    let (p, ids, _) = room.beat(11_000, "opening", None, Vec::new(), Vec::new());
    assert_eq!(ids, ["q"], "{}", p.reason);
    // A disclosure is answered at once, speech or no speech.
    room.speaking_until = Some(30_000);
    let (p, ids, _) = room.beat(
        15_000,
        "opening",
        None,
        Vec::new(),
        vec![safety("s", "sad")],
    );
    assert_eq!(ids, ["s"], "{}", p.reason);
    assert!(!p.speaking);
    // So is a closing beat: the sign-off is never held.
    room.closing = true;
    let (p, _, _) = room.beat(20_000, "opening", None, Vec::new(), Vec::new());
    assert!(!p.speaking, "{}", p.reason);
    assert_ne!(p.mode, BeatMode::Hold, "{}", p.reason);
}

#[test]
fn a_room_event_waits_for_the_drain_then_goes_ahead_of_the_bit() {
    let mut prep = sample_prep();
    prep.bits.retain(|b| b.id == "s-01");
    let mut room = Room::new();
    let mut t = 0;
    loop {
        t += 5_000;
        let c = ctx_with("opening", OPEN_P1);
        let (p, _, _, _) = room.beat_in(t, &c, Some(&prep), vec![], vec![]);
        if p.bit_id.as_deref() == Some("s-01") {
            break;
        }
    }
    // The story line is long: the poll closes while it is still being spoken.
    room.speaking_until = Some(t + 12_000);
    let closed = ctx_with("opening", "no poll open\nlast poll p1 closed: winner moat");
    let (p, _, _, _) = room.beat_in(t + 5_000, &closed, Some(&prep), vec![], vec![]);
    assert!(p.speaking, "{}", p.reason);
    assert!(p.room_note.is_none(), "not announced over his speech");
    // Drained: the announcement comes first, and the queued story yields to it.
    let (p, _, _, _) = room.beat_in(t + 15_000, &closed, Some(&prep), vec![], vec![]);
    assert!(p.reason.contains("announce poll p1 closed"), "{}", p.reason);
    assert!(p.reason.contains("bit s-01 expired"), "{}", p.reason);
    assert!(p.room_note.unwrap().contains("winner moat"));
}

#[test]
fn carried_chat_survives_a_long_drain_and_expires_25_s_after_it() {
    let mut room = Room::new();
    regular(&mut room, "beetle");
    regular(&mut room, "moth");
    // 30 s of speech queued when the question arrives.
    room.speaking_until = Some(35_000);
    let q = msg("q", "beetle", "malakor, who holds the moat?");
    let (p, _, e) = room.beat(5_000, "opening", None, vec![q.clone()], vec![cand(&q)]);
    assert!(p.speaking);
    assert!(reason(&e).contains("deferred: q"), "{}", reason(&e));
    for t in (10_000..=30_000).step_by(5_000) {
        let (p, ids, _, ev) = room.beat_in(t, &ctx("opening"), None, Vec::new(), Vec::new());
        assert!(p.speaking && ids.is_empty(), "{}", p.reason);
        assert!(
            !ev.iter()
                .any(|e| matches!(e, StreamEvent::Moderation { rule, .. } if rule == CARRY_RULE)),
            "not expired while he speaks"
        );
    }
    // 35 s after it arrived, once the speech aired: still answered.
    let (_, ids, e) = room.beat(35_000, "opening", None, Vec::new(), Vec::new());
    assert_eq!(ids, ["q"]);
    assert!(
        reason(&e).contains("carried over: q (30 s)"),
        "{}",
        reason(&e)
    );

    // A second one, never answered (a full ceiling of fresher picks every beat): it expires
    // 25 s after the drain.
    let mut room = Room::new();
    regular(&mut room, "beetle");
    room.speaking_until = Some(20_000);
    let q = msg("q2", "beetle", "malakor what is the plan tonight?");
    room.beat(5_000, "opening", None, vec![q.clone()], vec![cand(&q)]);
    let mut expired_at = None;
    for t in (10_000..=50_000).step_by(5_000) {
        let (inbox, cands) = fresh_pair(&t.to_string());
        let (_, ids, _, ev) = room.beat_in(t, &ctx("opening"), None, inbox, cands);
        assert!(!ids.contains(&"q2".to_string()), "{ids:?}");
        if expired(&ev, "q2") {
            expired_at = Some(t);
        }
    }
    assert_eq!(expired_at, Some(45_000), "the drain at 20 s, plus 25 s");
}

#[test]
fn an_owned_request_passes_the_cold_open_filter_and_is_never_capped() {
    let prep = sample_prep();
    let mut room = Room::new();
    // A regular (seen before) asks to be roasted while the room fills: it reaches the voice.
    room.o.observe_inbox(&[msg("old", "pigeon", "hi")]);
    let r = msg(
        "r",
        "pigeon",
        "i lost yesterday's vote, roast me hard no mercy",
    );
    let (p, ids, e) = room.beat(
        5_000,
        COLD_OPEN_STATE,
        Some(&prep),
        vec![r.clone()],
        vec![cand(&r)],
    );
    assert_eq!(p.mode, BeatMode::ColdOpen);
    assert_eq!(ids, ["r"]);
    let StreamEvent::BeatPlan { reason, .. } = e else {
        panic!("a beat_plan")
    };
    assert!(reason.contains("owed: r"), "{reason}");

    // After the cold open: a hold carries the request with seven questions; the cap of six
    // drops questions, never the request.
    let mut inbox = vec![msg("r2", "otter", "roast me hard malakor, i can take it")];
    for k in 0..7 {
        inbox.push(msg(
            &format!("q{k}"),
            &format!("asker{k}"),
            "malakor what is in the moat?",
        ));
    }
    room.speaking_until = Some(60_000);
    let cands: Vec<Candidate> = inbox.iter().map(cand).collect();
    let (p, _, _, ev) = room.beat_in(50_000, &ctx("opening"), Some(&prep), inbox, cands);
    assert_eq!(p.mode, BeatMode::Hold);
    let capped: Vec<&StreamEvent> = ev
        .iter()
        .filter(|e| matches!(e, StreamEvent::Moderation { action, .. } if action == CAPPED))
        .collect();
    assert_eq!(capped.len(), 2, "{ev:?}");
    assert!(!format!("{capped:?}").contains("\"r2\""), "{capped:?}");
}

// ── The segment leads (2026-10-24 rebalance) ──────────────────────────

/// `n` regulars (seen before, so never first contacts) named `v0..`.
fn regulars(room: &mut Room, n: usize) {
    for i in 0..n {
        regular(room, &format!("v{i}"));
    }
}

/// A SEG-02A context whose rules name the case.
fn case_ctx() -> SegmentContext {
    SegmentContext {
        segment: SegmentId::Seg02aHexmap,
        rules_prompt: "SEGMENT SEG-02A.\nCase: The Lantern Guild Swindle (case-7), 3 hexes. THIS \
                       CASE IS FICTIONAL DEMO DATA."
            .into(),
        state_summary: "segment Hexmap Investigation (Hexmap), state investigating\nno poll open"
            .into(),
        state: "investigating".into(),
        allowed_ops: Vec::new(),
        ccv: 40,
        elapsed_s: 300,
        hexes: Vec::new(),
    }
}

#[test]
fn a_busy_room_of_off_topic_chat_gets_segment_beats_and_one_off_topic_answer_per_30_s() {
    // The diagnosis: 260 sim runs turned every segment into a roast show, 46% of lines opening
    // with a viewer's handle. Twenty regulars chat remarks and remarks to the host by name.
    let prep = sample_prep();
    let mut room = Room::new();
    regulars(&mut room, 20);
    let mut answered: Vec<TsMs> = Vec::new();
    let mut modes: Vec<BeatMode> = Vec::new();
    for k in 0..36i64 {
        let t = 5_000 * (k + 1);
        let inbox: Vec<ChatMsg> = (0..8i64)
            .map(|j| {
                let v = format!("v{}", (k * 8 + j) % 20);
                let text = if j % 2 == 0 {
                    format!("malakor you look tired {j}")
                } else {
                    format!("my soup went cold, round {k}")
                };
                msg(&format!("m{k}-{j}"), &v, &text)
            })
            .collect();
        let cands: Vec<Candidate> = inbox.iter().map(cand).collect();
        let (p, ids, e) = room.beat(t, "opening", Some(&prep), inbox, cands);
        assert!(!room.o.small_room(), "twenty chatters");
        assert!(ids.len() <= 1, "{ids:?}");
        assert_ne!(p.mode, BeatMode::Hold, "the segment goes on: {}", p.reason);
        let r = reason(&e);
        if ids.is_empty() {
            assert!(r.starts_with("segment (carried "), "{r}");
        } else {
            assert!(r.starts_with("segment+off-topic: mention"), "{r}");
            answered.push(t);
        }
        modes.push(p.mode);
    }
    assert!(
        !answered.is_empty(),
        "a direct remark is still answered now and then"
    );
    assert!(
        answered
            .windows(2)
            .all(|w| w[1] - w[0] >= OFF_TOPIC_ANSWER_MS),
        "at most one off-topic answer per 30 s: {answered:?}"
    );
    assert!(answered.len() <= 7, "{answered:?}");
    // The segment's own material carries the beats between: its prepared bits.
    assert!(modes.contains(&BeatMode::Bit), "{modes:?}");
}

#[test]
fn a_question_about_the_case_is_folded_in_as_the_one_pick() {
    let mut room = Room::new();
    regulars(&mut room, 8);
    let c = case_ctx();
    room.topic = crate::topic::topic_words(&c, &[]);
    assert!(
        room.topic.contains(&"lantern".to_string()),
        "{:?}",
        room.topic
    );
    let mut inbox: Vec<ChatMsg> = (1..8)
        .map(|i| {
            msg(
                &format!("off{i}"),
                &format!("v{i}"),
                "malakor are you even awake rn?",
            )
        })
        .collect();
    inbox.push(msg("q", "v0", "wait who actually runs the lantern guild?"));
    let cands: Vec<Candidate> = inbox.iter().map(cand).collect();
    let (p, ids, e, _) = room.beat_in(5_000, &c, None, inbox, cands);
    assert_eq!(ids, ["q"], "{}", p.reason);
    assert_eq!(p.ceiling, 1);
    assert!(
        reason(&e).starts_with("segment+chat: topic"),
        "{}",
        reason(&e)
    );
}

#[test]
fn a_gift_is_always_folded_in() {
    let prep = sample_prep();
    let mut room = Room::new();
    regulars(&mut room, 10);
    let c = case_ctx();
    room.topic = crate::topic::topic_words(&c, &[]);
    let mut inbox = vec![
        msg("q", "v0", "who runs the lantern guild?"),
        msg("idea", "v1", "what if the guild is a front for the goblins"),
        msg("g", "v9", "gifting 5 subs to the minions"),
    ];
    inbox.extend((2..8).map(|i| msg(&format!("m{i}"), &format!("v{i}"), "malakor hi?")));
    let cands: Vec<Candidate> = inbox.iter().map(cand).collect();
    let (p, ids, e, _) = room.beat_in(5_000, &c, Some(&prep), inbox, cands);
    assert_eq!(ids, ["g"], "required chat first: {}", p.reason);
    assert!(reason(&e).contains("segment+chat: gift"), "{}", reason(&e));
    // The next beat folds in the on-topic chat it carried.
    let (_, ids, _, _) = room.beat_in(10_000, &c, Some(&prep), Vec::new(), Vec::new());
    assert!(ids == ["q"] || ids == ["idea"], "{ids:?}");

    // Mid-story, a gift skips the story's next line and is the pick.
    let mut prep2 = sample_prep();
    prep2.bits.retain(|b| b.id == "s-01");
    let mut room = Room::new();
    regulars(&mut room, 10);
    let mut t = 0;
    loop {
        t += 5_000;
        if quiet(&mut room, t, "opening", Some(&prep2))
            .bit_id
            .as_deref()
            == Some("s-01")
        {
            break;
        }
    }
    let g = msg("g2", "v3", "just subbed for 3 months, take my tribute");
    let (p, ids, _) = room.beat(
        t + 5_000,
        "opening",
        Some(&prep2),
        vec![g.clone()],
        vec![cand(&g)],
    );
    assert_eq!(ids, ["g2"], "{}", p.reason);
    assert!(
        p.reason
            .contains("room answered story s-01: line 3 skipped"),
        "{}",
        p.reason
    );
}

#[test]
fn on_topic_chat_folds_into_the_bit_being_told() {
    let mut prep = sample_prep();
    prep.bits.retain(|b| b.id == "s-01");
    let mut room = Room::new();
    regulars(&mut room, 10);
    let mut t = 0;
    loop {
        t += 5_000;
        if quiet(&mut room, t, "opening", Some(&prep))
            .bit_id
            .as_deref()
            == Some("s-01")
        {
            break;
        }
    }
    // A big room, so only the topic makes this on-topic.
    room.topic = vec!["ghosts".into()];
    let inbox: Vec<ChatMsg> = (0..8)
        .map(|i| {
            let text = if i == 0 {
                "did the ghosts really sue you?"
            } else {
                "malakor you are so old lol"
            };
            msg(&format!("m{i}"), &format!("v{i}"), text)
        })
        .collect();
    let cands: Vec<Candidate> = inbox.iter().map(cand).collect();
    let (p, ids, _) = room.beat(t + 5_000, "opening", Some(&prep), inbox, cands);
    assert_eq!(p.mode, BeatMode::Mixed, "{}", p.reason);
    assert_eq!(ids, ["m0"]);
    assert!(
        p.reason
            .starts_with("segment+chat: topic: story s-01 lines 3-4/4"),
        "the story goes on, no line skipped: {}",
        p.reason
    );
}

#[test]
fn a_small_room_gets_every_message_answered_one_a_beat() {
    let mut room = Room::new();
    regulars(&mut room, 3);
    // Three regulars, plain remarks: in a big room none would be worth a beat.
    let inbox = vec![
        msg("a", "v0", "the soup in my kitchen went cold"),
        msg("b", "v1", "my cat is watching with me tonight"),
        msg("c", "v2", "rainy evening over here"),
    ];
    let cands: Vec<Candidate> = inbox.iter().map(cand).collect();
    let (p, ids, e) = room.beat(5_000, "opening", None, inbox, cands);
    assert!(room.o.small_room());
    assert_eq!(ids.len(), 1, "{}", p.reason);
    assert!(
        reason(&e).starts_with("small-room segment+chat: chat"),
        "{}",
        reason(&e)
    );
    let mut answered: Vec<String> = ids;
    for t in [10_000, 15_000, 20_000] {
        let (_, ids, _) = room.beat(t, "opening", None, Vec::new(), Vec::new());
        assert!(ids.len() <= 1, "{ids:?}");
        answered.extend(ids);
    }
    answered.sort();
    assert_eq!(answered, ["a", "b", "c"], "all three within three beats");

    // Hysteresis: six chatters keep the room small, seven end it; it is small again once the
    // window holds five or fewer.
    for (k, v) in ["v3", "v4", "v5"].iter().enumerate() {
        let m = msg(&format!("x{k}"), v, "hello all");
        room.beat(
            25_000 + 1_000 * k as i64,
            "opening",
            None,
            vec![m.clone()],
            vec![cand(&m)],
        );
    }
    assert!(room.o.small_room(), "six chatters: still small");
    let m = msg("x9", "v6", "hello all");
    room.beat(30_000, "opening", None, vec![m.clone()], vec![cand(&m)]);
    assert!(!room.o.small_room(), "seven chatters: not small");
    let m = msg("late", "v0", "still here");
    room.beat(
        30_000 + SMALL_ROOM_WINDOW_MS,
        "opening",
        None,
        vec![m.clone()],
        vec![cand(&m)],
    );
    assert!(room.o.small_room(), "the window holds one chatter");
}

#[test]
fn a_bare_remark_never_makes_a_beat_in_a_big_room() {
    let mut room = Room::new();
    regulars(&mut room, 10);
    let inbox: Vec<ChatMsg> = (0..8)
        .map(|i| {
            msg(
                &format!("r{i}"),
                &format!("v{i}"),
                &format!("my evening walk went fine, part {i}"),
            )
        })
        .collect();
    let cands: Vec<Candidate> = inbox.iter().map(cand).collect();
    let (p, ids, e) = room.beat(5_000, "opening", None, inbox, cands);
    assert!(ids.is_empty(), "{}", p.reason);
    assert!(
        reason(&e).starts_with("segment (carried 8)"),
        "{}",
        reason(&e)
    );
    assert_eq!(p.mode, BeatMode::Improvise);
}
