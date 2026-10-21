//! `shadowrunr voice`: probe the `[voice]` setup and speak one line through the sidecar,
//! without a daemon. Both work whether or not `[voice] enabled` is set.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Subcommand;
use tokio::sync::watch;
use tokio::time::Instant;

use super::renderer::{b64_decode, duration_ms, Part, Renderer, RendererState};
use super::replay::compare;
use super::sidecar::{SidecarRenderer, STOP_GRACE};
use super::{probe, replay};
use crate::config::Config;
use crate::runs;

#[derive(Debug, Subcommand)]
pub enum VoiceCommand {
    /// Probe [voice]: the engine and Kokoro's files, the python and the sidecar import, and
    /// the mouth-shape extras (durations model, numpy/onnxruntime). Prints a table.
    #[command(after_help = VOICE_CHECK_EXIT_CODES)]
    Check {
        /// Print the items as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Speak one line through the sidecar once: each sentence as it arrives, then the audio
    /// length, the mouth shapes and the time to the first sentence.
    #[command(after_help = VOICE_SAY_EXIT_CODES)]
    Say {
        /// The line, as Malakor would say it.
        text: String,
        /// Write the audio as a WAV: to PATH, or with no PATH to <data>/voice/say-<ms>.wav.
        #[arg(long, value_name = "PATH", num_args = 0..=1)]
        out: Option<Option<PathBuf>>,
    },
    /// Replay a recorded run (its transcript.jsonl) at its recorded times through the sidecar
    /// and the voice service, and print the lines spoken and skipped, the audio, the first-audio
    /// latency and what the stage's player would have played. No model is called.
    #[command(after_help = VOICE_REPLAY_EXIT_CODES)]
    Replay {
        /// A run's transcript.jsonl.
        transcript: PathBuf,
        /// Time scale: 2 replays twice as fast.
        #[arg(long, default_value_t = 1.0)]
        speed: f64,
        /// Override [voice] render_workers.
        #[arg(long)]
        workers: Option<usize>,
        /// Override [voice] lines_in_flight.
        #[arg(long)]
        lines_in_flight: Option<usize>,
        /// Override [voice] render_threads.
        #[arg(long)]
        threads: Option<u32>,
        /// Compare configs: a grid of `key=v1,v2` over workers, flight (lines in flight) and
        /// threads, e.g. "workers=2,3 flight=1,3". Each arm is diffed line by line against the
        /// baseline.
        #[arg(long, value_name = "GRID")]
        arms: Option<String>,
        /// Replay every config N times, interleaved, and print each figure as its median with
        /// its range and n.
        #[arg(long, value_name = "N")]
        repeat: Option<usize>,
        /// The config the arms are diffed against, one value per key (e.g. "workers=3,flight=3");
        /// by default [voice] with the flags above.
        #[arg(long, value_name = "ARM")]
        baseline: Option<String>,
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
        /// Print each frame to stderr as it arrives.
        #[arg(long)]
        progress: bool,
    },
}

/// `shadowrunr voice replay` exit codes (also in docs/guide/voice.md).
pub const VOICE_REPLAY_EXIT_CODES: &str = "Exit codes:
  0  the replay ran (whatever it measured)
  1  the transcript could not be read, or the sidecar did not start
  2  usage error";

/// `shadowrunr voice check` exit codes (also in docs/guide/voice.md).
pub const VOICE_CHECK_EXIT_CODES: &str = "Exit codes:
  0  everything needed to speak is present (mouth shapes are optional)
  1  something needed is missing
  2  usage error";

/// `shadowrunr voice say` exit codes (also in docs/guide/voice.md).
pub const VOICE_SAY_EXIT_CODES: &str = "Exit codes:
  0  the whole line was spoken
  1  the sidecar did not start, or a sentence failed or timed out
  2  usage error";

pub async fn run(cfg: &Config, cmd: VoiceCommand) -> u8 {
    match cmd {
        VoiceCommand::Check { json } => check_cmd(cfg, json).await,
        VoiceCommand::Say { text, out } => {
            let out = out.map(|o| o.unwrap_or_else(|| default_wav(&cfg.paths.data)));
            say_cmd(cfg, &text, out).await
        }
        VoiceCommand::Replay {
            transcript,
            speed,
            workers,
            lines_in_flight,
            threads,
            arms,
            repeat,
            baseline,
            json,
            progress,
        } => {
            let plan = match compare::plan(arms.as_deref(), repeat, baseline.as_deref()) {
                Ok(p) => p,
                Err(why) => {
                    eprintln!("error: {why}");
                    return 2;
                }
            };
            let mut voice = cfg.voice.clone();
            voice.render_workers = workers.unwrap_or(voice.render_workers);
            voice.lines_in_flight = lines_in_flight.unwrap_or(voice.lines_in_flight);
            voice.render_threads = threads.or(voice.render_threads);
            match plan {
                None => replay_cmd(voice, &transcript, speed, json, progress).await,
                Some(plan) => compare_cmd(voice, &transcript, speed, json, progress, &plan).await,
            }
        }
    }
}

async fn replay_cmd(
    voice: super::VoiceConfig,
    transcript: &Path,
    speed: f64,
    json: bool,
    progress: bool,
) -> u8 {
    if let Some(why) = probe::engine_missing(&voice) {
        eprintln!("error: {why} (run `shadowrunr voice check`)");
        return 1;
    }
    let events = match replay::read_transcript(transcript) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    match replay::replay(voice, events, speed, progress).await {
        Ok(r) if json => {
            println!("{}", serde_json::to_string(&r).unwrap_or_default());
            0
        }
        Ok(r) => {
            println!("{}", replay::HEADER);
            println!("{}", replay::row(&r));
            0
        }
        Err(why) => {
            eprintln!("error: {why}");
            1
        }
    }
}

async fn compare_cmd(
    voice: super::VoiceConfig,
    transcript: &Path,
    speed: f64,
    json: bool,
    progress: bool,
    plan: &compare::Plan,
) -> u8 {
    if let Some(why) = probe::engine_missing(&voice) {
        eprintln!("error: {why} (run `shadowrunr voice check`)");
        return 1;
    }
    let events = match replay::read_transcript(transcript) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let one = |v: super::VoiceConfig| replay::replay(v, events.clone(), speed, progress);
    match compare::run_plan(&voice, plan, one, true).await {
        Ok(grid) if json => {
            println!("{}", compare::to_json(&grid));
            0
        }
        Ok(grid) => {
            print!("{}", compare::table(&grid));
            0
        }
        Err(why) => {
            eprintln!("error: {why}");
            1
        }
    }
}

async fn check_cmd(cfg: &Config, json: bool) -> u8 {
    let items = probe::check(&cfg.voice).await;
    let ok = probe::all_needed(&items);
    if json {
        match serde_json::to_string_pretty(&items) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("error: {e}"),
        }
    } else {
        println!(
            "[voice] is {} (voice {}, sid {})",
            if cfg.voice.enabled {
                "enabled"
            } else {
                "disabled"
            },
            cfg.voice.voice,
            cfg.voice.sid
        );
        print!("{}", probe::table(&items));
        let missing = items.iter().filter(|i| i.needed && !i.ok()).count();
        if ok {
            let shapes = items.iter().filter(|i| !i.needed).all(|i| i.ok());
            println!(
                "voice: ready to speak, {}",
                if shapes {
                    "with mouth shapes"
                } else {
                    "without mouth shapes"
                }
            );
        } else {
            println!("voice: {missing} needed item(s) missing");
        }
    }
    u8::from(!ok)
}

