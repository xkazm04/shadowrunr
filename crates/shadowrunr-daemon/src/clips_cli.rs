//! `shadowrunr clips ...`: the clip factory on the command line (docs/guide/cli.md).

use std::path::PathBuf;
use std::process::ExitCode;

use shadowrunr_clips::clock;
use shadowrunr_types::clips::{
    ClipCandidate, ClipPlatform, ClipScanSource, ClipStatus, ClipSummary, ClipsListParams,
};

use crate::clips::{input_from_dir, inputs, options, render};
use crate::config::Config;
use crate::rpc::RpcError;

#[derive(Debug, clap::Subcommand)]
pub enum ClipsCommand {
    /// Score a transcript's moments and draft its best five candidates (pending the owner).
    Scan {
        /// A run directory (any directory with a transcript.jsonl, read-only), a run id or
        /// `<tournament_id>/<run_id>`.
        #[arg(
            long,
            conflicts_with = "tournament",
            required_unless_present = "tournament"
        )]
        run: Option<String>,
        /// Every run of a tournament under <runs>/sim/ (engine_failed runs are left out).
        #[arg(long)]
        tournament: Option<String>,
        /// Skip the model's hook read (its weight is redistributed; reported unmeasured).
        #[arg(long)]
        no_hook: bool,
        /// Use the scripted engine (its replies are not hook reads, so the hook is unmeasured).
        #[arg(long)]
        scripted: bool,
        #[arg(long)]
        json: bool,
    },
    /// Stored candidates, best first.
    List {
        /// rendered_draft | pending_owner_gate | approved_for_upload | published | rejected.
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        tournament: Option<String>,
        #[arg(long)]
        run: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// One candidate: window, signals, gate checklist, captions, render.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Re-render a draft. A recorded run is cut from its recording with ffmpeg: a 16:9 trim
    /// (cut-16x9.mp4) and a 9:16 reframe around the face with the lines burnt in
    /// (cut-9x16.mp4). Otherwise the stage re-draws it: a 1080x1920 MP4 when node, ffmpeg and
    /// a headless Chromium are available, else a poster frame plus caption tracks.
    Render {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// The owner's approval, recorded with the signature. Refused when a check failed; an
    /// unmeasured check needs --override with the reason.
    Approve {
        id: String,
        /// Who approves.
        #[arg(long)]
        signature: String,
        /// Why an unmeasured check is acceptable; kept beside the hold reasons it overrides.
        #[arg(long = "override", value_name = "REASON")]
        override_reason: Option<String>,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Reject a candidate, with the reason.
    Reject {
        id: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        json: bool,
    },
    /// DRY RUN: write the exact upload requests (YouTube, TikTok, Twitch) without sending
    /// anything. Refused unless the owner approved the candidate.
    Publish {
        id: String,
        /// youtube | tiktok | twitch (repeatable; default all).
        #[arg(long = "platform", value_name = "PLATFORM")]
        platforms: Vec<String>,
        #[arg(long)]
        json: bool,
    },
}

fn print_json<T: serde::Serialize>(v: &T) {
    match serde_json::to_string_pretty(v) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("error: {e}"),
    }
}

fn wire<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

fn parse_wire<T: serde::de::DeserializeOwned>(kind: &str, s: &str) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::String(s.to_ascii_lowercase()))
        .map_err(|_| format!("unknown {kind} {s:?}"))
}

fn summary_line(s: &ClipSummary) -> String {
    format!(
        "{:.3}  #{}  {:<6}  {:<7}  {:>5.1}s  {:<18}  {}{:<6}  {}  {}",
        s.score,
        s.rank,
        wire(&s.lane),
        wire(&s.segment),
        s.duration_s,
        wire(&s.status),
        if s.held { "HELD " } else { "" },
        s.render.as_deref().unwrap_or("-"),
        s.id,
        s.title
    )
}

fn unmeasured_or(v: Option<f64>) -> String {
    v.map_or_else(|| "unmeasured".to_string(), |v| format!("{v:.3}"))
}

