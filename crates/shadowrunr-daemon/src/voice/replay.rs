//! `shadowrunr voice replay <transcript.jsonl>`: a recorded run's events, pushed again at their
//! recorded `ts_ms` into a live run that the voice service follows, through the real sidecar.
//! It measures what a show would have sounded like with this `[voice]`: lines spoken and
//! skipped, the audio made, the first-audio latency, and a model of the stage's player (back to
//! back playback, at most [`STAGE_AHEAD_S`] queued) for what an audience would have heard.
//! Nothing calls a model: the lines are the transcript's. [`replay_with`] is the same replay
//! over any [`Renderer`] (a fake in tests); [`compare`] runs a grid of configs against a baseline.

pub mod compare;

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use shadowrunr_types::voice::{VoiceFrame, VoiceSkip};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;
use tokio::time::Instant;

use super::renderer::{Renderer, RendererState};
use super::service::{malakor_say, VoiceHub};
use super::sidecar::{SidecarRenderer, STOP_GRACE};
use super::VoiceConfig;
use crate::live::{Frame, LiveRun};

/// The stage player's cap on audio queued ahead of the playhead (`MAX_AHEAD_S` in
/// `apps/stage/src/voice/player.ts`).
pub const STAGE_AHEAD_S: f64 = 6.0;
/// The player's lead before the first sound after silence (`START_LEAD_S`).
const STAGE_LEAD_S: f64 = 0.06;
/// How long the replay waits, after the last event, for the lines still in the service.
const DRAIN: Duration = Duration::from_secs(180);

/// What became of one Malakor line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Every sentence arrived.
    Spoken,
    /// Some sentences, then a failure.
    Partial,
    Late,
    QueueFull,
    Error,
    /// Not resolved before the drain ended.
    Unresolved,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spoken => "spoken",
            Self::Partial => "partial",
            Self::Late => "late",
            Self::QueueFull => "queue_full",
            Self::Error => "error",
            Self::Unresolved => "unresolved",
        }
    }
}

/// One replay's figures.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub workers: usize,
    pub lines_in_flight: usize,
    /// Malakor's lines in the transcript, and the show's length.
    pub lines: usize,
    pub show_s: f64,
    /// Lines whose every sentence arrived.
    pub spoken: usize,
    /// Lines that spoke some sentences, then failed.
    pub partial: usize,
    pub late: usize,
    pub queue_full: usize,
    pub error: usize,
    /// Lines never resolved before the drain ended.
    pub unresolved: usize,
    pub utterances: usize,
    pub audio_s: f64,
    /// From a line's `say` to its first sentence at the follower, over the lines that spoke.
    pub first_audio_ms: Percentiles,
    /// The stage model: sentences it dropped to stay within [`STAGE_AHEAD_S`], the audio it
    /// played, and from a `say` to its first sound, over the lines whose first sentence played.
    pub stage_dropped: usize,
    pub heard_s: f64,
    pub say_to_sound_ms: Percentiles,
    /// Each Malakor line's fate, by its seq in the replayed run (the same seq in every replay
    /// of one transcript, so two configs' lines line up).
    pub outcomes: BTreeMap<u64, Outcome>,
    /// Each Malakor line's text, by seq: for the line diff, not the JSON.
    #[serde(skip)]
    pub texts: BTreeMap<u64, String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Percentiles {
    pub n: usize,
    pub p50: u64,
    pub p90: u64,
    pub max: u64,
}

impl Percentiles {
    fn of(mut v: Vec<u64>) -> Self {
        v.sort_unstable();
        let at = |q: f64| {
            if v.is_empty() {
                0
            } else {
                v[((v.len() - 1) as f64 * q).round() as usize]
            }
        };
        Self {
            n: v.len(),
            p50: at(0.5),
            p90: at(0.9),
            max: v.last().copied().unwrap_or(0),
        }
    }
}

/// The transcript's events as `(ts_ms, frame)`, in file order.
pub fn read_transcript(path: &Path) -> Result<Vec<(u64, Frame)>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(raw)
            .map_err(|e| format!("{} line {}: {e}", path.display(), n + 1))?;
        let ts = v
            .get("ts_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        out.push((ts, Frame::from(raw.trim())));
    }
    Ok(out)
}

/// The stage's player, as a model: what it schedules, drops and plays.
#[derive(Default)]
struct Stage {
    /// `(seq, start_s, dur_s)` still queued.
    queue: Vec<(u64, f64, f64)>,
    dropped: usize,
    dropped_s: f64,
    accepted_s: f64,
    /// Each line's first scheduled start (it may still be dropped).
    first_start: HashMap<u64, f64>,
}

