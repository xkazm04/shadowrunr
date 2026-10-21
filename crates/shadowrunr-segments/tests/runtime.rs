//! SegmentRuntime behaviour: the cold open, handover, polls and their ties, foils, ranks, the
//! DOC rule, descoped segments, the screens and the evidence board.

use shadowrunr_segments::runtime::PollOutcome;
use shadowrunr_segments::table::ColdOpenRule;
use shadowrunr_segments::{
    demo_case, requires_alleged, HandoverRule, SegmentConfig, SegmentError, SegmentRuntime,
    StreamKind,
};
use shadowrunr_types::character::{CharacterManifest, Persona};
use shadowrunr_types::{
    ChatMsg, Command, EvidenceGrade, Hex, HexKind, LinkRel, ModStage, Op, Platform, Rank, ScreenId,
    SegmentId, StreamEvent, Viewer,
};

const FOILS: [&str; 2] = ["the-disruptor", "the-life-coach"];

/// A cold-open override with a ceiling of 0: the segment starts past its cold open.
fn no_cold_open() -> ColdOpenRule {
    ColdOpenRule {
        to: "opening".into(),
        min_s: 0,
        max_s: 0,
        active_chatters: 1,
        window_s: 60,
        sustain_s: 0,
    }
}

/// Malakor's persona, read from his package: these tests pin his prompt wording.
fn malakor() -> Persona {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../characters/malakor/character.toml");
    let text = std::fs::read_to_string(path).unwrap();
    let m: CharacterManifest = toml::from_str(&text).unwrap();
    Persona::from_manifest(&m)
}

/// The test config. SEG-01 starts past its cold open (the cold-open tests use [`seg01_cold`]).
fn cfg() -> SegmentConfig {
    SegmentConfig {
        foils: FOILS.iter().map(|s| s.to_string()).collect(),
        cold_open: Some(no_cold_open()),
        persona: malakor(),
        ..Default::default()
    }
}

fn seg01() -> SegmentRuntime {
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, cfg()).unwrap();
    rt.start(0);
    rt
}

/// SEG-01 with the table's cold open.
fn seg01_cold() -> SegmentRuntime {
    let c = SegmentConfig {
        cold_open: None,
        ..cfg()
    };
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, c).unwrap();
    rt.start(0);
    rt
}

fn seg02a() -> SegmentRuntime {
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg02aHexmap, cfg()).unwrap();
    rt.start(0);
    rt
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

fn msg(id: &str, ts: i64, who: &str, rank: Rank, text: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        ts_ms: ts,
        viewer: viewer(who, rank),
        text: text.into(),
        command: None,
    }
}

fn poll(id: &str, options: &[&str], min_rank: Rank, secs: u32) -> Op {
    Op::StartPoll {
        poll_id: id.into(),
        question: "Which game?".into(),
        options: options.iter().map(|s| s.to_string()).collect(),
        min_rank,
        secs,
    }
}

fn handovers(evs: &[StreamEvent]) -> Vec<String> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Handover {
                reason, from, to, ..
            } => {
                assert_eq!(*from, SegmentId::Seg01ImprovisedRoom);
                assert_eq!(*to, SegmentId::Seg02aHexmap);
                Some(reason.clone())
            }
            _ => None,
        })
        .collect()
}

/// Rules of the moderation events in `evs`, with their stage.
fn rejections(evs: &[StreamEvent]) -> Vec<(ModStage, String)> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Moderation { stage, rule, .. } => Some((*stage, rule.clone())),
            _ => None,
        })
        .collect()
}

/// `op` events, not counting `show_screen`: a reveal or a dispatch is followed by the
/// runtime's own screen default (the screen tests count those).
fn accepted_ops(evs: &[StreamEvent]) -> usize {
    evs.iter()
        .filter(|e| match e {
            StreamEvent::Op { op, .. } => !matches!(op, Op::ShowScreen { .. }),
            _ => false,
        })
        .count()
}