fn show(c: &ClipCandidate) {
    let w = &c.window;
    println!(
        "{}  {}  score {:.3}  rank #{}  lane {}",
        c.id,
        wire(&c.status),
        c.score,
        c.rank,
        wire(&c.lane)
    );
    println!("title: {}", c.title);
    println!(
        "source: {} lines {}-{} ({})",
        c.source.run_id, c.source.line_from, c.source.line_to, c.source.transcript
    );
    println!(
        "window: stream {}-{} (anchor {}, {}-driven), {:.1} s {}, pre-roll {:.1} s, band {}-{} s ({}){}",
        clock(w.start_ms),
        clock(w.end_ms),
        clock(w.anchor_ms),
        w.driver,
        w.duration_s,
        if w.voice_timed {
            "on the voice's real timing"
        } else {
            "spoken (estimated)"
        },
        w.preroll_ms as f64 / 1000.0,
        w.band_s[0],
        w.band_s[1],
        w.style,
        if w.needs_reauthoring { "  NEEDS RE-AUTHORING" } else { "" }
    );
    let s = &c.signals;
    println!(
        "signals: marker {:.2}  chat {} (z {})  audio {}  hook {}",
        s.marker,
        unmeasured_or(s.chat),
        s.chat_z
            .map_or_else(|| "-".to_string(), |z| format!("{z:.2}")),
        unmeasured_or(s.audio),
        unmeasured_or(s.hook)
    );
    if let (Some(w), Some(st)) = (s.audio_db, s.audio_stream_db) {
        println!("audio: window {w:.1} dBFS RMS against the recording's {st:.1} dBFS");
    }
    for (k, why) in &s.unmeasured {
        println!("{k} unmeasured: {why}");
    }
    if let Some(why) = &s.hook_why {
        println!("hook read: {why}");
    }
    let e = &c.scoring.effective;
    println!(
        "weights unmeasured; applied: marker {:.3} chat {:.3} audio {:.3} hook {:.3}; agreeing: {}",
        e.marker,
        e.chat,
        e.audio,
        e.hook,
        if c.scoring.agreeing.is_empty() {
            "none".to_string()
        } else {
            c.scoring.agreeing.join(", ")
        }
    );
    for m in &c.markers {
        println!(
            "marker {} {} {}{}{}",
            clock(m.ts_ms),
            wire(&m.kind),
            m.label,
            m.capped
                .as_deref()
                .map(|r| format!("  [capped: {r}]"))
                .unwrap_or_default(),
            if m.chat_requested {
                "  [chat asked for a clip]"
            } else {
                ""
            }
        );
    }
    if let Some(ev) = &c.evidence {
        println!(
            "evidence: {} ({}){}, badge {}, citation: {}",
            ev.case_id,
            ev.case_title,
            if ev.fictional { " FICTIONAL" } else { "" },
            shadowrunr_clips::render::grade_name(ev.worst_grade),
            if ev.citation.is_empty() {
                "-".to_string()
            } else {
                ev.citation.join(" ")
            }
        );
    }
    println!(
        "gate ({}): {}",
        if c.gate.requires_mandatory_gate {
            "exclusions + mandatory checklist"
        } else {
            "exclusions"
        },
        if c.gate.held { "HELD" } else { "clear" }
    );
    for k in &c.gate.checks {
        println!("  {:<14} {:<20} {}", wire(&k.state), k.id, k.detail);
    }
    if let Some(rec) = &c.recording {
        println!(
            "recording: {} (stream offset {} ms, {}x{} {} fps, av offset +{} ms)",
            rec.file, rec.stream_offset_ms, rec.width, rec.height, rec.fps, rec.av_offset_ms
        );
    }
    if let Some(r) = &c.render {
        println!(
            "render: {} {} {}x{} {:.1} s{}",
            match r.footage {
                Some(shadowrunr_types::clips::ClipFootage::Recording) => {
                    shadowrunr_clips::render::FOOTAGE_RECORDING
                }
                _ => shadowrunr_clips::render::FOOTAGE_REDRAWN,
            },
            r.kind,
            r.width,
            r.height,
            r.duration_s,
            r.fallback_reason
                .as_deref()
                .map(|f| format!(" (poster frame only: {f})"))
                .unwrap_or_default()
        );
    }
    if let Some(f) = &c.feedback {
        let tags: Vec<String> = f.tags.iter().map(wire).collect();
        println!(
            "watched: {}{}",
            if tags.is_empty() {
                "-".to_string()
            } else {
                tags.join(", ")
            },
            f.note
                .as_deref()
                .map(|n| format!(" ({n})"))
                .unwrap_or_default()
        );
    }
    println!("captions:");
    for cap in &c.captions {
        println!(
            "  [{:>5.1}s] {}: {}{}",
            cap.start_ms as f64 / 1000.0,
            cap.speaker,
            cap.text,
            if cap.unvoiced { "  [never heard]" } else { "" }
        );
    }
}

fn scan_inputs(
    cfg: &Config,
    run: Option<String>,
    tournament: Option<String>,
) -> Result<Vec<shadowrunr_clips::RunInput>, RpcError> {
    match (run, tournament) {
        (Some(r), _) => {
            let as_dir = std::env::current_dir()
                .map(|d| d.join(&r))
                .unwrap_or_else(|_| PathBuf::from(&r));
            if as_dir.join("transcript.jsonl").is_file() {
                Ok(vec![input_from_dir(cfg, &as_dir)?])
            } else {
                inputs(
                    cfg,
                    &ClipScanSource::Run {
                        run_id: r,
                        tournament_id: None,
                    },
                )
            }
        }
        (None, Some(t)) => inputs(cfg, &ClipScanSource::Tournament { tournament_id: t }),
        (None, None) => Err(RpcError::invalid(
            "give --run <dir|id> or --tournament <id>",
        )),
    }
}