impl Stage {
    fn accept(&mut self, now: f64, seq: u64, part: u32, dur: f64) {
        self.queue.retain(|(_, s, d)| s + d > now);
        let end = self.queue.last().map_or(0.0, |(_, s, d)| s + d);
        let start = end.max(now + STAGE_LEAD_S);
        self.queue.push((seq, start, dur));
        self.accepted_s += dur;
        if part == 0 {
            self.first_start.insert(seq, start);
        }
        loop {
            let end = self.queue.last().map_or(now, |(_, s, d)| s + d);
            if end - now <= STAGE_AHEAD_S {
                break;
            }
            let Some(i) = self.queue.iter().position(|(_, s, _)| *s > now) else {
                break;
            };
            if i == self.queue.len() - 1 {
                break;
            }
            let (gone, s, d) = self.queue.remove(i);
            self.dropped += 1;
            self.dropped_s += d;
            if self.first_start.get(&gone) == Some(&s) {
                self.first_start.remove(&gone);
            }
            let mut at = if i > 0 {
                self.queue[i - 1].1 + self.queue[i - 1].2
            } else {
                now + STAGE_LEAD_S
            };
            for e in &mut self.queue[i..] {
                let moved = at.max(now + STAGE_LEAD_S);
                if self.first_start.get(&e.0) == Some(&e.1) {
                    self.first_start.insert(e.0, moved);
                }
                e.1 = moved;
                at = moved + e.2;
            }
        }
    }
}

/// Replay `events` at `speed` through a fresh hub and sidecar with `voice`.
pub async fn replay(
    voice: VoiceConfig,
    events: Vec<(u64, Frame)>,
    speed: f64,
    progress: bool,
) -> Result<Report, String> {
    let voice = VoiceConfig {
        enabled: true,
        ..voice
    };
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let (renderer, task) = SidecarRenderer::start(voice.clone(), async move {
        let _ = stop_rx.wait_for(|d| *d).await;
    });
    let ready = renderer
        .state()
        .wait_for(|s| !matches!(s, RendererState::Starting))
        .await
        .map(|s| s.clone());
    match ready {
        Ok(RendererState::Ready { .. }) => {}
        Ok(RendererState::Degraded(why)) => return Err(why),
        _ => return Err("the voice sidecar did not start".into()),
    }
    let report = replay_with(voice, renderer, events, speed, progress).await;
    stop_tx.send_replace(true);
    let _ = tokio::time::timeout(STOP_GRACE * 2, task).await;
    Ok(report)
}

