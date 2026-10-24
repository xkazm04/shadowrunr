//! The hub's bounds, driven by the fake renderer: no process, no Python.

use std::time::Duration;

use shadowrunr_types::voice::{VoiceFrame, VoiceSkip, VoiceState};

use super::*;
use crate::voice::renderer::fake::{FakeRenderer, Script};

fn cfg() -> VoiceConfig {
    VoiceConfig {
        enabled: true,
        ..VoiceConfig::default()
    }
}

fn hub_with(cfg: VoiceConfig) -> (Arc<VoiceHub>, Arc<FakeRenderer>) {
    let hub = VoiceHub::new(cfg);
    let fake = Arc::new(FakeRenderer::new());
    hub.attach(fake.clone(), std::future::pending());
    (hub, fake)
}

pub(crate) fn say(text: &str) -> Frame {
    Frame::from(
        serde_json::json!({"type":"say","ts_ms":1,"speaker":"malakor","text":text}).to_string(),
    )
}

async fn next(rx: &mut broadcast::Receiver<VoiceItem>) -> VoiceFrame {
    let item = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a frame in time")
        .expect("open");
    (*item.frame).clone()
}

fn skipped(seq: u64, part: u32, reason: VoiceSkip) -> VoiceFrame {
    VoiceFrame::Skipped {
        run_id: "r".into(),
        seq,
        part,
        reason,
    }
}

async fn wait_said(fake: &FakeRenderer, n: usize) {
    for _ in 0..500 {
        if fake.said.lock().unwrap().len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("the renderer was never asked");
}

#[tokio::test]
async fn a_disabled_hub_is_off_and_spawns_nothing() {
    let hub = VoiceHub::new(VoiceConfig::default());
    let fake = Arc::new(FakeRenderer::new());
    hub.attach(fake.clone(), std::future::pending());
    let st = hub.status().borrow().frame.clone();
    assert!(
        matches!(
            &*st,
            VoiceFrame::Status {
                state: VoiceState::Off,
                voice: None,
                ..
            }
        ),
        "{st:?}"
    );
    assert!(lock(&hub.tasks).is_empty());
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, true);
    run.push(say("Kneel."));
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(f.rx.try_recv().is_err());
    assert!(fake.said.lock().unwrap().is_empty());
}

