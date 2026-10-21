//! The op menu (`context().allowed_ops`) is a view of the op gate (`apply_ops`): a tag is on
//! the menu exactly when the gate would take it now, and a tag withheld from the menu is named
//! in the state summary's `not now:` line with the reason the gate refuses it.

use std::collections::BTreeMap;

use shadowrunr_segments::table::{op_tag, ColdOpenRule};
use shadowrunr_segments::{SegmentConfig, SegmentRuntime};
use shadowrunr_types::character::{CharacterManifest, Persona};
use shadowrunr_types::{
    ChatMsg, EvidenceGrade, LinkRel, Op, Platform, Rank, ScreenId, SegmentId, StreamEvent, Viewer,
};

const FOILS: [&str; 2] = ["the-disruptor", "the-life-coach"];

/// The refusals that hold for every argument of their op: the gate's tag-level rules. An op
/// on the menu is never refused for one of these (`no_such_poll` and `screen_not_allowed`
/// are argument checks too, so they count only for the op whose canonical form cannot fail
/// them by its argument).
const TAG_LEVEL: [&str; 8] = [
    "signed_off",
    "op_not_allowed",
    "not_in_state",
    "poll_already_open",
    "no_active_foil",
    "plebiscite_pending",
    "arc_in_progress",
    "dossier_not_sealed",
];

fn malakor() -> Persona {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../characters/malakor/character.toml");
    let text = std::fs::read_to_string(path).unwrap();
    let m: CharacterManifest = toml::from_str(&text).unwrap();
    Persona::from_manifest(&m)
}

fn cfg() -> SegmentConfig {
    SegmentConfig {
        foils: FOILS.iter().map(|s| s.to_string()).collect(),
        cold_open: Some(ColdOpenRule {
            to: "opening".into(),
            min_s: 0,
            max_s: 0,
            active_chatters: 1,
            window_s: 60,
            sustain_s: 0,
        }),
        persona: malakor(),
        ..Default::default()
    }
}

fn start(segment: SegmentId, c: SegmentConfig) -> SegmentRuntime {
    let mut rt = SegmentRuntime::with_config(segment, c).unwrap();
    rt.start(0);
    rt
}

fn seg01() -> SegmentRuntime {
    start(SegmentId::Seg01ImprovisedRoom, cfg())
}

fn msg(id: &str, ts: i64, who: &str, rank: Rank, text: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        ts_ms: ts,
        viewer: Viewer {
            id: who.into(),
            name: who.into(),
            platform: Platform::Sim,
            rank,
            sub_tier: None,
            months: 0,
            gifted: 0,
            is_mod: false,
        },
        text: text.into(),
        command: None,
    }
}

fn poll(id: &str) -> Op {
    Op::StartPoll {
        poll_id: id.into(),
        question: "Which game?".into(),
        options: vec!["a".into(), "b".into()],
        min_rank: Rank::Thrall,
        secs: 30,
    }
}

/// The rule of the refusal of `tag` in `evs`, if it was refused.
fn refusal(evs: &[StreamEvent], tag: &str) -> Option<String> {
    let action = format!("rejected:{tag}");
    evs.iter().find_map(|e| match e {
        StreamEvent::Moderation {
            rule, action: a, ..
        } if *a == action => Some(rule.clone()),
        _ => None,
    })
}

/// The `not now:` line of a state summary, as tag -> reason.
fn not_now(summary: &str) -> Option<BTreeMap<String, String>> {
    let line = summary.lines().find_map(|l| l.strip_prefix("not now: "))?;
    Some(
        line.split(", ")
            .map(|item| {
                let (tag, reason) = item
                    .strip_suffix(')')
                    .and_then(|s| s.split_once(" ("))
                    .unwrap_or_else(|| panic!("malformed not-now item {item:?} in {line:?}"));
                (tag.to_string(), reason.to_string())
            })
            .collect(),
    )
}

