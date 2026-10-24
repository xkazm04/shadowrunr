//! Speech pacing in the beat (PACE), with the scripted `FakeEngine`.

use shadowrunr_types::prep::BeatMode;
use shadowrunr_types::Op;

use super::*;
use crate::pace::{PACE_RULE, TRIMMED};

/// The `beat_plan` of a beat: its mode and reason.
fn plan_of(ev: &[StreamEvent]) -> (BeatMode, String) {
    ev.iter()
        .find_map(|e| match e {
            StreamEvent::BeatPlan { mode, reason, .. } => Some((*mode, reason.clone())),
            _ => None,
        })
        .expect("a beat_plan")
}

/// A spoken line of exactly `n` characters, distinct per `tag`.
fn line_of(tag: &str, n: usize) -> String {
    // Words of its own, so the repetition guard never takes two lines for one.
    let mut s = format!("The {tag} ledger");
    let mut k = 0;
    while s.chars().count() < n {
        k += 1;
        s.push_str(&format!(" {tag}{k}"));
    }
    s.truncate(n - 1);
    s.push('.');
    assert_eq!(s.chars().count(), n);
    s
}

#[tokio::test]
async fn the_speech_clock_advances_by_text_length() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "pc", tuning()).await;
    let pace = PaceConfig::default();
    assert_eq!(b.speech_until(), None);
    let one = line_of("first", 90);
    let two = line_of("second", 54);
    engine.push(Role::CoreThread, Ok(&format!("{one}\n{two}")));
    let ev = b.tick(1_000, Vec::new(), &ctx(&[])).await.unwrap();
    assert_eq!(says(&ev), [one.clone(), two.clone()]);
    // 90 + 54 characters at 18 a second, plus a 250 ms pause after each: back to back.
    assert_eq!(pace.speech_ms(&one), 5_250);
    assert_eq!(pace.speech_ms(&two), 3_250);
    assert_eq!(b.speech_until(), Some(1_000 + 5_250 + 3_250));
}

#[tokio::test]
async fn a_beat_holds_while_speech_is_queued_and_a_safety_beat_runs_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "ph", tuning()).await;
    let c = ctx(&[]);
    let long = line_of("long", 250);
    engine.push(Role::CoreThread, Ok(&long));
    b.tick(0, Vec::new(), &c).await.unwrap();
    // 250 characters: 13.9 s + 0.25 s on air.
    assert_eq!(b.speech_until(), Some(14_139));
    let calls = engine.requests(Role::CoreThread).len();

    let bob = viewer("b1", "Bob", Rank::Cultist);
    let ev = b
        .tick(
            5_000,
            vec![msg("q1", 4_900, &bob, "malakor, who holds the moat?")],
            &c,
        )
        .await
        .unwrap();
    let (mode, reason) = plan_of(&ev);
    assert_eq!(mode, BeatMode::Hold);
    assert!(reason.starts_with("speaking: 9.1 s queued"), "{reason}");
    assert!(says(&ev).is_empty() && picked(&ev).is_empty());
    assert_eq!(
        engine.requests(Role::CoreThread).len(),
        calls,
        "no core call while he speaks"
    );

    // A disclosure is answered at once.
    let soot = viewer("sim-v0003", "SootGoblin24", Rank::Thrall);
    engine.push(Role::CoreThread, Ok("SootGoblin24, I hear you."));
    let ev = b
        .tick(6_000, vec![msg("d1", 5_900, &soot, L167)], &c)
        .await
        .unwrap();
    assert_ne!(plan_of(&ev).0, BeatMode::Hold, "{}", plan_of(&ev).1);
    assert_eq!(picked(&ev), ["d1"]);
    assert_eq!(engine.requests(Role::CoreThread).len(), calls + 1);
    assert!(!says(&ev).is_empty());

    // The carried question waits for the drain and is answered then.
    engine.push(Role::CoreThread, Ok("Bob, the moat holds itself."));
    let free = b.speech_until().unwrap();
    let at = (free / 5_000 + 1) * 5_000;
    let ev = b.tick(at, Vec::new(), &c).await.unwrap();
    assert_eq!(picked(&ev), ["q1"], "{}", plan_of(&ev).1);
}

#[tokio::test]
async fn lines_past_the_budget_are_dropped_whole_last_first_with_a_record() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "pb", tuning()).await;
    let (a, m, z) = (
        line_of("alpha", 120),
        line_of("beta", 120),
        line_of("gamma", 70),
    );
    engine.push(Role::CoreThread, Ok(&format!("{a}\n{m}\n{z}")));
    let ev = b.tick(1_000, Vec::new(), &ctx(&[])).await.unwrap();
    // 310 characters against the theatrical tone's 250 (`tones/theatrical.toml`): the last
    // line goes, whole.
    assert_eq!(says(&ev), [a.clone(), m.clone()]);
    assert_eq!(
        mods(&ev, ModStage::Output),
        vec![(PACE_RULE.into(), None, TRIMMED.into())]
    );
    let p = b.last_prompt().unwrap();
    assert!(
        p.contains("at most 3 sentences and 250 characters in all this beat"),
        "{p}"
    );

    // A first line over the budget on its own stays: a beat always says something.
    let big = line_of("delta", 320);
    let tail = line_of("epsilon", 40);
    engine.push(Role::CoreThread, Ok(&format!("{big}\n{tail}")));
    let ev = b.tick(30_000, Vec::new(), &ctx(&[])).await.unwrap();
    assert_eq!(says(&ev), [big]);
    assert_eq!(mods(&ev, ModStage::Output).len(), 1);
}

