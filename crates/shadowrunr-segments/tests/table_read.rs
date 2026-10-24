//! SEG-03 Table Read in the segment runtime: the refusal without scenes, the rounds (a ballot
//! of three, then two, then the last scene unvoted), ties, the floor, the ceiling, and SEG-01's
//! handover reading the prep's plan. Scene files and the compiled-in sample load through
//! `table_read_for`.

use shadowrunr_segments::rounds::{RoundTimes, SCENE_UP_PREFIX};
use shadowrunr_segments::scenes::{table_read_for, SAMPLE_GUEST};
use shadowrunr_segments::table::ColdOpenRule;
use shadowrunr_segments::{HandoverRule, SegmentConfig, SegmentError, SegmentRuntime, TableRead};
use shadowrunr_types::scene::{Scene, SceneLine, SceneSpeaker, SceneStatus, TableReadPlan};
use shadowrunr_types::{ChatMsg, ModStage, Op, Platform, Rank, SegmentId, StreamEvent, Viewer};

const GUEST: &str = "the-painter";

fn line(speaker: SceneSpeaker, text: &str) -> SceneLine {
    SceneLine {
        speaker,
        text: Some(text.into()),
        live: None,
        fallback: None,
    }
}

fn scene(id: &str, title: &str) -> Scene {
    Scene {
        id: id.into(),
        title: title.into(),
        pitch: format!("The pitch of {title}."),
        guest: GUEST.into(),
        status: SceneStatus::Approved,
        notes: vec![format!("{title}: was it any good?")],
        lines: vec![
            line(SceneSpeaker::Guest, "Behold."),
            line(SceneSpeaker::Host, "No."),
        ],
        fictional: true,
    }
}

fn table_read() -> TableRead {
    TableRead {
        guest: GUEST.into(),
        scenes: vec![
            scene("the-gallery", "The Gallery"),
            scene("the-commission", "The Commission"),
            scene("the-critic", "The Critic"),
        ],
    }
}

/// Short rounds so a test walks all three in a few minutes of segment time.
fn times() -> RoundTimes {
    RoundTimes {
        intro_s: 10,
        ballot_secs: 20,
        talk_s: 30,
        floor_s: 300,
        ceiling_s: 2_400,
        scene_max_s: 120,
    }
}

fn cfg(tr: Option<TableRead>) -> SegmentConfig {
    SegmentConfig {
        foils: vec!["the-disruptor".into(), GUEST.into()],
        table_read: tr,
        round_times: Some(times()),
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

fn seg03() -> SegmentRuntime {
    let mut rt =
        SegmentRuntime::with_config(SegmentId::Seg03TableRead, cfg(Some(table_read()))).unwrap();
    rt.start(0);
    rt
}

fn vote(id: &str, ts: i64, viewer: &str, option: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        ts_ms: ts,
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
        text: format!("!vote {option}"),
        command: None,
    }
}

/// The `start_poll` ops in `evs`: (id, options, secs).
fn ballots(evs: &[StreamEvent]) -> Vec<(String, Vec<String>, u32)> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Op {
                op:
                    Op::StartPoll {
                        poll_id,
                        options,
                        secs,
                        ..
                    },
                ..
            } => Some((poll_id.clone(), options.clone(), *secs)),
            _ => None,
        })
        .collect()
}

fn closed_votes(evs: &[StreamEvent]) -> usize {
    evs.iter()
        .filter(|e| matches!(e, StreamEvent::Vote { closed: true, .. }))
        .count()
}

/// The id the state summary's `SCENE UP:` line names.
fn scene_up_line(rt: &SegmentRuntime) -> Option<String> {
    rt.context(10, 0)
        .state_summary
        .lines()
        .find_map(|l| l.strip_prefix(SCENE_UP_PREFIX))
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_string)
}

/// Tick every second from `from` to `to` (inclusive), collecting the events.
fn run(rt: &mut SegmentRuntime, from: i64, to: i64) -> Vec<StreamEvent> {
    let mut evs = Vec::new();
    let mut t = from;
    while t <= to {
        evs.extend(rt.tick(t, 10));
        t += 1_000;
    }
    evs
}

