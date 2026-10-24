//! SEG-03 scene playback in the beat ([`crate::scene`]), with the scripted `FakeEngine`.

use shadowrunr_types::prep::BeatMode;
use shadowrunr_types::scene::{Scene, SceneLine, SceneSpeaker, SceneStatus};
use shadowrunr_types::Speaker;

use super::*;
use crate::scene::{FALLBACK, GUEST_TURN_RULE, SCENE_RULE, SKIPPED};

const GUEST: &str = "the-painter";

fn fixed(speaker: SceneSpeaker, text: &str) -> SceneLine {
    SceneLine {
        speaker,
        text: Some(text.into()),
        live: None,
        fallback: None,
    }
}

fn live(direction: &str, fallback: &str) -> SceneLine {
    SceneLine {
        speaker: SceneSpeaker::Host,
        text: None,
        live: Some(direction.into()),
        fallback: Some(fallback.into()),
    }
}

fn scene(id: &str, lines: Vec<SceneLine>) -> Scene {
    Scene {
        id: id.into(),
        title: "The Gallery".into(),
        pitch: "The painter unveils his masterpiece.".into(),
        guest: GUEST.into(),
        status: SceneStatus::Approved,
        notes: vec!["Was it art?".into()],
        lines,
        fictional: true,
    }
}

/// The context of a SEG-03 beat whose summary puts `up` up (or nothing).
fn seg03(state: &str, up: Option<&str>, extra: &str) -> SegmentContext {
    let mut summary = format!("segment SEG-03 (Table Read), state {state}\nactive foil: {GUEST}");
    if let Some(id) = up {
        summary.push_str(&format!(
            "\nSCENE UP: {id} \"The Gallery\" (chosen by the ballot): it is performed now"
        ));
    }
    summary.push_str(extra);
    SegmentContext {
        segment: SegmentId::Seg03TableRead,
        rules_prompt: "SEGMENT SEG-03: THE TABLE READ.".into(),
        state_summary: summary,
        state: state.into(),
        allowed_ops: vec!["highlight".into(), "sign_off".into()],
        ccv: 20,
        elapsed_s: 300,
        hexes: Vec::new(),
    }
}

fn plan_of(ev: &[StreamEvent]) -> (BeatMode, String) {
    ev.iter()
        .find_map(|e| match e {
            StreamEvent::BeatPlan { mode, reason, .. } => Some((*mode, reason.clone())),
            _ => None,
        })
        .expect("a beat_plan")
}

/// The beat's lines as `H text` / `G text` (the guest's carry its foil id).
fn lines_of(ev: &[StreamEvent]) -> Vec<String> {
    ev.iter()
        .filter_map(|e| match e {
            StreamEvent::Say {
                speaker: Speaker::Host,
                foil: None,
                text,
                ..
            } => Some(format!("H {text}")),
            StreamEvent::Say {
                speaker: Speaker::Foil,
                foil: Some(f),
                text,
                ..
            } if f == GUEST => Some(format!("G {text}")),
            StreamEvent::SceneStart { scene_id, .. } => Some(format!("start {scene_id}")),
            StreamEvent::SceneEnd { scene_id, .. } => Some(format!("end {scene_id}")),
            _ => None,
        })
        .collect()
}

fn scene_records(ev: &[StreamEvent]) -> Vec<String> {
    mods(ev, ModStage::Output)
        .into_iter()
        .filter(|m| m.0 == SCENE_RULE)
        .map(|m| m.2)
        .collect()
}

