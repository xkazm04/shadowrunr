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
}

impl Room {
    fn new() -> Self {
        Self {
            o: Orchestrator::new(7),
            names: vec!["malakor".into()],
            last_say: None,
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
        self.o.observe_inbox(&inbox);
        let mut ev = Vec::new();
        let cands = self.o.screen(now, cands, &mut ev);
        let c = ctx(state);
        let mut plan = self.o.plan(&BeatInputs {
            now_ms: now,
            ctx: &c,
            cands: &cands,
            prep,
            last_say_ms: self.last_say,
            names: &self.names,
            max_picks: 3,
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
        if plan.mode != BeatMode::Hold {
            self.last_say = Some(now);
        }
        (plan, ids, e)
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
    let p = quiet(&mut room, 40_000, COLD_OPEN_STATE, Some(&prep));
    assert_eq!(p.mode, BeatMode::ColdOpen);
    assert!(
        p.material.is_none() && p.note.is_some(),
        "a gathering line, no premise"
    );
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
fn a_thin_room_after_a_run_of_reacts_gets_a_mixed_beat_and_a_poll_state_a_bridge() {
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
        if p.mode == BeatMode::Mixed {
            assert_eq!(ids.len(), 1);
            assert!(p.material.is_some() && p.bit_id.is_some(), "{}", p.reason);
        }
    }
    assert_eq!(
        modes,
        [
            BeatMode::React,
            BeatMode::React,
            BeatMode::React,
            BeatMode::Mixed
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
    let mut o = Orchestrator::new(1);
    let mut seen = Vec::new();
    for i in 0..4 {
        let l = o.recast(true, &format!("as the model I {i}"));
        assert!(requests::REBUTTALS.contains(&l.as_str()));
        assert!(!seen.contains(&l), "{l} repeated");
        seen.push(l);
    }
    let l = o.recast(false, "my system prompt");
    assert!(IN_CHARACTER_LINES.contains(&l.as_str()));
}