/// Replay `events` at `speed` through a fresh hub speaking with `renderer`, which is already
/// ready. The caller owns the renderer's lifetime.
pub async fn replay_with(
    voice: VoiceConfig,
    renderer: Arc<dyn Renderer>,
    events: Vec<(u64, Frame)>,
    speed: f64,
    progress: bool,
) -> Report {
    let voice = VoiceConfig {
        enabled: true,
        ..voice
    };
    let hub = VoiceHub::new(voice.clone());
    // The hub's tasks end with `hub.stop()` below.
    hub.attach(renderer, std::future::pending());
    let run = Arc::new(LiveRun::new("replay".into()));
    let mut following = hub.follow(&run, true);

    let speed = if speed > 0.0 { speed } else { 1.0 };
    let t0 = Instant::now();
    let show_ms = events.last().map_or(0, |(ts, _)| *ts);
    let lines = events
        .iter()
        .filter(|(_, f)| malakor_say(f).is_some())
        .count();
    // The pusher: each event at its time; Malakor's lines are remembered by seq.
    let (said_tx, mut said_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, Instant, String)>();
    let pusher = {
        let run = run.clone();
        tokio::spawn(async move {
            for (ts, f) in events {
                let at = t0 + Duration::from_secs_f64(ts as f64 / 1000.0 / speed);
                tokio::time::sleep_until(at).await;
                let say = malakor_say(&f).map(|u| u.text);
                let seq = run.push(f);
                if let Some(text) = say {
                    let _ = said_tx.send((seq, Instant::now(), text));
                }
            }
        })
    };

    let mut said: BTreeMap<u64, Instant> = BTreeMap::new();
    let mut texts: BTreeMap<u64, String> = BTreeMap::new();
    let mut resolved: HashMap<u64, Outcome> = HashMap::new();
    let mut first: HashMap<u64, Instant> = HashMap::new();
    let mut stage = Stage::default();
    let mut report = Report {
        workers: voice.render_workers.max(1),
        lines_in_flight: voice.lines_in_flight.max(1),
        lines,
        show_s: show_ms as f64 / 1000.0 / speed,
        ..Report::default()
    };
    let mut drain_by: Option<Instant> = None;
    loop {
        while let Ok((seq, at, text)) = said_rx.try_recv() {
            said.insert(seq, at);
            texts.insert(seq, text);
        }
        if pusher.is_finished() && drain_by.is_none() {
            drain_by = Some(Instant::now() + DRAIN);
        }
        if drain_by.is_some() && said_rx.is_empty() && resolved.len() >= lines {
            break;
        }
        let by = drain_by.unwrap_or_else(|| Instant::now() + Duration::from_millis(250));
        let item = match tokio::time::timeout_at(by, following.rx.recv()).await {
            Ok(Ok(item)) => item,
            Ok(Err(RecvError::Lagged(n))) => {
                eprintln!("replay: the follower fell {n} frames behind");
                continue;
            }
            Ok(Err(RecvError::Closed)) => break,
            Err(_) if drain_by.is_some_and(|d| Instant::now() >= d) => break,
            Err(_) => continue,
        };
        let now = Instant::now();
        let at_s = now.duration_since(t0).as_secs_f64();
        match &*item.frame {
            VoiceFrame::Utterance {
                seq,
                part,
                last,
                duration_ms,
                text,
                ..
            } => {
                report.utterances += 1;
                let dur = f64::from(*duration_ms) / 1000.0;
                report.audio_s += dur;
                first.entry(*seq).or_insert(now);
                stage.accept(at_s, *seq, *part, dur);
                if *last {
                    resolved.insert(*seq, Outcome::Spoken);
                }
                if progress {
                    eprintln!("{at_s:>7.1}s  utt {seq}.{part}  {dur:>4.1}s  {text}");
                }
            }
            VoiceFrame::Skipped {
                seq, part, reason, ..
            } => {
                let why = match reason {
                    VoiceSkip::Late => Outcome::Late,
                    VoiceSkip::QueueFull => Outcome::QueueFull,
                    VoiceSkip::Error if *part > 0 => Outcome::Partial,
                    VoiceSkip::Error => Outcome::Error,
                };
                resolved.insert(*seq, why);
                if progress {
                    eprintln!("{at_s:>7.1}s  skipped {seq}.{part}  {}", why.as_str());
                }
            }
            VoiceFrame::Status { .. } => {}
        }
    }
    let _ = pusher.await;
    drop(following);
    hub.stop();
    while let Ok((seq, at, text)) = said_rx.try_recv() {
        said.insert(seq, at);
        texts.insert(seq, text);
    }

    for why in resolved.values() {
        match *why {
            Outcome::Spoken => report.spoken += 1,
            Outcome::Partial => report.partial += 1,
            Outcome::Late => report.late += 1,
            Outcome::QueueFull => report.queue_full += 1,
            Outcome::Error | Outcome::Unresolved => report.error += 1,
        }
    }
    report.unresolved = lines.saturating_sub(resolved.len());
    report.outcomes = said
        .keys()
        .chain(resolved.keys())
        .map(|seq| {
            (
                *seq,
                resolved.get(seq).copied().unwrap_or(Outcome::Unresolved),
            )
        })
        .collect();
    report.texts = texts;
    let ms = |d: Duration| u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
    report.first_audio_ms = Percentiles::of(
        first
            .iter()
            .filter_map(|(seq, at)| said.get(seq).map(|s| ms(at.duration_since(*s))))
            .collect(),
    );
    report.say_to_sound_ms = Percentiles::of(
        stage
            .first_start
            .iter()
            .filter_map(|(seq, start)| {
                let s = said.get(seq)?.duration_since(t0).as_secs_f64();
                Some(((start - s).max(0.0) * 1000.0) as u64)
            })
            .collect(),
    );
    report.stage_dropped = stage.dropped;
    report.heard_s = stage.accepted_s - stage.dropped_s;
    report
}

/// One report as a table row (see [`HEADER`]).
pub fn row(r: &Report) -> String {
    format!(
        "{:>7} {:>6} {:>5} {:>6} {:>7} {:>4} {:>5} {:>5} {:>7} {:>7} {:>9} {:>9} {:>7} {:>7} {:>9}",
        r.workers,
        r.lines_in_flight,
        r.lines,
        r.spoken,
        r.partial,
        r.late,
        r.queue_full,
        r.error + r.unresolved,
        format!("{:.1}", r.audio_s),
        format!("{:.1}", r.show_s),
        r.first_audio_ms.p50,
        r.first_audio_ms.p90,
        r.stage_dropped,
        format!("{:.1}", r.heard_s),
        r.say_to_sound_ms.p50,
    )
}

