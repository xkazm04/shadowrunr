//! `shadowrunr prep ...`: stream prep on the command line (docs/guide/cli.md, "Stream prep").
//! Every command works on the store directly, like `shadowrunr case`; no daemon is needed.

use std::process::ExitCode;

use shadowrunr_reconcile::{ReconcileConfig, Reconciler};
use shadowrunr_types::prep::{BitKind, PrepCheckState, StreamPrep};
use shadowrunr_types::{Engine, ErrorCode};

use super::revise::Revision;
use super::store::status_name;
use crate::config::Config;
use crate::engine::EngineHandle;
use crate::rpc::RpcError;
use crate::runs;

/// `shadowrunr prep` exit codes (also in docs/guide/cli.md).
pub const PREP_EXIT_CODES: &str = "Exit codes:
  0  done (generate, revise: staged, every check passed)
  1  refused or failed (a failed check holds the approval, a prep in the wrong state, the
     engine or the store failed, the prep does not exist)
  2  usage error (a malformed id, an empty signature, a note naming no line, nothing to
     revise)
  6  generate or revise staged a prep with a failed check (shown; it cannot be approved)";

/// Exit code: a staged prep failed a check.
pub const EXIT_CHECK_FAILED: u8 = 6;

#[derive(Debug, clap::Subcommand)]
pub enum PrepCommand {
    /// Write tonight's prep with the planner (one engine call) from the current plan, or from
    /// a Reconcile report's chosen plan; check it and stage it for the owner.
    #[command(after_help = PREP_EXIT_CODES)]
    Generate {
        /// Write from this Reconcile report's chosen plan instead of the current plan.
        #[arg(long = "report", value_name = "REPORT_ID")]
        report: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Answer a staged prep's failed checks, and your notes, with an edit plan (one engine
    /// call): only the lines a finding or a note names change, the result is staged as a new
    /// prep and checked again. The base is never changed.
    #[command(after_help = PREP_EXIT_CODES)]
    Revise {
        id: String,
        /// `<line>: <what to change>`, e.g. `cb-02: too flat`; makes that line a target even
        /// when it passes. Repeatable.
        #[arg(long = "note", value_name = "NOTE")]
        notes: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Stored preps, newest first.
    #[command(after_help = PREP_EXIT_CODES)]
    List {
        #[arg(long)]
        json: bool,
    },
    /// One prep, all of its material and its checks.
    #[command(after_help = PREP_EXIT_CODES)]
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// The owner approves a staged prep. Refused while any check fails (no override).
    #[command(after_help = PREP_EXIT_CODES)]
    Approve {
        id: String,
        /// Who approves.
        #[arg(long)]
        signature: String,
        #[arg(long)]
        json: bool,
    },
    /// Retire a prep: no stream uses it again.
    #[command(after_help = PREP_EXIT_CODES)]
    Retire {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

/// 2 for a usage error, 1 for anything else.
pub fn exit_for(e: &RpcError) -> u8 {
    match e.code {
        ErrorCode::InvalidParams => 2,
        _ => 1,
    }
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

fn kind_name(k: BitKind) -> &'static str {
    match k {
        BitKind::Premise => "premise",
        BitKind::Story => "story",
        BitKind::Callback => "callback",
    }
}

fn header(p: &StreamPrep) -> String {
    let mut s = format!(
        "{}  {}  {}{}\n",
        p.prep_id,
        status_name(p.status),
        p.character,
        if p.fictional { "  (fictional)" } else { "" }
    );
    if let Some(r) = &p.plan_report {
        s.push_str(&format!("plan: report {r}\n"));
    }
    if let (Some(by), Some(at)) = (&p.approved_by, p.approved_ms) {
        s.push_str(&format!(
            "approved by {by} at {}\n",
            shadowrunr_segments::cases::utc_iso(at)
        ));
    }
    s
}

fn failed_checks(p: &StreamPrep) -> String {
    let failed: Vec<String> = p
        .checks
        .iter()
        .filter(|c| c.state == PrepCheckState::Fail)
        .map(|c| format!("  FAIL {}: {}\n", c.id, c.detail))
        .collect();
    if failed.is_empty() {
        format!("checks: all {} passed\n", p.checks.len())
    } else {
        format!(
            "checks: {} of {} FAILED (it cannot be approved)\n{}",
            failed.len(),
            p.checks.len(),
            failed.concat()
        )
    }
}

/// `prep generate`: the title, the cold open, the bits per segment and the failed checks.
pub fn summary_text(p: &StreamPrep) -> String {
    let mut s = header(p);
    s.push_str(&format!("cold open: {}\n", p.cold_open.title));
    for (i, b) in p.cold_open.beats.iter().enumerate() {
        s.push_str(&format!("  {}. {b}\n", i + 1));
    }
    s.push_str(&format!("  -> {}\n", p.cold_open.handoff));
    let mut segs = Vec::new();
    for b in &p.bits {
        if !segs.contains(&b.segment) {
            segs.push(b.segment);
        }
    }
    for seg in segs {
        let count = |k| {
            p.bits
                .iter()
                .filter(|b| b.segment == seg && b.kind == k)
                .count()
        };
        s.push_str(&format!(
            "{}: {} premises, {} stories, {} callbacks\n",
            wire(&seg),
            count(BitKind::Premise),
            count(BitKind::Story),
            count(BitKind::Callback)
        ));
    }
    s.push_str(&format!("connectors: {}\n", p.connectors.len()));
    s.push_str(&failed_checks(p));
    s
}

/// A revised prep's lineage: the base, each applied operation and each refused one.
pub fn revision_text(r: &Revision) -> String {
    let mut s = format!("revised from {}: {} ops\n", r.base, r.ops.len());
    for n in &r.notes {
        s.push_str(&format!("  note {}: {}\n", n.target, n.text));
    }
    for o in &r.ops {
        s.push_str(&format!("  {} {}: {}\n", o.op, o.target, o.why));
    }
    for x in &r.refused_ops {
        s.push_str(&format!(
            "  refused {} {}: {}\n",
            x.op.op, x.op.target, x.reason
        ));
    }
    s
}

/// `prep show`: the whole material and every check.
pub fn show_text(p: &StreamPrep) -> String {
    show_text_with(p, None)
}

/// `prep show`, with the lineage of a revised prep under its header.
pub fn show_text_with(p: &StreamPrep, rev: Option<&Revision>) -> String {
    let mut s = header(p);
    if let Some(r) = rev {
        s.push_str(&revision_text(r));
    }
    s.push_str(&format!("\ncold open: {}\n", p.cold_open.title));
    for (i, b) in p.cold_open.beats.iter().enumerate() {
        s.push_str(&format!("  {}. {b}\n", i + 1));
    }
    s.push_str(&format!("  handoff: {}\n", p.cold_open.handoff));
    s.push_str("\nbits:\n");
    for b in &p.bits {
        s.push_str(&format!(
            "  {} [{} {}]\n",
            b.id,
            wire(&b.segment),
            kind_name(b.kind)
        ));
        for l in &b.lines {
            s.push_str(&format!("    {l}\n"));
        }
        if let Some(pay) = &b.payoff {
            s.push_str(&format!("    payoff: {pay}\n"));
        }
    }
    s.push_str("\nconnectors:\n");
    for c in &p.connectors {
        s.push_str(&format!("  {} [{}] {}\n", c.id, wire(&c.kind), c.line));
    }
    s.push_str("\nchecks:\n");
    for c in &p.checks {
        let state = match c.state {
            PrepCheckState::Pass => "pass",
            PrepCheckState::Fail => "FAIL",
        };
        s.push_str(&format!("  {:<14} {:<5} {}\n", c.id, state, c.detail));
    }
    s
}

fn reconciler(cfg: &Config, engine: std::sync::Arc<dyn Engine>) -> Reconciler {
    Reconciler::new(
        ReconcileConfig {
            data_dir: cfg.paths.data.clone(),
            runs_root: cfg.paths.sim_runs(),
            characters_root: Some(cfg.paths.characters.clone()),
        },
        engine,
    )
}

/// `prep generate` on `engine` (the live fleet from [`run`]; a fake in tests).
pub async fn generate_cmd(
    cfg: &Config,
    engine: std::sync::Arc<dyn Engine>,
    report: Option<&str>,
    json: bool,
) -> u8 {
    let res = async {
        let plan = super::plan_for(&reconciler(cfg, engine.clone()), report)?;
        super::generate_with(
            cfg,
            engine.as_ref(),
            &cfg.sim.character,
            plan,
            runs::now_ms(),
        )
        .await
    }
    .await;
    match res {
        Ok(p) => {
            if json {
                print_json(&p);
            } else {
                print!("{}", summary_text(&p));
            }
            if super::checks::failed(&p.checks).is_empty() {
                0
            } else {
                EXIT_CHECK_FAILED
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            exit_for(&e)
        }
    }
}

/// `prep revise` on `engine` (the live fleet from [`run`]; a fake in tests).
pub async fn revise_cmd(
    cfg: &Config,
    engine: std::sync::Arc<dyn Engine>,
    id: &str,
    notes: &[String],
    json: bool,
) -> u8 {
    match super::revise_with(cfg, engine.as_ref(), id, notes, runs::now_ms()).await {
        Ok((p, rev)) => {
            if json {
                print_json(&serde_json::json!({ "prep": p, "revision": rev }));
            } else {
                print!("{}{}", summary_text(&p), revision_text(&rev));
            }
            if super::checks::failed(&p.checks).is_empty() {
                0
            } else {
                EXIT_CHECK_FAILED
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            exit_for(&e)
        }
    }
}

/// Every command but `generate` and `revise`, on the store.
pub fn store_cmd(cfg: &Config, cmd: PrepCommand) -> u8 {
    let store = super::store(cfg);
    let res: Result<(), RpcError> = (|| {
        match cmd {
            PrepCommand::Generate { .. } => {
                return Err(RpcError::internal("generate runs through generate_cmd"))
            }
            PrepCommand::Revise { .. } => {
                return Err(RpcError::internal("revise runs through revise_cmd"))
            }
            PrepCommand::List { json } => {
                let rows = store.list()?;
                if json {
                    print_json(&rows);
                } else if rows.is_empty() {
                    println!("no stream prep yet: `shadowrunr prep generate` writes one");
                } else {
                    for r in &rows {
                        println!(
                            "{}  {:<8}  {:>3} bits  {}  {}",
                            r.prep_id,
                            status_name(r.status),
                            r.bits,
                            if r.failed_checks > 0 {
                                format!("{} FAILED", r.failed_checks)
                            } else {
                                "checks ok".to_string()
                            },
                            r.title
                        );
                    }
                }
            }
            PrepCommand::Show { id, json } => {
                let p = store.load(&id)?;
                if json {
                    print_json(&p);
                } else {
                    let rev = store.revision(&id)?;
                    print!("{}", show_text_with(&p, rev.as_ref()));
                }
            }
            PrepCommand::Approve {
                id,
                signature,
                json,
            } => {
                let p = super::approve_in(cfg, &id, &signature, runs::now_ms())?;
                if json {
                    print_json(&p);
                } else {
                    println!("{} approved by {signature}", p.prep_id);
                }
            }
            PrepCommand::Retire { id, json } => {
                let p = store.retire(&id)?;
                if json {
                    print_json(&p);
                } else {
                    println!("{} retired", p.prep_id);
                }
            }
        }
        Ok(())
    })();
    match res {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            exit_for(&e)
        }
    }
}

pub async fn run(cfg: &Config, cmd: PrepCommand) -> ExitCode {
    let code = match cmd {
        PrepCommand::Generate { report, json } => {
            let engine = EngineHandle::build(cfg, false);
            generate_cmd(cfg, engine.engine, report.as_deref(), json).await
        }
        PrepCommand::Revise { id, notes, json } => {
            let engine = EngineHandle::build(cfg, false);
            revise_cmd(cfg, engine.engine, &id, &notes, json).await
        }
        other => store_cmd(cfg, other),
    };
    ExitCode::from(code)
}
