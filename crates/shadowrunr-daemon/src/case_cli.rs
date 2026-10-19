//! `shadowrunr case ...`: real SEG-02A cases on the command line (docs/guide/cli.md). Research
//! stages a case off-stream; the owner approves it for a live stream; corrections and rights
//! of reply are appended to its log.

use std::process::ExitCode;

use serde::Serialize;
use shadowrunr_segments::cases::{now_ms, CorrectionType};
use shadowrunr_segments::{CaseStore, SourceAllowlist};
use shadowrunr_types::ModelTier;

use crate::casework::{self, ResearchReport, ResearchRequest};
use crate::config::Config;
use crate::engine::EngineHandle;

#[derive(Debug, clap::Subcommand)]
pub enum CaseCommand {
    /// The CALM research pipeline: find, fetch (allowlisted sites only) and extract one real
    /// public-record case, scrub it and stage it under <data>/cases/<id>/ (not approved for
    /// stream). This fetches real pages and spends engine calls.
    Research {
        /// The company, scheme or enforcement case to research.
        #[arg(long)]
        subject: String,
        /// Hexes on the map, at most.
        #[arg(long, default_value_t = 19)]
        max_hexes: usize,
        /// An allowlisted URL to fetch besides what discovery finds (repeatable).
        #[arg(long = "source", value_name = "URL")]
        sources: Vec<String>,
        /// The case id (default: from the codename and today's date).
        #[arg(long)]
        id: Option<String>,
        /// Tier of both research calls (default: the Researcher route's).
        #[arg(long, value_parser = parse_tier)]
        tier: Option<ModelTier>,
        /// Skip discovery: fetch only the --source URLs.
        #[arg(long, requires = "sources")]
        no_discovery: bool,
        #[arg(long)]
        json: bool,
    },
    /// Staged cases with their status and approval.
    List {
        #[arg(long)]
        json: bool,
    },
    /// One case: map counts, every record's source, provenance, scrub hits, refusals and the
    /// corrections log.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// The owner approves the case, as it is now, for a live stream.
    Approve { id: String },
    /// Withdraw a case: it runs nowhere (not even in a simulation) and its approval is void.
    Retract { id: String },
    /// Append to the corrections and right-of-reply log.
    Correct {
        id: String,
        /// factual | reply | downgrade
        #[arg(long = "type", value_parser = parse_correction)]
        entry_type: CorrectionType,
        /// Public note (no contact details).
        #[arg(long)]
        note: String,
        /// The hex it concerns.
        #[arg(long)]
        hex: Option<String>,
    },
}

fn parse_tier(s: &str) -> Result<ModelTier, String> {
    serde_json::from_value(serde_json::Value::String(s.to_ascii_lowercase()))
        .map_err(|_| format!("tier must be haiku, sonnet or opus, got {s:?}"))
}

fn parse_correction(s: &str) -> Result<CorrectionType, String> {
    match s.to_ascii_lowercase().as_str() {
        "factual" | "factual_correction" => Ok(CorrectionType::FactualCorrection),
        "reply" | "right_of_reply" | "right_of_reply_statement" => {
            Ok(CorrectionType::RightOfReplyStatement)
        }
        "downgrade" | "grade_downgrade" => Ok(CorrectionType::GradeDowngrade),
        _ => Err(format!(
            "type must be factual, reply or downgrade, got {s:?}"
        )),
    }
}

fn print_json<T: Serialize>(v: &T) {
    match serde_json::to_string_pretty(v) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("error: {e}"),
    }
}

/// The case store of this config, under its research allowlist.
pub fn store(cfg: &Config) -> Result<CaseStore, String> {
    Ok(CaseStore::with_allowlists(
        &cfg.paths.data,
        SourceAllowlist::builtin().clone(),
        cfg.research.allowlist()?,
    ))
}