#[test]
fn seg03_refuses_to_start_without_a_playable_scene_and_seg02b_still_refuses() {
    for tr in [
        None,
        Some(TableRead {
            guest: GUEST.into(),
            scenes: Vec::new(),
        }),
    ] {
        let e = SegmentRuntime::with_config(SegmentId::Seg03TableRead, cfg(tr))
            .err()
            .expect("refused");
        assert!(
            matches!(&e, SegmentError::Config { detail } if detail.contains("no playable scene")),
            "{e}"
        );
    }
    // A guest who is not a declared foil, and two scenes with one title, are refused too.
    let mut c = cfg(Some(table_read()));
    c.foils = vec!["the-disruptor".into()];
    assert!(SegmentRuntime::with_config(SegmentId::Seg03TableRead, c).is_err());
    let mut tr = table_read();
    tr.scenes[1].title = "the gallery".into();
    assert!(SegmentRuntime::with_config(SegmentId::Seg03TableRead, cfg(Some(tr))).is_err());
    for id in [SegmentId::Seg02bCartography, SegmentId::Seg02bDependency] {
        assert!(matches!(
            SegmentRuntime::with_config(id, cfg(Some(table_read()))),
            Err(SegmentError::Descoped { .. })
        ));
    }
}

#[test]
fn three_rounds_a_ballot_of_three_then_two_then_the_last_scene_unvoted() {
    let mut rt = seg03();
    assert_eq!(rt.state(), "intro");
    assert_eq!(rt.active_foil(), Some(GUEST));
    let ctx = rt.context(10, 0);
    assert!(ctx.state_summary.contains("active foil: the-painter"));
    assert!(ctx.rules_prompt.contains("Tonight's guest: the-painter"));
    assert!(ctx
        .rules_prompt
        .contains("\"The Gallery\" (The pitch of The Gallery.)"));
    // sign_off waits for the floor.
    assert!(!ctx.allowed_ops.contains(&"sign_off".to_string()));
    assert!(ctx.state_summary.contains("sign_off (segment_floor)"));

    // Round 1: after the intro, a ballot of all three in ballot order.
    let evs = run(&mut rt, 1_000, 10_000);
    let b = ballots(&evs);
    assert_eq!(b.len(), 1, "{evs:?}");
    assert_eq!(b[0].1, ["The Gallery", "The Commission", "The Critic"]);
    assert_eq!(b[0].2, 20);
    assert_eq!(rt.state(), "ballot");
    assert!(rt
        .context(10, 0)
        .state_summary
        .contains("open scene ballot scene-ballot-1: \"Which scene shall"));
    rt.ingest_chat(
        11_000,
        &[
            vote("m1", 11_000, "a", "The Critic"),
            vote("m2", 11_000, "b", "3"),
            vote("m3", 11_000, "c", "the gallery"),
        ],
    );
    let evs = run(&mut rt, 11_000, 30_000);
    assert_eq!(closed_votes(&evs), 1);
    assert_eq!(rt.state(), "scene");
    assert_eq!(rt.scene_up(), Some("the-critic"));
    assert_eq!(scene_up_line(&rt).as_deref(), Some("the-critic"));
    assert!(rt
        .context(10, 0)
        .state_summary
        .contains("last poll scene-ballot-1 closed: winner The Critic"));
    // The scene plays as long as it takes: nothing moves until the brain reports its end.
    assert!(run(&mut rt, 31_000, 60_000).is_empty());
    assert_eq!(rt.state(), "scene");
    rt.scene_ended("the-gallery", 61_000);
    assert_eq!(rt.state(), "scene", "not the scene up");
    rt.scene_ended("the-critic", 61_000);
    assert_eq!(rt.state(), "talk");
    let talk = rt.context(10, 0).state_summary;
    assert!(
        talk.contains("FREE TALK after \"The Critic\"")
            && talk.contains("The Critic: was it any good?"),
        "{talk}"
    );
    assert!(
        talk.contains("Then the next ballot opens by itself."),
        "{talk}"
    );
    assert_eq!(scene_up_line(&rt), None);

    // Round 2: after the talk, a ballot of the two left; a tie goes to the first in order.
    let evs = run(&mut rt, 62_000, 91_000);
    let b = ballots(&evs);
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].0, "scene-ballot-2");
    assert_eq!(b[0].1, ["The Gallery", "The Commission"]);
    rt.ingest_chat(
        92_000,
        &[
            vote("m4", 92_000, "a", "The Commission"),
            vote("m5", 92_000, "b", "The Gallery"),
        ],
    );
    run(&mut rt, 92_000, 112_000);
    assert_eq!(rt.scene_up(), Some("the-gallery"));
    let s = rt.context(10, 0).state_summary;
    assert!(
        s.contains(
            "last poll scene-ballot-2 closed: TIE between The Gallery and The Commission (1 \
             votes each): you break it on air, and say you are breaking it, in favour of \"The \
             Gallery\""
        ),
        "{s}"
    );
    rt.scene_ended("the-gallery", 113_000);

    // Round 3: the last scene plays without a ballot.
    let evs = run(&mut rt, 114_000, 150_000);
    assert!(ballots(&evs).is_empty(), "{evs:?}");
    assert_eq!(rt.scene_up(), Some("the-commission"));
    assert!(rt
        .context(10, 0)
        .state_summary
        .contains("SCENE UP: the-commission \"The Commission\" (the last scene, unvoted)"));
    rt.scene_ended("the-commission", 151_000);
    assert_eq!(
        rt.scenes_played(),
        ["the-critic", "the-gallery", "the-commission"]
    );
    assert!(rt
        .context(10, 0)
        .state_summary
        .contains("That was the last scene: the closing follows."));

    // The last talk lasts until the floor; then the closing, and sign_off is allowed.
    run(&mut rt, 152_000, 299_000);
    assert_eq!(rt.state(), "talk");
    let evs = rt.apply_ops(
        299_000,
        &[Op::SignOff {
            reason: "done".into(),
        }],
    );
    assert!(evs.iter().any(
        |e| matches!(e, StreamEvent::Moderation { stage: ModStage::Output, rule, .. } if rule == "segment_floor")
    ));
    run(&mut rt, 300_000, 301_000);
    assert_eq!(rt.state(), "closing");
    assert!(rt
        .context(10, 0)
        .allowed_ops
        .contains(&"sign_off".to_string()));
}

