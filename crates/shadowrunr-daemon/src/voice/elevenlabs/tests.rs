//! The ElevenLabs renderer against a fake API on a local listener: never the real one.

use std::sync::atomic::AtomicUsize;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};
use shadowrunr_types::delivery::Pace;

use super::*;

/// What the fake API saw: each TTS request's voice, key header and body.
#[derive(Default)]
pub(crate) struct Seen {
    pub(crate) tts: Mutex<Vec<(String, String, Value)>>,
}

/// The fake API. Voice ids pick the behaviour: `ok`, `noalign`, `reject`, `hang`, `nospeed`
/// (400 on a body with `voice_settings.speed`) and `missing` (404 on the voice check).
pub(crate) async fn fake() -> (String, Arc<Seen>) {
    async fn tts(
        State(seen): State<Arc<Seen>>,
        Path(voice): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> (StatusCode, String) {
        let key = headers
            .get("xi-api-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        seen.tts
            .lock()
            .unwrap()
            .push((voice.clone(), key, body.clone()));
        let audio = "AAAAAAAA"; // 6 bytes of PCM
        match voice.as_str() {
            "reject" => (StatusCode::UNAUTHORIZED, r#"{"detail":{"status":"invalid_api_key"}}"#.into()),
            "hang" => {
                tokio::time::sleep(Duration::from_secs(30)).await;
                (StatusCode::OK, String::new())
            }
            "nospeed" if body["voice_settings"]["speed"].is_number() => (
                StatusCode::BAD_REQUEST,
                r#"{"detail":{"status":"invalid_voice_settings","message":"speed is not supported"}}"#.into(),
            ),
            "noalign" => (
                StatusCode::OK,
                json!({ "audio_base64": audio, "alignment": null }).to_string(),
            ),
            _ => (
                StatusCode::OK,
                json!({
                    "audio_base64": audio,
                    "alignment": { "characters": ["K", "n"], "character_start_times_seconds": [0.0, 0.1] }
                })
                .to_string(),
            ),
        }
    }
    async fn voice(Path(id): Path<String>, headers: HeaderMap) -> (StatusCode, String) {
        if headers.get("xi-api-key").is_none() {
            return (StatusCode::UNAUTHORIZED, String::new());
        }
        match id.as_str() {
            "reject" => (StatusCode::UNAUTHORIZED, String::new()),
            "missing" => (StatusCode::NOT_FOUND, String::new()),
            _ => (
                StatusCode::OK,
                json!({ "name": format!("Voice {id}") }).to_string(),
            ),
        }
    }
    async fn user(headers: HeaderMap) -> (StatusCode, String) {
        match headers.get("xi-api-key").and_then(|v| v.to_str().ok()) {
            Some("good-key") => (StatusCode::OK, "{}".into()),
            Some("scoped-key") => (
                StatusCode::UNAUTHORIZED,
                r#"{"detail":{"status":"missing_permissions"}}"#.into(),
            ),
            Some("busy-key") => (StatusCode::TOO_MANY_REQUESTS, String::new()),
            _ => (
                StatusCode::UNAUTHORIZED,
                r#"{"detail":{"status":"invalid_api_key"}}"#.into(),
            ),
        }
    }
    let seen = Arc::new(Seen::default());
    let app = Router::new()
        .route("/v1/text-to-speech/{voice}/with-timestamps", post(tts))
        .route("/v1/voices/{id}", get(voice))
        .route("/v1/user", get(user))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), seen)
}

/// A local address nothing listens on: a connection there is refused at once.
async fn closed_port() -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    format!("http://{addr}")
}

/// A key in an env var of the test's own (tests run in parallel).
pub(crate) fn key_env(value: Option<&str>) -> String {
    static N: AtomicUsize = AtomicUsize::new(0);
    let name = format!(
        "SHADOWRUNR_TEST_ELEVENLABS_KEY_{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    );
    match value {
        Some(v) => std::env::set_var(&name, v),
        None => std::env::remove_var(&name),
    }
    name
}

pub(crate) fn cfg(base: &str, env: &str, timeout_ms: u64) -> ElevenlabsConfig {
    ElevenlabsConfig {
        api_key_env: env.into(),
        base_url: base.into(),
        timeout_ms,
        ..ElevenlabsConfig::default()
    }
}

fn spec(voice: &str) -> ElevenSpec {
    ElevenSpec {
        voice_id: Some(voice.into()),
        model: "eleven_v4_turbo".into(),
        speed: DEFAULT_SPEED,
    }
}

async fn settled(r: &ElevenlabsRenderer) -> RendererState {
    let mut st = r.state();
    let s = st
        .wait_for(|s| !matches!(s, RendererState::Starting))
        .await
        .unwrap()
        .clone();
    s
}

async fn parts(r: &ElevenlabsRenderer, id: &str, utter: Utter) -> Vec<Part> {
    let mut rx = r.say(id, &utter);
    let mut out = Vec::new();
    while let Ok(Some(p)) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
        let end = matches!(&p, Part::Error { .. }) || matches!(&p, Part::Audio(a) if a.last);
        out.push(p);
        if end {
            break;
        }
    }
    out
}

