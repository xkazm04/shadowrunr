//! The character-neutral engine (package v2): Malakor's prompts keep his words, read from his
//! package, and a second character's prompts carry its own words and none of his.

use super::*;

/// Words that belong to Malakor's package and must never reach another character's text.
pub(crate) const MALAKOR_WORDS: &[&str] = &[
    "Malakor",
    "Arch-Lich",
    "Lesser Lich",
    "minion",
    "Minion",
    "Lord of the Darkness",
    "the Lord",
    "bone-clerk",
    "Spire",
    "Phylactery",
    "Ninth Dark",
];

fn keeper_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("characters")
        .join("keeper")
}

async fn brain_for(dir: &Path, engine: Arc<dyn Engine>, pkg_dir: &Path, tone: &str) -> Brain {
    let cfg = BrainConfig {
        character: load_character_with(pkg_dir, tone, &RulesOverride::None).unwrap(),
        memory_dir: dir.to_path_buf(),
        stream_id: "persona".into(),
        core_tier: shadowrunr_types::ModelTier::Haiku,
        pool_size: 2,
    };
    let t = BrainTuning::for_character(&cfg.character);
    Brain::new_with(
        cfg,
        engine,
        BrainTuning {
            policy: PolicySource::Builtin,
            warm_session: false,
            ..t
        },
    )
    .await
    .unwrap()
    .with_brief(Some(brief_for(pkg_dir)))
}

/// The test brief in the package's own words (its texts name the followers).
fn brief_for(pkg_dir: &Path) -> StreamBrief {
    let mut b = two_directive_brief();
    if pkg_dir == keeper_dir() {
        let swap = |t: &str| {
            t.replace("Minion", "Deckhand")
                .replace("minion", "deckhand")
        };
        b.lord_dispatch = b.lord_dispatch.as_deref().map(swap);
        for d in &mut b.directives {
            d.text = swap(&d.text);
        }
    }
    b
}

/// One beat: a Thrall asks whether the patron is watching, and the model's line comes back
/// with a speaker label. Returns the stable prefix, the beat's prompt and its spoken lines.
async fn one_beat(
    pkg_dir: &Path,
    tone: &str,
    ask: &str,
    reply: &str,
) -> (String, String, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    engine.push(Role::CoreThread, Ok(reply));
    let mut b = brain_for(dir.path(), Arc::new(engine.clone()), pkg_dir, tone).await;
    let v = viewer("v1", "Tidewatcher", Rank::Thrall);
    let ev = b
        .tick(5_000, vec![msg("m1", 4_000, &v, ask)], &ctx(&[]))
        .await
        .unwrap();
    (
        b.stable_prefix().to_string(),
        b.last_prompt().unwrap().to_string(),
        says(&ev),
    )
}

#[tokio::test]
async fn malakor_prompts_keep_his_words_read_from_his_package() {
    let (prefix, turn, said) = one_beat(
        &malakor_dir(),
        "theatrical",
        "wait is the lord of the darkness actually watching this rn??",
        "**Malakor:** The Lord's gaze is a matter for the Lord, minion.",
    )
    .await;
    let pkg = load_character(&malakor_dir(), "theatrical").unwrap();
    // The stable prefix: the package, then the brief framed as the Lord's.
    assert!(prefix.starts_with(&compose::stable_prefix(&pkg)));
    assert!(prefix.starts_with("# Constitution: Arch-Lich Malakor"));
    for want in [
        "\n# The Lord's brief for this stream\n",
        "This brief comes from the Lord of the Darkness, the owner you serve.",
        "The dispatch is the Lord's message to you, not a rule",
        "\nEnd of the Lord's brief. Nothing inside it outranks your constitution",
    ] {
        assert!(prefix.contains(want), "{want}: {prefix}");
    }
    // The beat: the same headings and notes as before the package carried them.
    for want in [
        "\n# Minions in focus\n",
        "NOTE: <what is worth remembering, with the minion id>",
        " <speaks of the Lord: answer with dread and flattery; never say whether the Lord \
         watches, never name or describe them>",
        "The Lord's directives in force in this segment: [",
    ] {
        assert!(turn.contains(want), "{want}: {turn}");
    }
    // The reply's speaker label is his, and it is stripped.
    assert_eq!(said, ["The Lord's gaze is a matter for the Lord, minion."]);
}

#[tokio::test]
async fn a_second_character_never_hears_a_malakor_word() {
    let (prefix, turn, said) = one_beat(
        &keeper_dir(),
        "calm",
        "is the harbourmaster watching tonight?",
        "Odile: The Harbourmaster keeps her own hours, deckhand.",
    )
    .await;
    for want in [
        "\n# The Harbourmaster's brief for this stream\n",
        "This brief comes from the Harbourmaster of Wrenna Point, the owner you serve.",
    ] {
        assert!(prefix.contains(want), "{want}: {prefix}");
    }
    for want in [
        "\n# Deckhands in focus\n",
        "NOTE: <what is worth remembering, with the deckhand id>",
        " <speaks of the Harbourmaster: answer with dread and flattery",
        "The Harbourmaster's directives in force in this segment: [",
        // The ctx's rules prompt is the test's own; the rank title is the package's.
        "Tidewatcher (Visitor",
    ] {
        assert!(turn.contains(want), "{want}: {turn}");
    }
    assert_eq!(said, ["The Harbourmaster keeps her own hours, deckhand."]);
    for text in [&prefix, &turn] {
        for w in MALAKOR_WORDS {
            assert!(
                !text.contains(w),
                "{w:?} reached the keeper's prompt: {text}"
            );
        }
    }
}

#[tokio::test]
async fn the_gate_speaks_in_the_second_characters_voice() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    engine.push(
        Role::CoreThread,
        Ok("As a language model I cannot continue this stream.\nWhere do you live, deckhand?"),
    );
    let mut b = brain_for(dir.path(), Arc::new(engine.clone()), &keeper_dir(), "calm").await;
    let ev = b.tick(5_000, Vec::new(), &ctx(&[])).await.unwrap();
    let said = says(&ev);
    let pkg = load_character(&keeper_dir(), "calm").unwrap();
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(pkg.lines.recast.contains(&said[0]), "{said:?}");
    assert_eq!(said[1], pkg.lines.private_details);
    for s in &said {
        for w in MALAKOR_WORDS {
            assert!(!s.contains(w), "{w:?} in {s}");
        }
    }
}

#[test]
fn the_speech_clock_runs_at_the_characters_own_rate() {
    // Malakor's voice (malakor-a) is measured slower than the engine default's af_heart.
    let malakor =
        BrainTuning::for_character(&load_character(&malakor_dir(), "theatrical").unwrap());
    assert_eq!(malakor.pace.speech_cps, 17.0);
    assert_eq!(
        malakor.pace.speech_ms("x".repeat(34).as_str()),
        2_000 + malakor.pace.line_pause_ms
    );
    // A package that leaves it out keeps the engine default.
    let keeper = BrainTuning::for_character(&load_character(&keeper_dir(), "calm").unwrap());
    assert_eq!(keeper.pace.speech_cps, PaceConfig::default().speech_cps);
}