fn default_wav(data: &Path) -> PathBuf {
    data.join("voice")
        .join(format!("say-{}.wav", runs::now_ms()))
}

/// A mono PCM16 WAV.
pub fn wav_bytes(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let mut w = Vec::with_capacity(44 + pcm.len());
    let len = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36u32.saturating_add(len)).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&sample_rate.to_le_bytes());
    w.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&len.to_le_bytes());
    w.extend_from_slice(pcm);
    w
}

async fn say_cmd(cfg: &Config, text: &str, out: Option<PathBuf>) -> u8 {
    if text.trim().is_empty() {
        eprintln!("error: nothing to say");
        return 2;
    }
    if let Some(why) = probe::engine_missing(&cfg.voice) {
        eprintln!("error: {why} (run `shadowrunr voice check`)");
        return 1;
    }
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let t0 = Instant::now();
    let (renderer, task) = SidecarRenderer::start(cfg.voice.clone(), async move {
        let _ = stop_rx.wait_for(|d| *d).await;
    });
    let code = speak(cfg, renderer.as_ref(), text, out, t0).await;
    stop_tx.send_replace(true);
    let _ = tokio::time::timeout(STOP_GRACE * 2, task).await;
    code
}

async fn speak(
    cfg: &Config,
    renderer: &SidecarRenderer,
    text: &str,
    out: Option<PathBuf>,
    t0: Instant,
) -> u8 {
    let mut st = renderer.state();
    let ready = st
        .wait_for(|s| !matches!(s, RendererState::Starting))
        .await
        .map(|s| s.clone());
    match ready {
        Ok(RendererState::Ready { visemes, reason }) => {
            let why = reason.map(|r| format!(" ({r})")).unwrap_or_default();
            println!(
                "sidecar ready in {} ms, mouth shapes {}{why}",
                t0.elapsed().as_millis(),
                if visemes { "on" } else { "off" }
            );
        }
        Ok(RendererState::Degraded(why)) => {
            eprintln!("error: {why}");
            return 1;
        }
        _ => {
            eprintln!("error: the voice sidecar did not start");
            return 1;
        }
    }
    let t1 = Instant::now();
    let mut parts = renderer.say("cli:1", text);
    let per_part = Duration::from_millis(cfg.voice.render_timeout_ms.max(1));
    let (mut n, mut audio_ms, mut shapes) = (0u32, 0u64, 0usize);
    let mut first: Option<Duration> = None;
    let mut pcm = Vec::new();
    let mut rate = 0u32;
    let failed = loop {
        match tokio::time::timeout(per_part, parts.recv()).await {
            Ok(Some(Part::Audio(r))) => {
                let at = t1.elapsed();
                first.get_or_insert(at);
                let d = duration_ms(&r.pcm, r.sample_rate);
                println!(
                    "part {}  at {:>5} ms  {:>5} ms audio  {:>3} visemes  {}",
                    r.part,
                    at.as_millis(),
                    d,
                    r.visemes.len(),
                    r.text
                );
                n += 1;
                audio_ms += u64::from(d);
                shapes += r.visemes.len();
                rate = r.sample_rate;
                if out.is_some() {
                    pcm.extend(b64_decode(&r.pcm).unwrap_or_default());
                }
                if r.last {
                    break false;
                }
            }
            Ok(Some(Part::Error { part, reason })) => {
                eprintln!("part {part} failed: {reason}");
                break true;
            }
            Ok(None) => {
                eprintln!("the sidecar stopped mid-line");
                break true;
            }
            Err(_) => {
                renderer.cancel("cli:1");
                eprintln!("a sentence took longer than {} ms", per_part.as_millis());
                break true;
            }
        }
    };
    println!(
        "{n} part(s), {audio_ms} ms of audio, {shapes} visemes, first part after {} ms, \
         all in {} ms",
        first.map_or_else(|| "-".to_string(), |f| f.as_millis().to_string()),
        t1.elapsed().as_millis()
    );
    if let (Some(path), true) = (out, n > 0) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&path, wav_bytes(&pcm, rate)) {
            Ok(()) => println!("wrote {}", path.display()),
            Err(e) => {
                eprintln!("error: {}: {e}", path.display());
                return 1;
            }
        }
    }
    u8::from(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_header_describes_mono_pcm16() {
        let w = wav_bytes(&[1, 0, 2, 0], 24_000);
        assert_eq!(w.len(), 48);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 40);
        assert_eq!(u16::from_le_bytes(w[22..24].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 4);
    }

    fn voice(args: &[&str]) -> Result<VoiceCommand, clap::Error> {
        use clap::Parser;
        let argv = ["shadowrunr", "voice"]
            .into_iter()
            .chain(args.iter().copied());
        match crate::cli::Cli::try_parse_from(argv)?.command {
            crate::cli::Command::Voice(c) => Ok(c),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn voice_commands_parse() {
        assert!(matches!(
            voice(&["check"]).unwrap(),
            VoiceCommand::Check { json: false }
        ));
        assert!(matches!(
            voice(&["check", "--json"]).unwrap(),
            VoiceCommand::Check { json: true }
        ));
        match voice(&["say", "Kneel, mortal."]).unwrap() {
            VoiceCommand::Say { text, out } => {
                assert_eq!(text, "Kneel, mortal.");
                assert_eq!(out, None);
            }
            other => panic!("{other:?}"),
        }
        match voice(&["say", "x", "--out"]).unwrap() {
            VoiceCommand::Say { out, .. } => assert_eq!(out, Some(None)),
            other => panic!("{other:?}"),
        }
        match voice(&["say", "--out", "a.wav", "x"]).unwrap() {
            VoiceCommand::Say { text, out } => {
                assert_eq!(text, "x");
                assert_eq!(out, Some(Some(PathBuf::from("a.wav"))));
            }
            other => panic!("{other:?}"),
        }
        assert!(voice(&["say"]).is_err(), "say needs its text");
        assert!(voice(&["shout", "x"]).is_err());
    }

    #[tokio::test]
    async fn replay_arms_are_checked_before_anything_runs() {
        let cmd = voice(&["replay", "t.jsonl", "--arms", "workers=0"]).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let cfg = Config::from_file_config(crate::config::FileConfig::default(), tmp.path());
        assert_eq!(
            run(&cfg, cmd).await,
            2,
            "a usage error, before the transcript is read"
        );
        let cmd = voice(&["replay", "t.jsonl", "--repeat", "0"]).unwrap();
        assert_eq!(run(&cfg, cmd).await, 2);
        match voice(&[
            "replay",
            "t.jsonl",
            "--arms",
            "workers=2,3 flight=1,3",
            "--repeat",
            "3",
            "--baseline",
            "workers=3,flight=3",
        ])
        .unwrap()
        {
            VoiceCommand::Replay {
                arms,
                repeat,
                baseline,
                ..
            } => {
                assert_eq!(arms.as_deref(), Some("workers=2,3 flight=1,3"));
                assert_eq!(repeat, Some(3));
                assert_eq!(baseline.as_deref(), Some("workers=3,flight=3"));
            }
            other => panic!("{other:?}"),
        }
        // No compare flag: the plain one-row replay.
        match voice(&["replay", "t.jsonl"]).unwrap() {
            VoiceCommand::Replay {
                arms,
                repeat,
                baseline,
                ..
            } => assert_eq!(
                compare::plan(arms.as_deref(), repeat, baseline.as_deref()),
                Ok(None)
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_default_wav_lands_under_data_voice() {
        let p = default_wav(Path::new("data"));
        assert!(p.starts_with(Path::new("data").join("voice")));
        assert_eq!(p.extension().unwrap(), "wav");
    }
}
