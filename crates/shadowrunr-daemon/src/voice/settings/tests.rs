//! The voice settings and RPCs over a temp copy of the packages, a temp data dir and the fake
//! ElevenLabs API (never the real one, never a sidecar).

use shadowrunr_types::delivery::{Delivery, Emotion, Pace};

use super::*;
use crate::e2e::{copy_dir, repo, test_config};
use crate::engine::EngineHandle;
use crate::voice::elevenlabs::tests::{cfg as el_cfg, fake, key_env};

/// Malakor and the `keeper` fixture (no bench) in a temp characters dir; data in `tmp`. The
/// ElevenLabs key's env var is the test's own, with `key` in it.
fn setup(tmp: &Path, el: Option<ElevenlabsConfig>) -> Arc<AppState> {
    let chars = tmp.join("characters");
    copy_dir(&repo().join("characters/malakor"), &chars.join("malakor"));
    copy_dir(
        &repo().join("crates/shadowrunr-brain/tests/fixtures/characters/keeper"),
        &chars.join("keeper"),
    );
    let mut cfg = test_config(tmp, chars);
    cfg.voice_daemon.elevenlabs = el.unwrap_or_else(|| ElevenlabsConfig {
        api_key_env: key_env(None),
        ..ElevenlabsConfig::default()
    });
    AppState::new(cfg, EngineHandle::scripted())
}

fn eleven(voice_id: &str) -> VoiceChoice {
    VoiceChoice {
        provider: VoiceProvider::Elevenlabs,
        kokoro: None,
        elevenlabs: Some(ElevenlabsVoice {
            voice_id: Some(voice_id.into()),
            model: None,
            speed: None,
        }),
    }
}

#[test]
fn the_effective_choice_is_package_then_override_per_field() {
    let tmp = tempfile::tempdir().unwrap();
    let st = setup(tmp.path(), None);
    let cfg = &st.cfg;
    let package = package_choice(cfg, "malakor");
    assert_eq!(package.provider, VoiceProvider::Kokoro);
    let k = package.kokoro.clone().unwrap();
    assert_eq!(
        (k.voice.as_deref(), k.sid, k.voices_file.as_deref(), k.fx),
        (
            Some("malakor-a"),
            Some(52),
            Some("malakor-voices.bin"),
            Some(VoiceFx::Light)
        )
    );
    let e = package.elevenlabs.clone().unwrap();
    assert_eq!(
        (e.voice_id, e.model.as_deref(), e.speed),
        (None, Some("eleven_v4_turbo"), Some(1.08))
    );
    assert_eq!(label(cfg, &package), "Kokoro malakor-a (sid 52), fx light");

    // The override wins per field and keeps the rest.
    let over = VoiceChoice {
        provider: VoiceProvider::Elevenlabs,
        kokoro: Some(KokoroVoice {
            sid: Some(7),
            ..KokoroVoice::default()
        }),
        elevenlabs: Some(ElevenlabsVoice {
            voice_id: Some("abc123".into()),
            model: None,
            speed: Some(1.1),
        }),
    };
    let eff = effective(&package, Some(&over));
    assert_eq!(eff.provider, VoiceProvider::Elevenlabs);
    let k = eff.kokoro.clone().unwrap();
    assert_eq!((k.voice.as_deref(), k.sid), (Some("malakor-a"), Some(7)));
    let e = eff.elevenlabs.clone().unwrap();
    assert_eq!(
        (e.voice_id.as_deref(), e.model.as_deref(), e.speed),
        (Some("abc123"), Some("eleven_v4_turbo"), Some(1.1))
    );
    assert_eq!(label(cfg, &eff), "ElevenLabs abc123, eleven_v4_turbo");
    let s = spoken(cfg, &eff);
    assert_eq!(s.provider, VoiceProvider::Elevenlabs);
    assert!(matches!(&s.spec, Spec::Elevenlabs(_, spec) if spec.speed == 1.1));
    // The kokoro voice is the daemon's [voice] with the choice's fields over it.
    let kv = kokoro_config(cfg, &eff);
    assert_eq!((kv.voice.as_str(), kv.sid), ("malakor-a", 7));
    assert_ne!(spoken(cfg, &package).key, s.key);
}