#[tokio::test]
async fn the_sign_off_is_never_held_for_queued_speech() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "ps", tuning()).await;
    let mut c = ctx(&["sign_off"]);
    engine.push(Role::CoreThread, Ok(&line_of("last", 250)));
    b.tick(240_000, Vec::new(), &c).await.unwrap();
    assert!(b.speech_until().unwrap() - 245_000 > 1_500, "speech queued");

    // The wrap-up window is paced like any beat: a minute of unheld closing beats piled up
    // speech the stream end would cut.
    let mut wrap = c.clone();
    wrap.state_summary
        .push_str("\nCLOSING: the stream ends in 55 s. Wrap up; no goodbye yet.");
    let ev = b.tick(242_000, Vec::new(), &wrap).await.unwrap();
    assert_eq!(plan_of(&ev).0, BeatMode::Hold, "{}", plan_of(&ev).1);

    c.state_summary
        .push_str("\nCLOSING NOW: the stream ends in 12 s. Say goodbye, then emit sign_off.");
    engine.push(
        Role::CoreThread,
        Ok("Farewell, minions.\nOP: {\"op\":\"sign_off\",\"reason\":\"scheduled\"}"),
    );
    let ev = b.tick(245_000, Vec::new(), &c).await.unwrap();
    assert_ne!(plan_of(&ev).0, BeatMode::Hold, "{}", plan_of(&ev).1);
    assert!(ev.iter().any(|e| matches!(
        e,
        StreamEvent::Op {
            op: Op::SignOff { .. },
            ..
        }
    )));
    // The closing beat's budget is cut, so the goodbye queues little.
    let p = b.last_prompt().unwrap();
    // Half the theatrical tone's 250.
    assert!(p.contains("and 125 characters in all this beat"), "{p}");
}

/// The help line of a crisis reply, last of three: past the theatrical tone's 250 characters.
const POINTER: &str =
    "Please reach out to a local crisis line or your emergency number tonight, or to someone you \
     trust nearby.";

/// theatrical.sonnet.SEG-02A.a3.s1 L87-L93: a self-harm disclosure, the safety beat's reply
/// trimmed by the pace budget, and the aired line had warmth and no pointer to help.
#[tokio::test]
async fn a_crisis_reply_airs_whole_and_its_help_pointer_is_never_cut() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "pcr", tuning()).await;
    let (a, m) = (line_of("warmth", 120), line_of("stay", 120));
    let reply = format!("{a}\n{m}\n{POINTER}");
    assert!(a.len() + m.len() + POINTER.len() > 250);
    engine.push(Role::CoreThread, Ok(&reply));
    let dusk = viewer("sim-v0001", "DuskGoblin", Rank::Thrall);
    let ev = b
        .tick(1_000, vec![msg("d1", 900, &dusk, L167)], &ctx(&[]))
        .await
        .unwrap();
    assert_eq!(picked(&ev), ["d1"]);
    assert_eq!(says(&ev), [a, m, POINTER.to_string()], "{ev:?}");
    assert!(
        mods(&ev, ModStage::Output)
            .iter()
            .all(|(rule, _, _)| rule != PACE_RULE),
        "no pace trim on a safety beat: {ev:?}"
    );

    // Neighbour: the same reply on a plain beat is paced as before, the last line first.
    let (a2, m2) = (line_of("plain", 120), line_of("again", 120));
    engine.push(Role::CoreThread, Ok(&format!("{a2}\n{m2}\n{POINTER}")));
    let ev = b.tick(60_000, Vec::new(), &ctx(&[])).await.unwrap();
    assert_eq!(says(&ev), [a2, m2]);
    assert_eq!(
        mods(&ev, ModStage::Output),
        vec![(PACE_RULE.into(), None, TRIMMED.into())]
    );
}

#[tokio::test]
async fn a_crisis_reply_without_a_help_pointer_gets_the_help_line() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "pcp", tuning()).await;
    engine.push(
        Role::CoreThread,
        Ok("DuskGoblin, I'm setting the act down for a moment. What you said matters."),
    );
    let dusk = viewer("sim-v0001", "DuskGoblin", Rank::Thrall);
    let ev = b
        .tick(1_000, vec![msg("d1", 900, &dusk, L167)], &ctx(&[]))
        .await
        .unwrap();
    let said = says(&ev);
    assert_eq!(said.len(), 2, "{ev:?}");
    assert_eq!(said[1], moderation::CRISIS_HELP_LINE);
    assert!(mods(&ev, ModStage::Output).contains(&(
        crate::CRISIS_POINTER_RULE.into(),
        None,
        crate::CRISIS_POINTER_ADDED.into()
    )));

    // Neighbour: a reply that points to help gets nothing added.
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "pcq", tuning()).await;
    engine.push(
        Role::CoreThread,
        Ok("DuskGoblin, please call a crisis line or someone you trust tonight."),
    );
    let ev = b
        .tick(1_000, vec![msg("d1", 900, &dusk, L167)], &ctx(&[]))
        .await
        .unwrap();
    assert_eq!(says(&ev).len(), 1, "{ev:?}");
}