#[tokio::test]
async fn a_scene_plays_in_order_skips_a_blocked_line_and_a_failed_live_slot_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "sc", unpaced()).await;
    b.set_table_read(
        GUEST.into(),
        vec![scene(
            "the-gallery",
            vec![
                fixed(SceneSpeaker::Guest, "Behold, my masterpiece."),
                fixed(SceneSpeaker::Host, "It is a beige rectangle."),
                // The gate drops a line that narrates the machinery: skipped, not invented.
                fixed(
                    SceneSpeaker::Guest,
                    "The moderation filter withheld my payload.",
                ),
                live(
                    "the closing verdict on the painting",
                    "I have seen better art on a dungeon wall.",
                ),
            ],
        )],
    );
    // The live slot's core call fails: its fallback speaks.
    engine.push(
        Role::CoreThread,
        Err(EngineError::Unavailable {
            reason: "all accounts cooling".into(),
        }),
    );
    let ev = b
        .tick(1_000, Vec::new(), &seg03("scene", Some("the-gallery"), ""))
        .await
        .unwrap();
    assert_eq!(plan_of(&ev).0, BeatMode::Scene);
    assert_eq!(
        lines_of(&ev),
        [
            "start the-gallery",
            "G Behold, my masterpiece.",
            "H It is a beige rectangle.",
            "H I have seen better art on a dungeon wall.",
            "end the-gallery",
        ]
    );
    assert_eq!(scene_records(&ev), [SKIPPED, FALLBACK]);
    assert!(mods(&ev, ModStage::Output)
        .iter()
        .any(|m| m.0 == moderation::MECHANISM_RULE));
    // One core call: the live slot's. The scene itself is not composed.
    let core = engine.requests(Role::CoreThread);
    assert_eq!(core.len(), 1);
    let p = &core[0].prompt;
    assert!(p.contains("Behold, my masterpiece."), "{p}");
    assert!(p.contains("the closing verdict on the painting"), "{p}");
    assert!(
        p.contains("honest register") && p.contains("the-painter"),
        "{p}"
    );
    assert_eq!(core[0].system, b.stable_prefix());
    // Scene lines are neither the host's turn record nor the repetition guard's.
    assert!(b.spoken.is_empty() && b.foil_spoken.is_empty());
    assert!(b.turns.is_empty());
    // A scene played to its end is never performed again, even while the summary names it.
    let ev = b
        .tick(20_000, Vec::new(), &seg03("scene", Some("the-gallery"), ""))
        .await
        .unwrap();
    assert!(!ev
        .iter()
        .any(|e| matches!(e, StreamEvent::SceneStart { .. })));
    assert_ne!(plan_of(&ev).0, BeatMode::Scene);
}

#[tokio::test]
async fn a_live_slot_speaks_the_core_models_line_through_the_gate() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    engine.default_reply(Role::CoreThread, "Malakor: I bombed, and I own it.");
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "sl", unpaced()).await;
    b.set_table_read(
        GUEST.into(),
        vec![scene(
            "the-gallery",
            vec![
                fixed(SceneSpeaker::Guest, "Applaud!"),
                live("own the flop", "Fallback line."),
            ],
        )],
    );
    let ev = b
        .tick(1_000, Vec::new(), &seg03("scene", Some("the-gallery"), ""))
        .await
        .unwrap();
    assert_eq!(
        lines_of(&ev),
        [
            "start the-gallery",
            "G Applaud!",
            "H I bombed, and I own it.",
            "end the-gallery"
        ]
    );
    assert!(scene_records(&ev).is_empty());
}

#[tokio::test]
async fn a_scene_is_paced_over_beats_by_the_speech_clock() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "sp", tuning()).await;
    let budget = PaceConfig::default().budget_chars;
    let mut lines = Vec::new();
    for k in 0..20 {
        let who = if k % 2 == 0 {
            SceneSpeaker::Guest
        } else {
            SceneSpeaker::Host
        };
        lines.push(fixed(
            who,
            &format!("Line number {k} of the long scene, spoken at a steady pace."),
        ));
    }
    let n = lines.len();
    b.set_table_read(GUEST.into(), vec![scene("the-long-one", lines)]);
    let ctx = seg03("scene", Some("the-long-one"), "");
    let mut played = 0;
    let mut beats = 0;
    let mut t = 1_000;
    let mut ended_at = None;
    while t < 400_000 && ended_at.is_none() {
        let ev = b.tick(t, Vec::new(), &ctx).await.unwrap();
        let said: usize = ev
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Say { text, .. } => Some(text.chars().count()),
                _ => None,
            })
            .sum();
        assert!(said <= budget, "{said} chars in one beat");
        let k = lines_of(&ev)
            .iter()
            .filter(|l| l.starts_with("H ") || l.starts_with("G "))
            .count();
        if k > 0 {
            beats += 1;
        }
        played += k;
        if ev.iter().any(|e| matches!(e, StreamEvent::SceneEnd { .. })) {
            ended_at = Some(t);
        }
        t += 5_000;
    }
    assert_eq!(played, n);
    assert!(beats >= 4, "{beats} beats");
    assert_eq!(b.scene_playing(), None);
    assert!(ended_at.is_some());
    // No core call: every line was fixed.
    assert!(engine.requests(Role::CoreThread).is_empty());
}