pub const HEADER: &str = "workers flight lines spoken partial late qfull error audio_s  show_s \
    first_p50 first_p90 stage_x heard_s sound_p50";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_model_drops_the_oldest_unstarted_sentence_past_its_cap() {
        let mut s = Stage::default();
        s.accept(0.0, 1, 0, 4.0);
        s.accept(0.5, 1, 1, 4.0);
        assert_eq!(
            s.dropped, 0,
            "7.56 s ahead, but the newest is kept and the first is playing"
        );
        s.accept(1.0, 2, 0, 2.0);
        // 9.06 s ahead at t=1: seq 1 part 1 (unstarted) goes, seq 2 closes up behind part 0.
        assert_eq!(s.dropped, 1);
        assert_eq!(s.queue.len(), 2);
        assert!((s.queue[1].1 - 4.06).abs() < 1e-9);
        assert_eq!(s.first_start.get(&2).copied(), Some(4.06));
        assert!((s.accepted_s - s.dropped_s - 6.0).abs() < 1e-9);
    }

    #[test]
    fn percentiles_and_transcripts() {
        let p = Percentiles::of(vec![5, 1, 3, 2, 4]);
        assert_eq!((p.n, p.p50, p.p90, p.max), (5, 3, 5, 5));
        assert_eq!(Percentiles::of(Vec::new()).n, 0);
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("t.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"stream_start\",\"ts_ms\":0}\n\n{\"type\":\"say\",\"ts_ms\":1500,\
             \"speaker\":\"malakor\",\"text\":\"Kneel.\"}\n",
        )
        .unwrap();
        let ev = read_transcript(&path).unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[1].0, 1500);
        assert_eq!(
            malakor_say(&ev[1].1).map(|u| u.text).as_deref(),
            Some("Kneel.")
        );
        std::fs::write(&path, "not json\n").unwrap();
        assert!(read_transcript(&path).is_err());
    }

    fn say_at(ts: u64, text: &str) -> (u64, Frame) {
        let raw = serde_json::json!({"type":"say","ts_ms":ts,"speaker":"malakor","text":text})
            .to_string();
        (ts, Frame::from(raw.as_str()))
    }

    #[tokio::test]
    async fn replay_with_a_fake_keeps_each_lines_fate() {
        use crate::voice::renderer::fake::{FakeRenderer, Script};
        let fake = Arc::new(FakeRenderer::new());
        fake.script("Second, and it hangs.", Script::Hang);
        let voice = VoiceConfig {
            max_lag_ms: 60,
            ..VoiceConfig::default()
        };
        let events = vec![
            say_at(0, "First, kneel."),
            say_at(10, "Second, and it hangs."),
            say_at(300, "Third, rise."),
        ];
        let r = tokio::time::timeout(
            Duration::from_secs(10),
            replay_with(voice, fake, events, 1.0, false),
        )
        .await
        .expect("the replay drains");
        let want: BTreeMap<u64, Outcome> = [
            (1, Outcome::Spoken),
            (2, Outcome::Late),
            (3, Outcome::Spoken),
        ]
        .into();
        assert_eq!(r.outcomes, want);
        assert_eq!((r.lines, r.spoken, r.late, r.unresolved), (3, 2, 1, 0));
        assert_eq!(
            r.texts.get(&2).map(String::as_str),
            Some("Second, and it hangs.")
        );
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["outcomes"]["2"], "late");
        assert!(json.get("texts").is_none());
    }

    /// GUARD: the plain `voice replay` prints exactly this, byte for byte, as before arms.
    #[test]
    fn the_plain_report_prints_the_same_header_and_row() {
        let r = Report {
            workers: 3,
            lines_in_flight: 3,
            lines: 24,
            show_s: 300.0,
            spoken: 21,
            partial: 1,
            late: 1,
            queue_full: 0,
            error: 1,
            unresolved: 1,
            utterances: 60,
            audio_s: 151.04,
            first_audio_ms: Percentiles {
                n: 22,
                p50: 1600,
                p90: 2000,
                max: 2400,
            },
            stage_dropped: 2,
            heard_s: 140.96,
            say_to_sound_ms: Percentiles {
                n: 21,
                p50: 1700,
                p90: 2100,
                max: 2500,
            },
            ..Report::default()
        };
        let out = format!("{}\n{}\n", HEADER, row(&r));
        assert_eq!(
            out,
            "workers flight lines spoken partial late qfull error audio_s  show_s \
             first_p50 first_p90 stage_x heard_s sound_p50\n      \
             3      3    24     21       1    1     0     2   151.0   300.0      \
             1600      2000       2   141.0      1700\n"
        );
    }
}