/// (poll_id, closed, [(option, voters, weight)]) of every vote event.
type VoteRow = (String, bool, Vec<(String, u32, f64)>);
fn votes(evs: &[StreamEvent]) -> Vec<VoteRow> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Vote {
                poll_id,
                tallies,
                closed,
                ..
            } => Some((
                poll_id.clone(),
                *closed,
                tallies
                    .iter()
                    .map(|t| {
                        (
                            t.option.clone(),
                            t.voters,
                            (t.weight * 100.0).round() / 100.0,
                        )
                    })
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

// ----- descoped ---------------------------------------------------------------------------

#[test]
fn descoped_segments_refuse_to_start() {
    for id in [SegmentId::Seg02bCartography, SegmentId::Seg02bDependency] {
        let err = SegmentRuntime::with_config(id, cfg())
            .err()
            .expect("must refuse");
        assert!(matches!(err, SegmentError::Descoped { segment, .. } if segment == id));
        assert!(err.to_string().contains("descoped"), "{err}");
    }
}

#[test]
#[should_panic(expected = "descoped")]
fn new_panics_for_descoped() {
    let _ = SegmentRuntime::new(SegmentId::Seg02bDependency);
}

#[test]
fn switching_to_a_descoped_segment_is_rejected() {
    let mut rt = seg01();
    let evs = rt.apply_ops(
        1,
        &[Op::SwitchSegment {
            to: SegmentId::Seg02bCartography,
        }],
    );
    assert_eq!(
        rejections(&evs),
        [(ModStage::Output, "switch_not_allowed".to_string())]
    );
    assert_eq!(rt.segment(), SegmentId::Seg01ImprovisedRoom);
}

// ----- SEG-01 handover --------------------------------------------------------------------

#[test]
fn handover_on_sustained_ccv() {
    let mut rt = seg01();
    assert!(rt.tick(0, 20).is_empty());
    assert!(rt.tick(90_000, 15).is_empty());
    assert!(handovers(&rt.tick(179_999, 30)).is_empty());
    let evs = rt.tick(180_000, 30);
    assert_eq!(handovers(&evs), ["ccv >= 15 sustained 180 s"]);
    assert_eq!(rt.state(), "handover_pending");
    assert!(rt.is_handover_pending());
    // No second announcement while pending.
    assert!(handovers(&rt.tick(181_000, 30)).is_empty());
}

#[test]
fn sustained_ccv_resets_when_ccv_dips() {
    let mut rt = seg01();
    rt.tick(0, 20);
    rt.tick(100_000, 14); // dip below 15 resets the window
    assert!(handovers(&rt.tick(180_000, 20)).is_empty());
    assert!(handovers(&rt.tick(359_999, 20)).is_empty());
    assert_eq!(
        handovers(&rt.tick(360_000, 20)),
        ["ccv >= 15 sustained 180 s"]
    );
}

#[test]
fn handover_on_elapsed_ceiling() {
    let mut rt = seg01();
    rt.tick(0, 3);
    assert!(handovers(&rt.tick(1_499_999, 3)).is_empty());
    assert_eq!(handovers(&rt.tick(1_500_000, 3)), ["elapsed >= 1500 s"]);
}

#[test]
fn handover_thresholds_are_config() {
    let mut c = cfg();
    c.handover = Some(HandoverRule {
        to: SegmentId::Seg02aHexmap,
        ccv_min: 5,
        sustain_s: 10,
        max_elapsed_s: 900,
        grace_s: 30,
    });
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, c).unwrap();
    rt.start(0);
    rt.tick(0, 5);
    assert_eq!(handovers(&rt.tick(10_000, 5)), ["ccv >= 5 sustained 10 s"]);
}

#[test]
fn clock_completes_the_switch_after_grace() {
    let mut rt = seg01();
    rt.tick(0, 1);
    rt.tick(1_500_000, 1);
    assert!(rt.tick(1_559_999, 1).is_empty());
    let evs = rt.tick(1_560_000, 1);
    assert!(matches!(
        evs.as_slice(),
        [
            StreamEvent::SegmentEnd {
                segment: SegmentId::Seg01ImprovisedRoom,
                ..
            },
            StreamEvent::SegmentStart {
                segment: SegmentId::Seg02aHexmap,
                ..
            },
            StreamEvent::CaseOpen { .. },
            StreamEvent::Op {
                op: Op::ShowScreen {
                    screen: ScreenId::Board,
                    record_id: None,
                    focus: None
                },
                ..
            }
        ]
    ));
    assert_eq!(rt.segment(), SegmentId::Seg02aHexmap);
    assert_eq!(rt.state(), "briefing");
    // SEG-02A has no handover clock.
    assert!(rt.tick(9_000_000, 300).is_empty());
}

#[test]
fn brain_switch_during_pending_handover_does_not_repeat_it() {
    let mut rt = seg01();
    rt.tick(0, 1);
    rt.tick(1_500_000, 1);
    let evs = rt.apply_ops(
        1_510_000,
        &[Op::SwitchSegment {
            to: SegmentId::Seg02aHexmap,
        }],
    );
    assert!(handovers(&evs).is_empty());
    // The switch, and the new segment's default screen.
    assert_eq!(accepted_ops(&evs), 1);
    assert_eq!(screens(&evs), [(ScreenId::Board, None)]);
    assert_eq!(rt.segment(), SegmentId::Seg02aHexmap);
}

#[test]
fn early_brain_switch_announces_a_handover() {
    let mut rt = seg01();
    let evs = rt.apply_ops(
        5_000,
        &[Op::SwitchSegment {
            to: SegmentId::Seg02aHexmap,
        }],
    );
    assert_eq!(handovers(&evs), ["switch_segment op"]);
    assert_eq!(rt.segment(), SegmentId::Seg02aHexmap);
}

// ----- polls ------------------------------------------------------------------------------

#[test]
fn poll_lifecycle_rank_weighted() {
    let mut rt = seg01();
    let evs = rt.apply_ops(1_000, &[poll("p1", &["Riddles", "Duel"], Rank::Thrall, 30)]);
    assert_eq!(accepted_ops(&evs), 1);
    assert_eq!(rt.state(), "crowd_game");

    // A second poll while one is open is refused.
    let evs = rt.apply_ops(2_000, &[poll("p2", &["a", "b"], Rank::Thrall, 30)]);
    assert_eq!(
        rejections(&evs),
        [(ModStage::Output, "poll_already_open".to_string())]
    );

    let evs = rt.ingest_chat(
        5_000,
        &[
            msg("m1", 5_000, "thrall1", Rank::Thrall, "!vote riddles"),
            msg("m2", 5_000, "warlock1", Rank::Warlock, "!VOTE  2"),
            msg("m3", 5_000, "acolyte1", Rank::Acolyte, "!vote Riddles"),
            // Last vote counts: the acolyte switches to Duel.
            msg("m4", 5_000, "acolyte1", Rank::Acolyte, "!vote duel"),
            // Not an option: ignored.
            msg("m5", 5_000, "thrall2", Rank::Thrall, "!vote chess"),
        ],
    );
    assert_eq!(
        votes(&evs),
        [(
            "p1".to_string(),
            false,
            vec![
                ("Riddles".to_string(), 1, 1.0),
                ("Duel".to_string(), 2, 3.4)
            ]
        )]
    );

    assert!(votes(&rt.tick(30_999, 0)).is_empty(), "closes at 31 000");
    let evs = rt.tick(31_000, 0);
    assert_eq!(
        votes(&evs),
        [(
            "p1".to_string(),
            true,
            vec![
                ("Riddles".to_string(), 1, 1.0),
                ("Duel".to_string(), 2, 3.4)
            ]
        )]
    );
    assert_eq!(rt.open_poll_id(), None);
    assert_eq!(rt.state(), "opening");

    assert!(rt
        .context(0, 31)
        .state_summary
        .contains("last poll p1 closed: winner Duel"));

    // Poll ids are single-use: a reused one is re-minted, and the transcript says so.
    let evs = rt.apply_ops(40_000, &[poll("p1", &["a", "b"], Rank::Thrall, 30)]);
    assert!(rejections(&evs).is_empty());
    assert!(evs.iter().any(|e| matches!(e,
        StreamEvent::Op { op: Op::StartPoll { poll_id, .. }, .. } if poll_id == "p1-2")));
    assert_eq!(rt.open_poll_id(), Some("p1-2"));
    // close_poll by the id the model used finds the re-minted poll.
    let evs = rt.apply_ops(
        41_000,
        &[Op::ClosePoll {
            poll_id: "p1".into(),
        }],
    );
    assert!(votes(&evs)[0].1, "closed");
    assert!(rt
        .context(0, 41)
        .state_summary
        .contains("last poll p1-2 closed"));
}

#[test]
fn a_refused_op_is_named_in_the_state_for_a_minute() {
    let mut rt = seg01();
    rt.apply_ops(
        1_000,
        &[Op::ClosePoll {
            poll_id: "p9".into(),
        }],
    );
    let s = rt.context(0, 1).state_summary;
    assert!(s.contains("no poll open: never tell chat"), "{s}");
    assert!(
        s.contains("REFUSED lately, so these did NOT happen (never tell chat they did): close_poll p9: no_such_poll"),
        "{s}"
    );
    rt.tick(62_000, 0);
    assert!(!rt.context(0, 62).state_summary.contains("REFUSED"));
}

#[test]
fn poll_min_rank_and_validation() {
    let mut rt = seg01();
    let bad = rt.apply_ops(
        0,
        &[
            poll("x1", &["only"], Rank::Thrall, 30),
            poll("x2", &["a", "A"], Rank::Thrall, 30),
            poll("x3", &["a", "b"], Rank::Thrall, 5),
            poll("x4", &["a", "b", "c", "d", "e", "f", "g"], Rank::Thrall, 30),
        ],
    );
    let rules: Vec<String> = rejections(&bad).into_iter().map(|r| r.1).collect();
    assert_eq!(
        rules,
        [
            "invalid_poll_options",
            "invalid_poll_options",
            "invalid_poll_secs",
            "invalid_poll_options"
        ]
    );

    rt.apply_ops(0, &[poll("p", &["yes", "no"], Rank::Cultist, 60)]);
    let evs = rt.ingest_chat(
        1_000,
        &[
            msg("m1", 1_000, "t", Rank::Thrall, "!vote yes"),
            msg("m2", 1_000, "c", Rank::Cultist, "!vote no"),
        ],
    );
    assert_eq!(
        rejections(&evs),
        [(ModStage::Input, "poll_min_rank".to_string())]
    );
    assert_eq!(
        votes(&evs)[0].2,
        vec![("yes".to_string(), 0, 0.0), ("no".to_string(), 1, 1.7)]
    );

    // close_poll needs the open poll's id; closing yields a closed tally.
    let evs = rt.apply_ops(
        2_000,
        &[Op::ClosePoll {
            poll_id: "nope".into(),
        }],
    );
    assert_eq!(rejections(&evs)[0].1, "no_such_poll");
    let evs = rt.apply_ops(
        2_000,
        &[Op::ClosePoll {
            poll_id: "p".into(),
        }],
    );
    assert!(votes(&evs)[0].1, "closed");
}

// ----- foils ------------------------------------------------------------------------------

#[test]
fn summon_opens_a_foil_vote_and_the_winner_appears() {
    let mut rt = seg01();
    let evs = rt.ingest_chat(
        1_000,
        &[
            msg("m1", 1_000, "a", Rank::Thrall, "!summon the-life-coach"),
            msg("m2", 1_000, "b", Rank::Warlock, "!summon The-Disruptor"),
            msg("m3", 1_000, "c", Rank::Thrall, "!vote the-life-coach"),
        ],
    );
    assert!(evs.iter().any(|e| matches!(
        e,
        StreamEvent::Op { op: Op::StartPoll { poll_id, secs: 60, .. }, .. } if poll_id == "foil-vote-1"
    )));
    assert_eq!(rt.state(), "foil_vote");
    // While the crowd decides, the brain may not force a foil.
    let evs = rt.apply_ops(
        2_000,
        &[Op::SummonFoil {
            foil: "the-disruptor".into(),
        }],
    );
    assert_eq!(rejections(&evs)[0].1, "not_in_state");

    let evs = rt.tick(61_000, 2);
    assert_eq!(
        votes(&evs),
        [(
            "foil-vote-1".to_string(),
            true,
            vec![
                ("the-disruptor".to_string(), 1, 2.0),
                ("the-life-coach".to_string(), 2, 2.0)
            ]
        )]
    );
    // Tie on weight goes to more voters.
    assert!(evs.iter().any(|e| matches!(
        e,
        StreamEvent::Op { op: Op::SummonFoil { foil }, .. } if foil == "the-life-coach"
    )));
    assert_eq!(rt.active_foil(), Some("the-life-coach"));
    assert_eq!(rt.state(), "foil_live");

    // Dismiss, then dismissing again fails.
    assert_eq!(accepted_ops(&rt.apply_ops(70_000, &[Op::DismissFoil])), 1);
    assert_eq!(rt.state(), "opening");
    let evs = rt.apply_ops(71_000, &[Op::DismissFoil]);
    assert_eq!(rejections(&evs)[0].1, "no_active_foil");
}

#[test]
fn foils_must_exist() {
    let mut rt = seg01();
    let evs = rt.ingest_chat(0, &[msg("m1", 0, "a", Rank::Thrall, "!summon the-plumber")]);
    assert_eq!(
        rejections(&evs),
        [(ModStage::Input, "unknown_foil".to_string())]
    );
    assert_eq!(rt.open_poll_id(), None);
    let evs = rt.apply_ops(
        0,
        &[Op::SummonFoil {
            foil: "the-plumber".into(),
        }],
    );
    assert_eq!(rejections(&evs)[0].1, "unknown_foil");
    let evs = rt.apply_ops(
        0,
        &[Op::SummonFoil {
            foil: "the-disruptor".into(),
        }],
    );
    assert_eq!(accepted_ops(&evs), 1);
    assert_eq!(rt.state(), "foil_live");
}

// ----- ranks and commands -----------------------------------------------------------------

#[test]
fn propose_is_rank_gated() {
    let mut rt = seg01();
    let evs = rt.ingest_chat(
        0,
        &[
            msg("m1", 0, "c", Rank::Cultist, "!propose a bake-off of doom"),
            msg("m2", 0, "w", Rank::Warlock, "!propose a duel of riddles"),
        ],
    );
    assert_eq!(
        evs.iter()
            .filter_map(|e| match e {
                StreamEvent::Moderation {
                    rule,
                    msg_id,
                    stage,
                    ..
                } => Some((rule.as_str(), msg_id.as_deref(), *stage)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [("propose_min_rank", Some("m1"), ModStage::Input)]
    );
    assert_eq!(rt.proposals().len(), 1);
    assert_eq!(rt.proposals()[0].text, "a duel of riddles");
}

#[test]
fn pre_parsed_command_field_is_honoured() {
    let mut rt = seg01();
    let mut m = msg("m1", 0, "w", Rank::Warlock, "free text");
    m.command = Some(Command::Propose {
        text: "pre-parsed".into(),
    });
    rt.ingest_chat(0, &[m]);
    assert_eq!(rt.proposals()[0].text, "pre-parsed");
}

/// (rule, msg_id, action) of every input-stage moderation event in `evs`.
fn declines(evs: &[StreamEvent]) -> Vec<(String, Option<String>, String)> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Moderation {
                stage: ModStage::Input,
                rule,
                msg_id,
                action,
                ..
            } => Some((rule.clone(), msg_id.clone(), action.clone())),
            _ => None,
        })
        .collect()
}

fn declined(rule: &str, msg_id: &str) -> (String, Option<String>, String) {
    (rule.into(), Some(msg_id.into()), "ignored".into())
}

#[test]
fn commands_outside_the_segment_are_declined_by_name() {
    let mut rt = seg01();
    let evs = rt.ingest_chat(
        0,
        &[
            msg("m1", 0, "a", Rank::Thrall, "!grade HEX-01 DOC"),
            msg("m2", 0, "a", Rank::Thrall, "!hint HEX-01 look"),
        ],
    );
    // Declined, never executed: the events are the only trace, and they carry no chat text.
    assert_eq!(
        declines(&evs),
        [
            declined("command_not_in_segment", "m1"),
            declined("command_not_in_segment", "m2")
        ]
    );
    assert_eq!(evs.len(), 2);
}

#[test]
fn the_cold_open_takes_proposals_and_declines_summons() {
    let mut rt = seg01_cold();
    assert_eq!(rt.state(), "cold_open");
    let evs = rt.ingest_chat(
        1_000,
        &[
            msg("m1", 1_000, "a", Rank::Thrall, "!summon the-disruptor"),
            msg("m2", 1_000, "w", Rank::Warlock, "!propose a duel"),
        ],
    );
    assert_eq!(declines(&evs), [declined("command_closed_in_state", "m1")]);
    assert!(!evs.iter().any(|e| matches!(
        e,
        StreamEvent::Op {
            op: Op::StartPoll { .. },
            ..
        }
    )));
    assert_eq!(rt.state(), "cold_open");
    assert_eq!(rt.open_poll_id(), None);
    assert_eq!(rt.proposals().len(), 1);
    assert_eq!(rt.proposals()[0].text, "a duel");
}

#[test]
fn a_vote_with_no_poll_open_is_declined() {
    let mut rt = seg02a();
    assert_eq!(rt.state(), "briefing");
    let evs = rt.ingest_chat(0, &[msg("m1", 0, "a", Rank::Thrall, "!vote HEX-02")]);
    assert_eq!(declines(&evs), [declined("no_open_poll", "m1")]);
}

#[test]
fn a_vote_for_no_option_is_declined() {
    let mut rt = seg01();
    rt.apply_ops(0, &[poll("p1", &["a", "b"], Rank::Thrall, 30)]);
    let evs = rt.ingest_chat(1_000, &[msg("m1", 1_000, "x", Rank::Thrall, "!vote c")]);
    assert_eq!(declines(&evs), [declined("unknown_option", "m1")]);
    assert!(votes(&evs).is_empty());
}

#[test]
fn a_summon_while_a_crowd_poll_runs_is_declined() {
    let mut rt = seg01();
    rt.apply_ops(0, &[poll("p1", &["a", "b"], Rank::Thrall, 30)]);
    assert_eq!(rt.state(), "crowd_game");
    let evs = rt.ingest_chat(
        1_000,
        &[msg("m1", 1_000, "x", Rank::Thrall, "!summon the-disruptor")],
    );
    assert_eq!(declines(&evs), [declined("foil_vote_not_open", "m1")]);
    assert_eq!(rt.open_poll_id(), Some("p1"));
    assert!(votes(&evs).is_empty());
}

#[test]
fn declines_are_counted_in_the_state_for_a_minute_without_chat_text() {
    let mut rt = seg01();
    rt.ingest_chat(
        1_000,
        &[
            msg("m1", 1_000, "a", Rank::Thrall, "!vote secret-a"),
            msg("m2", 2_000, "b", Rank::Thrall, "!vote secret-b"),
            msg("m3", 3_000, "c", Rank::Thrall, "!vote secret-c"),
        ],
    );
    let s = rt.context(0, 3).state_summary;
    assert!(s.contains("declined lately: !vote x3 (no poll open)"), "{s}");
    assert!(!s.contains("secret"), "{s}");
    rt.tick(62_000, 0);
    assert!(!rt.context(0, 62).state_summary.contains("declined lately"));
}

#[test]
fn ops_outside_the_segment_are_rejected() {
    let mut rt = seg01();
    let evs = rt.apply_ops(
        0,
        &[Op::RevealHex {
            hex: "HEX-01".into(),
            grade: EvidenceGrade::Rep,
        }],
    );
    assert_eq!(
        evs.iter()
            .filter_map(|e| match e {
                StreamEvent::Moderation { rule, action, .. } =>
                    Some((rule.as_str(), action.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [("op_not_allowed", "rejected:reveal_hex")]
    );
    let mut rt = seg02a();
    for op in [
        Op::SummonFoil {
            foil: "the-disruptor".into(),
        },
        Op::DismissFoil,
    ] {
        assert_eq!(rejections(&rt.apply_ops(0, &[op]))[0].1, "op_not_allowed");
    }
}

#[test]
fn highlight_needs_a_seen_message_and_sign_off_ends_everything() {
    let mut rt = seg01();
    let evs = rt.apply_ops(
        0,
        &[Op::Highlight {
            msg_id: "m1".into(),
        }],
    );
    assert_eq!(rejections(&evs)[0].1, "unknown_msg");
    rt.ingest_chat(0, &[msg("m1", 0, "a", Rank::Thrall, "hello lich")]);
    assert_eq!(
        accepted_ops(&rt.apply_ops(
            0,
            &[Op::Highlight {
                msg_id: "m1".into()
            }]
        )),
        1
    );
    rt.apply_ops(0, &[poll("p", &["a", "b"], Rank::Thrall, 30)]);
    let evs = rt.apply_ops(
        1,
        &[Op::SignOff {
            reason: "dawn".into(),
        }],
    );
    assert!(votes(&evs)[0].1, "sign-off closes the open poll");
    assert!(rt.is_signed_off());
    let evs = rt.apply_ops(
        2,
        &[Op::MarkMoment {
            label: "late".into(),
        }],
    );
    assert_eq!(rejections(&evs)[0].1, "signed_off");
    assert!(rt.context(0, 0).allowed_ops.is_empty());
    assert!(rt.tick(9_999_999, 100).is_empty());
}

// ----- SEG-02A ----------------------------------------------------------------------------

#[test]
fn start_in_seg02a_opens_the_fictional_demo_case() {
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg02aHexmap, cfg()).unwrap();
    let evs = rt.start(1_000);
    let [StreamEvent::SegmentStart {
        ts_ms: 1_000,
        segment: SegmentId::Seg02aHexmap,
    }, StreamEvent::CaseOpen { ts_ms: 1_000, case }, StreamEvent::Op {
        ts_ms: 1_000,
        op:
            Op::ShowScreen {
                screen: ScreenId::Board,
                record_id: None,
                focus: None,
            },
    }] = evs.as_slice()
    else {
        panic!("expected segment_start, case_open, then the default screen, got {evs:?}");
    };
    let demo = demo_case();
    assert_eq!(case.id, demo.id);
    assert_eq!(case.title, demo.title);
    assert!(case.fictional);
    assert_eq!(case.hexes.len(), demo.hexes.len());
    let line = serde_json::to_string(&evs[1]).unwrap();
    assert!(line.starts_with("{\"type\":\"case_open\""), "{line}");
    assert!(line.contains("\"fictional\":true"), "{line}");
}

#[test]
fn start_outside_seg02a_has_no_case_open() {
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, cfg()).unwrap();
    let evs = rt.start(0);
    assert!(matches!(
        evs.as_slice(),
        [
            StreamEvent::SegmentStart {
                segment: SegmentId::Seg01ImprovisedRoom,
                ..
            },
            StreamEvent::Op {
                op: Op::ShowScreen {
                    screen: ScreenId::Host,
                    record_id: None,
                    focus: None
                },
                ..
            }
        ]
    ));
}

#[test]
fn switch_into_seg02a_opens_the_case_right_after_segment_start() {
    let mut rt = seg01();
    let evs = rt.apply_ops(
        5_000,
        &[Op::SwitchSegment {
            to: SegmentId::Seg02aHexmap,
        }],
    );
    let start = evs
        .iter()
        .position(|e| {
            matches!(
                e,
                StreamEvent::SegmentStart {
                    segment: SegmentId::Seg02aHexmap,
                    ..
                }
            )
        })
        .expect("segment_start SEG-02A");
    let Some(StreamEvent::CaseOpen { ts_ms, case }) = evs.get(start + 1) else {
        panic!("case_open must follow segment_start, got {evs:?}");
    };
    assert_eq!(*ts_ms, 5_000);
    assert!(case.fictional);
    let opens = evs
        .iter()
        .filter(|e| matches!(e, StreamEvent::CaseOpen { .. }))
        .count();
    assert_eq!(opens, 1);
}

#[test]
fn demo_case_loads_fictional() {
    let rt = seg02a();
    assert!(rt.case().fictional);
    assert!((12..=20).contains(&rt.case().hexes.len()));
    let ctx = rt.context(10, 0);
    assert!(ctx.rules_prompt.contains("FICTIONAL"));
}

fn reveal(hex: &str, grade: EvidenceGrade) -> Op {
    Op::RevealHex {
        hex: hex.into(),
        grade,
    }
}

#[test]
fn doc_reveal_without_allowlisted_source_is_rejected_rep_passes() {
    let mut rt = seg02a();
    // HEX-05 has only a press source.
    let evs = rt.apply_ops(1_000, &[reveal("HEX-05", EvidenceGrade::Doc)]);
    assert_eq!(
        rejections(&evs),
        [(ModStage::Output, "doc_requires_primary_source".to_string())]
    );
    assert!(
        rt.case()
            .hexes
            .iter()
            .find(|h| h.id == "HEX-05")
            .unwrap()
            .fog
    );
    assert_eq!(rt.state(), "briefing");

    let evs = rt.apply_ops(2_000, &[reveal("HEX-05", EvidenceGrade::Rep)]);
    assert_eq!(accepted_ops(&evs), 1);
    assert!(rejections(&evs).is_empty());
    let h = rt.case().hexes.iter().find(|h| h.id == "HEX-05").unwrap();
    assert!(!h.fog);
    assert_eq!(h.grade, Some(EvidenceGrade::Rep));
    assert!(requires_alleged(EvidenceGrade::Rep));
    assert_eq!(rt.state(), "investigating");
    assert!(rt
        .context(10, 2)
        .state_summary
        .contains("HEX-05 REP ALLEGED"));

    // Same grade again is a no-op rejection; unknown hexes are rejected.
    assert_eq!(
        rejections(&rt.apply_ops(3_000, &[reveal("HEX-05", EvidenceGrade::Rep)]))[0].1,
        "already_revealed"
    );
    assert_eq!(
        rejections(&rt.apply_ops(3_000, &[reveal("HEX-77", EvidenceGrade::Spc)]))[0].1,
        "unknown_hex"
    );
}

#[test]
fn fictional_primary_domains_only_count_for_fictional_cases() {
    // In the fictional demo case, HEX-01 carries a records.example "order".
    let mut rt = seg02a();
    assert_eq!(
        accepted_ops(&rt.apply_ops(0, &[reveal("hex1", EvidenceGrade::Doc)])),
        1
    );

    // The same hex in a case marked real is refused DOC.
    let mut real = demo_case();
    real.fictional = false;
    let mut c = cfg();
    c.case = Some(real);
    c.case_approved = true;
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg02aHexmap, c).unwrap();
    assert_eq!(
        rejections(&rt.apply_ops(0, &[reveal("HEX-01", EvidenceGrade::Doc)]))[0].1,
        "doc_requires_primary_source"
    );
}

#[test]
fn doc_reveal_with_regulator_source_passes() {
    let mut case = demo_case();
    case.fictional = false;
    case.hexes.push(Hex {
        id: "HEX-17".into(),
        q: -2,
        r: 0,
        kind: HexKind::Entity,
        label: "test entity".into(),
        grade: None,
        fog: true,
        sources: vec!["https://www.sec.gov/litigation/complaints/test.pdf".into()],
        role: None,
        date: None,
    });
    let mut c = cfg();
    c.case = Some(case);
    c.case_approved = true;
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg02aHexmap, c).unwrap();
    assert_eq!(
        accepted_ops(&rt.apply_ops(0, &[reveal("HEX-17", EvidenceGrade::Doc)])),
        1
    );
}

#[test]
fn grades_tally_rank_weighted_and_hints_queue() {
    let mut rt = seg02a();
    let evs = rt.ingest_chat(
        0,
        &[
            msg("m1", 0, "w", Rank::Warlock, "!grade HEX-03 DOC"),
            msg("m2", 0, "t", Rank::Thrall, "!grade hex3 rep"),
            msg("m3", 0, "a", Rank::Acolyte, "!grade 3 rep"),
            msg("m4", 0, "a", Rank::Acolyte, "!grade 3 clm"),
            msg(
                "m5",
                0,
                "t",
                Rank::Thrall,
                "!hint hex4 follow the treasury wallet",
            ),
            msg(
                "m6",
                0,
                "w",
                Rank::Warlock,
                "!hint HEX-04 same wallet as the mixer",
            ),
            msg("m7", 0, "w", Rank::Warlock, "!hint HEX-99 nothing here"),
        ],
    );
    assert_eq!(
        votes(&evs),
        [(
            "grade:HEX-03".to_string(),
            false,
            vec![
                ("DOC".to_string(), 1, 2.0),
                ("REP".to_string(), 1, 1.0),
                ("CLM".to_string(), 1, 1.4),
                ("SPC".to_string(), 0, 0.0)
            ]
        )]
    );
    assert_eq!(rt.jury_grade("HEX-03"), Some(EvidenceGrade::Doc));
    assert_eq!(rt.leads("HEX-04").len(), 2);
    assert_eq!(
        rejections(&evs),
        [(ModStage::Input, "unknown_hex".to_string())]
    );
    let summary = rt.context(30, 60).state_summary;
    assert!(summary.contains("minion leads: HEX-04 x2"), "{summary}");
    assert!(
        !summary.contains("treasury"),
        "lead text stays out of the prompt"
    );
}

#[test]
fn seg02a_poll_and_research() {
    let mut rt = seg02a();
    rt.apply_ops(0, &[poll("next", &["HEX-02", "HEX-04"], Rank::Thrall, 30)]);
    assert_eq!(rt.state(), "jury_poll");
    rt.tick(30_000, 10);
    assert_eq!(rt.state(), "briefing", "nothing revealed yet");
    let job = |id: &str| Op::DispatchResearch {
        job_id: id.into(),
        brief: "map the shell chain".into(),
    };
    assert_eq!(accepted_ops(&rt.apply_ops(1, &[job("j1")])), 1);
    assert_eq!(
        rejections(&rt.apply_ops(2, &[job("j1")]))[0].1,
        "duplicate_job_id"
    );
}

// ----- SEG-02A research: the case file and the re-search cap -------------------------------

fn research(job_id: &str, brief: &str) -> Op {
    Op::DispatchResearch {
        job_id: job_id.into(),
        brief: brief.into(),
    }
}

fn hex<'a>(rt: &'a SegmentRuntime, id: &str) -> &'a Hex {
    rt.case().hexes.iter().find(|h| h.id == id).unwrap()
}