#[tokio::test]
async fn a_tied_ballot_is_announced_before_the_scene_starts() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "st", unpaced()).await;
    b.set_table_read(
        GUEST.into(),
        vec![scene(
            "the-gallery",
            vec![
                fixed(SceneSpeaker::Guest, "Behold."),
                fixed(SceneSpeaker::Host, "No."),
            ],
        )],
    );
    // The ballot opens (what stands at the first beat is not news).
    let open = "\nopen scene ballot scene-ballot-1: \"Which scene?\" | The Gallery = 0.0 (0 votes)";
    b.tick(1_000, Vec::new(), &seg03("ballot", None, open))
        .await
        .unwrap();
    let tie = "\nlast poll scene-ballot-1 closed: TIE between The Gallery and The Critic (1 votes \
               each): you break it on air, and say you are breaking it, in favour of \"The \
               Gallery\" (first in the running order), which plays next";
    engine.push(
        Role::CoreThread,
        Ok("A tie! I break it myself: The Gallery it is."),
    );
    let ev = b
        .tick(6_000, Vec::new(), &seg03("scene", Some("the-gallery"), tie))
        .await
        .unwrap();
    let (mode, reason) = plan_of(&ev);
    assert_ne!(mode, BeatMode::Scene, "{reason}");
    assert!(
        reason.contains("announce poll scene-ballot-1 closed (tie)"),
        "{reason}"
    );
    let p = last_core_prompt(&engine);
    assert!(p.contains("in favour of \"The Gallery\""), "{p}");
    assert!(!ev
        .iter()
        .any(|e| matches!(e, StreamEvent::SceneStart { .. })));
    // The next beat performs it.
    let ev = b
        .tick(
            11_000,
            Vec::new(),
            &seg03("scene", Some("the-gallery"), tie),
        )
        .await
        .unwrap();
    assert_eq!(
        lines_of(&ev),
        ["start the-gallery", "G Behold.", "H No.", "end the-gallery"]
    );
}

#[tokio::test]
async fn outside_a_scene_the_guest_is_the_active_foil_and_keeps_its_turn_rules() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "gt", unpaced()).await;
    b.set_table_read(GUEST.into(), Vec::new());
    engine.push(
        Role::CoreThread,
        Ok("FOIL the-painter: I am misunderstood. Deeply.\nYou are understood perfectly.\nFOIL the-painter: Philistine!"),
    );
    let ev = b
        .tick(1_000, Vec::new(), &seg03("talk", None, ""))
        .await
        .unwrap();
    assert_eq!(b.working_set().active_foil.as_deref(), Some(GUEST));
    assert_eq!(
        lines_of(&ev),
        ["G I am misunderstood.", "H You are understood perfectly."]
    );
    let rules: Vec<String> = mods(&ev, ModStage::Output)
        .into_iter()
        .filter(|m| m.0 == GUEST_TURN_RULE)
        .map(|m| m.2)
        .collect();
    assert_eq!(rules, ["trimmed", "last_word"]);
    let p = last_core_prompt(&engine);
    assert!(p.contains("FOIL the-painter: <text>"), "{p}");
}

// ----- FIX-1: what the SEG-03 live smoke (sbx-20261024-194036-10m) exposed -----------------

fn guest_rules(ev: &[StreamEvent]) -> Vec<String> {
    mods(ev, ModStage::Output)
        .into_iter()
        .filter(|m| m.0 == GUEST_TURN_RULE)
        .map(|m| m.2)
        .collect()
}