#[test]
fn a_ballot_with_no_votes_puts_up_the_first_scene_and_a_stale_scene_ends_itself() {
    let mut rt = seg03();
    run(&mut rt, 1_000, 31_000);
    assert_eq!(rt.scene_up(), Some("the-gallery"));
    assert!(rt
        .context(10, 0)
        .state_summary
        .contains("closed: no votes: you choose \"The Gallery\""));
    // Nobody performs it: after scene_max_s the runtime ends it itself, on the record.
    let evs = run(&mut rt, 32_000, 152_000);
    assert!(evs.iter().any(
        |e| matches!(e, StreamEvent::Moderation { rule, action, .. } if rule == "scene_timeout" && action == "ended:the-gallery")
    ));
    assert_eq!(rt.scenes_played(), ["the-gallery"]);
}

#[test]
fn past_the_ceiling_no_new_round_starts() {
    let mut c = cfg(Some(table_read()));
    c.round_times = Some(RoundTimes {
        ceiling_s: 300,
        ..times()
    });
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg03TableRead, c).unwrap();
    rt.start(0);
    run(&mut rt, 1_000, 31_000);
    assert_eq!(rt.state(), "scene");
    rt.scene_ended("the-gallery", 299_000);
    run(&mut rt, 300_000, 400_000);
    assert_eq!(rt.state(), "closing");
}

fn seg01(tr: Option<TableRead>) -> SegmentRuntime {
    let mut c = cfg(tr);
    c.handover = Some(HandoverRule {
        to: SegmentId::Seg02aHexmap,
        ccv_min: 1_000,
        sustain_s: 1,
        max_elapsed_s: 60,
        grace_s: 5,
    });
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, c).unwrap();
    rt.start(0);
    rt
}

fn segment_starts(evs: &[StreamEvent]) -> Vec<SegmentId> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::SegmentStart { segment, .. } => Some(*segment),
            _ => None,
        })
        .collect()
}