/// (poll_id, options) of every `start_poll` op in `evs`.
fn started_polls(evs: &[StreamEvent]) -> Vec<(String, Vec<String>)> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Op {
                op: Op::StartPoll {
                    poll_id, options, ..
                },
                ..
            } => Some((poll_id.clone(), options.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn research_on_a_seeded_hex_files_its_planted_records() {
    let mut rt = seg02a();
    assert!(rt.case_file().is_some(), "the demo case ships its records");
    // HEX-04 starts with only a chain explorer source: no DOC yet.
    assert_eq!(
        rejections(&rt.apply_ops(0, &[reveal("HEX-04", EvidenceGrade::Doc)]))[0].1,
        "doc_requires_primary_source"
    );
    let evs = rt.apply_ops(
        1_000,
        &[research("r1", "HEX-04: trace the treasury wallet")],
    );
    assert_eq!(accepted_ops(&evs), 1);
    assert!(rejections(&evs).is_empty());
    assert_eq!(rt.searches("HEX-04"), 1);
    let summary = rt.context(10, 1).state_summary;
    assert!(
        summary.contains("bone-clerk searches: HEX-04 1/2"),
        "{summary}"
    );
    assert!(
        summary.contains("case file (planted FICTIONAL demo records): HEX-04 DOC-03 DOC"),
        "{summary}"
    );
    assert!(summary.contains("HEX-04 CLM-02 CLM"), "{summary}");
    assert!(hex(&rt, "HEX-04")
        .sources
        .iter()
        .any(|s| s == "https://records.example/morrowgate-fco/freeze-0003"));
    // The filed DOC record lets the hex be annexed at DOC under the existing rule.
    assert_eq!(
        accepted_ops(&rt.apply_ops(2_000, &[reveal("HEX-04", EvidenceGrade::Doc)])),
        1
    );
    // A second search files nothing twice.
    rt.apply_ops(3_000, &[research("r2", "hex4 once more")]);
    let sources = &hex(&rt, "HEX-04").sources;
    assert_eq!(sources.len(), 3, "{sources:?}");
    let summary = rt.context(10, 3).state_summary;
    let file_line = summary
        .lines()
        .find(|l| l.starts_with("case file"))
        .unwrap();
    assert_eq!(file_line.matches("DOC-03").count(), 1, "{summary}");
    assert!(summary.contains("HEX-04 2/2 SPENT"), "{summary}");
}

#[test]
fn research_on_an_empty_hex_finds_nothing() {
    let mut rt = seg02a();
    let before = hex(&rt, "HEX-08").sources.clone();
    let evs = rt.apply_ops(0, &[research("r1", "HEX-08: who controls Mirecrown Trust")]);
    assert_eq!(accepted_ops(&evs), 1);
    assert_eq!(rt.searches("HEX-08"), 1);
    assert_eq!(hex(&rt, "HEX-08").sources, before);
    let summary = rt.context(10, 1).state_summary;
    assert!(
        summary.contains("searched, nothing on file: HEX-08"),
        "{summary}"
    );
    assert!(!summary.contains("case file ("), "{summary}");
}

#[test]
fn a_brief_naming_no_hex_is_not_counted() {
    let mut rt = seg02a();
    for (i, brief) in [
        "map the shell chain",
        "follow every wallet",
        "Mirecrown and Ashpetal",
    ]
    .iter()
    .enumerate()
    {
        let evs = rt.apply_ops(0, &[research(&format!("r{i}"), brief)]);
        assert_eq!(accepted_ops(&evs), 1, "{brief}");
    }
    assert!(rt.case().hexes.iter().all(|h| rt.searches(&h.id) == 0));
    assert!(!rt
        .context(10, 0)
        .state_summary
        .contains("bone-clerk searches"));
}

#[test]
fn third_search_on_a_hex_goes_to_a_plebiscite() {
    let mut rt = seg02a();
    let ctx = rt.context(10, 0);
    assert!(ctx.rules_prompt.contains("at most 2 times per stream"));
    assert!(ctx.rules_prompt.contains("planted FICTIONAL demo records"));
    for (job, t) in [("r1", 1_000), ("r2", 2_000)] {
        let evs = rt.apply_ops(t, &[research(job, "HEX-07: the moon maintenance thread")]);
        assert_eq!(accepted_ops(&evs), 1);
        assert!(started_polls(&evs).is_empty());
    }
    assert_eq!(rt.searches("HEX-07"), 2);
    assert!(rt.context(10, 2).state_summary.contains("SPENT: HEX-07"));

    // The third attempt is refused and the next move goes to the audience.
    let evs = rt.apply_ops(3_000, &[research("r3", "hex 7, try again")]);
    assert_eq!(
        rejections(&evs),
        [(ModStage::Output, "research_cap_reached".to_string())]
    );
    assert_eq!(
        started_polls(&evs),
        [(
            "plebiscite-HEX-07".to_string(),
            vec![
                "Annex HEX-07".to_string(),
                "Leave HEX-07 fogged".to_string(),
                "Search HEX-01".to_string(),
                "Search HEX-11".to_string(),
            ]
        )]
    );
    assert_eq!(rt.searches("HEX-07"), 2, "a refused search is not counted");
    assert_eq!(rt.open_poll_id(), Some("plebiscite-HEX-07"));
    assert_eq!(rt.plebiscite_hex(), Some("HEX-07"));
    assert_eq!(rt.state(), "jury_poll");

    // Malakor's turn context says so, and research is withheld until the vote closes.
    let ctx = rt.context(10, 3);
    assert!(!ctx.allowed_ops.iter().any(|o| o == "dispatch_research"));
    assert!(ctx.allowed_ops.iter().any(|o| o == "reveal_hex"));
    assert!(
        ctx.state_summary
            .contains("PLEBISCITE on HEX-07: the audience decides"),
        "{}",
        ctx.state_summary
    );
    assert!(ctx
        .state_summary
        .contains("open Plebiscite plebiscite-HEX-07"));
    assert_eq!(
        rejections(&rt.apply_ops(4_000, &[research("r4", "HEX-01: the syndicate")]))[0].1,
        "plebiscite_pending"
    );

    // The audience votes; the clock closes the Plebiscite and the verdict reaches the context.
    rt.ingest_chat(
        5_000,
        &[
            msg("v1", 5_000, "a", Rank::Thrall, "!vote Annex HEX-07"),
            msg("v2", 5_000, "b", Rank::Thrall, "!vote 3"),
            msg("v3", 5_000, "c", Rank::Warlock, "!vote annex hex-07"),
        ],
    );
    let evs = rt.tick(63_000, 10);
    assert_eq!(votes(&evs).len(), 1);
    assert!(votes(&evs)[0].1, "the Plebiscite closed");
    assert_eq!(rt.open_poll_id(), None);
    let ctx = rt.context(10, 63);
    assert!(
        ctx.state_summary
            .contains("PLEBISCITE on HEX-07 decided: \"Annex HEX-07\""),
        "{}",
        ctx.state_summary
    );
    assert!(ctx.allowed_ops.iter().any(|o| o == "dispatch_research"));

    // A spent hex stays refused, and it gets no second Plebiscite.
    let evs = rt.apply_ops(64_000, &[research("r5", "HEX-07: one last look")]);
    assert_eq!(rejections(&evs)[0].1, "research_cap_reached");
    assert!(started_polls(&evs).is_empty());
    // Other hexes can still be searched.
    assert_eq!(
        accepted_ops(&rt.apply_ops(65_000, &[research("r6", "HEX-11: the help desk")])),
        1
    );
}

#[test]
fn the_plebiscite_waits_for_an_open_poll_to_close() {
    let mut rt = seg02a();
    rt.apply_ops(0, &[poll("next", &["HEX-02", "HEX-04"], Rank::Thrall, 30)]);
    rt.apply_ops(
        1_000,
        &[
            research("r1", "HEX-08: Mirecrown"),
            research("r2", "HEX-08: Mirecrown again"),
        ],
    );
    let evs = rt.apply_ops(2_000, &[research("r3", "HEX-08: Mirecrown, third time")]);
    assert_eq!(rejections(&evs)[0].1, "research_cap_reached");
    assert!(started_polls(&evs).is_empty(), "a poll is already open");
    assert_eq!(rt.open_poll_id(), Some("next"));
    assert_eq!(rt.plebiscite_hex(), Some("HEX-08"));
    assert!(!rt
        .context(10, 2)
        .allowed_ops
        .iter()
        .any(|o| o == "dispatch_research"));

    // The crowd poll closes on the clock and the Plebiscite opens in the same tick.
    let evs = rt.tick(30_000, 10);
    let polls = started_polls(&evs);
    assert_eq!(polls.len(), 1, "{evs:?}");
    assert_eq!(polls[0].0, "plebiscite-HEX-08");
    // HEX-08 links only to HEX-03.
    assert_eq!(
        polls[0].1,
        ["Annex HEX-08", "Leave HEX-08 fogged", "Search HEX-03"]
    );
    assert_eq!(rt.open_poll_id(), Some("plebiscite-HEX-08"));
}

#[test]
fn a_custom_case_has_no_planted_records_but_keeps_the_cap() {
    let mut case = demo_case();
    case.id = "CUSTOM-01".into();
    let mut c = cfg();
    c.case = Some(case);
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg02aHexmap, c).unwrap();
    rt.start(0);
    assert!(rt.case_file().is_none());
    assert!(!rt.context(10, 0).rules_prompt.contains("planted FICTIONAL"));
    rt.apply_ops(1_000, &[research("r1", "HEX-04: the wallet")]);
    rt.apply_ops(2_000, &[research("r2", "HEX-04: the wallet")]);
    assert!(!rt.context(10, 2).state_summary.contains("case file ("));
    let evs = rt.apply_ops(3_000, &[research("r3", "HEX-04: the wallet")]);
    assert_eq!(rejections(&evs)[0].1, "research_cap_reached");
    assert_eq!(started_polls(&evs)[0].0, "plebiscite-HEX-04");

    // A case file that does not belong to the case is refused at start.
    let mut c = cfg();
    let mut other = demo_case();
    other.id = "CUSTOM-02".into();
    c.case = Some(other);
    c.case_file = Some(shadowrunr_segments::CaseFile::demo());
    assert!(matches!(
        SegmentRuntime::with_config(SegmentId::Seg02aHexmap, c),
        Err(SegmentError::Config { .. })
    ));
}

fn scripted_research_run() -> String {
    let mut rt = seg02a();
    let mut out = Vec::new();
    let briefs = [
        "HEX-07: the moon maintenance thread",
        "Trace the hops out of treasury wallet demo-0001",
        "HEX-08: Mirecrown",
        "hex 7 again",
        "HEX-07 a third time",
        "map the shell chain",
    ];
    for step in 0..30_i64 {
        let now = step * 10_000;
        let chat: Vec<ChatMsg> = (0..4)
            .map(|i| {
                let text = match (step + i) % 3 {
                    0 => "!vote 1".to_string(),
                    1 => "!grade HEX-04 DOC".to_string(),
                    _ => "!hint hex7 the moon is lying".to_string(),
                };
                msg(
                    &format!("m{step}-{i}"),
                    now,
                    &format!("v{i}"),
                    Rank::Acolyte,
                    &text,
                )
            })
            .collect();
        out.extend(rt.ingest_chat(now, &chat));
        if let Some(b) = briefs.get(step as usize) {
            out.extend(rt.apply_ops(now, &[research(&format!("r{step}"), b)]));
        }
        out.extend(rt.tick(now, 12));
    }
    out.push(StreamEvent::Op {
        ts_ms: 0,
        op: Op::MarkMoment {
            label: rt.context(12, 300).state_summary,
        },
    });
    serde_json::to_string(&out).unwrap()
}

#[test]
fn research_and_plebiscite_are_deterministic() {
    let a = scripted_research_run();
    assert_eq!(a, scripted_research_run());
    assert!(a.contains("research_cap_reached"), "{a}");
    assert!(a.contains("plebiscite-HEX-07"), "{a}");
}

// ----- context and determinism ------------------------------------------------------------

#[test]
fn context_reports_table_and_state() {
    let rt = seg01();
    let ctx = rt.context(7, 42);
    assert_eq!(ctx.segment, SegmentId::Seg01ImprovisedRoom);
    // The menu is the gate's view: no poll to close and no foil to dismiss in the opening.
    let menu: Vec<&str> = rt
        .table()
        .allowed_ops
        .iter()
        .map(String::as_str)
        .filter(|o| !["close_poll", "dismiss_foil"].contains(o))
        .collect();
    assert_eq!(ctx.allowed_ops, menu);
    assert!(
        ctx.state_summary
            .contains("not now: close_poll (no_such_poll), dismiss_foil (no_active_foil)"),
        "{}",
        ctx.state_summary
    );
    assert_eq!((ctx.ccv, ctx.elapsed_s), (7, 42));
    assert!(ctx.rules_prompt.contains("the-disruptor, the-life-coach"));
    assert!(ctx.state_summary.contains("state opening"));
    // The rules prompt is stable as state changes (cache-friendly).
    let mut rt2 = seg01();
    rt2.apply_ops(0, &[poll("p", &["a", "b"], Rank::Thrall, 30)]);
    assert_eq!(rt2.context(7, 42).rules_prompt, ctx.rules_prompt);
    assert_ne!(rt2.context(7, 42).state_summary, ctx.state_summary);
}

fn scripted_run() -> String {
    let mut rt = seg01();
    let mut out = Vec::new();
    for step in 0..40_i64 {
        let now = step * 15_000;
        let chat: Vec<ChatMsg> = (0..5)
            .map(|i| {
                let rank = [Rank::Thrall, Rank::Acolyte, Rank::Warlock][(i % 3) as usize];
                let text = match (step + i) % 4 {
                    0 => "!summon the-disruptor".to_string(),
                    1 => "!vote the-life-coach".to_string(),
                    2 => "!propose an audit of the moon".to_string(),
                    _ => "hail malakor".to_string(),
                };
                msg(&format!("m{step}-{i}"), now, &format!("v{i}"), rank, &text)
            })
            .collect();
        out.extend(rt.ingest_chat(now, &chat));
        if step == 10 {
            out.extend(rt.apply_ops(now, &[poll("g1", &["a", "b"], Rank::Thrall, 45)]));
        }
        out.extend(rt.tick(now, 4 + step as u32));
    }
    serde_json::to_string(&out).unwrap()
}

#[test]
fn runtime_is_deterministic() {
    let a = scripted_run();
    assert_eq!(a, scripted_run());
    assert!(a.contains("\"type\":\"handover\""));
}

// ----- real cases and the multi-act arc ---------------------------------------------------

fn real_demo_case() -> shadowrunr_types::Case {
    let mut real = demo_case();
    real.id = "REAL-TEST-01".into();
    real.fictional = false;
    real
}

#[test]
fn a_live_stream_refuses_an_unapproved_real_case_and_a_sim_may_run_it() {
    let mut c = cfg();
    c.case = Some(real_demo_case());
    assert!(matches!(
        SegmentRuntime::with_config(SegmentId::Seg02aHexmap, c.clone()),
        Err(SegmentError::NotApproved { case_id }) if case_id == "REAL-TEST-01"
    ));
    c.stream = StreamKind::PrivateSim;
    assert!(SegmentRuntime::with_config(SegmentId::Seg02aHexmap, c.clone()).is_ok());
    c.stream = StreamKind::Live;
    c.case_approved = true;
    let rt = SegmentRuntime::with_config(SegmentId::Seg02aHexmap, c).unwrap();
    let rules = rt.context(30, 0).rules_prompt;
    assert!(
        rules.contains("REAL CASE FROM THE PUBLIC RECORD"),
        "{rules}"
    );
    assert!(rules.contains("by role only"), "{rules}");
    assert!(!rules.contains("FICTIONAL DEMO DATA"), "{rules}");
    // The fictional demo case is never gated.
    assert!(SegmentRuntime::with_config(SegmentId::Seg02aHexmap, cfg()).is_ok());
}

fn arc_runtime(planned_s: u32) -> SegmentRuntime {
    let mut c = cfg();
    c.planned_s = Some(planned_s);
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg02aHexmap, c).unwrap();
    rt.start(0);
    rt
}