#[tokio::test]
async fn the_override_round_trips_inside_data_and_bad_writes_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let st = setup(tmp.path(), None);
    let set = |character: &str, choice: Option<VoiceChoice>| {
        settings_set(
            &st,
            VoiceSettingsSetParams {
                character: character.into(),
                choice,
            },
        )
    };
    let got = set("malakor", Some(eleven("43UFEkkGDp1aWG45TVp0")))
        .await
        .unwrap();
    let path = tmp
        .path()
        .join("data")
        .join("characters")
        .join("malakor")
        .join("voice.json");
    assert!(path.is_file());
    let stored: VoiceChoice =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(stored, eleven("43UFEkkGDp1aWG45TVp0"));
    assert_eq!(got.r#override, Some(stored));
    assert_eq!(got.effective.provider, VoiceProvider::Elevenlabs);
    assert_eq!(got.package.provider, VoiceProvider::Kokoro);
    assert!(got.label.starts_with("ElevenLabs "), "{}", got.label);
    let v = serde_json::to_value(&got).unwrap();
    assert!(!crate::e2e::has_null(&v), "{v}");

    // Refused: nothing changes on disk.
    let before = std::fs::read_to_string(&path).unwrap();
    let mut fast = eleven("v");
    fast.elevenlabs.as_mut().unwrap().speed = Some(1.5);
    let mut slow_kokoro = eleven("v");
    slow_kokoro.kokoro = Some(KokoroVoice {
        speed: Some(0.4),
        ..KokoroVoice::default()
    });
    let mut path_voices = eleven("v");
    path_voices.kokoro = Some(KokoroVoice {
        voices_file: Some("../../secret.bin".into()),
        ..KokoroVoice::default()
    });
    let cases = [
        ("malakor", Some(fast), "speed"),
        ("malakor", Some(slow_kokoro), "speed"),
        ("malakor", Some(eleven("a/b")), "voice_id"),
        ("malakor", Some(path_voices), "voices_file"),
        ("nobody", Some(eleven("v")), "unknown character"),
        ("../malakor", Some(eleven("v")), "character"),
    ];
    for (character, choice, says) in cases {
        let e = set(character, choice).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{e}");
        assert!(e.message.contains(says), "{e}");
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert!(!tmp.path().join("data").join("malakor").exists());
    // The boundaries themselves are fine.
    let mut edge = eleven("v");
    edge.elevenlabs.as_mut().unwrap().speed = Some(1.2);
    edge.kokoro = Some(KokoroVoice {
        speed: Some(0.5),
        ..KokoroVoice::default()
    });
    set("malakor", Some(edge)).await.unwrap();

    // Null clears it.
    let got = set("malakor", None).await.unwrap();
    assert!(!path.exists());
    assert_eq!(got.r#override, None);
    assert_eq!(got.effective, got.package);
    let e = settings_get(
        &st,
        VoiceSettingsGetParams {
            character: "nobody".into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
}

#[tokio::test]
async fn providers_name_the_key_state_and_keep_the_last_verify() {
    let (base, _) = fake().await;
    let tmp = tempfile::tempdir().unwrap();
    let st = setup(tmp.path(), None);
    let get = || {
        settings_get(
            &st,
            VoiceSettingsGetParams {
                character: "malakor".into(),
            },
        )
    };
    let got = get().await.unwrap();
    assert_eq!(got.providers.len(), 2);
    let el = &got.providers[1];
    assert_eq!(el.provider, VoiceProvider::Elevenlabs);
    assert_eq!((el.key, el.ready), (VoiceKeyState::Missing, false));
    assert!(el
        .reason
        .as_deref()
        .unwrap()
        .contains("SHADOWRUNR_TEST_ELEVENLABS_KEY"));
    assert_eq!(got.providers[0].key, VoiceKeyState::NotNeeded);

    let tmp2 = tempfile::tempdir().unwrap();
    let env = key_env(Some("good-key"));
    let st = setup(tmp2.path(), Some(el_cfg(&base, &env, 5_000)));
    let status = verify(
        &st,
        VoiceVerifyParams {
            provider: VoiceProvider::Elevenlabs,
        },
    )
    .await
    .unwrap();
    assert_eq!(status.key, VoiceKeyState::Verified);
    store_override(&st.cfg, "malakor", Some(&eleven("ok"))).unwrap();
    let got = settings_get(
        &st,
        VoiceSettingsGetParams {
            character: "malakor".into(),
        },
    )
    .await
    .unwrap();
    let el = &got.providers[1];
    assert_eq!((el.key, el.ready), (VoiceKeyState::Verified, true));
    assert!(el.checked_ms.is_some(), "the verdict is kept with its time");
    let text = serde_json::to_string(&got).unwrap();
    assert!(!text.contains("good-key"), "{text}");
}

#[tokio::test]
async fn a_preview_is_gated_and_capped_before_anything_renders() {
    let tmp = tempfile::tempdir().unwrap();
    let st = setup(tmp.path(), None);
    let preview_of = |character: &str, text: &str| {
        preview(
            &st,
            VoicePreviewParams {
                character: character.into(),
                text: text.into(),
                delivery: None,
            },
        )
    };
    let e = preview_of("malakor", "Where do you live, mortal?")
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert!(e.message.contains("output gate"), "{e}");
    let e = preview_of("malakor", &"a".repeat(401)).await.unwrap_err();
    assert!(e.message.contains("400"), "{e}");
    let e = preview_of("malakor", "  ").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    let e = preview_of("nobody", "Kneel.").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert!(
        st.voice.preview.try_lock().unwrap().is_none(),
        "no renderer was started"
    );

    // One at a time.
    let held = st.voice.preview.lock().await;
    let e = preview_of("malakor", "Kneel.").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    drop(held);
}

#[tokio::test]
async fn a_preview_and_the_bench_speak_through_their_own_renderer() {
    let (base, seen) = fake().await;
    let tmp = tempfile::tempdir().unwrap();
    let env = key_env(Some("good-key"));
    let st = setup(tmp.path(), Some(el_cfg(&base, &env, 5_000)));
    store_override(&st.cfg, "malakor", Some(&eleven("ok"))).unwrap();
    let got = preview(
        &st,
        VoicePreviewParams {
            character: "malakor".into(),
            text: "Kneel.".into(),
            delivery: Some(Delivery::new(Emotion::Menace, 0.8, Pace::Slow)),
        },
    )
    .await
    .unwrap();
    assert_eq!(got.provider, VoiceProvider::Elevenlabs);
    assert_eq!(got.label, "ElevenLabs Voice ok, eleven_v4_turbo");
    assert_eq!((got.sample_rate, got.pcm.as_str()), (24_000, "AAAAAAAA"));
    assert_eq!(got.chars, "[menacing] Kneel.".chars().count() as u32);
    assert!(!got.visemes);
    assert_eq!(got.delivery.unwrap().emotion, Emotion::Menace);
    assert_eq!(st.voice.queued(), 0, "never the show's queue");
    assert_eq!(seen.tts.lock().unwrap()[0].2["text"], "[menacing] Kneel.");

    let bench_of = |character: &str| {
        bench(
            &st,
            VoiceBenchParams {
                character: character.into(),
            },
        )
    };
    let b = bench_of("malakor").await.unwrap();
    assert_eq!(b.provider, VoiceProvider::Elevenlabs);
    assert_eq!(b.lines.len(), 10);
    assert!(
        b.lines
            .iter()
            .all(|l| l.error.is_none() && !l.pcm.is_empty()),
        "{:?}",
        b.lines
    );
    assert_eq!(b.lines[0].id, "L01-menace");
    assert_eq!(b.lines[0].delivery.emotion, Emotion::Menace);
    let e = bench_of("keeper").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn a_failed_bench_line_carries_its_error_and_the_rest_go_on() {
    let (base, _) = fake().await;
    let tmp = tempfile::tempdir().unwrap();
    let env = key_env(Some("good-key"));
    let st = setup(tmp.path(), Some(el_cfg(&base, &env, 5_000)));
    let bench_file = tmp
        .path()
        .join("characters")
        .join("keeper")
        .join("voice")
        .join("bench.toml");
    std::fs::create_dir_all(bench_file.parent().unwrap()).unwrap();
    std::fs::write(
        &bench_file,
        "[[line]]\nid = \"a\"\ntext = \"Where do you live, mortal?\"\nemotion = \"glee\"\nintensity = 0.5\npace = \"fast\"\n\n\
         [[line]]\nid = \"b\"\ntext = \"Kneel.\"\nemotion = \"neutral\"\nintensity = 0.5\npace = \"normal\"\n",
    )
    .unwrap();
    store_override(&st.cfg, "keeper", Some(&eleven("ok"))).unwrap();
    let b = bench(
        &st,
        VoiceBenchParams {
            character: "keeper".into(),
        },
    )
    .await
    .unwrap();
    assert!(b.lines[0].error.as_deref().unwrap().contains("output gate"));
    assert_eq!((b.lines[0].pcm.as_str(), b.lines[0].chars), ("", 0));
    assert!(b.lines[1].error.is_none());
    assert_eq!(b.lines[1].chars, 6, "neutral: no tag");
}