#[test]
fn deliveries_map_to_audio_tags_above_the_intensity_floor() {
    let d = |e, i| Delivery::new(e, i, Pace::Normal);
    let cases = [
        (Emotion::Sarcasm, "[sarcastic]"),
        (Emotion::Hushed, "[whispers]"),
        (Emotion::Menace, "[menacing]"),
        (Emotion::Glee, "[excited]"),
        (Emotion::Deadpan, "[deadpan]"),
        (Emotion::Outrage, "[outraged]"),
        (Emotion::Weary, "[sighs]"),
        (Emotion::Triumph, "[proclaiming]"),
        (Emotion::Tender, "[softly]"),
    ];
    for (e, tag) in cases {
        assert_eq!(audio_tag(Some(&d(e, 0.8))), Some(tag), "{e:?}");
        assert_eq!(audio_tag(Some(&d(e, 0.39))), None, "{e:?} under the floor");
    }
    assert_eq!(
        audio_tag(Some(&d(Emotion::Sarcasm, 0.4))),
        Some("[sarcastic]")
    );
    assert_eq!(audio_tag(Some(&d(Emotion::Neutral, 1.0))), None);
    assert_eq!(audio_tag(None), None);
    // The pace has no tag.
    let fast = Delivery::new(Emotion::Neutral, 1.0, Pace::Fast);
    assert_eq!(audio_tag(Some(&fast)), None);
}

#[tokio::test]
async fn a_line_speaks_one_request_per_sentence_with_its_tag_speed_and_key() {
    let (base, seen) = fake().await;
    let env = key_env(Some("good-key"));
    let r = ElevenlabsRenderer::start(cfg(&base, &env, 5_000), spec("ok"), std::future::pending());
    match settled(&r).await {
        RendererState::Ready { visemes, reason } => {
            assert!(!visemes);
            assert_eq!(reason.as_deref(), Some("Voice ok, eleven_v4_turbo"));
        }
        other => panic!("{other:?}"),
    }
    let utter = Utter {
        text: "You brought me a tax form. To my crypt.".into(),
        delivery: Some(Delivery::new(Emotion::Outrage, 0.9, Pace::Fast)),
    };
    let got = parts(&r, "l:1", utter).await;
    assert_eq!(got.len(), 2, "{got:?}");
    let Part::Audio(first) = &got[0] else {
        panic!("{got:?}")
    };
    let Part::Audio(second) = &got[1] else {
        panic!("{got:?}")
    };
    assert_eq!(
        (first.part, first.last, second.part, second.last),
        (0, false, 1, true)
    );
    assert_eq!(
        first.text, "You brought me a tax form.",
        "captions carry no tag"
    );
    assert_eq!(first.sample_rate, 24_000);
    assert!(first.visemes.is_empty(), "the mouth follows loudness");
    assert_eq!(first.timings, VoiceTimings::Provider);
    assert_eq!(
        first.chars,
        "[outraged] You brought me a tax form.".chars().count() as u32
    );
    let tts = seen.tts.lock().unwrap().clone();
    assert_eq!(tts.len(), 2);
    assert_eq!(tts[0].0, "ok");
    assert_eq!(tts[0].1, "good-key", "the key goes in xi-api-key");
    assert_eq!(tts[0].2["text"], "[outraged] You brought me a tax form.");
    assert_eq!(tts[0].2["model_id"], "eleven_v4_turbo");
    let speed = tts[0].2["voice_settings"]["speed"].as_f64().unwrap();
    assert!((speed - 1.08).abs() < 1e-6, "{speed}");
    assert_eq!(tts[1].2["text"], "[outraged] To my crypt.");
}