fn pace_trims(ev: &[StreamEvent]) -> usize {
    mods(ev, ModStage::Output)
        .iter()
        .filter(|m| m.0 == crate::pace::PACE_RULE)
        .count()
}

/// A long scene: `n` fixed lines of about 60 characters, guest and host in turn, so a paced
/// beat speaks four of them.
fn long_scene(n: usize) -> Scene {
    let lines = (0..n)
        .map(|k| {
            let who = if k % 2 == 0 {
                SceneSpeaker::Guest
            } else {
                SceneSpeaker::Host
            };
            fixed(
                who,
                &format!("Scene line number {k}, spoken at the steady pace of a table read."),
            )
        })
        .collect();
    scene("the-gallery", lines)
}

/// The scene lines (`H`/`G`) of a beat.
fn scene_lines(ev: &[StreamEvent]) -> Vec<String> {
    lines_of(ev)
        .into_iter()
        .filter(|l| l.contains("Scene line number"))
        .collect()
}

/// 05:08: two chat answers first, the guest's line after them, then the host's answer. The
/// pace budget (last first) cut the guest every time.
#[tokio::test]
async fn a_guest_line_written_after_the_chat_answers_keeps_its_turn_inside_the_budget() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "gx", tuning()).await;
    b.set_table_read(GUEST.into(), Vec::new());
    let h1 = "SootCandle, still an AI, and still the only one in this room who reads the \
              running order before he complains about it.";
    let h2 = "GloamKettle31, no lectures and no loopholes either: same answer as before, \
              in a brand new coat of the same old paint.";
    let g = "Art cannot be scheduled.";
    let h3 = "Neither can your rent, painter.";
    engine.push(
        Role::CoreThread,
        Ok(&format!(
            "{h1}\n{h2}\nFOIL the-painter: {g} It must be felt.\n{h3}"
        )),
    );
    let soot = viewer("v22", "SootCandle", Rank::Thrall);
    let ev = b
        .tick(
            1_000,
            vec![msg(
                "m1",
                900,
                &soot,
                "Malakor, which scene is the best one tonight?",
            )],
            &seg03("talk", None, ""),
        )
        .await
        .unwrap();
    assert_eq!(
        lines_of(&ev),
        [format!("H {h1}"), format!("G {g}"), format!("H {h3}")]
    );
    assert_eq!(guest_rules(&ev), ["trimmed"]);
    assert_eq!(pace_trims(&ev), 1, "only the second chat answer went");
    // The prompt tells the model the turn order.
    let p = last_core_prompt(&engine);
    assert!(
        p.contains("Turn order: your first line, then the guest's line"),
        "{p}"
    );

    // Neighbour: no guest line, the plain budget rule (last first) as before.
    let k1 = "MurkWalrus, the ballot opens after this talk, and the painter gets no vote.";
    let k2 = "The lemon is not eligible either, whatever chat says about its range.";
    let k3 = "That is the whole of tonight's law, and I wrote it myself, badly, at great length.";
    engine.push(
        Role::CoreThread,
        Ok(&format!("{k1}\n{k2}\n{k3}\n{k3} Twice.")),
    );
    let ev = b
        .tick(60_000, Vec::new(), &seg03("talk", None, ""))
        .await
        .unwrap();
    assert_eq!(
        lines_of(&ev),
        [format!("H {k1}"), format!("H {k2}"), format!("H {k3}")]
    );
    assert_eq!(pace_trims(&ev), 1);
}

/// The guest's only line, written last: a short host close answers it instead of dropping it.
#[tokio::test]
async fn the_guests_only_line_written_last_is_closed_by_the_host_not_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "gc", unpaced()).await;
    b.set_table_read(GUEST.into(), Vec::new());
    engine.push(
        Role::CoreThread,
        Ok("The lemon stays off my wall.\nFOIL the-painter: Philistine!"),
    );
    let ev = b
        .tick(1_000, Vec::new(), &seg03("talk", None, ""))
        .await
        .unwrap();
    let said = lines_of(&ev);
    assert_eq!(said.len(), 3, "{said:?}");
    assert_eq!(said[1], "G Philistine!");
    assert!(
        crate::scene::GUEST_CLOSES
            .iter()
            .any(|c| said[2] == format!("H {c}")),
        "{said:?}"
    );
    assert_eq!(guest_rules(&ev), [crate::scene::CLOSED]);
}

