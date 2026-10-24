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
