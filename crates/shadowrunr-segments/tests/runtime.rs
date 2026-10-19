//! SegmentRuntime behaviour: handover, polls, foils, ranks, the DOC rule, descoped segments.

use shadowrunr_segments::{
    demo_case, requires_alleged, HandoverRule, SegmentConfig, SegmentError, SegmentRuntime,
};
use shadowrunr_types::{
    ChatMsg, Command, EvidenceGrade, Hex, HexKind, ModStage, Op, Platform, Rank, SegmentId,
    StreamEvent, Viewer,
};

const FOILS: [&str; 2] = ["the-disruptor", "the-life-coach"];

fn cfg() -> SegmentConfig {
    SegmentConfig {
        foils: FOILS.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

fn seg01() -> SegmentRuntime {
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, cfg()).unwrap();
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

fn accepted_ops(evs: &[StreamEvent]) -> usize {
    evs.iter()
        .filter(|e| matches!(e, StreamEvent::Op { .. }))
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
            StreamEvent::CaseOpen { .. }
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
    assert_eq!(accepted_ops(&evs), 1);
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

    // Poll ids are single-use.
    let evs = rt.apply_ops(40_000, &[poll("p1", &["a", "b"], Rank::Thrall, 30)]);
    assert_eq!(
        rejections(&evs),
        [(ModStage::Output, "duplicate_poll_id".to_string())]
    );
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

#[test]
fn commands_outside_the_segment_are_ignored() {
    let mut rt = seg01();
    let evs = rt.ingest_chat(
        0,
        &[
            msg("m1", 0, "a", Rank::Thrall, "!grade HEX-01 DOC"),
            msg("m2", 0, "a", Rank::Thrall, "!hint HEX-01 look"),
        ],
    );
    assert!(evs.is_empty());
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
    }, StreamEvent::CaseOpen { ts_ms: 1_000, case }] = evs.as_slice()
    else {
        panic!("expected segment_start then case_open, got {evs:?}");
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
        [StreamEvent::SegmentStart {
            segment: SegmentId::Seg01ImprovisedRoom,
            ..
        }]
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
    });
    let mut c = cfg();
    c.case = Some(case);
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
    assert_eq!(summary.matches("DOC-03").count(), 1, "{summary}");
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
    assert_eq!(ctx.allowed_ops, rt.table().allowed_ops);
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