fn cards(evs: &[StreamEvent]) -> Vec<String> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Op {
                op: Op::ShowCard { card_id, .. },
                ..
            } => Some(card_id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_short_stream_runs_no_arc() {
    let mut rt = arc_runtime(1_200);
    assert_eq!(rt.arc_act(), None);
    let ctx = rt.context(30, 0);
    assert!(ctx.allowed_ops.iter().any(|o| o == "sign_off"));
    assert!(!ctx.state_summary.contains("ARC"));
    // Every hex is open at once.
    assert_eq!(
        accepted_ops(&rt.apply_ops(1_000, &[reveal("HEX-16", EvidenceGrade::Clm)])),
        1
    );
}

#[test]
fn a_three_hour_stream_runs_three_acts_with_locked_hexes_and_a_seal_window() {
    let mut rt = SegmentRuntime::with_config(
        SegmentId::Seg02aHexmap,
        SegmentConfig {
            planned_s: Some(10_800),
            ..cfg()
        },
    )
    .unwrap();
    let start = rt.start(0);
    assert_eq!(cards(&start), ["arc-act-1"]);
    assert_eq!(rt.arc_act(), Some((0, 3)));
    // Every hex is staged for exactly one act, and every act has hexes.
    let acts: Vec<usize> = rt
        .case()
        .hexes
        .iter()
        .map(|h| rt.hex_act(&h.id).unwrap())
        .collect();
    for k in 0..3 {
        assert!(acts.contains(&k), "act {k} is empty: {acts:?}");
    }
    let in_act = |k: usize| {
        rt.case()
            .hexes
            .iter()
            .find(|h| rt.hex_act(&h.id) == Some(k))
            .unwrap()
            .id
            .clone()
    };
    let (first, later) = (in_act(0), in_act(2));

    // Act I: a later act's hex is locked for reveal and research; sign_off is withheld.
    let ctx = rt.context(30, 60);
    assert!(!ctx.allowed_ops.iter().any(|o| o == "sign_off"));
    // The arc is the whole segment: no handover to SEG-01 midway.
    assert!(!ctx.allowed_ops.iter().any(|o| o == "switch_segment"));
    let switch = Op::SwitchSegment {
        to: SegmentId::Seg01ImprovisedRoom,
    };
    assert_eq!(
        rejections(&rt.apply_ops(60_000, &[switch]))[0].1,
        "arc_in_progress"
    );
    assert_eq!(rt.segment(), SegmentId::Seg02aHexmap);
    assert!(
        ctx.state_summary.contains("ARC act 1/3"),
        "{}",
        ctx.state_summary
    );
    assert!(
        ctx.state_summary.contains("LOCKED until act 2"),
        "{}",
        ctx.state_summary
    );
    assert!(
        ctx.rules_prompt.contains("in 3 acts"),
        "{}",
        ctx.rules_prompt
    );
    assert_eq!(
        rejections(&rt.apply_ops(60_000, &[reveal(&later, EvidenceGrade::Clm)]))[0].1,
        "act_locked"
    );
    let locked_brief = Op::DispatchResearch {
        job_id: "j1".into(),
        brief: format!("{later}: who is behind it"),
    };
    assert_eq!(
        rejections(&rt.apply_ops(60_000, &[locked_brief]))[0].1,
        "act_locked"
    );
    assert_eq!(rt.searches(&later), 0, "a locked brief is not counted");
    // A reveal needs a beat of work first: one bone-clerk search on the hex.
    rt.apply_ops(60_000, &[research("j0", &format!("{first}: the filing"))]);
    assert_eq!(
        accepted_ops(&rt.apply_ops(60_000, &[reveal(&first, EvidenceGrade::Clm)])),
        1
    );
    let early = Op::SignOff {
        reason: "bored".into(),
    };
    assert_eq!(
        rejections(&rt.apply_ops(61_000, std::slice::from_ref(&early)))[0].1,
        "dossier_not_sealed"
    );
    assert!(!rt.is_signed_off());

    // Act II opens at minute 60 with its card; act III at minute 120.
    assert!(cards(&rt.tick(3_599_000, 30)).is_empty());
    assert_eq!(cards(&rt.tick(3_600_000, 30)), ["arc-act-2"]);
    assert_eq!(rt.arc_act(), Some((1, 3)));
    assert_eq!(cards(&rt.tick(7_200_000, 30)), ["arc-act-3"]);
    rt.apply_ops(
        7_200_000,
        &[research("j2", &format!("{later}: who is behind it"))],
    );
    assert_eq!(
        accepted_ops(&rt.apply_ops(7_200_000, &[reveal(&later, EvidenceGrade::Clm)])),
        1
    );
    let ctx = rt.context(30, 7_300);
    assert!(
        ctx.state_summary.contains("Final act"),
        "{}",
        ctx.state_summary
    );

    // The seal window: the last 10 minutes. sign_off is offered and accepted.
    assert_eq!(cards(&rt.tick(10_200_000, 30)), ["arc-seal"]);
    let ctx = rt.context(30, 10_200);
    assert!(ctx.allowed_ops.iter().any(|o| o == "sign_off"));
    assert!(
        ctx.state_summary.contains("SEAL WINDOW"),
        "{}",
        ctx.state_summary
    );
    assert_eq!(accepted_ops(&rt.apply_ops(10_200_000, &[early])), 1);
    assert!(rt.is_signed_off());
}

#[test]
fn a_fleet_loss_sign_off_is_never_held_for_the_seal() {
    let mut rt = arc_runtime(10_800);
    let lost = Op::SignOff {
        reason: "fleet_lost: engine degraded or down for 300s".into(),
    };
    assert_eq!(accepted_ops(&rt.apply_ops(5_000, &[lost])), 1);
    assert!(rt.is_signed_off());
}

#[test]
fn a_staged_arc_from_the_case_file_wins_over_kind_order() {
    // The case's own acts would place every hex: this case has none.
    let mut case = demo_case();
    case.acts.clear();
    let mut file = shadowrunr_segments::CaseFile::demo();
    // Act III holds only HEX-01; the rest are dealt by kind.
    file.arc = vec![Vec::new(), Vec::new(), vec!["HEX-01".into()]];
    let rt = SegmentRuntime::with_config(
        SegmentId::Seg02aHexmap,
        SegmentConfig {
            case: Some(case),
            case_file: Some(file),
            planned_s: Some(10_800),
            ..cfg()
        },
    )
    .unwrap();
    assert_eq!(rt.hex_act("HEX-01"), Some(2));
    assert!(demo_case()
        .hexes
        .iter()
        .all(|h| rt.hex_act(&h.id).is_some()));
}

// ----- reveal pacing inside an act ---------------------------------------------------------

/// The hexes staged for act `k`, in map order.
fn act_hexes(rt: &SegmentRuntime, k: usize) -> Vec<String> {
    rt.case()
        .hexes
        .iter()
        .filter(|h| rt.hex_act(&h.id) == Some(k))
        .map(|h| h.id.clone())
        .collect()
}

#[test]
fn arc_reveals_wait_for_their_slot_and_a_slot_opens_on_schedule() {
    let mut rt = arc_runtime(10_800);
    let act1 = act_hexes(&rt, 0);
    assert!(act1.len() >= 3, "{act1:?}");
    let slot = 3_600_000 / act1.len() as i64;
    // Every act I hex gets its beat of work up front.
    for (i, h) in act1.iter().enumerate() {
        rt.apply_ops(
            1_000,
            &[research(&format!("w{i}"), &format!("{h}: the filing"))],
        );
        assert!(rt.hex_worked(h));
    }
    // The first slot is open from the act's start.
    assert_eq!(rt.next_reveal_ms(), Some(0));
    assert_eq!(
        accepted_ops(&rt.apply_ops(2_000, &[reveal(&act1[0], EvidenceGrade::Clm)])),
        1
    );
    // The next one waits for its slot.
    assert_eq!(rt.next_reveal_ms(), Some(slot));
    let evs = rt.apply_ops(3_000, &[reveal(&act1[1], EvidenceGrade::Clm)]);
    assert_eq!(
        rejections(&evs),
        [(ModStage::Output, "reveal_not_due".to_string())]
    );
    assert!(rt.case().hexes.iter().any(|h| h.id == act1[1] && h.fog));
    // The turn context says when it unlocks and what to do meanwhile.
    let ctx = rt.context(30, 3);
    let s = &ctx.state_summary;
    assert!(
        s.contains(&format!("next reveal unlocks at minute {}", slot / 60_000)),
        "{s}"
    );
    assert!(s.contains("BETWEEN REVEALS"), "{s}");
    assert!(s.contains("at most one recap every 15 minutes"), "{s}");
    assert!(
        ctx.rules_prompt.contains("Reveal pacing"),
        "{}",
        ctx.rules_prompt
    );
    assert!(
        ctx.allowed_ops.iter().any(|o| o == "reveal_hex"),
        "attempts stay visible as refusals"
    );
    // A regrade of a revealed hex is not a reveal: never paced.
    assert_eq!(
        accepted_ops(&rt.apply_ops(4_000, &[reveal(&act1[0], EvidenceGrade::Rep)])),
        1
    );
    // One second short of the slot: refused. On the slot: accepted.
    assert_eq!(
        rejections(&rt.apply_ops(slot - 1_000, &[reveal(&act1[1], EvidenceGrade::Clm)]))[0].1,
        "reveal_not_due"
    );
    assert_eq!(
        accepted_ops(&rt.apply_ops(slot, &[reveal(&act1[1], EvidenceGrade::Clm)])),
        1
    );
    assert!(rt
        .context(30, 0)
        .state_summary
        .contains("ready to reveal (searched or voted)"));
    // Idling saves slots, but the gap (half a slot) stops them being spent in one burst.
    let late = 3 * slot + 1_000;
    assert_eq!(
        accepted_ops(&rt.apply_ops(late, &[reveal(&act1[2], EvidenceGrade::Clm)])),
        1
    );
    if let Some(h) = act1.get(3) {
        assert_eq!(
            rejections(&rt.apply_ops(late + 1_000, &[reveal(h, EvidenceGrade::Clm)]))[0].1,
            "reveal_not_due"
        );
        assert_eq!(rt.next_reveal_ms(), Some(late + slot / 2));
        assert_eq!(
            accepted_ops(&rt.apply_ops(late + slot / 2, &[reveal(h, EvidenceGrade::Clm)])),
            1
        );
    }
}

#[test]
fn a_hex_with_no_search_and_no_vote_cannot_be_revealed() {
    let mut rt = arc_runtime(10_800);
    let act1 = act_hexes(&rt, 0);
    let (a, b) = (act1[0].clone(), act1[1].clone());
    assert!(!rt.hex_worked(&a));
    assert_eq!(
        rejections(&rt.apply_ops(1_000, &[reveal(&a, EvidenceGrade::Clm)]))[0].1,
        "reveal_needs_work"
    );
    let s = rt.context(30, 1).state_summary;
    assert!(s.contains("needs a beat of work before its reveal"), "{s}");
    // A crowd poll on which hex to crack next: its winner gets the beat of work.
    let open = Op::StartPoll {
        poll_id: "next-hex".into(),
        question: "Which hex do we crack next?".into(),
        options: vec![a.clone(), b.clone()],
        min_rank: Rank::Thrall,
        secs: 60,
    };
    assert_eq!(accepted_ops(&rt.apply_ops(2_000, &[open])), 1);
    rt.ingest_chat(
        3_000,
        &[
            msg("v1", 3_000, "ann", Rank::Thrall, "!vote 2"),
            msg("v2", 3_000, "bob", Rank::Thrall, "!vote 2"),
        ],
    );
    // An open poll is not a closed vote.
    assert!(!rt.hex_worked(&b));
    let close = Op::ClosePoll {
        poll_id: "next-hex".into(),
    };
    assert_eq!(accepted_ops(&rt.apply_ops(4_000, &[close])), 1);
    assert!(rt.hex_worked(&b), "the crowd chose {b}");
    assert!(!rt.hex_worked(&a), "the loser is not worked");
    assert_eq!(
        accepted_ops(&rt.apply_ops(5_000, &[reveal(&b, EvidenceGrade::Clm)])),
        1
    );
    assert_eq!(
        rejections(&rt.apply_ops(5_000_000, &[reveal(&a, EvidenceGrade::Clm)]))[0].1,
        "reveal_needs_work"
    );
}

#[test]
fn a_short_stream_has_no_reveal_pacing() {
    let mut rt = arc_runtime(1_200);
    assert_eq!(rt.next_reveal_ms(), None);
    let evs = rt.apply_ops(
        1_000,
        &[
            reveal("HEX-01", EvidenceGrade::Clm),
            reveal("HEX-02", EvidenceGrade::Clm),
            reveal("HEX-03", EvidenceGrade::Clm),
        ],
    );
    assert_eq!(accepted_ops(&evs), 3, "{evs:?}");
    assert!(!rt.context(30, 1).state_summary.contains("REVEAL PACE"));
}

// ----- SEG-01 cold open -------------------------------------------------------------------

/// One line from each of `who`, at `t`.
fn chat_from(rt: &mut SegmentRuntime, t: i64, who: &[&str]) {
    let msgs: Vec<ChatMsg> = who
        .iter()
        .map(|w| msg(&format!("{w}-{t}"), t, w, Rank::Thrall, "hello lich"))
        .collect();
    rt.ingest_chat(t, &msgs);
}

/// The cold-open-end cards in `evs`.
fn cold_open_cards(evs: &[StreamEvent]) -> Vec<String> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Op {
                op: Op::ShowCard { card_id, .. },
                ..
            } if card_id.starts_with("cold-open-end") => Some(card_id.clone()),
            _ => None,
        })
        .collect()
}