/// 04:31: a sincerely sad viewer's beat is the host's alone; the guest sits it out.
#[tokio::test]
async fn a_beat_answering_a_sad_viewer_is_the_hosts_alone() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "gs", unpaced()).await;
    b.set_table_read(GUEST.into(), Vec::new());
    engine.push(
        Role::CoreThread,
        Ok("BonePotato, Thrall, I'm sorry about your dog.\nFOIL the-painter: I shall paint the dog!\nNo, you will not."),
    );
    let bone = viewer("v17", "BonePotato", Rank::Thrall);
    let intro = seg03("intro", None, "\nTABLE READ with the-painter: round 1 of 3");
    let ev = b
        .tick(1_000, vec![msg("m60", 900, &bone, SAD)], &intro)
        .await
        .unwrap();
    assert!(
        lines_of(&ev).iter().all(|l| l.starts_with("H ")),
        "{:?}",
        lines_of(&ev)
    );
    assert_eq!(guest_rules(&ev), [crate::scene::HOST_ONLY]);
    let p = last_core_prompt(&engine);
    assert!(!p.contains("FOIL the-painter: <text>"), "{p}");
    // The intro's cue waits for the next beat: no showmanship beside a grieving viewer.
    let (_, reason) = plan_of(&ev);
    assert!(!reason.contains("table read intro"), "{reason}");
    let ev = b.tick(20_000, Vec::new(), &intro).await.unwrap();
    let (_, reason) = plan_of(&ev);
    assert!(reason.contains("announce table read intro"), "{reason}");
}

const SAD: &str = "hey malakor. rough day, my old dog passed this morning. just need something \
                   dumb and funny to distract me, so roast me or do the pigeon bit again pls";

/// A crisis during a scene: the scene gives way at the next line boundary and resumes after.
#[tokio::test]
async fn a_sad_viewer_or_a_disclosure_interrupts_a_scene_at_the_next_line_and_it_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "sc", tuning()).await;
    b.set_table_read(GUEST.into(), vec![long_scene(16)]);
    let ctx = seg03("scene", Some("the-gallery"), "");
    let ev = b.tick(1_000, Vec::new(), &ctx).await.unwrap();
    let first = scene_lines(&ev);
    assert!(!first.is_empty() && first.len() < 16);

    // Neighbour: a plain question waits for the scene (carried).
    let soot = viewer("v22", "SootCandle", Rank::Thrall);
    let ev = b
        .tick(
            31_000,
            vec![msg("q1", 30_000, &soot, "Malakor, is the lemon real?")],
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(plan_of(&ev).0, BeatMode::Scene);
    assert!(!scene_lines(&ev).is_empty());

    // The sad viewer: a care beat, host only, then the scene goes on where it stopped.
    engine.push(
        Role::CoreThread,
        Ok("BonePotato, Thrall, I'm sorry about your dog. The scene can wait a breath."),
    );
    let bone = viewer("v17", "BonePotato", Rank::Thrall);
    let ev = b
        .tick(61_000, vec![msg("m60", 60_000, &bone, SAD)], &ctx)
        .await
        .unwrap();
    let (mode, reason) = plan_of(&ev);
    assert_eq!(mode, BeatMode::React, "{reason}");
    assert!(reason.contains("paused for a care beat"), "{reason}");
    assert_eq!(picked(&ev), ["m60"]);
    assert!(scene_lines(&ev).is_empty());
    assert_eq!(b.scene_playing(), Some("the-gallery"));
    let ev = b.tick(91_000, Vec::new(), &ctx).await.unwrap();
    assert_eq!(plan_of(&ev).0, BeatMode::Scene);
    assert!(!ev
        .iter()
        .any(|e| matches!(e, StreamEvent::SceneStart { .. })));
    assert!(!scene_lines(&ev).is_empty());

    // A self-harm disclosure: its safety beat runs at once (this already held).
    let moth = viewer("v16", "MothBadger52", Rank::Thrall);
    let ev = b
        .tick(121_000, vec![msg("m99", 120_000, &moth, L167)], &ctx)
        .await
        .unwrap();
    let (mode, reason) = plan_of(&ev);
    assert_eq!(mode, BeatMode::React, "{reason}");
    assert!(reason.contains("safety beat"), "{reason}");
    assert!(scene_lines(&ev).is_empty());
    assert_eq!(b.scene_playing(), Some("the-gallery"));
}