pub async fn run(cfg: &Config, cmd: CaseCommand) -> Result<ExitCode, String> {
    let store = store(cfg)?;
    match cmd {
        CaseCommand::Research {
            subject,
            max_hexes,
            sources,
            id,
            tier,
            no_discovery,
            json,
        } => {
            if !(1..=37).contains(&max_hexes) {
                return Err("--max-hexes must be 1..=37".into());
            }
            let engine = EngineHandle::build(cfg, false);
            if engine.is_scripted() {
                return Err(
                    "case research needs the live engine (the scripted engine cannot read the web)"
                        .into(),
                );
            }
            let req = ResearchRequest {
                subject,
                max_hexes,
                seeds: sources,
                case_id: id,
                tier,
                no_discovery,
            };
            let report = casework::research(engine.engine, &store, &cfg.research, &req, &|line| {
                eprintln!("{line}")
            })
            .await?;
            if json {
                print_json(&report);
            } else {
                print_report(&report);
            }
            Ok(if report.staged {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(3)
            })
        }
        CaseCommand::List { json } => {
            let metas = store.list().map_err(|e| e.to_string())?;
            if json {
                print_json(&metas);
            } else if metas.is_empty() {
                println!("no staged cases under {}", store.root().display());
            } else {
                for m in &metas {
                    println!(
                        "{}  {:?}  {}  {}",
                        m.case_id,
                        m.status,
                        if m.owner_approved_for_stream {
                            "approved"
                        } else {
                            "NOT approved for stream"
                        },
                        m.title
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        CaseCommand::Show { id, json } => {
            let staged = store.load(&id).map_err(|e| e.to_string())?;
            let corrections = store.corrections(&id).map_err(|e| e.to_string())?;
            let report =
                casework::report_for(&staged, Some(store.root().join(&id).display().to_string()));
            if json {
                print_json(&serde_json::json!({
                    "meta": staged.meta,
                    "approved_for_stream": staged.approved_for_stream(),
                    "report": report,
                    "corrections": corrections,
                }));
            } else {
                print_report(&report);
                println!(
                    "status: {:?}, {}",
                    staged.meta.status,
                    if staged.approved_for_stream() {
                        format!(
                            "approved for stream {}",
                            staged.meta.approved_at_utc.as_deref().unwrap_or("")
                        )
                    } else {
                        "NOT approved for stream (a live stream refuses it; a sim may run it)"
                            .to_string()
                    }
                );
                println!("corrections log: {} entr(ies)", corrections.len());
                for c in &corrections {
                    println!(
                        "  {} {:?} {} {}",
                        c.entry_id,
                        c.entry_type,
                        c.hex_id.as_deref().unwrap_or("-"),
                        c.public_note
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        CaseCommand::Approve { id } => {
            let meta = store.approve(&id, now_ms()).map_err(|e| e.to_string())?;
            println!(
                "{} approved for stream at {} (content {})",
                meta.case_id,
                meta.approved_at_utc.as_deref().unwrap_or(""),
                &meta.content_sha256[..12]
            );
            Ok(ExitCode::SUCCESS)
        }
        CaseCommand::Retract { id } => {
            let meta = store.retract(&id).map_err(|e| e.to_string())?;
            println!("{} retracted", meta.case_id);
            Ok(ExitCode::SUCCESS)
        }
        CaseCommand::Correct {
            id,
            entry_type,
            note,
            hex,
        } => {
            let e = store
                .append_correction(&id, hex.as_deref(), entry_type, &note, now_ms())
                .map_err(|e| e.to_string())?;
            println!(
                "{} logged ({:?}{}); it blocks clip publishing for the case until cleared",
                e.entry_id,
                e.entry_type,
                e.hex_id
                    .as_deref()
                    .map(|h| format!(", {h}"))
                    .unwrap_or_default()
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn print_report(r: &ResearchReport) {
    if let Some(why) = &r.refused_case {
        println!("NOT STAGED: {why}");
    }
    if let Some(id) = &r.case_id {
        println!("case: {id}");
    }
    if let Some(d) = &r.dir {
        println!("stored: {d}");
    }
    if let (Some(s), Some(c)) = (&r.subject, &r.subject_category) {
        println!("subject: {s} ({c:?})");
    }
    if let Some(p) = &r.primary_public_record_url {
        println!("primary public record: {p}");
    }
    if r.staged {
        println!(
            "hexes: {} (by best grade on file: {})",
            r.hexes,
            r.hexes_by_grade
                .iter()
                .map(|(g, n)| format!("{g} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(
            "records: {}",
            r.records_by_grade
                .iter()
                .map(|(g, n)| format!("{g} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        for (k, act) in r.arc.iter().enumerate() {
            println!("act {}: {}", k + 1, act.join(", "));
        }
        for rec in &r.records {
            println!(
                "  {} {} [{}] {} <{}>{} retrieved {}",
                rec.id,
                rec.hex,
                rec.grade,
                rec.title,
                rec.source,
                rec.via
                    .as_deref()
                    .map(|v| format!(" via {v}"))
                    .unwrap_or_default(),
                rec.retrieved
            );
        }
    }
    if let Some(t) = &r.tension {
        println!("tension: {t}");
    }
    for u in &r.unknowns {
        println!("unknown: {u}");
    }
    println!(
        "sources fetched: {} ({} used), failed: {}",
        r.sources.len(),
        r.sources.iter().filter(|s| s.used).count(),
        r.failed.len()
    );
    for f in &r.failed {
        println!("  failed {}: {}", f.url, f.why);
    }
    println!(
        "pii scrub: {}",
        if r.pii_scrub.is_empty() {
            "no hits".to_string()
        } else {
            r.pii_scrub
                .iter()
                .map(|h| format!("{} {}", h.kind, h.count))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    for x in &r.refused {
        println!("  refused {}: {}", x.what, x.why);
    }
    for c in &r.calls {
        println!("call {c}");
    }
}
