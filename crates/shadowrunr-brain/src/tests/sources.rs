//! Beat sources (C1): every beat names the duty that drove it; a package may give a duty its
//! own effect (`effects.toml` version 2), and a package without one composes the prompts it
//! did before; calls to action and purpose pushes stop repeating. Scripted `FakeEngine`.

use shadowrunr_types::prep::BeatSource;

use super::*;
use crate::character::Effect;
use crate::orchestrate::case::CASE_JOKE;
use crate::orchestrate::{CTA_COOLDOWN_MS, PURPOSE_NOTE};

async fn brain_pkg(
    dir: &Path,
    engine: Arc<dyn Engine>,
    stream: &str,
    pkg: CharacterPackage,
    t: BrainTuning,
) -> Brain {
    let cfg = BrainConfig {
        character: pkg,
        memory_dir: dir.to_path_buf(),
        stream_id: stream.into(),
        core_tier: shadowrunr_types::ModelTier::Haiku,
        pool_size: 2,
    };
    Brain::new_with(cfg, engine, t).await.unwrap()
}

/// Malakor's package with its source effects removed: a package with none.
fn no_source_pkg() -> CharacterPackage {
    let mut pkg = load_character(&malakor_dir(), "theatrical").unwrap();
    pkg.effects.retain(|e| e.source.is_none());
    pkg
}

fn source_effect(source: &str, text: &str) -> Effect {
    Effect {
        source: Some(source.into()),
        text: text.into(),
        ..Default::default()
    }
}

/// The beat's `beat_plan` source.
fn source_of(ev: &[StreamEvent]) -> Option<BeatSource> {
    ev.iter().find_map(|e| match e {
        StreamEvent::BeatPlan { source, .. } => {
            Some(source.expect("every beat_plan names a source"))
        }
        _ => None,
    })
}

/// The voice section of a prompt, if it has one.
fn voice(p: &str) -> Option<String> {
    let at = p.find(&format!("# {}", compose::VOICE_HEADING))?;
    let v = &p[at..];
    Some(v[..v.find("\n# ").unwrap_or(v.len())].to_string())
}

fn dispatch_ctx() -> SegmentContext {
    let mut c = ctx(&["dispatch_research"]);
    c.segment = SegmentId::Seg02aHexmap;
    c.state = "investigating".into();
    c.state_summary = "No poll open.".into();
    let mut h = hex("HEX-02", "Ashpetal Holdings Ltd", None);
    h.fog = true;
    c.hexes = vec![hex("HEX-01", "Glimmerquay Yield Syndicate", None), h];
    c
}

/// A SEG-01 state whose purpose names no chat command.
fn purpose_ctx(state: &str, purpose: &str) -> SegmentContext {
    let mut c = ctx(&[]);
    c.state = state.into();
    c.state_summary = format!("segment SEG-01, state {state}\n{purpose}\nNo poll open.");
    c
}

fn unpaced_quiet() -> BrainTuning {
    unpaced()
}

#[tokio::test]
async fn every_beat_plan_names_its_source() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    engine.default_reply(Role::CoreThread, "The crypt keeps its own counsel tonight.");
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "src", unpaced()).await;
    let bob = viewer("b1", "Bob", Rank::Cultist);
    let mut cold = ctx(&[]);
    cold.state = "cold_open".into();
    let ev = b.tick(1_000, Vec::new(), &cold).await.unwrap();
    assert_eq!(
        source_of(&ev),
        Some(BeatSource::Welcome),
        "a gathering open"
    );
    let ev = b
        .tick(
            6_000,
            vec![msg("q1", 5_900, &bob, "malakor, who holds the moat?")],
            &ctx(&[]),
        )
        .await
        .unwrap();
    assert!(source_of(&ev).is_some());
    let ev = b.tick(11_000, Vec::new(), &dispatch_ctx()).await.unwrap();
    assert_eq!(source_of(&ev), Some(BeatSource::CaseMove));
    let ev = b
        .tick(
            16_000,
            Vec::new(),
            &purpose_ctx("dealing", "Deal the next premise."),
        )
        .await
        .unwrap();
    assert_eq!(source_of(&ev), Some(BeatSource::Purpose));
    let soot = viewer("sim-v0003", "SootGoblin24", Rank::Thrall);
    let ev = b
        .tick(21_000, vec![msg("d1", 20_900, &soot, L167)], &ctx(&[]))
        .await
        .unwrap();
    assert_eq!(source_of(&ev), Some(BeatSource::Care));
}