/// The canonical op of each tag: arguments chosen so that only the gate's tag-level rules
/// (and the runtime's live state) decide.
fn canonical(rt: &SegmentRuntime, tag: &str) -> Op {
    match tag {
        "highlight" => Op::Highlight {
            msg_id: "m0-0".into(),
        },
        "start_poll" => poll("probe"),
        "close_poll" => Op::ClosePoll {
            poll_id: rt.open_poll_id().unwrap_or("probe").into(),
        },
        "summon_foil" => Op::SummonFoil {
            foil: FOILS
                .iter()
                .find(|f| rt.active_foil() != Some(**f))
                .unwrap()
                .to_string(),
        },
        "dismiss_foil" => Op::DismissFoil,
        "reveal_hex" => Op::RevealHex {
            hex: rt
                .case()
                .hexes
                .first()
                .map_or("HEX-01".into(), |h| h.id.clone()),
            grade: EvidenceGrade::Clm,
        },
        "dispatch_research" => Op::DispatchResearch {
            job_id: "probe-job".into(),
            brief: "map the shell chain".into(),
        },
        "show_card" => Op::ShowCard {
            card_id: "probe".into(),
            title: "Probe".into(),
            body: String::new(),
        },
        "mark_moment" => Op::MarkMoment {
            label: "probe".into(),
        },
        "switch_segment" => Op::SwitchSegment {
            to: if rt.segment() == SegmentId::Seg01ImprovisedRoom {
                SegmentId::Seg02aHexmap
            } else {
                SegmentId::Seg01ImprovisedRoom
            },
        },
        "sign_off" => Op::SignOff {
            reason: "probe".into(),
        },
        "show_screen" => Op::ShowScreen {
            screen: ScreenId::Host,
            record_id: None,
            focus: None,
        },
        "pin_link" => Op::PinLink {
            from: "HEX-04".into(),
            to: "HEX-09".into(),
            rel: LinkRel::Paid,
            record_id: "CLM-02".into(),
        },
        other => panic!("no canonical op for {other}"),
    }
}

/// At step `k` of a scripted run (rebuilt by `run_to`, at time `now`): the menu and the
/// `not now:` line partition the table's ops, and each tag's canonical op fares at the gate
/// as the menu says.
fn assert_menu_is_the_gate(k: i64, now: i64, run_to: &dyn Fn(i64) -> SegmentRuntime) {
    let rt = run_to(k);
    let ctx = rt.context(10, 0);
    let table: Vec<String> = rt.table().allowed_ops.clone();
    let where_ = format!("step {k}, state {}", rt.state());
    if rt.is_signed_off() {
        assert!(ctx.allowed_ops.is_empty(), "{where_}");
        assert_eq!(not_now(&ctx.state_summary), None, "{where_}");
        return;
    }
    let withheld = not_now(&ctx.state_summary).unwrap_or_default();
    // The menu keeps the table's order.
    let kept: Vec<String> = table
        .iter()
        .filter(|t| ctx.allowed_ops.contains(t))
        .cloned()
        .collect();
    assert_eq!(kept, ctx.allowed_ops, "{where_}");
    for tag in &table {
        let on_menu = ctx.allowed_ops.contains(tag);
        assert_ne!(
            on_menu,
            withheld.contains_key(tag),
            "{where_}: {tag} must be on the menu or named not now, exactly one\n{}",
            ctx.state_summary
        );
        let mut probe = run_to(k);
        let op = canonical(&probe, tag);
        assert_eq!(op_tag(&op), tag);
        let got = refusal(&probe.apply_ops(now, &[op]), tag);
        match withheld.get(tag) {
            Some(reason) => assert_eq!(
                got.as_deref(),
                Some(reason.as_str()),
                "{where_}: {tag} is named not now ({reason}), the gate said {got:?}"
            ),
            None => {
                let tag_level = |r: &str| {
                    TAG_LEVEL.contains(&r)
                        || (tag == "close_poll" && r == "no_such_poll")
                        || (tag == "pin_link" && r == "screen_not_allowed")
                };
                assert!(
                    !got.as_deref().is_some_and(tag_level),
                    "{where_}: {tag} is on the menu, the gate refused it {got:?}"
                );
            }
        }
    }
}