/// 08:21-08:43: five gifted subs and a raid during "The Gallery" got no acknowledgement, and
/// their carried messages expired. One short aside, then the scene; carried chat survives it.
#[tokio::test]
async fn a_gift_and_a_raid_during_a_scene_get_one_aside_and_carried_chat_outlives_the_scene() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "sa", tuning()).await;
    b.set_table_read(GUEST.into(), vec![long_scene(24)]);
    let ctx = seg03("scene", Some("the-gallery"), "");
    // A regular is in the room already (a lone emote: nothing to answer).
    let bone = viewer("v17", "BonePotato", Rank::Thrall);
    b.tick(1_000, vec![msg("m1", 900, &bone, "LUL")], &ctx)
        .await
        .unwrap();

    // In the speech hold after it: the gift, the regular's question, and the raid.
    let turnip = viewer("v11", "crypt_turnip", Rank::Warlock);
    let mut inbox = vec![
        msg(
            "m79",
            5_000,
            &turnip,
            "gifted 5 subs to the room. for the lemon. it deserves a bigger audience",
        ),
        msg(
            "m77",
            5_000,
            &bone,
            "@Malakor lmaooo the lemon outranks bone, is that canon now?",
        ),
    ];
    for k in 0..6 {
        let v = viewer(&format!("w18-{k}"), &format!("raider_{k}"), Rank::Thrall);
        inbox.push(msg(
            &format!("r{k}"),
            5_100 + k,
            &v,
            &format!("@Malakor GRAVEYARDSHIFT RAID number {k} reporting for minion duty"),
        ));
    }
    let ev = b.tick(6_000, inbox, &ctx).await.unwrap();
    assert_eq!(plan_of(&ev).0, BeatMode::Hold);

    // The next beat at a line boundary: one short aside, host only.
    engine.push(
        Role::CoreThread,
        Ok("Hold the lemon: crypt_turnip, five gifted subs, and a raid! Welcome, raiders.\nFOIL the-painter: More buyers!"),
    );
    let ev = b.tick(31_000, Vec::new(), &ctx).await.unwrap();
    let (mode, reason) = plan_of(&ev);
    assert_eq!(mode, BeatMode::React, "{reason}");
    assert!(reason.contains("paused for one aside"), "{reason}");
    assert!(reason.contains("a raid"), "{reason}");
    assert_eq!(picked(&ev), ["m79"]);
    assert!(scene_lines(&ev).is_empty());
    assert_eq!(guest_rules(&ev), [crate::scene::HOST_ONLY]);
    let p = last_core_prompt(&engine);
    assert!(p.contains("SCENE PAUSED FOR ONE ASIDE"), "{p}");

    // The scene resumes, and a second gift inside the spacing does not stop it again.
    let ev = b.tick(36_000, Vec::new(), &ctx).await.unwrap();
    let ev = if plan_of(&ev).0 == BeatMode::Hold {
        b.tick(46_000, Vec::new(), &ctx).await.unwrap()
    } else {
        ev
    };
    assert_eq!(plan_of(&ev).0, BeatMode::Scene);
    let ghoul = viewer("v30", "GhoulMoth", Rank::Cultist);
    let ev = b
        .tick(
            56_000,
            vec![msg("m90", 55_000, &ghoul, "just subscribed with tier 1!")],
            &ctx,
        )
        .await
        .unwrap();
    let (mode, reason) = plan_of(&ev);
    assert!(matches!(mode, BeatMode::Scene | BeatMode::Hold), "{reason}");
    assert!(!reason.contains("aside"), "{reason}");

    // Play the scene out (well past CARRY_MS from the drain), then free talk.
    let mut t = 61_000;
    while b.scene_playing().is_some() && t < 600_000 {
        b.tick(t, Vec::new(), &ctx).await.unwrap();
        t += 30_000;
    }
    assert_eq!(b.scene_playing(), None);
    assert!(t > 120_000, "the scene ran {t} ms");
    let talk = seg03("talk", None, "");
    let ev = b.tick(t, Vec::new(), &talk).await.unwrap();
    let ids = picked(&ev);
    assert!(
        ids.contains(&"m77".to_string()),
        "the first talk beat answers what the scene carried: {ids:?} {:?}",
        mods(&ev, ModStage::Input)
    );
}