/// A case-move beat says the package's `case_move` effect in the voice section and the
/// directive drops the engine's comic contract; a package without one keeps the engine's.
#[tokio::test]
async fn a_case_beat_takes_the_package_case_effect_or_the_engine_default() {
    let malakor = load_character(&malakor_dir(), "theatrical").unwrap();
    let case_effect = malakor
        .effects
        .iter()
        .find(|e| e.source.as_deref() == Some("case_move"))
        .expect("Malakor's package names a case_move effect")
        .text
        .clone();
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_pkg(
        dir.path(),
        Arc::new(engine.clone()),
        "pk",
        malakor,
        tuning(),
    )
    .await;
    b.tick(1_000, Vec::new(), &dispatch_ctx()).await.unwrap();
    let p = last_core_prompt(&engine);
    assert!(p.contains("Advance the case this beat"), "{p}");
    assert!(!p.contains(CASE_JOKE), "the engine default gives way: {p}");
    let v = voice(&p).expect("a voice section");
    assert!(
        v.contains(&format!("{}\n- {case_effect}\n", working::EFFECTS_HEADING)),
        "the package's case effect comes first: {v}"
    );

    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_pkg(
        dir.path(),
        Arc::new(engine.clone()),
        "np",
        no_source_pkg(),
        tuning(),
    )
    .await;
    b.tick(1_000, Vec::new(), &dispatch_ctx()).await.unwrap();
    let p = last_core_prompt(&engine);
    assert!(
        p.contains(&format!("by name, and {CASE_JOKE}; then put")),
        "{p}"
    );
    assert!(!p.contains(&case_effect), "{p}");
}

/// A package with no source effect composes the prompts the engine composed before beat
/// sources existed: the golden bytes were written by this scenario at f7fd3d2 (nonces masked).
#[tokio::test]
async fn a_package_with_no_source_effects_composes_byte_identical_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_pkg(
        dir.path(),
        Arc::new(engine.clone()),
        "golden",
        no_source_pkg(),
        tuning(),
    )
    .await;
    let got = golden_turns(&mut b, &engine).await;
    let want = include_str!("golden_no_source_turns.txt").replace("\r\n", "\n");
    assert!(got == want, "the composed turns moved:\n{got}");
}

/// Care and crisis beats get no source effect, whatever the package names.
#[tokio::test]
async fn care_and_crisis_beats_get_no_source_effect() {
    let mut pkg = load_character(&malakor_dir(), "theatrical").unwrap();
    for s in ["chat", "welcome", "purpose", "improvise"] {
        pkg.effects
            .push(source_effect(s, &format!("SOURCE-EFFECT-{s}")));
    }
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_pkg(
        dir.path(),
        Arc::new(engine.clone()),
        "care",
        pkg.clone(),
        unpaced(),
    )
    .await;
    let care = pkg.effects.iter().find(|e| e.tag == "care").unwrap();
    let sad = viewer("s1", "RexOwner", Rank::Cultist);
    let ev = b
        .tick(
            1_000,
            vec![msg(
                "s1",
                900,
                &sad,
                "my old dog passed this morning. i'm just here to be distracted for a bit, no \
                 roast tonight. thanks for having me",
            )],
            &purpose_ctx("dealing", "Deal the next premise."),
        )
        .await
        .unwrap();
    assert_eq!(source_of(&ev), Some(BeatSource::Care));
    let v = voice(&last_core_prompt(&engine)).expect("the care effect");
    assert!(v.contains(&care.text), "{v}");
    assert!(!v.contains("SOURCE-EFFECT"), "{v}");

    let soot = viewer("sim-v0003", "SootGoblin24", Rank::Thrall);
    let ev = b
        .tick(6_000, vec![msg("d1", 5_900, &soot, L167)], &ctx(&[]))
        .await
        .unwrap();
    assert_eq!(source_of(&ev), Some(BeatSource::Care));
    let p = last_core_prompt(&engine);
    assert!(!p.contains("SOURCE-EFFECT"), "{p}");

    // A plain quiet beat does get its duty's effect.
    engine.push(Role::CoreThread, Ok("The ledger hums."));
    let ev = b
        .tick(
            60_000,
            Vec::new(),
            &purpose_ctx("dealing", "Deal the next premise."),
        )
        .await
        .unwrap();
    let src = source_of(&ev).unwrap();
    let p = last_core_prompt(&engine);
    if ["chat", "welcome", "purpose", "improvise"].contains(&src.as_str()) {
        assert!(
            p.contains(&format!("SOURCE-EFFECT-{}", src.as_str())),
            "{src:?}: {p}"
        );
    }
}