#[tokio::test]
async fn malakor_lines_are_spoken_as_ordered_utterances() {
    let (hub, fake) = hub_with(cfg());
    fake.script(
        "Kneel. Now. Forever.",
        Script::Speak(
            vec!["Kneel.".into(), "Now.".into(), "Forever.".into()],
            Duration::from_millis(5),
        ),
    );
    let run = Arc::new(LiveRun::new("r".into()));
    run.push(Frame::from(
        r#"{"type":"segment_start","ts_ms":0,"segment":"SEG-01"}"#,
    ));
    let mut f = hub.follow(&run, false);
    run.push(Frame::from(
        serde_json::json!({"type":"say","ts_ms":1,"speaker":"foil","foil":"x","text":"Not me."})
            .to_string(),
    ));
    run.push(say("Kneel. Now. Forever."));
    for (part, text) in ["Kneel.", "Now.", "Forever."].iter().enumerate() {
        match next(&mut f.rx).await {
            VoiceFrame::Utterance {
                id,
                run_id,
                seq,
                part: p,
                last,
                text: t,
                sample_rate,
                duration_ms,
                visemes,
                ..
            } => {
                assert_eq!(id, format!("r:3:{part}"));
                assert_eq!((run_id.as_str(), seq, p), ("r", 3, part as u32));
                assert_eq!(last, part == 2);
                assert_eq!(&t, text);
                assert_eq!((sample_rate, duration_ms), (24_000, 100));
                assert_eq!(visemes.len(), 1);
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        *fake.said.lock().unwrap(),
        ["r:3"],
        "the foil is not voiced"
    );
    let st = hub.status().borrow().frame.clone();
    assert!(
        matches!(
            &*st,
            VoiceFrame::Status {
                state: VoiceState::Ready,
                visemes: true,
                ..
            }
        ),
        "{st:?}"
    );
}

#[tokio::test]
async fn past_max_queue_the_oldest_line_is_skipped() {
    let (hub, fake) = hub_with(VoiceConfig {
        max_queue: 2,
        lines_in_flight: 1,
        ..cfg()
    });
    fake.script("busy", Script::Hang);
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, false);
    run.push(say("busy"));
    wait_said(&fake, 1).await;
    for t in ["b", "c", "d", "e"] {
        run.push(say(t));
    }
    assert_eq!(next(&mut f.rx).await, skipped(2, 0, VoiceSkip::QueueFull));
    assert_eq!(next(&mut f.rx).await, skipped(3, 0, VoiceSkip::QueueFull));
    assert_eq!(hub.queued(), 2);
}

#[tokio::test]
async fn a_line_without_audio_within_max_lag_is_late() {
    let (hub, fake) = hub_with(VoiceConfig {
        max_lag_ms: 60,
        lines_in_flight: 1,
        ..cfg()
    });
    fake.script("slow", Script::Hang);
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, false);
    run.push(say("slow"));
    run.push(say("waits"));
    assert_eq!(next(&mut f.rx).await, skipped(1, 0, VoiceSkip::Late));
    assert_eq!(fake.cancelled.lock().unwrap().front().unwrap(), "r:1");
    // The second line was as old as the bound when it left the queue: never rendered.
    assert_eq!(next(&mut f.rx).await, skipped(2, 0, VoiceSkip::Late));
    assert_eq!(*fake.said.lock().unwrap(), ["r:1"]);
    // Late lines are not failures.
    let st = hub.status().borrow().frame.clone();
    assert!(matches!(
        &*st,
        VoiceFrame::Status {
            state: VoiceState::Ready,
            ..
        }
    ));
}

#[tokio::test]
async fn a_failed_part_skips_the_rest_of_its_line() {
    let (hub, fake) = hub_with(cfg());
    fake.script(
        "one two three",
        Script::FailAfter(vec!["one".into(), "two".into(), "three".into()], 1),
    );
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, false);
    run.push(say("one two three"));
    assert!(matches!(
        next(&mut f.rx).await,
        VoiceFrame::Utterance {
            part: 0,
            last: false,
            ..
        }
    ));
    assert_eq!(next(&mut f.rx).await, skipped(1, 1, VoiceSkip::Error));
}

#[tokio::test]
async fn render_timeouts_are_errors_and_repeated_failures_degrade() {
    let (hub, fake) = hub_with(VoiceConfig {
        render_timeout_ms: 40,
        ..cfg()
    });
    fake.script("stuck", Script::Hang);
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, false);
    let mut status = hub.status();
    for seq in 1..=FAILING_AFTER as u64 {
        run.push(say("stuck"));
        assert_eq!(next(&mut f.rx).await, skipped(seq, 0, VoiceSkip::Error));
    }
    let st = status.borrow_and_update().frame.clone();
    match &*st {
        VoiceFrame::Status {
            state: VoiceState::Degraded,
            reason: Some(r),
            ..
        } => assert!(r.contains("renders failing"), "{r}"),
        other => panic!("{other:?}"),
    }
    // One line that speaks clears it.
    run.push(say("fine"));
    assert!(matches!(
        next(&mut f.rx).await,
        VoiceFrame::Utterance { .. }
    ));
    let st = status.borrow_and_update().frame.clone();
    assert!(matches!(
        &*st,
        VoiceFrame::Status {
            state: VoiceState::Ready,
            ..
        }
    ));
}

