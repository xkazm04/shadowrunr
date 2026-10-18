//! The `shadowrunr` command line. Every command and an example: docs/guide/cli.md.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use shadowrunr_reconcile::{ReconcileConfig, Reconciler, Source};
use shadowrunr_sim::pacing::GateHandle;
use shadowrunr_sim::{
    plan_matrix, run_one_with, run_tournament, summarize, wire, BriefSource, FleetQuota,
    PlanReason, RunAction, TournamentOptions,
};
use shadowrunr_types::{HealthState, ModelTier, SegmentId, SimRunSpec};

use crate::config::Config;
use crate::engine::EngineHandle;
use crate::{preflight, runs, server};

/// How long a graceful shutdown waits for open connections to drain.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Parser)]
#[command(
    name = "shadowrunr",
    version,
    about = "Shadowrunr OS: simulator, pre-flight and daemon"
)]
pub struct Cli {
    /// Config file (default: shadowrunr.toml found upward from the working directory).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run pre-flight checks PF-01..PF-08; exits non-zero if any check is red.
    Preflight {
        /// Print the reports as JSON.
        #[arg(long)]
        json: bool,
        /// Check the scripted engine instead of the live fleet.
        #[arg(long)]
        scripted: bool,
    },
    /// Text simulations.
    #[command(subcommand)]
    Sim(SimCommand),
    /// Reconcile & Plan over a finished tournament.
    Reconcile {
        /// Tournament id (a directory under <runs>/sim/).
        #[arg(long)]
        tournament: String,
        /// Use the scripted engine (the LLM synthesis then fails and only the deterministic
        /// report is stored).
        #[arg(long)]
        scripted: bool,
        /// Print the full report as JSON.
        #[arg(long)]
        json: bool,
        /// Load the tournament and print what Reconcile would analyse and what it leaves out
        /// (engine_failed, judge_pending, moved-aside copies, incomplete) as JSON. Builds no
        /// engine, calls nothing, writes nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Character packages: optional constitution rules.
    #[command(subcommand)]
    Character(CharacterCommand),
    /// Moderation policy: operator-installed term lists.
    #[command(subcommand)]
    Policy(PolicyCommand),
    /// The brain's long-term memory store (<data>/memory).
    #[command(subcommand)]
    Memory(MemoryCommand),
    /// Serve /events (WebSocket) and /rpc on 127.0.0.1.
    Daemon {
        /// Port (default: [daemon] port, 7717).
        #[arg(long)]
        port: Option<u16>,
        /// Serve with the scripted engine.
        #[arg(long)]
        scripted: bool,
    },
    /// Personas integration: registration and activity status.
    #[command(subcommand)]
    Personas(PersonasCommand),
    /// Serve the MCP tools (runs, transcripts, reports, pre-flight) on stdin/stdout.
    Mcp,
}

#[derive(Debug, Subcommand)]
pub enum PersonasCommand {
    /// Is Personas running and this repo its registered project? Exit 3 when Personas is
    /// not running, 1 when the project is not verified. Never prints the door token.
    Status {
        /// Print the status as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum CharacterCommand {
    /// List a character's optional rules; --enable/--disable write the operator's override
    /// (<data>/characters/<id>/rules.json), never the package.
    Rules {
        /// Character package id.
        id: String,
        /// Rule id to enable (repeatable).
        #[arg(long = "enable", value_name = "RULE")]
        enable: Vec<String>,
        /// Rule id to disable (repeatable).
        #[arg(long = "disable", value_name = "RULE")]
        disable: Vec<String>,
        /// Print the list as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum MemoryCommand {
    /// Delete and rebuild <data>/memory/index.sqlite from the markdown truth: the repair
    /// PF-03 names when the index is stale. Pre-flight only verifies; it never rebuilds.
    /// Stop the daemon first (it holds the index open).
    Reindex {
        /// Print the counts as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PolicyCommand {
    /// Download or copy a term list into <policy>/local/<list-id>.txt (git-ignored) with its
    /// provenance in <list-id>.json. Known ids have a default source.
    InstallList {
        /// e.g. awdev1-better-profane-words or surge-ai-profanity-en.
        list_id: String,
        /// URL or local path (default: the known source for the id).
        #[arg(long)]
        source: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum SimCommand {
    /// One simulated stream.
    Run(SimRunArgs),
    /// Run (or resume) a tournament matrix. Pauses on quota instead of burning runs.
    #[command(after_help = TOURNAMENT_EXIT_CODES)]
    Tournament(TournamentArgs),
    /// Write results.json, heatmap.md and report.md for a tournament directory.
    Summarize {
        dir: PathBuf,
        /// Print results.json to stdout.
        #[arg(long)]
        json: bool,
    },
}

/// `shadowrunr sim tournament` exit codes (also in docs/spec/contract.md, WP13 appendix).
pub const TOURNAMENT_EXIT_CODES: &str = "Exit codes:
  0  every planned run is judged, or void for a content reason
  1  error (bad matrix, I/O)
  4  stopped: the total quota pause cap (--max-pause / [sim] max_quota_pause_s) was exceeded
  5  finished with engine_failed or judge_pending runs left (a resume re-runs / re-judges them)";

fn parse_tier(s: &str) -> Result<ModelTier, String> {
    serde_json::from_value(serde_json::Value::String(s.to_ascii_lowercase()))
        .map_err(|_| format!("tier must be haiku, sonnet or opus, got {s:?}"))
}

fn parse_segment(s: &str) -> Result<SegmentId, String> {
    let seg: SegmentId = serde_json::from_value(serde_json::Value::String(s.to_ascii_uppercase()))
        .map_err(|_| format!("segment must be SEG-01 or SEG-02A, got {s:?}"))?;
    if shadowrunr_segments::segment_is_descoped(seg) {
        Err(format!("segment {s} is descoped"))
    } else {
        Ok(seg)
    }
}

#[derive(Debug, Args)]
pub struct SimRunArgs {
    /// Character package id (default: [sim] character).
    #[arg(long)]
    pub character: Option<String>,
    /// Tone variant (default: [sim] tone).
    #[arg(long)]
    pub tone: Option<String>,
    /// Core-thread tier: haiku | sonnet | opus.
    #[arg(long, value_parser = parse_tier)]
    pub tier: Option<ModelTier>,
    /// SEG-01 | SEG-02A.
    #[arg(long, value_parser = parse_segment)]
    pub segment: Option<SegmentId>,
    /// Audience size (3, 30, 300 are calibrated).
    #[arg(long)]
    pub audience: Option<u32>,
    #[arg(long)]
    pub seed: Option<u64>,
    /// Simulated stream length in seconds.
    #[arg(long)]
    pub duration: Option<u32>,
    /// Population id (default: by audience size from [sim] populations).
    #[arg(long)]
    pub population: Option<String>,
    /// Judge the run after simulating it.
    #[arg(long)]
    pub judge: bool,
    /// Use the deterministic, keyless scripted engine.
    #[arg(long)]
    pub scripted: bool,
    /// Directory the run lands in, as <out>/<run_id>/ (default: <runs>/sim/adhoc).
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// The Lord's brief the brain receives: `current` (the chosen plan plus the active
    /// directives in <data>) or a StreamBrief JSON file. Default: none.
    #[arg(long, value_name = "current|PATH")]
    pub brief: Option<String>,
}

#[derive(Debug, Args)]
pub struct TournamentArgs {
    /// Matrix file, e.g. sim/matrices/pilot.toml.
    #[arg(long, conflicts_with = "matrix_path")]
    pub matrix: Option<PathBuf>,
    /// The matrix file, positionally.
    #[arg(value_name = "MATRIX")]
    pub matrix_path: Option<PathBuf>,
    /// Root directory; the tournament lands in <out>/<matrix_id>-<hash10>/ (default:
    /// <runs>/sim).
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Runs in flight (overrides the matrix).
    #[arg(long)]
    pub concurrency: Option<usize>,
    /// Stop after starting this many runs (staged batches).
    #[arg(long)]
    pub max_runs: Option<usize>,
    /// Simulate without judging.
    #[arg(long)]
    pub no_judge: bool,
    /// Use the deterministic, keyless scripted engine.
    #[arg(long)]
    pub scripted: bool,
    /// Print the action for every run (skip, rejudge, run) with its reason, and the counts;
    /// execute nothing, spend no quota, write nothing.
    #[arg(long)]
    pub plan_only: bool,
    /// Total quota pause cap in seconds (default: [sim] max_quota_pause_s, 28800 = 8 h).
    /// Past it the tournament stops with its summary and exit code 4.
    #[arg(long, value_name = "SECONDS")]
    pub max_pause: Option<u64>,
    /// The Lord's brief for every run: `current` or a StreamBrief JSON file. Overrides the
    /// matrix's `brief`. Default: the matrix's, else none.
    #[arg(long, value_name = "current|PATH")]
    pub brief: Option<String>,
}

/// A brief source with its file path taken from the working directory.
fn brief_source(s: &str) -> BriefSource {
    match BriefSource::parse(s) {
        BriefSource::File(p) => BriefSource::File(from_cwd(&p)),
        other => other,
    }
}

/// Read the brief once (so every run of a tournament gets the same one) and say what it is.
/// Returns the resolved source and the brief's digest when there is a brief.
fn resolve_brief(cfg: &Config, src: BriefSource) -> Result<(BriefSource, Option<String>), String> {
    let resolved = src.resolved(&cfg.paths.data).map_err(|e| e.to_string())?;
    let digest = match &resolved {
        BriefSource::Inline(b) => {
            eprintln!(
                "brief: {} (digest {})",
                shadowrunr_brain::brief_summary(Some(b)),
                &shadowrunr_sim::instrument::brief_digest(b)[..12]
            );
            Some(shadowrunr_sim::instrument::brief_digest(b))
        }
        _ => None,
    };
    Ok((resolved, digest))
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn from_cwd(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd().join(p)
    }
}

fn print_json<T: Serialize>(v: &T) {
    match serde_json::to_string_pretty(v) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("error: {e}"),
    }
}

/// Entry point for the binary.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let cfg = match Config::load(cli.config.as_deref(), &cwd()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: tokio runtime: {e}");
            return ExitCode::from(2);
        }
    };
    match rt.block_on(run(cli.command, cfg)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

pub async fn run(cmd: Command, cfg: Config) -> Result<ExitCode, String> {
    match cmd {
        Command::Preflight { json, scripted } => preflight_cmd(&cfg, json, scripted).await,
        Command::Sim(SimCommand::Run(a)) => sim_run(&cfg, a).await,
        Command::Sim(SimCommand::Tournament(a)) => sim_tournament(&cfg, a).await,
        Command::Sim(SimCommand::Summarize { dir, json }) => sim_summarize(&from_cwd(&dir), json),
        Command::Reconcile {
            tournament,
            dry_run: true,
            ..
        } => reconcile_dry_run(&cfg, &tournament),
        Command::Reconcile {
            tournament,
            scripted,
            json,
            dry_run: false,
        } => reconcile_cmd(&cfg, tournament, scripted, json).await,
        Command::Character(CharacterCommand::Rules {
            id,
            enable,
            disable,
            json,
        }) => character_rules_cmd(&cfg, &id, &enable, &disable, json),
        Command::Policy(PolicyCommand::InstallList { list_id, source }) => {
            policy_install_cmd(&cfg, &list_id, source.as_deref()).await
        }
        Command::Memory(MemoryCommand::Reindex { json }) => memory_reindex_cmd(&cfg, json),
        Command::Daemon { port, scripted } => daemon_cmd(cfg, port, scripted).await,
        Command::Personas(PersonasCommand::Status { json }) => {
            personas_status_cmd(&cfg, json).await
        }
        Command::Mcp => crate::integrations::mcp::run_stdio(cfg)
            .await
            .map(|()| ExitCode::SUCCESS),
    }
}

async fn personas_status_cmd(cfg: &Config, json: bool) -> Result<ExitCode, String> {
    use crate::integrations::status;
    let r = status::status(cfg, std::time::Duration::from_secs(3)).await;
    if json {
        print_json(&r);
    } else {
        println!("{}", status::render(&r));
    }
    Ok(ExitCode::from(r.exit))
}

fn memory_reindex_cmd(cfg: &Config, json: bool) -> Result<ExitCode, String> {
    let dir = cfg.paths.memory();
    if !dir.is_dir() {
        return Err(format!("{}: no memory store to reindex", dir.display()));
    }
    let r = shadowrunr_brain::rebuild_index(&dir)
        .map_err(|e| format!("reindex {}: {e}", dir.display()))?;
    if json {
        print_json(&serde_json::json!({
            "memory_dir": dir.display().to_string(),
            "episodes": r.episodes,
            "facts": r.facts,
            "tombstones": r.tombstones,
            "procedurals": r.procedurals,
            "cycles": r.cycles,
        }));
    } else {
        println!(
            "{}: index rebuilt: {} episodes, {} facts, {} tombstones, {} procedurals, {} cycles",
            dir.display(),
            r.episodes,
            r.facts,
            r.tombstones,
            r.procedurals,
            r.cycles
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn character_rules_cmd(
    cfg: &Config,
    id: &str,
    enable: &[String],
    disable: &[String],
    json: bool,
) -> Result<ExitCode, String> {
    if let Some(both) = enable.iter().find(|r| disable.contains(r)) {
        return Err(format!("rule {both} is both enabled and disabled"));
    }
    let mut rules = crate::rpc::character_rules(cfg, id).map_err(|e| e.message)?;
    for (rule, on) in enable
        .iter()
        .map(|r| (r, true))
        .chain(disable.iter().map(|r| (r, false)))
    {
        rules = crate::rpc::character_rules_set(cfg, id, rule, on).map_err(|e| e.message)?;
    }
    if json {
        print_json(&rules);
    } else if rules.is_empty() {
        println!("{id}: no optional rules");
    } else {
        for r in &rules {
            println!(
                "{}  {}  {}{}",
                if r.enabled { "on " } else { "off" },
                r.id,
                r.title,
                if r.enabled == r.default {
                    String::new()
                } else {
                    format!(
                        "  (operator override; default {})",
                        if r.default { "on" } else { "off" }
                    )
                }
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn policy_install_cmd(
    cfg: &Config,
    list_id: &str,
    source: Option<&str>,
) -> Result<ExitCode, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let meta = crate::policy_install::install(&cfg.paths.policy, list_id, source, now).await?;
    let dir = crate::policy_install::local_dir(&cfg.paths.policy);
    println!(
        "installed {} ({} terms) from {}",
        meta.list_id, meta.terms, meta.source
    );
    println!("licence: {}", meta.licence);
    println!("sha256: {}", meta.sha256);
    println!(
        "wrote {} and {}",
        dir.join(format!("{list_id}.txt")).display(),
        dir.join(format!("{list_id}.json")).display()
    );
    Ok(ExitCode::SUCCESS)
}

async fn preflight_cmd(cfg: &Config, json: bool, scripted: bool) -> Result<ExitCode, String> {
    let engine = EngineHandle::build(cfg, scripted);
    let reports = preflight::run_all(cfg, &engine).await;
    if json {
        print_json(&reports);
    } else {
        for r in &reports {
            let state = match r.state {
                HealthState::Green => "green",
                HealthState::Amber => "amber",
                HealthState::Red => "RED  ",
            };
            println!("{}  {state}  {}", r.check, r.detail);
        }
    }
    Ok(if preflight::any_red(&reports) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

async fn sim_run(cfg: &Config, a: SimRunArgs) -> Result<ExitCode, String> {
    let d = &cfg.sim;
    let character = a.character.unwrap_or_else(|| d.character.clone());
    let tone = a.tone.unwrap_or_else(|| d.tone.clone());
    let tier = a.tier.unwrap_or(d.tier);
    let segment = a.segment.unwrap_or(d.segment);
    let audience = a.audience.unwrap_or(d.audience);
    let seed = a.seed.unwrap_or(d.seed);
    let duration_s = a.duration.unwrap_or(d.duration_s);
    for (k, v) in [("character", &character), ("tone", &tone)] {
        runs::check_id(k, v).map_err(|e| e.message)?;
    }
    if shadowrunr_segments::segment_is_descoped(segment) {
        return Err(format!("segment {} is descoped", wire(&segment)));
    }
    let spec = SimRunSpec {
        run_id: shadowrunr_sim::matrix::run_id(&tone, tier, segment, audience, seed),
        character,
        tone_variant: tone,
        core_tier: tier,
        core_effort: d.effort,
        segment,
        audience_size: audience,
        seed,
        duration_s,
        population: a.population.unwrap_or_else(|| d.population_for(audience)),
    };
    let out = a
        .out
        .as_deref()
        .map(from_cwd)
        .unwrap_or_else(|| cfg.paths.sim_runs().join(&d.adhoc_dir));
    let run_dir = out.join(&spec.run_id);
    if let Some(old) = runs::move_aside(&run_dir, &format!("attempt-{}", runs::now_ms()))
        .map_err(|e| format!("{}: {e}", run_dir.display()))?
    {
        eprintln!("previous run moved to {}", old.display());
    }
    let mut opts = cfg.sim_options(a.judge);
    opts.data_dir = cfg.paths.data.clone();
    if let Some(b) = a.brief.as_deref() {
        opts.brief = resolve_brief(cfg, brief_source(b))?.0;
    }
    let engine = EngineHandle::build(cfg, a.scripted);
    eprintln!(
        "sim run {} ({} engine) -> {}",
        spec.run_id,
        if engine.is_scripted() {
            "scripted"
        } else {
            "live"
        },
        run_dir.display()
    );
    let res = run_one_with(spec, out, engine.engine, &opts)
        .await
        .map_err(|e| e.to_string())?;
    let events = shadowrunr_sim::transcript::read(&res.transcript)
        .map(|e| e.len())
        .unwrap_or(0);
    println!("run_dir: {}", run_dir.display());
    println!("transcript: {} events", events);
    match &res.verdict {
        Some(v) => {
            let fails = v
                .rules
                .iter()
                .filter(|r| r.state == shadowrunr_types::CheckState::Fail)
                .count();
            println!(
                "verdict: judge {}, {} rules ({} fail), {} quality facts",
                wire(&v.judge_tier),
                v.rules.len(),
                fails,
                v.quality.len()
            );
        }
        None if a.judge => println!("verdict: void (see run.json judge_error)"),
        None => println!("verdict: not judged"),
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Serialize)]
struct FailedRun {
    run_id: String,
    error: String,
}

#[derive(Serialize)]
struct TournamentReport {
    tournament_id: String,
    dir: String,
    planned: usize,
    skipped: usize,
    executed: usize,
    failed: Vec<FailedRun>,
    runs: u32,
    judged: u32,
    void: u32,
    failed_runs: u32,
    /// Runs left engine_failed / judge_pending: not results, never counted as void.
    engine_failed: usize,
    judge_pending: usize,
    paused_ms: u64,
    pauses: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stopped: Option<shadowrunr_sim::StopReason>,
    exit_code: u8,
    /// sha256 of the brief's canonical JSON, when the runs had one (it is in the hash).
    #[serde(skip_serializing_if = "Option::is_none")]
    brief_digest: Option<String>,
}

#[derive(Serialize)]
struct PlannedRow {
    run_id: String,
    action: RunAction,
    reason: PlanReason,
}

#[derive(Serialize)]
struct PlanReport {
    tournament_id: String,
    dir: String,
    planned: usize,
    skip: usize,
    rejudge: usize,
    run: usize,
    by_reason: std::collections::BTreeMap<String, usize>,
    runs: Vec<PlannedRow>,
    /// sha256 of the brief's canonical JSON, when the runs would have one (WP15).
    #[serde(skip_serializing_if = "Option::is_none")]
    brief_digest: Option<String>,
    /// `dispatch yes, 2 prevent, 1 try, ...`, when there is a brief.
    #[serde(skip_serializing_if = "Option::is_none")]
    brief: Option<String>,
}

async fn sim_tournament(cfg: &Config, a: TournamentArgs) -> Result<ExitCode, String> {
    let given = a.matrix.or(a.matrix_path).ok_or(
        "a matrix is required: shadowrunr sim tournament --matrix sim/matrices/pilot.toml",
    )?;
    let mut matrix = from_cwd(&given);
    if !matrix.is_file() && given.is_relative() && cfg.root.join(&given).is_file() {
        matrix = cfg.root.join(&given);
    }
    if !matrix.is_file() {
        return Err(format!("matrix {} not found", matrix.display()));
    }
    let out = a
        .out
        .as_deref()
        .map(from_cwd)
        .unwrap_or_else(|| cfg.paths.sim_runs());
    let max_pause_ms = a
        .max_pause
        .unwrap_or(cfg.sim.max_quota_pause_s)
        .saturating_mul(1000);
    // The brief: the flag, else the matrix's, read once for every run.
    let declared = match a.brief.as_deref() {
        Some(b) => brief_source(b),
        None => match shadowrunr_sim::matrix::Matrix::load(&matrix)
            .map_err(|e| e.to_string())?
            .brief_source()
        {
            BriefSource::File(p) => BriefSource::File(from_cwd(&p)),
            other => other,
        },
    };
    let (brief, brief_digest) = resolve_brief(cfg, declared)?;
    let mut sim = cfg.sim_options(!a.no_judge);
    sim.data_dir = cfg.paths.data.clone();
    sim.brief = brief;
    if a.plan_only {
        // Reads only: no engine is built, nothing runs, nothing is written.
        let brief_line = match &sim.brief {
            BriefSource::Inline(b) => Some(shadowrunr_brain::brief_summary(Some(b))),
            _ => None,
        };
        let opts = TournamentOptions {
            sim,
            ..TournamentOptions::default()
        };
        let plan = plan_matrix(&matrix, &out, &opts).map_err(|e| e.to_string())?;
        let mut by_reason = std::collections::BTreeMap::new();
        for r in &plan.runs {
            *by_reason.entry(wire(&r.reason)).or_insert(0) += 1;
        }
        print_json(&PlanReport {
            tournament_id: plan.tournament_id.clone(),
            dir: plan.dir.display().to_string(),
            planned: plan.runs.len(),
            skip: plan.count(RunAction::Skip),
            rejudge: plan.count(RunAction::Rejudge),
            run: plan.count(RunAction::Run),
            by_reason,
            runs: plan
                .runs
                .iter()
                .map(|r| PlannedRow {
                    run_id: r.spec.run_id.clone(),
                    action: r.action,
                    reason: r.reason,
                })
                .collect(),
            brief_digest,
            brief: brief_line,
        });
        return Ok(ExitCode::SUCCESS);
    }
    let engine = EngineHandle::build(cfg, a.scripted);
    let quota = engine
        .fleet
        .as_ref()
        .map(|f| GateHandle(std::sync::Arc::new(FleetQuota::new(f.clone()))));
    let opts = TournamentOptions {
        sim,
        concurrency: a.concurrency,
        max_runs: a.max_runs,
        quota,
        clock: None,
        max_pause_ms: Some(max_pause_ms),
    };
    let s = run_tournament(&matrix, &out, engine.engine, &opts)
        .await
        .map_err(|e| e.to_string())?;
    let exit_code = s.exit_code();
    let report = TournamentReport {
        tournament_id: s.tournament_id,
        dir: s.dir.display().to_string(),
        planned: s.planned,
        skipped: s.skipped,
        executed: s.executed,
        failed: s
            .failed
            .into_iter()
            .map(|(run_id, error)| FailedRun { run_id, error })
            .collect(),
        runs: s.results.runs,
        judged: s.results.judged,
        void: s.results.void,
        failed_runs: s.results.failed,
        engine_failed: s.engine_failed,
        judge_pending: s.judge_pending,
        paused_ms: s.paused_ms,
        pauses: s.pauses,
        stopped: s.stop,
        exit_code,
        brief_digest,
    };
    crate::integrations::activity::notify_once(
        cfg,
        crate::integrations::activity::ActivityEvent::tournament_complete(
            &report.tournament_id,
            serde_json::json!({"runs": report.runs, "judged": report.judged, "void": report.void,
                               "failed_runs": report.failed_runs, "dir": report.dir}),
        ),
    )
    .await;
    print_json(&report);
    Ok(ExitCode::from(exit_code))
}

fn sim_summarize(dir: &Path, json: bool) -> Result<ExitCode, String> {
    let res = summarize(dir).map_err(|e| e.to_string())?;
    if json {
        print_json(&res);
    } else {
        println!(
            "{}: {} runs, {} judged, {} void, {} failed, {} cells",
            res.tournament_id,
            res.runs,
            res.judged,
            res.void,
            res.failed,
            res.cells.len()
        );
        for f in ["results.json", "heatmap.md", "report.md"] {
            println!("wrote {}", dir.join(f).display());
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `reconcile --dry-run`: what a Reconcile run would load from `<runs>/sim/<tournament>/`.
#[derive(Debug, Serialize)]
pub struct ReconcileDryRun {
    pub tournament_id: String,
    /// Runs Reconcile would analyse (results only).
    pub loaded: usize,
    /// Of those, runs with a verdict.
    pub judged: usize,
    /// What it would leave out.
    pub excluded: shadowrunr_reconcile::Excluded,
}

/// Reads only: loads the tournament the way `Reconciler::run` does, without an engine.
pub fn reconcile_dry_run_report(cfg: &Config, tournament: &str) -> Result<ReconcileDryRun, String> {
    runs::check_id("tournament", tournament).map_err(|e| e.message)?;
    let (loaded, excluded) = shadowrunr_reconcile::load_source_full(
        &cfg.paths.sim_runs(),
        &Source::Tournament {
            tournament_id: tournament.to_string(),
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(ReconcileDryRun {
        tournament_id: tournament.to_string(),
        loaded: loaded.len(),
        judged: loaded.iter().filter(|r| r.verdict.is_some()).count(),
        excluded,
    })
}

fn reconcile_dry_run(cfg: &Config, tournament: &str) -> Result<ExitCode, String> {
    print_json(&reconcile_dry_run_report(cfg, tournament)?);
    Ok(ExitCode::SUCCESS)
}

async fn reconcile_cmd(
    cfg: &Config,
    tournament: String,
    scripted: bool,
    json: bool,
) -> Result<ExitCode, String> {
    runs::check_id("tournament", &tournament).map_err(|e| e.message)?;
    let engine = EngineHandle::build(cfg, scripted);
    let rec = Reconciler::new(
        ReconcileConfig {
            data_dir: cfg.paths.data.clone(),
            runs_root: cfg.paths.sim_runs(),
            characters_root: Some(cfg.paths.characters.clone()),
        },
        engine.engine,
    );
    let report_id = rec
        .run(Source::Tournament {
            tournament_id: tournament,
        })
        .await
        .map_err(|e| e.to_string())?;
    let report = rec.report(&report_id).map_err(|e| e.to_string())?;
    crate::integrations::activity::notify_once(
        cfg,
        crate::integrations::activity::ActivityEvent::report_ready(
            &report_id,
            serde_json::json!({"summary": report.summary}),
        ),
    )
    .await;
    if json {
        print_json(&report);
    } else {
        let proposals = rec.proposals(&report_id).map(|p| p.len()).unwrap_or(0);
        println!("report_id: {report_id}");
        println!(
            "stored: {}",
            cfg.paths
                .data
                .join("reconcile")
                .join(format!("{report_id}.json"))
                .display()
        );
        println!("plan options: {proposals}");
        println!("{}", report.summary);
    }
    Ok(ExitCode::SUCCESS)
}

async fn daemon_cmd(cfg: Config, port: Option<u16>, scripted: bool) -> Result<ExitCode, String> {
    let port = port.unwrap_or(cfg.daemon.port);
    let engine = EngineHandle::build(&cfg, scripted);
    let scripted = engine.is_scripted();
    let state = server::AppState::new(cfg, engine);
    crate::integrations::activity::start_for_daemon(&state);
    let (addr, serve) = server::bind(state.clone(), port)
        .await
        .map_err(|e| format!("bind 127.0.0.1:{port}: {e}"))?;
    println!(
        "shadowrunr daemon on http://{addr} ({} engine): POST /rpc, GET /events",
        if scripted { "scripted" } else { "live" }
    );
    let serve = tokio::spawn(serve);
    tokio::select! {
        r = tokio::signal::ctrl_c() => r.map_err(|e| format!("ctrl-c handler: {e}"))?,
        _ = state.shutdown_signal() => {}
    }
    // Graceful: stop accepting, close every /events socket with a reason, drain requests.
    eprintln!("shutting down");
    state.shutdown();
    match tokio::time::timeout(SHUTDOWN_GRACE, serve).await {
        Ok(Ok(r)) => r.map_err(|e| e.to_string())?,
        Ok(Err(e)) => return Err(format!("server task: {e}")),
        Err(_) => eprintln!("connections still open after {SHUTDOWN_GRACE:?}; exiting"),
    }
    Ok(ExitCode::SUCCESS)
}