/// An unchanged state's purpose is pushed at most 3 times in 24 beats; a new state pushes its
/// own at once.
#[tokio::test]
async fn an_unchanged_purpose_is_pushed_at_most_three_times_in_24_beats() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "pp", unpaced_quiet()).await;
    let c = purpose_ctx("dealing", "Deal the next premise.");
    let mut pushes = 0;
    for k in 0..24i64 {
        engine.push(
            Role::CoreThread,
            Ok(&format!(
                "{} stirs in the crypt.",
                ["Rot", "Mist", "Dust", "Frost"][k as usize % 4]
            )),
        );
        b.tick(1_000 + k * 5_000, Vec::new(), &c).await.unwrap();
        if last_core_prompt(&engine).contains("Advance the segment this beat") {
            pushes += 1;
        }
    }
    assert!((1..=3).contains(&pushes), "{pushes} pushes in 24 beats");
    // A changed state pushes its own purpose on its next improvised beat, without waiting out
    // the spacing.
    let c2 = purpose_ctx("voting", "Settle the vote on the next card.");
    let want = PURPOSE_NOTE.replace("{purpose}", "Settle the vote on the next card.");
    let mut pushed = false;
    for (k, line) in [
        "Ravens circle the tally.",
        "Candles gutter in the vault.",
        "Ink dries on the decree.",
    ]
    .into_iter()
    .enumerate()
    {
        engine.push(Role::CoreThread, Ok(line));
        b.tick(200_000 + k as i64 * 5_000, Vec::new(), &c2)
            .await
            .unwrap();
        pushed |= last_core_prompt(&engine).contains(&want);
    }
    assert!(pushed, "the new state's purpose is pushed");
}