/// Run one `clips` subcommand.
pub async fn run(cfg: &Config, cmd: ClipsCommand) -> Result<ExitCode, String> {
    let store = cfg.clip_store();
    let msg = |e: RpcError| e.message;
    match cmd {
        ClipsCommand::Scan {
            run,
            tournament,
            no_hook,
            scripted,
            json,
        } => {
            let runs = scan_inputs(cfg, run, tournament).map_err(msg)?;
            let opts = options(cfg, !no_hook).map_err(msg)?;
            let engine = crate::engine::EngineHandle::build(cfg, scripted);
            eprintln!(
                "clips scan: {} run(s), hook {} ({} engine) -> {}",
                runs.len(),
                if no_hook { "off" } else { "on" },
                if engine.is_scripted() {
                    "scripted"
                } else {
                    "live"
                },
                store.root().display()
            );
            let report = shadowrunr_clips::scan(&store, &runs, Some(engine.engine.as_ref()), &opts)
                .await
                .map_err(|e| e.to_string())?;
            if json {
                print_json(&report);
            } else {
                for r in &report.runs {
                    println!(
                        "{}: {} candidate(s), hook {}, markers {} admitted / {} capped",
                        r.run_id, r.candidates, r.hook, r.markers_admitted, r.markers_capped
                    );
                }
                println!(
                    "written {}, replaced {}, kept {} (owner already acted)",
                    report.written, report.replaced, report.kept
                );
                println!("{}", report.weights_note);
                println!("top:");
                for s in &report.top {
                    println!("  {}", summary_line(s));
                }
            }
        }
        ClipsCommand::List {
            status,
            tournament,
            run,
            limit,
            json,
        } => {
            let status: Option<ClipStatus> = status
                .as_deref()
                .map(|s| parse_wire("status", s))
                .transpose()?;
            let page = store
                .page(&ClipsListParams {
                    status,
                    tournament_id: tournament,
                    run_id: run,
                    cursor: None,
                    limit,
                })
                .map_err(|e| e.to_string())?;
            if json {
                print_json(&page);
            } else {
                let counts: Vec<String> = page
                    .counts
                    .iter()
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect();
                println!("{} candidate(s): {}", page.total, counts.join(", "));
                println!("{}", shadowrunr_clips::WEIGHTS_NOTE);
                for s in &page.items {
                    println!("  {}", summary_line(s));
                }
            }
        }
        ClipsCommand::Show { id, json } => {
            let c = store.load(&id).map_err(|e| e.to_string())?;
            if json {
                print_json(&c);
            } else {
                show(&c);
            }
        }
        ClipsCommand::Render { id, json } => {
            let cfg2 = cfg.clone();
            let c = tokio::task::spawn_blocking(move || render(&cfg2, &id))
                .await
                .map_err(|e| e.to_string())?
                .map_err(msg)?;
            if json {
                print_json(&c);
            } else {
                show(&c);
                println!("files: {}", store.dir(&c.id).display());
            }
        }
        ClipsCommand::Approve {
            id,
            signature,
            override_reason,
            note,
            json,
        } => {
            let c = store
                .approve(&id, &signature, override_reason.as_deref(), note.as_deref())
                .map_err(|e| e.to_string())?;
            if json {
                print_json(&c);
            } else {
                println!("{} approved_for_upload, signed {signature}", c.id);
            }
        }
        ClipsCommand::Reject { id, reason, json } => {
            let c = store
                .reject(&id, &reason, None)
                .map_err(|e| e.to_string())?;
            if json {
                print_json(&c);
            } else {
                println!("{} rejected: {reason}", c.id);
            }
        }
        ClipsCommand::Publish {
            id,
            platforms,
            json,
        } => {
            let ps: Vec<ClipPlatform> = platforms
                .iter()
                .map(|p| parse_wire("platform", p))
                .collect::<Result<_, _>>()?;
            let r = shadowrunr_clips::upload::publish_dry_run(
                &store,
                &id,
                (!ps.is_empty()).then_some(ps.as_slice()),
            )
            .map_err(|e| e.to_string())?;
            if json {
                print_json(&r);
            } else {
                println!("DRY RUN: nothing was sent; each request was written instead");
                for q in &r.requests {
                    println!(
                        "  {:<8} {}  applicable {}{}  {}",
                        wire(&q.platform),
                        store.dir(&id).join(&q.file).display(),
                        q.applicable,
                        q.budget_left
                            .map(|b| format!(", {b} left today"))
                            .unwrap_or_default(),
                        q.note
                    );
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