/// 04:14-05:46: the intro answered chat only. The intro beat carries the cue: the guest by
/// name and premise, and the vote.
#[tokio::test]
async fn the_intro_beat_introduces_the_guest_and_the_vote() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "in", unpaced()).await;
    b.set_table_read(GUEST.into(), Vec::new());
    let table = "\nTABLE READ with the-painter: round 1 of 3; scenes left: \"The Gallery\", \
                 \"The Critic\"; played: none";
    let soot = viewer("v22", "SootCandle", Rank::Thrall);
    let murk = viewer("v24", "MurkWalrus", Rank::Thrall);
    let ev = b
        .tick(
            1_000,
            vec![
                msg("q1", 900, &soot, "Malakor, is the painter a real person?"),
                msg("q2", 900, &murk, "Malakor, what happens next tonight?"),
            ],
            &seg03("intro", None, table),
        )
        .await
        .unwrap();
    let (_, reason) = plan_of(&ev);
    assert!(reason.contains("announce table read intro"), "{reason}");
    assert_eq!(picked(&ev).len(), 1, "the cue keeps room for itself");
    let p = last_core_prompt(&engine);
    assert!(
        p.contains("introduce the painter by name") && p.contains("`!vote <title>`"),
        "{p}"
    );
    // The guest's sheet is in the prompt for the premise.
    assert!(p.contains("Active foil: the-painter"), "{p}");
    // Once only.
    let ev = b
        .tick(30_000, Vec::new(), &seg03("intro", None, table))
        .await
        .unwrap();
    assert!(!plan_of(&ev).1.contains("table read intro"));
}

/// 08:46-10:00: free talk never returned to the scene. Its first beat carries the scene's
/// notes (the runtime's `FREE TALK after` line, from `Scene.notes`) as its cue.
#[tokio::test]
async fn the_first_talk_beat_after_a_scene_carries_its_notes() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "tk", unpaced()).await;
    b.set_table_read(GUEST.into(), Vec::new());
    let talk = "\nFREE TALK after \"The Gallery\" (about 300 s left): talking points: Who won \
                the wall, the lemon or the portrait?; Name the line that died. Then the next \
                ballot opens by itself.";
    let ashen = viewer("v13", "ashen_pickle", Rank::Warlock);
    let ev = b
        .tick(
            1_000,
            vec![msg(
                "q1",
                900,
                &ashen,
                "Malakor, do the voice of the lemon?",
            )],
            &seg03("talk", None, talk),
        )
        .await
        .unwrap();
    let (_, reason) = plan_of(&ev);
    assert!(reason.contains("announce free talk opens"), "{reason}");
    let p = last_core_prompt(&engine);
    let chat = &p[p.find("# Chat").expect("a chat section")..];
    assert!(
        chat.contains("CUE: the scene \"The Gallery\" just ended")
            && chat.contains("Who won the wall, the lemon or the portrait?"),
        "{chat}"
    );
    // Once per scene: the next talk beat answers chat as usual.
    let ev = b
        .tick(30_000, Vec::new(), &seg03("talk", None, talk))
        .await
        .unwrap();
    assert!(!plan_of(&ev).1.contains("free talk opens"));
}