/// An aired call to action (`!hint`) cools for 90 s in an unchanged state: the beats say not to
/// ask again; after the cooldown, or in a changed state, they do not. A call the pace gate
/// trimmed starts no cooldown.
#[tokio::test]
async fn an_aired_call_to_action_cools_for_90_s_and_a_trimmed_one_never_starts() {
    let note = |p: &str| p.contains("You already asked chat for");
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "cta", unpaced()).await;
    let c = purpose_ctx("dealing", "Deal the next premise.");
    engine.push(
        Role::CoreThread,
        Ok("Send your !hint now, minions, the ledger hungers."),
    );
    b.tick(1_000, Vec::new(), &c).await.unwrap();
    engine.push(Role::CoreThread, Ok("The quills scratch on."));
    b.tick(6_000, Vec::new(), &c).await.unwrap();
    let p = last_core_prompt(&engine);
    assert!(note(&p) && p.contains("!hint"), "{p}");
    engine.push(Role::CoreThread, Ok("Still the quills scratch."));
    b.tick(1_000 + CTA_COOLDOWN_MS - 1_000, Vec::new(), &c)
        .await
        .unwrap();
    assert!(note(&last_core_prompt(&engine)));
    engine.push(Role::CoreThread, Ok("The quills rest at last."));
    b.tick(1_000 + CTA_COOLDOWN_MS + 1_000, Vec::new(), &c)
        .await
        .unwrap();
    assert!(!note(&last_core_prompt(&engine)), "cooled down");

    // A changed state may invite again at once.
    engine.push(Role::CoreThread, Ok("Which card do we turn next, minions?"));
    b.tick(200_000, Vec::new(), &c).await.unwrap();
    engine.push(Role::CoreThread, Ok("A pause for the drama of it."));
    b.tick(205_000, Vec::new(), &c).await.unwrap();
    assert!(last_core_prompt(&engine).contains("a choice"));
    let c2 = purpose_ctx("voting", "Settle the vote on the next card.");
    engine.push(Role::CoreThread, Ok("And now the vote begins."));
    b.tick(210_000, Vec::new(), &c2).await.unwrap();
    assert!(
        !note(&last_core_prompt(&engine)),
        "a new state invites again"
    );

    // Trimmed: the call to action was the line the budget dropped.
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "cta2", tuning()).await;
    let long = "The ledger of the night is long and the quills are many and the ink runs thin, \
                and every clerk in the crypt has an opinion about the paperwork of the damned, \
                which they share at length, at volume, and without being asked by anyone.";
    engine.push(
        Role::CoreThread,
        Ok(&format!(
            "{long}\nSend your !vote now, minions, before the candles gutter."
        )),
    );
    let ev = b.tick(1_000, Vec::new(), &c).await.unwrap();
    assert_eq!(
        says(&ev),
        [long.to_string()],
        "the call to action was trimmed"
    );
    engine.push(Role::CoreThread, Ok("The quills scratch on."));
    b.tick(30_000, Vec::new(), &c).await.unwrap();
    assert!(
        !note(&last_core_prompt(&engine)),
        "a trimmed call starts no cooldown"
    );
}

// ── Golden turns: a package with no source effect composes today's prompts ─

/// The prompts a fixed scripted scenario composes, nonces masked: a case beat, a roast
/// request, a newcomer on a state with a purpose, a case beat with chat, a gathering open.
async fn golden_turns(b: &mut Brain, engine: &FakeEngine) -> String {
    let mut seg02 = ctx(&["dispatch_research"]);
    seg02.segment = SegmentId::Seg02aHexmap;
    seg02.state = "investigating".into();
    seg02.state_summary = "No poll open.".into();
    let mut down = hex("HEX-02", "Ashpetal Holdings Ltd", None);
    down.fog = true;
    seg02.hexes = vec![hex("HEX-01", "Glimmerquay Yield Syndicate", None), down];
    let mut dealing = ctx(&[]);
    dealing.state = "dealing".into();
    dealing.state_summary =
        "segment SEG-01, state dealing\nDeal the next premise.\nNo poll open.".into();
    let mut cold = ctx(&[]);
    cold.state = "cold_open".into();
    let bob = viewer("b1", "Bob", Rank::Cultist);
    let moth = viewer("n1", "FreshMoth", Rank::Thrall);
    let beats: Vec<(i64, Vec<ChatMsg>, SegmentContext)> = vec![
        (1_000, Vec::new(), seg02.clone()),
        (
            20_000,
            vec![msg(
                "r1",
                19_900,
                &bob,
                "roast me hard malakor, i lost the vote",
            )],
            ctx(&[]),
        ),
        (
            40_000,
            vec![msg("h1", 39_900, &moth, "hi all, first time here")],
            dealing,
        ),
        (
            60_000,
            vec![msg(
                "q1",
                59_900,
                &bob,
                "@Malakor is that syndicate card a scheme?",
            )],
            seg02,
        ),
        (80_000, Vec::new(), cold),
    ];
    let nonce = regex::Regex::new(r"[0-9a-f]{16}").unwrap();
    let mut out = String::new();
    for (t, inbox, c) in beats {
        engine.push(
            Role::CoreThread,
            Ok(&format!("Beat {t} brings fresh ink to the ledger.")),
        );
        b.tick(t, inbox, &c).await.unwrap();
        let p = last_core_prompt(engine);
        let masked = nonce.replace_all(&p, "<nonce>").to_string();
        out.push_str(&format!("=== beat {t}\n{masked}\n"));
    }
    out
}