#[tokio::test]
async fn the_renderer_state_is_the_status() {
    let (hub, fake) = hub_with(cfg());
    let mut status = hub.status();
    fake.set_state(RendererState::Degraded(
        "the voice sidecar exited (exit code: 1)".into(),
    ));
    tokio::time::timeout(
        Duration::from_secs(5),
        status.wait_for(|s| {
            matches!(
                &*s.frame,
                VoiceFrame::Status {
                    state: VoiceState::Degraded,
                    ..
                }
            )
        }),
    )
    .await
    .unwrap()
    .unwrap();
    fake.set_state(RendererState::Ready {
        visemes: false,
        reason: Some("numpy is not installed".into()),
    });
    let st = tokio::time::timeout(
        Duration::from_secs(5),
        status.wait_for(|s| {
            matches!(
                &*s.frame,
                VoiceFrame::Status {
                    state: VoiceState::Ready,
                    ..
                }
            )
        }),
    )
    .await
    .unwrap()
    .unwrap()
    .json
    .clone();
    assert_eq!(
        &*st,
        r#"{"type":"status","state":"ready","reason":"numpy is not installed","voice":"af_heart","visemes":false,"provider":"kokoro"}"#
    );
}

#[tokio::test]
async fn the_feeder_stops_with_its_last_follower() {
    let (hub, fake) = hub_with(cfg());
    let run = Arc::new(LiveRun::new("r".into()));
    let a = hub.follow(&run, false);
    let b = hub.follow(&run, false);
    drop(a);
    assert!(hub.followed("r"));
    drop(b);
    assert!(!hub.followed("r"));
    run.push(say("nobody listens"));
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(fake.said.lock().unwrap().is_empty());
    // A run that started just now is voiced from its start.
    let fresh = Arc::new(LiveRun::new("r2".into()));
    fresh.push(say("first words"));
    let mut f = hub.follow(&fresh, true);
    assert!(matches!(
        next(&mut f.rx).await,
        VoiceFrame::Utterance { seq: 1, .. }
    ));
}

#[tokio::test]
async fn lines_in_flight_render_ahead_and_still_go_out_in_seq_order() {
    let (hub, fake) = hub_with(cfg());
    fake.script(
        "slow pair",
        Script::Speak(vec!["a1".into(), "a2".into()], Duration::from_millis(40)),
    );
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, false);
    run.push(say("slow pair"));
    run.push(say("quick"));
    // Both lines are with the renderer at once; the second is rendered first.
    wait_said(&fake, 2).await;
    let mut got = Vec::new();
    for _ in 0..3 {
        match next(&mut f.rx).await {
            VoiceFrame::Utterance {
                seq, part, text, ..
            } => got.push((seq, part, text)),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        got,
        [
            (1, 0, "a1".to_string()),
            (1, 1, "a2".to_string()),
            (2, 0, "quick".to_string())
        ]
    );
}

#[tokio::test]
async fn at_most_lines_in_flight_lines_are_with_the_renderer() {
    let (hub, fake) = hub_with(VoiceConfig {
        lines_in_flight: 2,
        ..cfg()
    });
    for t in ["h1", "h2", "h3"] {
        fake.script(t, Script::Hang);
    }
    let run = Arc::new(LiveRun::new("r".into()));
    let _f = hub.follow(&run, false);
    for t in ["h1", "h2", "h3"] {
        run.push(say(t));
    }
    wait_said(&fake, 2).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(*fake.said.lock().unwrap(), ["r:1", "r:2"]);
    assert_eq!(hub.queued(), 1, "the third line waits in the queue");
}

#[tokio::test]
async fn a_waiting_line_whose_audio_came_in_time_speaks_after_a_late_one() {
    let (hub, fake) = hub_with(VoiceConfig {
        max_lag_ms: 60,
        ..cfg()
    });
    fake.script("slow", Script::Hang);
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, false);
    run.push(say("slow"));
    run.push(say("ready"));
    assert_eq!(next(&mut f.rx).await, skipped(1, 0, VoiceSkip::Late));
    assert!(matches!(
        next(&mut f.rx).await,
        VoiceFrame::Utterance {
            seq: 2,
            last: true,
            ..
        }
    ));
    assert_eq!(fake.cancelled.lock().unwrap().front().unwrap(), "r:1");
}