#[test]
fn seg01_hands_over_to_seg03_with_a_table_read_and_to_seg02a_without() {
    let mut rt = seg01(Some(table_read()));
    assert!(rt.context(10, 0).rules_prompt.contains(
        "Tonight the handover goes to SEG-03 Table Read, with the-painter as the guest."
    ));
    let evs = rt.tick(60_000, 10);
    assert!(evs.iter().any(|e| matches!(
        e,
        StreamEvent::Handover {
            to: SegmentId::Seg03TableRead,
            ..
        }
    )));
    assert!(rt.context(10, 0).state_summary.contains(
        "HANDOVER DUE: announce the Table Read with tonight's guest, then emit switch_segment to SEG-03"
    ));
    // The investigation is not tonight's handover.
    let evs = rt.apply_ops(
        61_000,
        &[Op::SwitchSegment {
            to: SegmentId::Seg02aHexmap,
        }],
    );
    assert!(evs
        .iter()
        .any(|e| matches!(e, StreamEvent::Moderation { rule, .. } if rule == "not_the_handover")));
    let evs = rt.apply_ops(
        62_000,
        &[Op::SwitchSegment {
            to: SegmentId::Seg03TableRead,
        }],
    );
    assert_eq!(segment_starts(&evs), [SegmentId::Seg03TableRead]);
    assert_eq!(rt.state(), "intro");
    assert_eq!(rt.active_foil(), Some(GUEST));

    // Without a table read: SEG-02A, as before, and SEG-03 is refused.
    let mut rt = seg01(None);
    let evs = rt.apply_ops(
        1_000,
        &[Op::SwitchSegment {
            to: SegmentId::Seg03TableRead,
        }],
    );
    assert!(evs
        .iter()
        .any(|e| matches!(e, StreamEvent::Moderation { rule, .. } if rule == "not_the_handover")));
    let evs = rt.tick(60_000, 10);
    assert!(evs.iter().any(|e| matches!(
        e,
        StreamEvent::Handover {
            to: SegmentId::Seg02aHexmap,
            ..
        }
    )));
    // The clock completes the switch after the grace period.
    let evs = rt.tick(65_000, 10);
    assert_eq!(segment_starts(&evs), [SegmentId::Seg02aHexmap]);
}

#[test]
fn the_clock_switches_into_seg03_after_the_grace() {
    let mut rt = seg01(Some(table_read()));
    rt.tick(60_000, 10);
    let evs = rt.tick(65_000, 10);
    assert_eq!(segment_starts(&evs), [SegmentId::Seg03TableRead]);
    assert_eq!(rt.segment(), SegmentId::Seg03TableRead);
    // Its intro runs from the switch.
    assert!(ballots(&rt.tick(70_000, 10)).is_empty());
    assert_eq!(ballots(&rt.tick(75_000, 10)).len(), 1);
}

#[test]
fn the_sample_loads_when_the_guest_is_a_declared_foil() {
    let dir = tempfile::tempdir().unwrap();
    let foils = vec![SAMPLE_GUEST.to_string()];
    let tr = table_read_for(dir.path(), None, &foils).unwrap();
    assert_eq!(tr.guest, SAMPLE_GUEST);
    let ids: Vec<&str> = tr.scenes.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["the-gallery", "the-commission", "the-critic"]);
    assert!(tr.scenes.iter().all(|s| s.fictional));
    // The guest is not one of the package's foils: nothing is playable.
    let e = table_read_for(dir.path(), None, &["the-disruptor".to_string()]).unwrap_err();
    assert!(e.contains("guest_unknown"), "{e}");
}

#[test]
fn a_plan_keeps_its_playable_scenes_in_its_order() {
    let dir = tempfile::tempdir().unwrap();
    let guest_dir = dir.path().join("scenes").join(GUEST);
    std::fs::create_dir_all(&guest_dir).unwrap();
    let write = |s: &Scene| {
        std::fs::write(
            guest_dir.join(format!("{}.toml", s.id)),
            toml::to_string(s).unwrap(),
        )
        .unwrap();
    };
    let mut draft = scene("the-draft", "The Draft");
    draft.status = SceneStatus::Draft;
    let mut ends_on_guest = scene("the-cliffhanger", "The Cliffhanger");
    ends_on_guest.lines.pop();
    for s in [
        scene("b-scene", "B"),
        scene("a-scene", "A"),
        draft,
        ends_on_guest,
    ] {
        write(&s);
    }
    let foils = vec![GUEST.to_string()];
    let plan = TableReadPlan {
        guest: GUEST.into(),
        scenes: vec![
            "b-scene".into(),
            "the-draft".into(),
            "missing".into(),
            "a-scene".into(),
            "the-cliffhanger".into(),
        ],
    };
    let tr = table_read_for(dir.path(), Some(&plan), &foils).unwrap();
    let ids: Vec<&str> = tr.scenes.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["b-scene", "a-scene"]);
    let none = TableReadPlan {
        guest: GUEST.into(),
        scenes: vec!["the-draft".into(), "the-cliffhanger".into()],
    };
    let e = table_read_for(dir.path(), Some(&none), &foils).unwrap_err();
    assert!(
        e.contains("the-draft: not approved") && e.contains("ends_on_guest"),
        "{e}"
    );
}