#[tokio::test]
async fn a_delivery_s_face_direction_is_neither_spoken_nor_sent() {
    let (base, seen) = fake().await;
    let env = key_env(Some("good-key"));
    let r = ElevenlabsRenderer::start(cfg(&base, &env, 5_000), spec("ok"), std::future::pending());
    assert!(matches!(settled(&r).await, RendererState::Ready { .. }));
    let plain = Delivery::new(Emotion::Menace, 0.9, Pace::Slow);
    let face = plain
        .with_gesture(Some(shadowrunr_types::delivery::Gesture::Glare))
        .with_eye_light(Some(2.2));
    assert_eq!(audio_tag(Some(&face)), audio_tag(Some(&plain)));
    for (id, d) in [("l:p", plain), ("l:f", face)] {
        let got = parts(
            &r,
            id,
            Utter {
                text: "Kneel.".into(),
                delivery: Some(d),
            },
        )
        .await;
        assert!(
            matches!(&got[..], [Part::Audio(a)] if a.text == "Kneel."),
            "{got:?}"
        );
    }
    let tts = seen.tts.lock().unwrap().clone();
    assert_eq!(tts.len(), 2);
    assert_eq!(
        tts[0].2, tts[1].2,
        "the same request with or without the face"
    );
    assert_eq!(tts[1].2["text"], "[menacing] Kneel.");
    let body = tts[1].2.to_string();
    assert!(!body.contains("glare") && !body.contains("eye"), "{body}");
}