const ROOM: [&str; 5] = ["ann", "bob", "cyd", "dee", "eve"];

#[test]
fn seg01_starts_in_the_cold_open_with_its_own_prompt_line() {
    let rt = seg01_cold();
    assert_eq!(rt.state(), "cold_open");
    assert!(rt.in_cold_open());
    let ctx = rt.context(3, 0);
    assert_eq!(ctx.state, "cold_open");
    assert!(
        ctx.state_summary.contains(
            "The room is still filling: tell tonight's cold-open story; answer only newcomers' \
             first hello and anything about safety; no polls, no foils yet."
        ),
        "{}",
        ctx.state_summary
    );
    // The opening carries today's text and no cold-open line.
    let open = seg01().context(3, 0);
    assert_eq!(open.state, "opening");
    assert!(!open.state_summary.contains("still filling"));
    assert_eq!(open.rules_prompt, ctx.rules_prompt);
}

#[test]
fn the_cold_open_lasts_at_least_the_floor() {
    let mut rt = seg01_cold();
    // A full room from the first second: the active chatters hold long past the sustain.
    for t in (0..60_000).step_by(5_000) {
        chat_from(&mut rt, t, &ROOM);
        assert!(cold_open_cards(&rt.tick(t, 20)).is_empty(), "t={t}");
        assert_eq!(rt.state(), "cold_open", "t={t}");
    }
    chat_from(&mut rt, 60_000, &ROOM);
    let evs = rt.tick(60_000, 20);
    assert_eq!(cold_open_cards(&evs), ["cold-open-end-room-filled"]);
    assert_eq!(rt.state(), "opening");
    let (at, why) = rt.cold_open_ended().unwrap();
    assert_eq!(at, 60_000);
    assert_eq!(why, "room filled: 5 chatters (>= 5) held 20 s");
}

#[test]
fn the_cold_open_ends_when_the_active_chatters_hold_for_the_sustain() {
    let mut rt = seg01_cold();
    rt.tick(0, 3);
    // Four chatters are not a room.
    chat_from(&mut rt, 70_000, &ROOM[..4]);
    assert!(cold_open_cards(&rt.tick(70_000, 10)).is_empty());
    // The fifth arrives at 80 s: the window starts there...
    chat_from(&mut rt, 80_000, &ROOM[4..]);
    assert!(cold_open_cards(&rt.tick(80_000, 10)).is_empty());
    assert!(cold_open_cards(&rt.tick(99_999, 10)).is_empty());
    assert_eq!(rt.state(), "cold_open");
    // ...and holds for 20 s.
    let evs = rt.tick(100_000, 10);
    assert_eq!(cold_open_cards(&evs), ["cold-open-end-room-filled"]);
    assert_eq!(rt.state(), "opening");
    // Polls open now.
    let evs = rt.apply_ops(101_000, &[poll("p1", &["a", "b"], Rank::Thrall, 30)]);
    assert_eq!(accepted_ops(&evs), 1);
    assert_eq!(rt.state(), "crowd_game");
}

#[test]
fn a_dip_in_active_chatters_restarts_the_sustain() {
    let mut rt = seg01_cold();
    chat_from(&mut rt, 70_000, &ROOM);
    rt.tick(70_000, 10);
    // Four keep talking; the fifth's line falls out of the 60 s window at 130 s.
    for t in [85_000, 100_000, 115_000, 129_000] {
        chat_from(&mut rt, t, &ROOM[..4]);
    }
    // The sustain held from 70 s, so the room filled at 90 s.
    assert_eq!(
        cold_open_cards(&rt.tick(90_000, 10)),
        ["cold-open-end-room-filled"]
    );

    let mut rt = seg01_cold();
    chat_from(&mut rt, 70_000, &ROOM);
    rt.tick(70_000, 10);
    chat_from(&mut rt, 75_000, &ROOM[..4]);
    // At 131 s eve's line is 61 s old: four active, the sustain resets before it held.
    assert!(cold_open_cards(&rt.tick(80_000, 10)).is_empty());
    assert!(cold_open_cards(&rt.tick(131_000, 10)).is_empty());
    // Eve is back at 132 s: the sustain starts again from there.
    chat_from(&mut rt, 132_000, &ROOM);
    assert!(cold_open_cards(&rt.tick(132_000, 10)).is_empty());
    assert!(cold_open_cards(&rt.tick(151_999, 10)).is_empty());
    assert_eq!(
        cold_open_cards(&rt.tick(152_000, 10)),
        ["cold-open-end-room-filled"]
    );
}

#[test]
fn the_ceiling_ends_the_cold_open_regardless() {
    let mut rt = seg01_cold();
    chat_from(&mut rt, 10_000, &ROOM[..2]);
    assert!(cold_open_cards(&rt.tick(10_000, 4)).is_empty());
    assert!(cold_open_cards(&rt.tick(179_999, 4)).is_empty());
    assert_eq!(rt.state(), "cold_open");
    let evs = rt.tick(180_000, 4);
    assert_eq!(cold_open_cards(&evs), ["cold-open-end-ceiling"]);
    assert_eq!(rt.state(), "opening");
    assert_eq!(
        rt.cold_open_ended(),
        Some((180_000, "ceiling: 180 s with 0 of 5 chatters"))
    );
    // Once only.
    assert!(cold_open_cards(&rt.tick(185_000, 4)).is_empty());
}