/// The scripted SEG-01 run: a quiet opening with a crowd poll, then foil votes from
/// `!summon`, a second poll, a dismissal and the handover.
fn seg01_run_to(k: i64) -> SegmentRuntime {
    let mut rt = seg01();
    for step in 0..=k {
        let now = step * 15_000;
        if step == 1 {
            rt.apply_ops(now, &[poll("g0")]);
        }
        let quiet = step < 4;
        let chat: Vec<ChatMsg> = (0..if quiet { 0 } else { 5 })
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
        rt.ingest_chat(now, &chat);
        if step == 10 {
            rt.apply_ops(now, &[poll("g1")]);
        }
        if step == 14 {
            rt.apply_ops(now, &[Op::DismissFoil]);
        }
        rt.tick(now, 4 + step as u32);
    }
    rt
}

/// The scripted research run: searches up to the cap, the Plebiscite, its vote.
fn research_run_to(k: i64) -> SegmentRuntime {
    let mut rt = start(SegmentId::Seg02aHexmap, cfg());
    let briefs = [
        "HEX-07: the moon maintenance thread",
        "Trace the hops out of treasury wallet demo-0001",
        "HEX-08: Mirecrown",
        "hex 7 again",
        "HEX-07 a third time",
        "map the shell chain",
    ];
    for step in 0..=k {
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
        rt.ingest_chat(now, &chat);
        if let Some(b) = briefs.get(step as usize) {
            rt.apply_ops(
                now,
                &[Op::DispatchResearch {
                    job_id: format!("r{step}"),
                    brief: b.to_string(),
                }],
            );
        }
        if step == 20 {
            rt.apply_ops(now, &[poll("g1")]);
        }
        rt.tick(now, 12);
    }
    rt
}

#[test]
fn the_menu_is_the_gate_through_the_scripted_seg01_run() {
    let mut states = Vec::new();
    for k in 0..40 {
        assert_menu_is_the_gate(k, k * 15_000, &seg01_run_to);
        let rt = seg01_run_to(k);
        let s = format!("{:?}/{}", rt.segment(), rt.state());
        if !states.contains(&s) {
            states.push(s);
        }
    }
    // The run walks the states this property is about.
    for s in ["opening", "crowd_game", "foil_vote", "handover_pending"] {
        assert!(
            states.iter().any(|x| x.ends_with(s)),
            "{s} not in {states:?}"
        );
    }
}

#[test]
fn the_menu_is_the_gate_through_the_scripted_research_run() {
    let mut pleb = false;
    for k in 0..30 {
        assert_menu_is_the_gate(k, k * 10_000, &research_run_to);
        pleb |= research_run_to(k).plebiscite_hex().is_some();
    }
    assert!(pleb, "the run reaches a Plebiscite");
}

#[test]
fn opening_withholds_close_poll_and_dismiss_foil_with_reasons() {
    let rt = seg01();
    assert_eq!(rt.state(), "opening");
    let ctx = rt.context(7, 42);
    for tag in ["close_poll", "dismiss_foil"] {
        assert!(!ctx.allowed_ops.iter().any(|o| o == tag), "{tag}");
    }
    let nn = not_now(&ctx.state_summary).expect("a not-now line");
    assert_eq!(nn["close_poll"], "no_such_poll");
    assert_eq!(nn["dismiss_foil"], "no_active_foil");
    assert!(
        ctx.state_summary
            .contains("close_poll (no_such_poll), dismiss_foil (no_active_foil)"),
        "{}",
        ctx.state_summary
    );
}

#[test]
fn an_open_crowd_poll_withholds_start_poll_and_keeps_close_and_summon() {
    let mut rt = seg01();
    rt.apply_ops(1_000, &[poll("p1")]);
    assert_eq!(rt.state(), "crowd_game");
    let ctx = rt.context(7, 1);
    assert!(!ctx.allowed_ops.iter().any(|o| o == "start_poll"));
    assert!(ctx.allowed_ops.iter().any(|o| o == "close_poll"));
    assert!(ctx.allowed_ops.iter().any(|o| o == "summon_foil"));
    assert_eq!(
        not_now(&ctx.state_summary).unwrap()["start_poll"],
        "poll_already_open"
    );
}

