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
        r#"{"type":"status","state":"ready","reason":"numpy is not installed","voice":"af_heart","visemes":false}"#
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