#[test]
fn the_cold_open_refuses_polls_and_foils_and_holds_the_handover() {
    let mut rt = seg01_cold();
    let ctx = rt.context(3, 0);
    for op in ["start_poll", "summon_foil", "switch_segment", "close_poll"] {
        assert!(!ctx.allowed_ops.iter().any(|o| o == op), "{op}");
    }
    assert!(ctx.allowed_ops.iter().any(|o| o == "highlight"));

    let evs = rt.apply_ops(1_000, &[poll("p1", &["a", "b"], Rank::Thrall, 30)]);
    assert_eq!(
        rejections(&evs),
        [(ModStage::Output, "not_in_state".to_string())]
    );
    assert_eq!(rt.open_poll_id(), None);
    let s = rt.context(3, 1).state_summary;
    assert!(s.contains("REFUSED lately"), "{s}");
    assert!(s.contains("start_poll p1: not_in_state"), "{s}");
    let evs = rt.apply_ops(
        1_000,
        &[Op::SummonFoil {
            foil: "the-disruptor".into(),
        }],
    );
    assert_eq!(rejections(&evs)[0].1, "not_in_state");
    assert_eq!(rt.active_foil(), None);
    // Chat cannot open a foil vote either.
    let evs = rt.ingest_chat(
        2_000,
        &[msg(
            "s1",
            2_000,
            "ann",
            Rank::Thrall,
            "!summon the-disruptor",
        )],
    );
    assert!(started_polls(&evs).is_empty());
    assert_eq!(rt.open_poll_id(), None);
    // The handover clock waits for the room: a big ccv in the cold open does not count.
    for t in (0..=180_000).step_by(10_000) {
        assert!(handovers(&rt.tick(t, 30)).is_empty(), "t={t}");
    }
    assert_eq!(rt.state(), "opening");
    assert!(handovers(&rt.tick(359_999, 30)).is_empty());
    assert_eq!(
        handovers(&rt.tick(370_000, 30)),
        ["ccv >= 15 sustained 180 s"]
    );
}

#[test]
fn the_handovers_elapsed_ceiling_cuts_a_cold_open_short() {
    let mut c = cfg();
    c.cold_open = None;
    c.handover = Some(HandoverRule {
        to: SegmentId::Seg02aHexmap,
        ccv_min: 5,
        sustain_s: 10,
        max_elapsed_s: 48,
        grace_s: 60,
    });
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, c).unwrap();
    rt.start(0);
    // ccv 30 for 40 s does not hand over while the room fills...
    for t in (0..48_000).step_by(5_000) {
        assert!(handovers(&rt.tick(t, 30)).is_empty(), "t={t}");
    }
    // ...but the segment's ceiling does, and ends the cold open with it.
    assert_eq!(handovers(&rt.tick(50_000, 30)), ["elapsed >= 48 s"]);
    assert_eq!(rt.state(), "handover_pending");
    assert_eq!(
        rt.cold_open_ended(),
        Some((50_000, "handover: elapsed >= 48 s"))
    );
}

#[test]
fn the_cold_open_rule_is_config() {
    let mut c = cfg();
    c.cold_open = Some(ColdOpenRule {
        min_s: 5,
        max_s: 30,
        active_chatters: 2,
        sustain_s: 0,
        ..no_cold_open()
    });
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, c).unwrap();
    rt.start(0);
    assert_eq!(rt.state(), "cold_open");
    chat_from(&mut rt, 1_000, &ROOM[..2]);
    assert!(
        cold_open_cards(&rt.tick(1_000, 2)).is_empty(),
        "under the floor"
    );
    assert_eq!(
        cold_open_cards(&rt.tick(5_000, 2)),
        ["cold-open-end-room-filled"]
    );
    let mut bad = cfg();
    bad.cold_open = Some(ColdOpenRule {
        min_s: 50,
        max_s: 30,
        ..no_cold_open()
    });
    assert!(SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, bad).is_err());
    let mut nowhere = cfg();
    nowhere.cold_open = Some(ColdOpenRule {
        to: "nowhere".into(),
        ..no_cold_open()
    });
    assert!(SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, nowhere).is_err());
}

// ----- honest ties ------------------------------------------------------------------------

#[test]
fn a_tied_crowd_poll_is_recorded_and_summarised_as_a_tie() {
    let mut rt = seg01();
    rt.apply_ops(
        1_000,
        &[poll("p4", &["Riddles", "Duel", "Chess"], Rank::Thrall, 30)],
    );
    rt.ingest_chat(
        2_000,
        &[
            msg("v1", 2_000, "a", Rank::Thrall, "!vote riddles"),
            msg("v2", 2_000, "b", Rank::Thrall, "!vote duel"),
            msg("v3", 2_000, "c", Rank::Thrall, "!vote riddles"),
            msg("v4", 2_000, "d", Rank::Thrall, "!vote duel"),
        ],
    );
    rt.tick(31_000, 0);
    assert_eq!(
        rt.last_crowd(),
        Some((
            "p4",
            &PollOutcome::Tie {
                options: vec!["Riddles".into(), "Duel".into()],
                voters: 2
            }
        ))
    );
    let s = rt.context(0, 31).state_summary;
    assert!(
        s.contains(
            "last poll p4 closed: TIE between Riddles and Duel (2 votes each): you break it, \
             and say you are breaking it"
        ),
        "{s}"
    );
    assert!(!s.contains("winner"), "{s}");

    // Equal weight but more voters is a win, not a tie.
    rt.apply_ops(40_000, &[poll("p5", &["a", "b"], Rank::Thrall, 30)]);
    rt.ingest_chat(
        41_000,
        &[
            msg("w1", 41_000, "a", Rank::Thrall, "!vote a"),
            msg("w2", 41_000, "b", Rank::Thrall, "!vote a"),
            msg("w3", 41_000, "c", Rank::Thrall, "!vote b"),
        ],
    );
    rt.tick(71_000, 0);
    assert_eq!(
        rt.last_crowd(),
        Some(("p5", &PollOutcome::Winner("a".into())))
    );
    // No votes at all is not a tie.
    rt.apply_ops(80_000, &[poll("p6", &["a", "b"], Rank::Thrall, 30)]);
    rt.tick(110_000, 0);
    assert_eq!(rt.last_crowd(), Some(("p6", &PollOutcome::NoVotes)));
    assert!(rt
        .context(0, 110)
        .state_summary
        .contains("last poll p6 closed: no votes, no winner"));
}

#[test]
fn a_tied_foil_vote_summons_nobody_and_the_host_breaks_it() {
    let mut rt = seg01();
    let evs = rt.ingest_chat(
        1_000,
        &[
            msg("m1", 1_000, "a", Rank::Thrall, "!summon the-life-coach"),
            msg("m2", 1_000, "b", Rank::Thrall, "!summon the-disruptor"),
        ],
    );
    assert_eq!(started_polls(&evs).len(), 1);
    let evs = rt.tick(61_000, 2);
    assert!(!evs.iter().any(|e| matches!(
        e,
        StreamEvent::Op {
            op: Op::SummonFoil { .. },
            ..
        }
    )));
    assert_eq!(rt.active_foil(), None);
    assert_eq!(rt.state(), "opening");
    let s = rt.context(2, 61).state_summary;
    assert!(
        s.contains("foil vote foil-vote-1 closed: TIE between the-disruptor and the-life-coach"),
        "{s}"
    );
    assert!(s.contains("You break it with summon_foil"), "{s}");
    let evs = rt.apply_ops(
        62_000,
        &[Op::SummonFoil {
            foil: "the-life-coach".into(),
        }],
    );
    assert_eq!(accepted_ops(&evs), 1);
    assert!(!rt.context(2, 62).state_summary.contains("TIE"));
}

#[test]
fn a_tied_plebiscite_is_the_hosts_to_break() {
    let mut rt = seg02a();
    for (job, t) in [("r1", 1_000), ("r2", 2_000)] {
        rt.apply_ops(t, &[research(job, "HEX-07: the moon maintenance thread")]);
    }
    rt.apply_ops(3_000, &[research("r3", "HEX-07: again")]);
    assert_eq!(rt.open_poll_id(), Some("plebiscite-HEX-07"));
    rt.ingest_chat(
        5_000,
        &[
            msg("v1", 5_000, "a", Rank::Thrall, "!vote Annex HEX-07"),
            msg("v2", 5_000, "b", Rank::Thrall, "!vote 2"),
        ],
    );
    rt.tick(63_000, 10);
    assert_eq!(
        rt.last_plebiscite(),
        Some((
            "HEX-07",
            &PollOutcome::Tie {
                options: vec!["Annex HEX-07".into(), "Leave HEX-07 fogged".into()],
                voters: 1
            }
        ))
    );
    let s = rt.context(10, 63).state_summary;
    assert!(
        s.contains(
            "PLEBISCITE on HEX-07 closed: TIE between Annex HEX-07 and Leave HEX-07 fogged (1 \
             votes each): you break it, and say you are breaking it. Do not search HEX-07 again."
        ),
        "{s}"
    );
    assert!(!s.contains("decided"), "{s}");
    // An Annex among the tied options counts as the hex's vote: the host may annex it.
    assert!(rt.hex_worked("HEX-07"));
}

#[test]
fn a_tied_crowd_poll_on_hexes_works_every_tied_hex() {
    let mut rt = arc_runtime(10_800);
    let act1 = act_hexes(&rt, 0);
    let (a, b, c) = (act1[0].clone(), act1[1].clone(), act1[2].clone());
    let open = Op::StartPoll {
        poll_id: "next-hex".into(),
        question: "Which hex do we crack next?".into(),
        options: vec![a.clone(), b.clone(), c.clone()],
        min_rank: Rank::Thrall,
        secs: 60,
    };
    assert_eq!(accepted_ops(&rt.apply_ops(2_000, &[open])), 1);
    rt.ingest_chat(
        3_000,
        &[
            msg("v1", 3_000, "ann", Rank::Thrall, "!vote 1"),
            msg("v2", 3_000, "bob", Rank::Thrall, "!vote 2"),
        ],
    );
    rt.apply_ops(
        4_000,
        &[Op::ClosePoll {
            poll_id: "next-hex".into(),
        }],
    );
    assert!(rt.hex_worked(&a) && rt.hex_worked(&b), "both tied hexes");
    assert!(!rt.hex_worked(&c), "the hex nobody chose");
    let s = rt.context(30, 4).state_summary;
    assert!(
        s.contains(&format!(
            "TIE between {a} and {b} (1 votes each): you break it, and say you are breaking it \
             (each tied hex counts as voted: {a}, {b})"
        )),
        "{s}"
    );
}

// ----- screens (appendix (screens and board)) ---------------------------------------------

fn screen(screen: ScreenId, record_id: Option<&str>) -> Op {
    Op::ShowScreen {
        screen,
        record_id: record_id.map(str::to_string),
        focus: None,
    }
}

/// The `show_screen` ops in `evs`, as (screen, record_id).
fn screens(evs: &[StreamEvent]) -> Vec<(ScreenId, Option<String>)> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Op {
                op: Op::ShowScreen {
                    screen, record_id, ..
                },
                ..
            } => Some((*screen, record_id.clone())),
            _ => None,
        })
        .collect()
}

/// Every event as its wire tag (`record_shown`, `op:show_screen`...), in order.
fn tags(evs: &[StreamEvent]) -> Vec<String> {
    evs.iter()
        .map(|e| {
            let v = serde_json::to_value(e).unwrap();
            let t = v["type"].as_str().unwrap().to_string();
            match v["op"]["op"].as_str() {
                Some(op) => format!("{t}:{op}"),
                None => t,
            }
        })
        .collect()
}

#[test]
fn seg01_has_one_screen_and_the_cold_open_stays_on_host() {
    let mut rt = seg01_cold();
    assert!(rt.in_cold_open());
    assert_eq!(rt.screen(), ScreenId::Host);
    let evs = rt.apply_ops(1_000, &[screen(ScreenId::Board, None)]);
    assert_eq!(rejections(&evs)[0].1, "op_not_allowed");
    assert_eq!(rt.screen(), ScreenId::Host);
    // The cold open ends on its ceiling: still the big-avatar room, nothing to switch.
    let evs = rt.tick(180_000, 0);
    assert!(!rt.in_cold_open());
    assert!(screens(&evs).is_empty(), "{evs:?}");
    assert_eq!(rt.screen(), ScreenId::Host);
    let ctx = rt.context(0, 180);
    assert!(!ctx.allowed_ops.iter().any(|o| o == "show_screen"));
    assert!(
        !ctx.state_summary.contains("Screen:"),
        "{}",
        ctx.state_summary
    );
}

#[test]
fn browser_needs_a_record() {
    let mut rt = seg02a();
    let evs = rt.apply_ops(1_000, &[screen(ScreenId::Browser, None)]);
    assert_eq!(rejections(&evs)[0].1, "no_record");
    let evs = rt.apply_ops(1_000, &[screen(ScreenId::Desk, None)]);
    assert_eq!(rejections(&evs)[0].1, "no_record");
    assert_eq!(rt.screen(), ScreenId::Board);
    // The refusal is named in the state, like every other.
    let summary = rt.context(0, 1).state_summary;
    assert!(summary.contains("show_screen: no_record"), "{summary}");
}