#[test]
fn a_foil_vote_withholds_summon_foil_and_start_poll() {
    let mut rt = seg01();
    rt.ingest_chat(
        1_000,
        &[msg(
            "m1",
            1_000,
            "v1",
            Rank::Thrall,
            "!summon the-disruptor",
        )],
    );
    assert_eq!(rt.state(), "foil_vote");
    let ctx = rt.context(7, 1);
    assert!(!ctx.allowed_ops.iter().any(|o| o == "summon_foil"));
    assert!(!ctx.allowed_ops.iter().any(|o| o == "start_poll"));
    let nn = not_now(&ctx.state_summary).unwrap();
    assert_eq!(nn["summon_foil"], "not_in_state");
    assert_eq!(nn["start_poll"], "poll_already_open");
}

#[test]
fn a_pending_handover_withholds_poll_and_summon_and_offers_the_switch() {
    let mut rt = seg01();
    // The elapsed ceiling announces the handover.
    rt.tick(1_500_000, 0);
    assert!(rt.is_handover_pending());
    assert_eq!(rt.state(), "handover_pending");
    let ctx = rt.context(7, 1_500);
    assert!(!ctx.allowed_ops.iter().any(|o| o == "start_poll"));
    assert!(!ctx.allowed_ops.iter().any(|o| o == "summon_foil"));
    assert!(ctx.allowed_ops.iter().any(|o| o == "switch_segment"));
    let nn = not_now(&ctx.state_summary).unwrap();
    assert_eq!(nn["start_poll"], "not_in_state");
    assert_eq!(nn["summon_foil"], "not_in_state");
}

#[test]
fn a_wrong_state_summon_of_an_unknown_foil_says_not_in_state_first() {
    let mut rt = seg01();
    rt.tick(1_500_000, 0);
    assert_eq!(rt.state(), "handover_pending");
    let evs = rt.apply_ops(
        1_500_000,
        &[Op::SummonFoil {
            foil: "the-nobody".into(),
        }],
    );
    assert_eq!(
        refusal(&evs, "summon_foil").as_deref(),
        Some("not_in_state")
    );
}

#[test]
fn a_paced_arc_keeps_reveal_hex_and_withholds_seal_switch_and_research_as_before() {
    // GUARD: the menu's existing withholdings and reveal_hex under pacing are unchanged.
    let c = SegmentConfig {
        planned_s: Some(10_800),
        ..cfg()
    };
    let mut rt = start(SegmentId::Seg02aHexmap, c);
    let first = rt
        .case()
        .hexes
        .iter()
        .find(|h| rt.hex_act(&h.id) == Some(0))
        .unwrap()
        .id
        .clone();
    rt.apply_ops(
        1_000,
        &[Op::DispatchResearch {
            job_id: "w0".into(),
            brief: format!("{first}: the filing"),
        }],
    );
    rt.apply_ops(
        2_000,
        &[Op::RevealHex {
            hex: first,
            grade: EvidenceGrade::Clm,
        }],
    );
    assert!(rt.next_reveal_ms().is_some_and(|t| t > 3_000));
    let ctx = rt.context(30, 3);
    assert!(ctx.allowed_ops.iter().any(|o| o == "reveal_hex"));
    assert!(!ctx.allowed_ops.iter().any(|o| o == "sign_off"));
    assert!(!ctx.allowed_ops.iter().any(|o| o == "switch_segment"));
    assert!(ctx.allowed_ops.iter().any(|o| o == "dispatch_research"));
}

#[test]
fn a_signed_off_show_offers_nothing_and_names_nothing_not_now() {
    let mut rt = seg01();
    rt.apply_ops(
        1_000,
        &[Op::SignOff {
            reason: "goodnight".into(),
        }],
    );
    assert!(rt.is_signed_off());
    let ctx = rt.context(7, 1);
    assert!(ctx.allowed_ops.is_empty());
    assert_eq!(not_now(&ctx.state_summary), None, "{}", ctx.state_summary);
    assert!(!ctx.state_summary.contains("not now"));
}