#[tokio::test]
async fn no_alignment_is_timings_none() {
    let (base, _) = fake().await;
    let env = key_env(Some("good-key"));
    let r = ElevenlabsRenderer::start(
        cfg(&base, &env, 5_000),
        spec("noalign"),
        std::future::pending(),
    );
    let got = parts(&r, "l:1", Utter::plain("Kneel.")).await;
    match &got[..] {
        [Part::Audio(a)] => {
            assert_eq!(a.timings, VoiceTimings::None);
            assert_eq!(a.chars, 6, "no tag on a plain line");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_rejected_key_is_degraded_and_fails_later_lines_at_once() {
    let (base, seen) = fake().await;
    let env = key_env(Some("bad-key"));
    let r = ElevenlabsRenderer::start(
        cfg(&base, &env, 5_000),
        spec("reject"),
        std::future::pending(),
    );
    assert_eq!(
        settled(&r).await,
        RendererState::Degraded(KEY_REJECTED.into()),
        "a 401 at start"
    );
    let got = parts(&r, "l:1", Utter::plain("Kneel.")).await;
    assert_eq!(
        got,
        vec![Part::Error {
            part: 0,
            reason: KEY_REJECTED.into()
        }]
    );
    assert!(
        seen.tts.lock().unwrap().is_empty(),
        "no request after a refusal"
    );

    // A 401 on a line (the voice check passed) degrades too.
    let r = ElevenlabsRenderer::start(cfg(&base, &env, 5_000), spec("ok"), std::future::pending());
    settled(&r).await;
    let mut spec_reject = spec("reject");
    spec_reject.voice_id = Some("reject".into());
    let r2 = ElevenlabsRenderer {
        inner: Arc::new(Inner {
            spec: spec_reject,
            ..clone_inner(&r)
        }),
    };
    let got = parts(&r2, "l:2", Utter::plain("Kneel.")).await;
    assert_eq!(
        got,
        vec![Part::Error {
            part: 0,
            reason: KEY_REJECTED.into()
        }]
    );
    assert_eq!(
        *r2.state().borrow(),
        RendererState::Degraded(KEY_REJECTED.into())
    );
}

/// A fresh `Inner` like `r`'s, in the Ready state (to drive a line past the start check).
fn clone_inner(r: &ElevenlabsRenderer) -> Inner {
    Inner {
        cfg: r.inner.cfg.clone(),
        spec: r.inner.spec.clone(),
        http: r.inner.cfg.client(),
        state: watch::Sender::new(RendererState::Ready {
            visemes: false,
            reason: None,
        }),
        lines: Mutex::new(HashMap::new()),
        rejected: AtomicBool::new(false),
        speed_ok: AtomicBool::new(true),
    }
}

#[tokio::test]
async fn a_timeout_fails_the_part_and_leaves_the_voice_ready() {
    let (base, _) = fake().await;
    let env = key_env(Some("good-key"));
    let r = ElevenlabsRenderer::start(cfg(&base, &env, 300), spec("hang"), std::future::pending());
    assert!(matches!(settled(&r).await, RendererState::Ready { .. }));
    let got = parts(&r, "l:1", Utter::plain("Kneel. Rise, mortal.")).await;
    assert_eq!(
        got,
        vec![Part::Error {
            part: 0,
            reason: "elevenlabs: no answer within 300 ms".into()
        }]
    );
    assert!(matches!(*r.state().borrow(), RendererState::Ready { .. }));
}

#[tokio::test]
async fn a_model_that_refuses_speed_is_asked_without_it() {
    let (base, seen) = fake().await;
    let env = key_env(Some("good-key"));
    let r = ElevenlabsRenderer::start(
        cfg(&base, &env, 5_000),
        spec("nospeed"),
        std::future::pending(),
    );
    settled(&r).await;
    let got = parts(&r, "l:1", Utter::plain("Kneel.")).await;
    assert!(matches!(&got[..], [Part::Audio(_)]), "{got:?}");
    assert!(!r.speed_accepted());
    let tts = seen.tts.lock().unwrap().clone();
    assert_eq!(tts.len(), 2, "one refusal, one retry");
    assert!(tts[1].2.get("voice_settings").is_none());
    parts(&r, "l:2", Utter::plain("Rise.")).await;
    assert_eq!(seen.tts.lock().unwrap().len(), 3, "no speed from then on");
}

#[tokio::test]
async fn a_missing_key_or_voice_is_degraded_without_a_request() {
    let (base, seen) = fake().await;
    let env = key_env(None);
    let r = ElevenlabsRenderer::start(cfg(&base, &env, 5_000), spec("ok"), std::future::pending());
    assert_eq!(
        settled(&r).await,
        RendererState::Degraded(format!("elevenlabs: {env} is not set"))
    );
    let got = parts(&r, "l:1", Utter::plain("Kneel.")).await;
    assert!(matches!(&got[..], [Part::Error { part: 0, .. }]), "{got:?}");
    let env = key_env(Some("good-key"));
    let r = ElevenlabsRenderer::start(
        cfg(&base, &env, 5_000),
        spec("missing"),
        std::future::pending(),
    );
    assert_eq!(
        settled(&r).await,
        RendererState::Degraded("elevenlabs: voice missing is not in the account".into())
    );
    let mut none = spec("ok");
    none.voice_id = None;
    let r = ElevenlabsRenderer::start(cfg(&base, &env, 5_000), none, std::future::pending());
    assert!(
        matches!(settled(&r).await, RendererState::Degraded(why) if why.contains("no voice_id"))
    );
    assert!(seen.tts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_key_is_never_in_an_error_a_state_or_debug() {
    let secret = "sk_test_never_shown_0123456789";
    let (base, _) = fake().await;
    let env = key_env(Some(secret));
    let mut texts = Vec::new();
    for voice in ["reject", "hang", "ok"] {
        let r =
            ElevenlabsRenderer::start(cfg(&base, &env, 300), spec(voice), std::future::pending());
        texts.push(format!("{:?}", settled(&r).await));
        for p in parts(&r, "l:1", Utter::plain("Kneel.")).await {
            texts.push(format!("{p:?}"));
        }
        texts.push(format!("{r:?}"));
    }
    // The network failing outright.
    let closed = closed_port().await;
    let r = ElevenlabsRenderer::start(
        cfg(&closed, &env, 2_000),
        spec("ok"),
        std::future::pending(),
    );
    texts.push(format!("{:?}", settled(&r).await));
    for p in parts(&r, "l:1", Utter::plain("Kneel.")).await {
        texts.push(format!("{p:?}"));
    }
    texts.push(format!("{:?}", verify(&cfg(&base, &env, 300), 1).await));
    texts.push(format!("{:?}", cfg(&base, &env, 300)));
    for t in &texts {
        assert!(!t.contains(secret), "the key leaked: {t}");
    }
    // Refused or timed out (Windows retries a refused local connect): a short reason either way.
    assert!(
        texts
            .iter()
            .any(|t| t.contains("cannot reach the API") || t.contains("no answer within 2000 ms")),
        "{texts:?}"
    );
}

#[tokio::test]
async fn verify_maps_answers_to_key_states() {
    let (base, _) = fake().await;
    let check = |value: Option<&'static str>| {
        let base = base.clone();
        async move {
            let env = key_env(value);
            verify(&cfg(&base, &env, 2_000), 42).await
        }
    };
    let ok = check(Some("good-key")).await;
    assert_eq!(
        (ok.key, ok.ready, ok.checked_ms),
        (VoiceKeyState::Verified, true, Some(42))
    );
    assert_eq!(check(Some("bad-key")).await.key, VoiceKeyState::Rejected);
    let missing = check(None).await;
    assert_eq!(
        (missing.key, missing.ready),
        (VoiceKeyState::Missing, false)
    );
    let busy = check(Some("busy-key")).await;
    assert_eq!(busy.key, VoiceKeyState::Set);
    assert!(busy.reason.unwrap().contains("429"));
    let scoped = check(Some("scoped-key")).await;
    assert_eq!(
        scoped.key,
        VoiceKeyState::Set,
        "a scoped key is not a bad key"
    );
    let env = key_env(Some("good-key"));
    let offline = verify(&cfg(&closed_port().await, &env, 2_000), 1).await;
    assert_eq!(offline.key, VoiceKeyState::Set);
}