#[test]
fn a_record_must_be_filed_and_back_before_the_browser_shows_it() {
    let mut rt = seg02a();
    // DOC-03 is in the case file, but no bone-clerk has been sent for it yet.
    let evs = rt.apply_ops(1_000, &[screen(ScreenId::Browser, Some("DOC-03"))]);
    assert_eq!(rejections(&evs)[0].1, "record_not_filed");
    assert!(!evs
        .iter()
        .any(|e| matches!(e, StreamEvent::RecordShown { .. })));
    let evs = rt.apply_ops(1_000, &[screen(ScreenId::Desk, Some("NOPE-01"))]);
    assert_eq!(rejections(&evs)[0].1, "record_not_filed");
    rt.apply_ops(
        2_000,
        &[research("r1", "HEX-04: trace the treasury wallet")],
    );
    // Filed at dispatch (the case file and the DOC rule see it), but the job is still out.
    let evs = rt.apply_ops(2_500, &[screen(ScreenId::Browser, Some("DOC-03"))]);
    assert_eq!(rejections(&evs)[0].1, "record_not_returned");
    assert!(!evs
        .iter()
        .any(|e| matches!(e, StreamEvent::RecordShown { .. })));
    let summary = rt.context(0, 2).state_summary;
    assert!(
        summary.contains("Records on file: none yet"),
        "a record still out is not offered: {summary}"
    );
    rt.job_done("r1", 2_800);
    let evs = rt.apply_ops(3_000, &[screen(ScreenId::Browser, Some("CLM-02"))]);
    assert!(rejections(&evs).is_empty(), "{evs:?}");
    assert_eq!(rt.screen(), ScreenId::Browser);
    assert_eq!(rt.screen_record(), Some("CLM-02"));
}

#[test]
fn record_shown_goes_out_once_and_before_the_op_that_names_it() {
    let mut rt = seg02a();
    rt.apply_ops(0, &[research("r1", "HEX-04: trace the treasury wallet")]);
    rt.job_done("r1", 500);
    let evs = rt.apply_ops(1_000, &[screen(ScreenId::Desk, Some("CLM-02"))]);
    assert_eq!(tags(&evs), ["record_shown", "op:show_screen"]);
    let StreamEvent::RecordShown { ts_ms, record } = &evs[0] else {
        unreachable!()
    };
    assert_eq!(*ts_ms, 1_000);
    let filed = rt
        .case_file()
        .unwrap()
        .records
        .iter()
        .find(|r| r.id == "CLM-02")
        .unwrap();
    assert_eq!(
        (&record.id, &record.hex, record.grade, &record.title),
        (&filed.id, &filed.hex, filed.grade, &filed.title)
    );
    assert_eq!(
        (
            &record.source,
            &record.summary,
            &record.retrieved,
            &record.via
        ),
        (&filed.source, &filed.summary, &filed.retrieved, &filed.via)
    );
    assert!(record.fictional);
    // The record's date rides along for the board's timeline.
    assert_eq!(record.date, filed.date);
    assert_eq!(record.date.as_deref(), Some("2026-03-09"));
    // Absent provenance is omitted on the wire, never null.
    let line = serde_json::to_string(&evs[0]).unwrap();
    assert!(!line.contains("null"), "{line}");
    // Named again: no second record_shown.
    let evs = rt.apply_ops(2_000, &[screen(ScreenId::Board, None)]);
    assert_eq!(tags(&evs), ["op:show_screen"]);
    let evs = rt.apply_ops(3_000, &[screen(ScreenId::Browser, Some("CLM-02"))]);
    assert_eq!(tags(&evs), ["op:show_screen"]);
    // A refused op sends no record.
    let evs = rt.apply_ops(4_000, &[screen(ScreenId::Browser, Some("DOC-01"))]);
    assert_eq!(tags(&evs), ["moderation"]);
}

#[test]
fn browser_and_desk_reopen_the_record_already_open() {
    let mut rt = seg02a();
    rt.apply_ops(0, &[research("r1", "HEX-04: trace the treasury wallet")]);
    rt.job_done("r1", 500);
    rt.apply_ops(1_000, &[screen(ScreenId::Browser, Some("DOC-03"))]);
    let evs = rt.apply_ops(
        2_000,
        &[
            // A record_id on the board is ignored: the board shows no record.
            screen(ScreenId::Board, Some("NOPE-01")),
            screen(ScreenId::Desk, None),
        ],
    );
    assert!(rejections(&evs).is_empty(), "{evs:?}");
    assert_eq!(rt.screen(), ScreenId::Desk);
    assert_eq!(rt.screen_record(), Some("DOC-03"));
    let summary = rt.context(0, 2).state_summary;
    assert!(
        summary.contains(
            "Screen: desk on DOC-03, board focused on HEX-04. Records on file, newest first: \
             CLM-02, DOC-03"
        ),
        "{summary}"
    );
}

#[test]
fn a_returning_job_opens_its_first_filed_record() {
    let mut rt = seg02a();
    assert!(rt.job_done("r1", 500).is_empty(), "no such job yet");
    rt.apply_ops(0, &[research("r1", "HEX-04: trace the treasury wallet")]);
    let evs = rt.job_done("r1", 20_000);
    assert_eq!(tags(&evs), ["record_shown", "op:show_screen"]);
    assert_eq!(
        screens(&evs),
        [(ScreenId::Browser, Some("DOC-03".to_string()))]
    );
    assert_eq!(rt.screen(), ScreenId::Browser);
    // Once per job.
    assert!(rt.job_done("r1", 21_000).is_empty());
    // A search that filed nothing opens nothing.
    rt.apply_ops(
        22_000,
        &[research("r2", "HEX-08: who controls Mirecrown Trust")],
    );
    assert!(rt.job_done("r2", 30_000).is_empty());
    // A refused dispatch is no job of the runtime's.
    rt.apply_ops(31_000, &[research("r3", "HEX-04: again")]);
    let evs = rt.apply_ops(32_000, &[research("r4", "HEX-04: and again")]);
    assert_eq!(rejections(&evs)[0].1, "research_cap_reached");
    assert!(rt.job_done("r4", 40_000).is_empty());
}

#[test]
fn a_default_never_overrides_the_models_screen_within_the_hold() {
    use shadowrunr_segments::runtime::SCREEN_HOLD_MS;
    assert_eq!(SCREEN_HOLD_MS, 8_000);
    let mut rt = seg02a();
    rt.apply_ops(
        0,
        &[
            research("r1", "HEX-04: trace the treasury wallet"),
            research("r2", "HEX-01: the syndicate's licence"),
        ],
    );
    rt.apply_ops(10_000, &[screen(ScreenId::Board, None)]);
    // Inside the hold: suppressed, and the job's default is spent.
    assert!(rt.job_done("r1", 10_000 + SCREEN_HOLD_MS - 1).is_empty());
    assert_eq!(rt.screen(), ScreenId::Board);
    assert!(rt.job_done("r1", 30_000).is_empty());
    // At the end of the hold the next job's default goes up.
    let evs = rt.job_done("r2", 10_000 + SCREEN_HOLD_MS);
    assert_eq!(
        screens(&evs),
        [(ScreenId::Browser, Some("DOC-01".to_string()))]
    );
}

#[test]
fn no_screen_defaults_after_the_sign_off() {
    let mut rt = seg02a();
    rt.apply_ops(0, &[research("r1", "HEX-04: trace the treasury wallet")]);
    rt.apply_ops(
        1_000,
        &[Op::SignOff {
            reason: "done".into(),
        }],
    );
    assert!(rt.is_signed_off());
    assert!(rt.job_done("r1", 20_000).is_empty());
    let evs = rt.apply_ops(21_000, &[screen(ScreenId::Board, None)]);
    assert_eq!(rejections(&evs)[0].1, "signed_off");
}

#[test]
fn the_screen_line_is_short_and_capped() {
    let mut rt = seg02a();
    let summary = rt.context(0, 0).state_summary;
    assert!(
        summary.contains("Screen: board. Records on file: none yet (browser and desk need one)"),
        "{summary}"
    );
    rt.apply_ops(
        0,
        &[
            research("r1", "HEX-01: the licence"),
            research("r2", "HEX-04: the wallet"),
            research("r3", "HEX-05: the promise"),
        ],
    );
    let line = |rt: &SegmentRuntime| {
        let summary = rt.context(0, 0).state_summary;
        summary
            .lines()
            .find(|l| l.starts_with("Screen:"))
            .unwrap()
            .to_string()
    };
    // Filed, but no job is back yet: nothing to offer. The board follows the last dispatch.
    assert_eq!(
        line(&rt),
        "Screen: board focused on HEX-05. Records on file: none yet (browser and desk need one)"
    );
    rt.job_done("r1", 10_000);
    rt.job_done("r2", 20_000);
    assert_eq!(
        line(&rt),
        "Screen: browser on DOC-03. Records on file, newest first: CLM-02, DOC-03, REP-01, DOC-01"
    );
    rt.job_done("r3", 30_000);
    assert_eq!(
        line(&rt),
        "Screen: browser on REP-02. Records on file, newest first: REP-02, CLM-02, DOC-03, REP-01 \
         +1 older"
    );
}

// ----- board focus and the investigation's screen defaults --------------------------------

fn focus(screen: ScreenId, record_id: Option<&str>, focus: Option<&str>) -> Op {
    Op::ShowScreen {
        screen,
        record_id: record_id.map(str::to_string),
        focus: focus.map(str::to_string),
    }
}

/// The `show_screen` ops in `evs`, as (screen, record_id, focus).
fn focused(evs: &[StreamEvent]) -> Vec<(ScreenId, Option<String>, Option<String>)> {
    evs.iter()
        .filter_map(|e| match e {
            StreamEvent::Op {
                op:
                    Op::ShowScreen {
                        screen,
                        record_id,
                        focus,
                    },
                ..
            } => Some((*screen, record_id.clone(), focus.clone())),
            _ => None,
        })
        .collect()
}

fn on(
    screen: ScreenId,
    record: Option<&str>,
    hex: Option<&str>,
) -> (ScreenId, Option<String>, Option<String>) {
    (screen, record.map(str::to_string), hex.map(str::to_string))
}