#[tokio::test]
async fn a_waiting_line_with_no_audio_is_late_but_keeps_its_place() {
    let (hub, fake) = hub_with(VoiceConfig {
        max_lag_ms: 100,
        ..cfg()
    });
    fake.script(
        "long",
        Script::Speak(vec!["x1".into(), "x2".into()], Duration::from_millis(80)),
    );
    fake.script("stuck", Script::Hang);
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, false);
    run.push(say("long"));
    run.push(say("stuck"));
    // The second line is cancelled on its own clock, before the first is done...
    for _ in 0..200 {
        if !fake.cancelled.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(*fake.cancelled.lock().unwrap(), ["r:2"]);
    // ...and its skip still goes out after the first line's audio.
    for part in 0..2 {
        assert!(matches!(
            next(&mut f.rx).await,
            VoiceFrame::Utterance { seq: 1, part: p, .. } if p == part
        ));
    }
    assert_eq!(next(&mut f.rx).await, skipped(2, 0, VoiceSkip::Late));
}

// ── the ledger: every queued line in voice.jsonl ends in a known state ──

/// The run's `voice.jsonl` so far, as (event, seq, reason) and the raw lines.
fn ledger(hub: &VoiceHub, run_id: &str) -> Vec<(String, u64, Option<String>)> {
    let log = hub.logs().get(run_id).expect("the run's log is open");
    log.sync();
    std::fs::read_to_string(&log.path)
        .unwrap_or_default()
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            (
                v["event"].as_str().unwrap().to_string(),
                v["seq"].as_u64().unwrap(),
                v["reason"].as_str().map(String::from),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_line_is_queued_in_the_log_before_it_renders() {
    let d = tempfile::tempdir().unwrap();
    let (hub, _fake) = hub_with(cfg());
    hub.logs().open("r", d.path());
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, false);
    run.push(say("Rise."));
    assert!(matches!(
        next(&mut f.rx).await,
        VoiceFrame::Utterance { seq: 1, .. }
    ));
    let l = ledger(&hub, "r");
    assert_eq!(
        l.first(),
        Some(&("queued".to_string(), 1, None)),
        "queued comes first: {l:?}"
    );
    let queued_at = l.iter().position(|e| e.0 == "queued").unwrap();
    let rendered_at = l.iter().position(|e| e.0 == "rendered").unwrap();
    assert!(queued_at < rendered_at, "{l:?}");
    assert_eq!(l.iter().filter(|e| e.0 == "queued").count(), 1, "{l:?}");
}

#[tokio::test]
async fn a_queued_line_whose_run_is_unfollowed_is_skipped_in_the_log() {
    let d = tempfile::tempdir().unwrap();
    let (hub, fake) = hub_with(VoiceConfig {
        lines_in_flight: 1,
        ..cfg()
    });
    fake.script("busy", Script::Hang);
    hub.logs().open("r", d.path());
    let run = Arc::new(LiveRun::new("r".into()));
    let f = hub.follow(&run, false);
    run.push(say("busy"));
    wait_said(&fake, 1).await;
    run.push(say("never taken"));
    for _ in 0..500 {
        if hub.queued() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(hub.queued(), 1, "the second line waits behind the hung one");
    drop(f);
    let l = ledger(&hub, "r");
    assert!(l.contains(&("queued".into(), 2, None)), "{l:?}");
    assert_eq!(
        l.last(),
        Some(&("skipped".to_string(), 2, Some("unfollowed".to_string()))),
        "{l:?}"
    );
    assert_eq!(hub.queued(), 0, "an unfollowed run's lines leave the queue");
}

#[tokio::test]
async fn a_feeder_that_falls_behind_reattaches_and_loses_no_line() {
    let d = tempfile::tempdir().unwrap();
    let (hub, _fake) = hub_with(cfg());
    hub.logs().open("r", d.path());
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, false);
    // The feeder is on the run's broadcast before anything is pushed.
    for _ in 0..500 {
        if run.subscribers() >= 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(run.subscribers(), 1);
    // Without a yield the feeder cannot read: more than the broadcast holds goes past it, and
    // the first line is among what it misses.
    run.push(say("Lost in the flood."));
    let filler = Frame::from(r#"{"type":"segment_start","ts_ms":0,"segment":"SEG-01"}"#);
    for _ in 0..(crate::live::BROADCAST_CAPACITY + 50) {
        run.push(filler.clone());
    }
    let tail = run.push(say("After the flood."));
    let mut spoken = Vec::new();
    while spoken.len() < 2 {
        if let VoiceFrame::Utterance { seq, .. } = next(&mut f.rx).await {
            spoken.push(seq);
        }
    }
    assert_eq!(spoken, [1, tail], "the lagged line is voiced, in order");
    let l = ledger(&hub, "r");
    for seq in [1, tail] {
        let queued = l.iter().filter(|e| e.0 == "queued" && e.1 == seq).count();
        let terminal = l
            .iter()
            .filter(|e| (e.0 == "rendered" || e.0 == "skipped") && e.1 == seq)
            .count();
        assert_eq!((queued, terminal), (1, 1), "seq {seq}: {l:?}");
    }
}

/// Voices by character: `keeper` speaks with ElevenLabs, anyone else with Kokoro. Every renderer
/// it starts is a fake, kept with its key.
#[derive(Default)]
struct FakeVoices {
    started: Mutex<Vec<(String, Arc<FakeRenderer>)>>,
    /// Lines every ElevenLabs renderer fails.
    failing: Mutex<Vec<String>>,
    /// Scripts every renderer it starts takes.
    scripts: Mutex<Vec<(String, Script)>>,
}

/// The one foil [`FakeVoices`] voices: always on Kokoro, sid 52 of its own voices file.
const GUEST: &str = "the-painter";

fn guest_spoken() -> Spoken {
    Spoken {
        provider: VoiceProvider::Kokoro,
        voice: "the-painter-a".into(),
        key: format!("kokoro|foil|{GUEST}"),
        spec: crate::voice::settings::Spec::Kokoro(Box::new(VoiceConfig {
            voice: "the-painter-a".into(),
            sid: 52,
            voices_file: Some("the-painter-voices.bin".into()),
            ..VoiceConfig::default()
        })),
    }
}

impl FakeVoices {
    fn spoken(character: &str) -> Spoken {
        let provider = if character == "keeper" {
            VoiceProvider::Elevenlabs
        } else {
            VoiceProvider::Kokoro
        };
        Spoken {
            provider,
            voice: format!("{character}-voice"),
            key: format!("{}|{character}", provider.as_str()),
            spec: crate::voice::settings::Spec::Kokoro(Box::default()),
        }
    }

    fn keys(&self) -> Vec<String> {
        self.started
            .lock()
            .unwrap()
            .iter()
            .map(|(k, _)| k.clone())
            .collect()
    }

    fn renderer(&self, i: usize) -> Arc<FakeRenderer> {
        self.started.lock().unwrap()[i].1.clone()
    }

    /// The renderer started for `key`.
    fn renderer_of(&self, key: &str) -> Arc<FakeRenderer> {
        let started = self.started.lock().unwrap();
        started.iter().find(|(k, _)| k == key).expect(key).1.clone()
    }
}

impl Voices for FakeVoices {
    fn resolve(&self, character: &str) -> Option<Spoken> {
        Some(Self::spoken(character))
    }

    fn start(
        &self,
        spoken: &Spoken,
        _stop: watch::Receiver<bool>,
    ) -> (Arc<dyn Renderer>, Option<JoinHandle<()>>) {
        let fake = Arc::new(FakeRenderer::new());
        for (line, script) in self.scripts.lock().unwrap().iter() {
            fake.script(line, script.clone());
        }
        if spoken.provider == VoiceProvider::Elevenlabs {
            for line in self.failing.lock().unwrap().iter() {
                fake.script(line, Script::FailAfter(Vec::new(), 0));
            }
        }
        self.started
            .lock()
            .unwrap()
            .push((spoken.key.clone(), fake.clone()));
        (fake, None)
    }

    fn resolve_foil(&self, _character: &str, foil: &str) -> Result<Option<Spoken>, String> {
        Ok((foil == GUEST).then(guest_spoken))
    }
}

fn stream_start(character: &str) -> Frame {
    Frame::from(
        serde_json::json!({"type":"stream_start","ts_ms":0,"stream_id":"s","character":character,
            "tone_variant":"calm","core_tier":"haiku","audience_size":3})
        .to_string(),
    )
}

fn status_of(hub: &VoiceHub) -> VoiceFrame {
    (*hub.status().borrow().frame).clone()
}

#[tokio::test]
async fn a_stream_start_swaps_the_renderer_before_the_first_line() {
    let voices = Arc::new(FakeVoices::default());
    let hub = VoiceHub::new(cfg());
    hub.attach_voices(
        voices.clone(),
        &FakeVoices::spoken("malakor"),
        std::future::pending(),
    );
    assert_eq!(voices.keys(), ["kokoro|malakor"]);
    match status_of(&hub) {
        VoiceFrame::Status {
            provider, voice, ..
        } => {
            assert_eq!(provider, Some(VoiceProvider::Kokoro));
            assert_eq!(voice.as_deref(), Some("malakor-voice"));
        }
        other => panic!("{other:?}"),
    }

    let run = Arc::new(LiveRun::new("r".into()));
    run.push(stream_start("keeper"));
    let mut f = hub.follow(&run, true);
    run.push(say("Kneel."));
    assert!(matches!(
        next(&mut f.rx).await,
        VoiceFrame::Utterance { .. }
    ));
    assert_eq!(voices.keys(), ["kokoro|malakor", "elevenlabs|keeper"]);
    assert!(
        voices.renderer(0).said.lock().unwrap().is_empty(),
        "the old voice said nothing"
    );
    assert_eq!(*voices.renderer(1).said.lock().unwrap(), ["r:2"]);
    match status_of(&hub) {
        VoiceFrame::Status {
            provider, voice, ..
        } => {
            assert_eq!(provider, Some(VoiceProvider::Elevenlabs));
            assert_eq!(voice.as_deref(), Some("keeper-voice"));
        }
        other => panic!("{other:?}"),
    }

    // The same voice at the next stream start keeps the renderer.
    drop(f);
    let again = Arc::new(LiveRun::new("r2".into()));
    again.push(stream_start("keeper"));
    let mut f = hub.follow(&again, true);
    again.push(say("Rise."));
    assert!(matches!(
        next(&mut f.rx).await,
        VoiceFrame::Utterance { .. }
    ));
    assert_eq!(voices.keys().len(), 2);
}

#[tokio::test]
async fn a_failing_provider_never_falls_back_to_another() {
    let voices = Arc::new(FakeVoices::default());
    voices
        .failing
        .lock()
        .unwrap()
        .extend(["One.".to_string(), "Two.".into(), "Three.".into()]);
    let hub = VoiceHub::new(cfg());
    hub.attach_voices(
        voices.clone(),
        &FakeVoices::spoken("keeper"),
        std::future::pending(),
    );
    let run = Arc::new(LiveRun::new("r".into()));
    run.push(stream_start("keeper"));
    let mut f = hub.follow(&run, true);
    for (seq, text) in [(2u64, "One."), (3, "Two."), (4, "Three.")] {
        run.push(say(text));
        assert_eq!(next(&mut f.rx).await, skipped(seq, 0, VoiceSkip::Error));
    }
    assert_eq!(
        voices.keys(),
        ["elevenlabs|keeper"],
        "no other renderer was started"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match status_of(&hub) {
            VoiceFrame::Status {
                state: VoiceState::Degraded,
                provider,
                reason,
                ..
            } => {
                assert_eq!(provider, Some(VoiceProvider::Elevenlabs));
                assert!(reason.unwrap().contains("renders failing"));
                break;
            }
            _ if Instant::now() < deadline => tokio::time::sleep(Duration::from_millis(5)).await,
            other => panic!("{other:?}"),
        }
    }
}

#[tokio::test]
async fn rendered_parts_log_provider_chars_and_timings() {
    let tmp = tempfile::tempdir().unwrap();
    let voices = Arc::new(FakeVoices::default());
    let hub = VoiceHub::new(cfg());
    hub.attach_voices(
        voices.clone(),
        &FakeVoices::spoken("keeper"),
        std::future::pending(),
    );
    let log = hub.logs().open("r", tmp.path());
    let run = Arc::new(LiveRun::new("r".into()));
    let mut f = hub.follow(&run, true);
    run.push(Frame::from(
        serde_json::json!({"type":"say","ts_ms":1,"speaker":"malakor","text":"Kneel.",
            "delivery":{"emotion":"menace","intensity":0.7,"pace":"slow"}})
        .to_string(),
    ));
    assert!(matches!(
        next(&mut f.rx).await,
        VoiceFrame::Utterance { .. }
    ));
    log.sync();
    let text = std::fs::read_to_string(&log.path).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["event"], "queued");
    assert_eq!(lines[0]["provider"], "elevenlabs");
    assert_eq!(lines[0]["delivery"]["emotion"], "menace");
    assert_eq!(lines[1]["event"], "rendered");
    assert_eq!(lines[1]["provider"], "elevenlabs");
    assert_eq!(lines[1]["chars"], 6);
    assert_eq!(lines[1]["timings"], "durations");
}

fn foil_say(foil: &str, text: &str) -> Frame {
    Frame::from(
        serde_json::json!({"type":"say","ts_ms":1,"speaker":"foil","foil":foil,"text":text})
            .to_string(),
    )
}

fn scene_start(guest: &str) -> Frame {
    Frame::from(
        serde_json::json!({"type":"scene_start","ts_ms":1,"scene_id":"the-gallery",
            "title":"The Gallery","guest":guest})
        .to_string(),
    )
}

async fn wait_keys(voices: &FakeVoices, n: usize) {
    for _ in 0..500 {
        if voices.keys().len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("only {:?} were started", voices.keys());
}

/// SEG-03's acceptance: a host line, a guest line, a host line go out in that order and never
/// overlap (each line's frames end before the next line's begin, though the last host line
/// renders long before the slow guest line is done); the guest's carry `speaker: foil` and its
/// id and were rendered by the guest's own Kokoro voice even with the host on ElevenLabs; the
/// host's carry neither.
#[tokio::test]
async fn a_guest_line_speaks_in_its_own_voice_between_host_lines_without_overlap() {
    let voices = Arc::new(FakeVoices::default());
    voices.scripts.lock().unwrap().push((
        "Behold. My masterpiece.".into(),
        Script::Speak(
            vec!["Behold.".into(), "My masterpiece.".into()],
            Duration::from_millis(40),
        ),
    ));
    let hub = VoiceHub::new(cfg());
    hub.attach_voices(
        voices.clone(),
        &FakeVoices::spoken("keeper"),
        std::future::pending(),
    );
    let run = Arc::new(LiveRun::new("r".into()));
    run.push(stream_start("keeper"));
    let mut f = hub.follow(&run, true);
    // The scene's start warms the guest's voice before its first line.
    run.push(scene_start(GUEST));
    wait_keys(&voices, 2).await;
    assert_eq!(
        voices.keys(),
        ["elevenlabs|keeper", "kokoro|foil|the-painter"]
    );
    run.push(say("Sit down."));
    run.push(foil_say(GUEST, "Behold. My masterpiece."));
    run.push(say("It is a smudge."));

    let mut got = Vec::new();
    while got.len() < 4 {
        let item = tokio::time::timeout(Duration::from_secs(5), f.rx.recv())
            .await
            .expect("a frame in time")
            .expect("open");
        let json: serde_json::Value = serde_json::from_str(&item.json).unwrap();
        match (*item.frame).clone() {
            VoiceFrame::Utterance {
                seq,
                part,
                last,
                speaker,
                foil,
                ..
            } => {
                if speaker.is_none() {
                    assert!(json.get("speaker").is_none() && json.get("foil").is_none());
                } else {
                    assert_eq!(
                        (&json["speaker"], &json["foil"]),
                        (&serde_json::json!("foil"), &serde_json::json!(GUEST))
                    );
                }
                got.push((seq, part, last, speaker, foil));
            }
            other => panic!("{other:?}"),
        }
    }
    let guest = Some(GUEST.to_string());
    assert_eq!(
        got,
        [
            (3, 0, true, None, None),
            (4, 0, false, Some(Speaker::Foil), guest.clone()),
            (4, 1, true, Some(Speaker::Foil), guest),
            (5, 0, true, None, None),
        ]
    );
    // Each voice said only its own lines.
    assert_eq!(
        *voices.renderer_of("elevenlabs|keeper").said.lock().unwrap(),
        ["r:3", "r:5"]
    );
    assert_eq!(
        *voices
            .renderer_of("kokoro|foil|the-painter")
            .said
            .lock()
            .unwrap(),
        ["r:4"]
    );
    // The host's status still names the host's voice.
    match status_of(&hub) {
        VoiceFrame::Status {
            provider, voice, ..
        } => {
            assert_eq!(provider, Some(VoiceProvider::Elevenlabs));
            assert_eq!(voice.as_deref(), Some("keeper-voice"));
        }
        other => panic!("{other:?}"),
    }
    // The run's last follower leaves: the guest's renderer stops with it.
    drop(f);
    assert!(lock(&hub.foils).is_empty());
}

#[tokio::test]
async fn a_guest_line_is_logged_with_the_guest_s_provider() {
    let tmp = tempfile::tempdir().unwrap();
    let voices = Arc::new(FakeVoices::default());
    let hub = VoiceHub::new(cfg());
    hub.attach_voices(
        voices.clone(),
        &FakeVoices::spoken("keeper"),
        std::future::pending(),
    );
    let log = hub.logs().open("r", tmp.path());
    let run = Arc::new(LiveRun::new("r".into()));
    run.push(stream_start("keeper"));
    let mut f = hub.follow(&run, true);
    run.push(foil_say(GUEST, "Behold."));
    assert!(matches!(
        next(&mut f.rx).await,
        VoiceFrame::Utterance { .. }
    ));
    log.sync();
    let text = std::fs::read_to_string(&log.path).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        (&lines[0]["event"], &lines[0]["provider"]),
        (&serde_json::json!("queued"), &serde_json::json!("kokoro"))
    );
    assert_eq!(
        (&lines[1]["event"], &lines[1]["provider"]),
        (&serde_json::json!("rendered"), &serde_json::json!("kokoro"))
    );
}

#[tokio::test]
async fn a_guest_without_a_voice_is_captions_only_with_one_warning() {
    let voices = Arc::new(FakeVoices::default());
    let hub = VoiceHub::new(cfg());
    hub.attach_voices(
        voices.clone(),
        &FakeVoices::spoken("malakor"),
        std::future::pending(),
    );
    let run = Arc::new(LiveRun::new("r".into()));
    run.push(stream_start("malakor"));
    let mut f = hub.follow(&run, true);
    run.push(scene_start("the-disruptor"));
    run.push(foil_say("the-disruptor", "Chaos!"));
    run.push(foil_say("the-disruptor", "More chaos!"));
    run.push(say("Enough."));
    match next(&mut f.rx).await {
        VoiceFrame::Utterance {
            seq, speaker, foil, ..
        } => assert_eq!((seq, speaker, foil), (5, None, None)),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        voices.keys(),
        ["kokoro|malakor"],
        "no voice was started for it"
    );
    assert_eq!(*voices.renderer(0).said.lock().unwrap(), ["r:5"]);
    let warned: Vec<String> = hub
        .notes()
        .into_iter()
        .filter(|n| n.contains("the-disruptor"))
        .collect();
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(warned[0].contains("captions only"), "{warned:?}");
    assert_eq!(hub.queued(), 0);
}