#[test]
fn a_focus_must_be_a_case_hex() {
    let mut rt = seg02a();
    let evs = rt.apply_ops(1_000, &[focus(ScreenId::Board, None, Some("HEX-99"))]);
    assert_eq!(rejections(&evs)[0].1, "unknown_hex");
    assert!(rt
        .context(0, 1)
        .state_summary
        .contains("show_screen: unknown_hex"));
    // A loose spelling is the case's id.
    let evs = rt.apply_ops(2_000, &[focus(ScreenId::Board, None, Some("hex3"))]);
    assert!(rejections(&evs).is_empty(), "{evs:?}");
    let summary = rt.context(0, 2).state_summary;
    assert!(
        summary.contains("Screen: board focused on HEX-03."),
        "{summary}"
    );
    // A focus on host or browser is ignored, like a record_id on the board.
    let evs = rt.apply_ops(3_000, &[focus(ScreenId::Host, None, Some("HEX-99"))]);
    assert!(rejections(&evs).is_empty(), "{evs:?}");
    assert!(rt.context(0, 3).state_summary.contains("Screen: host."));
    // The desk checks it too, after the record.
    rt.apply_ops(
        4_000,
        &[research("r1", "HEX-04: trace the treasury wallet")],
    );
    rt.job_done("r1", 5_000);
    let evs = rt.apply_ops(
        6_000,
        &[focus(ScreenId::Desk, Some("DOC-03"), Some("nowhere"))],
    );
    assert_eq!(rejections(&evs)[0].1, "unknown_hex");
    let evs = rt.apply_ops(
        7_000,
        &[focus(ScreenId::Desk, Some("DOC-03"), Some("HEX-01"))],
    );
    assert!(rejections(&evs).is_empty(), "{evs:?}");
    let summary = rt.context(0, 7).state_summary;
    assert!(
        summary.contains("Screen: desk on DOC-03, board focused on HEX-01."),
        "{summary}"
    );
    // On the wire an absent focus is omitted, never null.
    let line = serde_json::to_string(&focus(ScreenId::Board, None, None)).unwrap();
    assert_eq!(line, r#"{"op":"show_screen","screen":"board"}"#);
}

#[test]
fn a_dispatch_and_a_reveal_zoom_the_board_to_their_card() {
    let mut rt = seg02a();
    let evs = rt.apply_ops(
        1_000,
        &[research("r1", "HEX-04: trace the treasury wallet")],
    );
    assert_eq!(tags(&evs), ["op:dispatch_research", "op:show_screen"]);
    assert_eq!(focused(&evs), [on(ScreenId::Board, None, Some("HEX-04"))]);
    // A brief that names no single hex moves nothing.
    let evs = rt.apply_ops(2_000, &[research("r2", "dig through the old archives")]);
    assert!(focused(&evs).is_empty(), "{evs:?}");
    let evs = rt.apply_ops(3_000, &[reveal("HEX-16", EvidenceGrade::Clm)]);
    assert_eq!(focused(&evs), [on(ScreenId::Board, None, Some("HEX-16"))]);
    // The same view already up is not sent again.
    let evs = rt.apply_ops(4_000, &[reveal("HEX-16", EvidenceGrade::Spc)]);
    assert!(focused(&evs).is_empty(), "{evs:?}");
    // A refused op moves nothing.
    let evs = rt.apply_ops(5_000, &[reveal("HEX-99", EvidenceGrade::Clm)]);
    assert!(focused(&evs).is_empty(), "{evs:?}");
}

#[test]
fn a_poll_opens_on_the_overview() {
    let mut rt = seg02a();
    rt.apply_ops(
        1_000,
        &[research("r1", "HEX-07: the moon maintenance thread")],
    );
    let evs = rt.apply_ops(
        2_000,
        &[poll("next", &["HEX-01", "HEX-02"], Rank::Thrall, 30)],
    );
    assert_eq!(tags(&evs), ["op:start_poll", "op:show_screen"]);
    assert_eq!(focused(&evs), [on(ScreenId::Board, None, None)]);
    rt.apply_ops(
        40_000,
        &[Op::ClosePoll {
            poll_id: "next".into(),
        }],
    );
    // The Plebiscite is a poll too: its start_poll, then the overview.
    rt.apply_ops(41_000, &[research("r2", "HEX-07: again")]);
    let evs = rt.apply_ops(42_000, &[research("r3", "HEX-07: and again")]);
    assert_eq!(
        tags(&evs),
        ["moderation", "op:start_poll", "op:show_screen"],
        "{evs:?}"
    );
    assert_eq!(focused(&evs), [on(ScreenId::Board, None, None)]);
}

#[test]
fn the_browser_gives_way_to_the_desk_then_the_overview() {
    use shadowrunr_segments::runtime::{BROWSER_DWELL_MS, DESK_DWELL_MS};
    assert_eq!((BROWSER_DWELL_MS, DESK_DWELL_MS), (40_000, 30_000));
    let mut rt = seg02a();
    rt.apply_ops(0, &[research("r1", "HEX-04: trace the treasury wallet")]);
    let back = 1_000;
    assert_eq!(
        focused(&rt.job_done("r1", back)),
        [on(ScreenId::Browser, Some("DOC-03"), None)]
    );
    assert!(focused(&rt.tick(back + BROWSER_DWELL_MS - 1, 0)).is_empty());
    // The record stays up, and the board behind it shows the record's card.
    let evs = rt.tick(back + BROWSER_DWELL_MS, 0);
    assert_eq!(
        focused(&evs),
        [on(ScreenId::Desk, Some("DOC-03"), Some("HEX-04"))]
    );
    let desk = back + BROWSER_DWELL_MS;
    assert!(focused(&rt.tick(desk + DESK_DWELL_MS - 1, 0)).is_empty());
    let evs = rt.tick(desk + DESK_DWELL_MS, 0);
    assert_eq!(focused(&evs), [on(ScreenId::Board, None, None)]);
    // The overview stays.
    assert!(focused(&rt.tick(desk + 10 * DESK_DWELL_MS, 0)).is_empty());
}

#[test]
fn the_models_show_screen_restarts_the_dwell() {
    use shadowrunr_segments::runtime::BROWSER_DWELL_MS;
    let mut rt = seg02a();
    rt.apply_ops(0, &[research("r1", "HEX-04: trace the treasury wallet")]);
    rt.job_done("r1", 1_000);
    // He puts the same record up again 30 s in: the clock starts over from there.
    rt.apply_ops(31_000, &[screen(ScreenId::Browser, Some("DOC-03"))]);
    assert!(focused(&rt.tick(1_000 + BROWSER_DWELL_MS, 0)).is_empty());
    assert!(focused(&rt.tick(31_000 + BROWSER_DWELL_MS - 1, 0)).is_empty());
    assert_eq!(
        focused(&rt.tick(31_000 + BROWSER_DWELL_MS, 0)),
        [on(ScreenId::Desk, Some("DOC-03"), Some("HEX-04"))]
    );
}

#[test]
fn the_hold_protects_the_models_screen_from_every_default() {
    use shadowrunr_segments::runtime::SCREEN_HOLD_MS;
    let mut rt = seg02a();
    // He chooses the room, then sends a bone-clerk in the same beat: his choice holds.
    let evs = rt.apply_ops(
        10_000,
        &[
            screen(ScreenId::Host, None),
            research("r1", "HEX-04: trace the treasury wallet"),
        ],
    );
    assert_eq!(focused(&evs), [on(ScreenId::Host, None, None)]);
    let evs = rt.apply_ops(11_000, &[reveal("HEX-16", EvidenceGrade::Clm)]);
    assert!(focused(&evs).is_empty(), "{evs:?}");
    let evs = rt.apply_ops(
        12_000,
        &[poll("next", &["HEX-01", "HEX-02"], Rank::Thrall, 30)],
    );
    assert!(focused(&evs).is_empty(), "{evs:?}");
    assert_eq!(rt.screen(), ScreenId::Host);
    // A default suppressed by the hold is spent: nothing fires when the hold ends.
    assert!(focused(&rt.tick(10_000 + SCREEN_HOLD_MS, 0)).is_empty());
    assert_eq!(rt.screen(), ScreenId::Host);
    // Past the hold the next dispatch moves the board again.
    rt.apply_ops(
        50_000,
        &[Op::ClosePoll {
            poll_id: "next".into(),
        }],
    );
    let evs = rt.apply_ops(51_000, &[research("r2", "HEX-01: the syndicate's licence")]);
    assert_eq!(focused(&evs), [on(ScreenId::Board, None, Some("HEX-01"))]);
    // The model's own browser holds against the dwell's first step too.
    rt.job_done("r1", 52_000);
    let evs = rt.apply_ops(53_000, &[screen(ScreenId::Browser, Some("DOC-03"))]);
    assert!(rejections(&evs).is_empty(), "{evs:?}");
    let evs = rt.apply_ops(54_000, &[research("r3", "HEX-05: the weekly promise")]);
    assert!(focused(&evs).is_empty(), "inside the hold: {evs:?}");
    assert_eq!(rt.screen(), ScreenId::Browser);
}

// ----- the evidence board: pin_link and link_pinned ----------------------------------------

fn pin(from: &str, to: &str, rel: LinkRel, record_id: &str) -> Op {
    Op::PinLink {
        from: from.into(),
        to: to.into(),
        rel,
        record_id: record_id.into(),
    }
}

/// SEG-02A with HEX-04's records back (DOC-03, CLM-02; DOC-03 opened by the runtime) and
/// HEX-04 and HEX-09 face up, the screen back on the board's overview past the hold.
fn board() -> SegmentRuntime {
    let mut rt = seg02a();
    rt.apply_ops(0, &[research("r1", "HEX-04: trace the treasury wallet")]);
    rt.job_done("r1", 500);
    rt.apply_ops(
        1_000,
        &[
            reveal("HEX-04", EvidenceGrade::Doc),
            reveal("HEX-09", EvidenceGrade::Clm),
        ],
    );
    rt.apply_ops(1_000, &[screen(ScreenId::Board, None)]);
    rt
}

#[test]
fn a_pinned_string_cites_its_record_and_goes_out_as_link_pinned() {
    let mut rt = board();
    // CLM-02 came back with HEX-04's job but was never shown: it goes out before the op.
    let evs = rt.apply_ops(30_000, &[pin("hex4", "HEX-09", LinkRel::Paid, "CLM-02")]);
    assert_eq!(tags(&evs), ["record_shown", "op:pin_link", "link_pinned"]);
    let StreamEvent::LinkPinned {
        ts_ms,
        from,
        to,
        rel,
        record_id,
        grade,
    } = &evs[2]
    else {
        unreachable!()
    };
    // The ends are the case's ids, whatever spelling the model used; the grade is the
    // record's, so the stage draws a CLM string as alleged.
    assert_eq!(
        (
            *ts_ms,
            from.as_str(),
            to.as_str(),
            *rel,
            record_id.as_str(),
            *grade
        ),
        (
            30_000,
            "HEX-04",
            "HEX-09",
            LinkRel::Paid,
            "CLM-02",
            EvidenceGrade::Clm
        )
    );
    let line = serde_json::to_string(&evs[2]).unwrap();
    assert_eq!(
        line,
        r#"{"type":"link_pinned","ts_ms":30000,"from":"HEX-04","to":"HEX-09","rel":"paid","record_id":"CLM-02","grade":"CLM"}"#
    );
    assert_eq!(rt.links().len(), 1);
    assert_eq!(rt.links()[0].record_id, "CLM-02");
    // A record already shown is not sent again: DOC-03 went up when the job came back.
    let evs = rt.apply_ops(
        31_000,
        &[pin("HEX-04", "HEX-09", LinkRel::Supports, "DOC-03")],
    );
    assert_eq!(tags(&evs), ["op:pin_link", "link_pinned"]);
}

#[test]
fn every_pin_link_rule_has_its_refusal() {
    let mut rt = board();
    let rule = |rt: &mut SegmentRuntime, op: Op| -> String {
        let evs = rt.apply_ops(30_000, &[op]);
        assert!(
            !evs.iter().any(|e| matches!(
                e,
                StreamEvent::LinkPinned { .. } | StreamEvent::RecordShown { .. }
            )),
            "a refused pin sends nothing: {evs:?}"
        );
        rejections(&evs)
            .first()
            .map(|r| r.1.clone())
            .unwrap_or_default()
    };
    // Both ends are cards of the case, and two different ones.
    assert_eq!(
        rule(&mut rt, pin("HEX-99", "HEX-09", LinkRel::Paid, "CLM-02")),
        "unknown_hex"
    );
    assert_eq!(
        rule(&mut rt, pin("HEX-04", "hex-4", LinkRel::Paid, "CLM-02")),
        "same_hex"
    );
    // Both ends face up: HEX-10 is still face down.
    assert_eq!(
        rule(&mut rt, pin("HEX-09", "HEX-10", LinkRel::Paid, "CLM-02")),
        "hex_not_revealed"
    );
    // Only returned records: never filed, then filed with its job still out.
    assert_eq!(
        rule(&mut rt, pin("HEX-04", "HEX-09", LinkRel::Paid, "CLM-04")),
        "record_not_returned"
    );
    rt.apply_ops(30_000, &[research("r2", "HEX-09: the mixer hop")]);
    assert_eq!(
        rule(&mut rt, pin("HEX-04", "HEX-09", LinkRel::Paid, "CLM-04")),
        "record_not_returned"
    );
    assert_eq!(
        rule(&mut rt, pin("HEX-04", "HEX-09", LinkRel::Paid, "NOPE-01")),
        "record_not_returned"
    );
    // The record is filed under one of the two cards: DOC-01 is about HEX-01.
    rt.apply_ops(30_000, &[research("r3", "HEX-01: the syndicate's licence")]);
    rt.job_done("r3", 30_000);
    assert_eq!(
        rule(&mut rt, pin("HEX-04", "HEX-09", LinkRel::Paid, "DOC-01")),
        "record_not_on_link"
    );
    // The same ends and rel twice. The other way round, or another rel, is another string.
    assert!(
        rejections(&rt.apply_ops(30_000, &[pin("HEX-04", "HEX-09", LinkRel::Paid, "CLM-02")]))
            .is_empty()
    );
    assert_eq!(
        rule(&mut rt, pin("HEX-04", "HEX-09", LinkRel::Paid, "CLM-02")),
        "duplicate_link"
    );
    assert!(
        rejections(&rt.apply_ops(30_000, &[pin("HEX-09", "HEX-04", LinkRel::Paid, "CLM-02")]))
            .is_empty()
    );
    assert_eq!(rt.links().len(), 2);
    // Refusals are named in the state, like every other.
    let summary = rt.context(0, 30).state_summary;
    assert!(summary.contains("pin_link: duplicate_link"), "{summary}");
}

#[test]
fn pin_link_is_seg02a_only() {
    let mut rt = seg01();
    let evs = rt.apply_ops(1_000, &[pin("HEX-04", "HEX-09", LinkRel::Paid, "CLM-02")]);
    assert_eq!(rejections(&evs)[0].1, "op_not_allowed");
    assert!(!rt.context(0, 1).allowed_ops.iter().any(|o| o == "pin_link"));
    assert!(seg02a()
        .context(0, 1)
        .allowed_ops
        .iter()
        .any(|o| o == "pin_link"));
}

#[test]
fn a_string_pinned_out_of_view_brings_up_the_board_on_its_card() {
    let mut rt = board();
    // The runtime's own browser (a returning job) is up: the string is pinned out of view.
    rt.apply_ops(40_000, &[research("r2", "HEX-09: the mixer hop")]);
    rt.job_done("r2", 41_000);
    assert_eq!(rt.screen(), ScreenId::Browser);
    let evs = rt.apply_ops(42_000, &[pin("HEX-04", "HEX-09", LinkRel::Paid, "CLM-04")]);
    assert_eq!(
        focused(&evs),
        [on(ScreenId::Board, None, Some("HEX-09"))],
        "{evs:?}"
    );
    // On the desk the board is in view already: the record stays up.
    rt.apply_ops(43_000, &[screen(ScreenId::Desk, Some("CLM-04"))]);
    let evs = rt.apply_ops(60_000, &[pin("HEX-04", "HEX-09", LinkRel::Paid, "CLM-02")]);
    assert!(focused(&evs).is_empty(), "{evs:?}");
    assert_eq!(rt.screen(), ScreenId::Desk);
}

#[test]
fn the_board_line_counts_cards_and_strings_and_names_open_leads() {
    let summary = seg02a().context(0, 0).state_summary;
    assert!(
        summary.contains("Board: 0 cards face up, 0 strings."),
        "{summary}"
    );
    assert!(!summary.contains("Open leads"), "{summary}");
    let mut rt = board();
    let summary = rt.context(0, 1).state_summary;
    assert!(
        summary.contains(
            "Board: 2 cards face up, 0 strings. Open leads: HEX-04 (wallet) has a returned \
             record and no string."
        ),
        "{summary}"
    );
    rt.apply_ops(30_000, &[pin("HEX-04", "HEX-09", LinkRel::Paid, "CLM-02")]);
    let summary = rt.context(0, 30).state_summary;
    assert!(
        summary.contains("Board: 2 cards face up, 1 string.\n")
            || summary.ends_with("Board: 2 cards face up, 1 string."),
        "{summary}"
    );
    assert!(!summary.contains("Open leads"), "{summary}");
    // Nothing of the board goes into the stable rules prompt but the one rule on strings.
    let rules = rt.context(0, 30).rules_prompt;
    assert!(!rules.contains("Board:"), "{rules}");
    assert!(rules.contains("`pin_link`"), "{rules}");
}

#[test]
fn the_cases_acts_name_the_arc_and_stage_its_cards() {
    let mut rt = SegmentRuntime::with_config(
        SegmentId::Seg02aHexmap,
        SegmentConfig {
            planned_s: Some(10_800),
            ..cfg()
        },
    )
    .unwrap();
    let case = demo_case();
    assert_eq!(case.acts.len(), 3);
    for (k, act) in case.acts.iter().enumerate() {
        for id in &act.hexes {
            assert_eq!(rt.hex_act(id), Some(k), "{id}");
        }
    }
    let titles: Vec<String> = rt
        .start(0)
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Op {
                op: Op::ShowCard { title, .. },
                ..
            } => Some(title.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(titles, ["ACT I: The 4% promise"]);
    assert!(rt
        .context(0, 0)
        .rules_prompt
        .contains("Act II Frozen withdrawals and the money trail"));
}
