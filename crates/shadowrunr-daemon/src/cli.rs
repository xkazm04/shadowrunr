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
use shadowrunr_types::review::{ReviewSessionRow, ReviewTagParams, SessionTags};
use shadowrunr_types::sandbox::{
    check_gates, SandboxChaos, SandboxCheckState, SandboxReport, SandboxStartParams,
    SandboxStarted, SandboxState, SandboxStatus,
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
    /// Clip factory: candidates from transcripts, vertical drafts, the owner's publish gate,
    /// dry-run uploads.
    #[command(subcommand)]
    Clips(crate::clips_cli::ClipsCommand),
    /// Real SEG-02A cases: off-stream research, the owner's stream approval, corrections.
    #[command(subcommand)]
    Case(crate::case_cli::CaseCommand),
    /// Personas integration: registration and activity status.
    #[command(subcommand)]
    Personas(PersonasCommand),
    /// Serve the MCP tools (runs, transcripts, reports, pre-flight) on stdin/stdout.
    Mcp,
    /// Streams on the running daemon.
    #[command(subcommand)]
    Stream(StreamCommand),
    /// Session review on the running daemon: line tags for the Review page.
    #[command(subcommand)]
    Review(ReviewCommand),
    /// Stream prep: the planner's cold open, premise deck and connectors, checked and staged
    /// for the owner's approval.
    #[command(subcommand)]
    Prep(crate::prep::cli::PrepCommand),
    /// Malakor's local voice: probe the [voice] setup, speak one line through the sidecar, or
    /// replay a recorded run through it.
    #[command(subcommand)]
    Voice(crate::voice::cli::VoiceCommand),
    /// The OBS recorder: probe OBS without recording (`record check`). A session is recorded
    /// with `stream sandbox --record`.
    #[command(subcommand)]
    Record(crate::record::cli::RecordCommand),
}

#[derive(Debug, Subcommand)]
pub enum ReviewCommand {
    /// Tag every Malakor line of a finished sandbox session (`review.tag` on the running
    /// daemon), or of every session with --all. A session whose tags.json is current is kept
    /// unless --force.
    #[command(after_help = REVIEW_TAG_EXIT_CODES)]
    Tag(ReviewTagArgs),
}

/// `shadowrunr review tag` exit codes (also in docs/guide/cli.md).
pub const REVIEW_TAG_EXIT_CODES: &str = "Exit codes:
  0  every session was tagged (or already was)
  1  tagging a session failed
  2  usage error, or the daemon is not reachable";

#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("which").required(true).args(["session_id", "all"])))]
pub struct ReviewTagArgs {
    /// The session id (sbx-<yyyymmdd>-<hhmmss>-<minutes>m).
    pub session_id: Option<String>,
    /// Every session under <runs>/sandbox.
    #[arg(long)]
    pub all: bool,
    /// Re-tag even when tags.json is current.
    #[arg(long)]
    pub force: bool,
    /// The daemon's port (default: [daemon] port, 7717).
    #[arg(long)]
    pub port: Option<u16>,
}

#[derive(Debug, Subcommand)]
pub enum StreamCommand {
    /// A wall-clock sandbox session on the running daemon (`shadowrunr daemon`): the real
    /// engine against the simulated room, on the stage, gated by a session report.
    #[command(after_help = SANDBOX_EXIT_CODES)]
    Sandbox(SandboxArgs),
    /// Recompute a finished session's watched checks (voiced, face, av_sync, dead_air) from
    /// its run dir offline, and print its checks and gate. No daemon needed.
    #[command(after_help = CHECK_EXIT_CODES)]
    Check(CheckArgs),
}

/// `shadowrunr stream check` exit codes (also in docs/guide/recording.md).
pub const CHECK_EXIT_CODES: &str = "Exit codes:
  0  the session passes its gate
  2  usage error, or the run dir has no readable session.json
  6  the session fails its gate";

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// The session's run dir (<runs>/sandbox/<session id>).
    pub dir: PathBuf,
    /// Write the recomputed checks into the run dir's session.json.
    #[arg(long)]
    pub write: bool,
}

/// `shadowrunr stream sandbox` exit codes (also in docs/guide/cli.md).
pub const SANDBOX_EXIT_CODES: &str = "Exit codes:
  0  the session passed every check
  1  the daemon refused the session (pre-flight red, another session running), OBS is not
     reachable or failed to record (--record), or another error
  2  usage error, or the daemon is not reachable
  6  the session ended and failed its gate (or failed outright)";

fn parse_minutes(s: &str) -> Result<u32, String> {
    s.parse::<u32>()
        .ok()
        .filter(|m| shadowrunr_types::sandbox::SANDBOX_MINUTES.contains(m))
        .ok_or_else(|| format!("minutes must be 5, 10 or 30, got {s:?}"))
}

fn parse_chaos(s: &str) -> Result<SandboxChaos, String> {
    serde_json::from_value(serde_json::Value::String(s.to_ascii_lowercase()))
        .map_err(|_| format!("chaos must be outage or lost, got {s:?}"))
}

#[derive(Debug, Args)]
pub struct SandboxArgs {
    /// Session length: 5, 10 or 30 minutes.
    #[arg(long, value_parser = parse_minutes)]
    pub minutes: u32,
    /// Seed for the simulated room.
    #[arg(long)]
    pub seed: Option<u64>,
    /// Inject an engine failure from half-way: outage (90 s) or lost (to the end; 10 and 30
    /// minute sessions only).
    #[arg(long, value_parser = parse_chaos)]
    pub chaos: Option<SandboxChaos>,
    /// Skip the judge (the `judge` check is then unmeasured, so the gate fails).
    #[arg(long)]
    pub no_judge: bool,
    /// Tone variant (default: the character's active tone).
    #[arg(long)]
    pub tone: Option<String>,
    /// The daemon's port (default: [daemon] port, 7717).
    #[arg(long)]
    pub port: Option<u16>,
    /// Record the session in OBS: the stage through a browser source, picture and voice in one
    /// file, left in the run dir with recording.json. Refuses to start when OBS is not
    /// reachable ([record] in shadowrunr.toml; docs/guide/recording.md).
    #[arg(long)]
    pub record: bool,
}

impl SandboxArgs {
    pub fn params(&self) -> SandboxStartParams {
        SandboxStartParams {
            minutes: self.minutes,
            seed: self.seed,
            chaos: self.chaos,
            judge: !self.no_judge,
            tone: self.tone.clone(),
            record: self.record,
        }
    }
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
    /// A staged real case (<data>/cases/<id>/) for a SEG-02A run, instead of the fictional
    /// demo case. A sim is private: an unapproved case runs and the run is marked.
    #[arg(long, value_name = "CASE_ID")]
    pub case: Option<String>,
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
        Command::Clips(c) => crate::clips_cli::run(&cfg, c).await,
        Command::Case(c) => crate::case_cli::run(&cfg, c).await,
        Command::Personas(PersonasCommand::Status { json }) => {
            personas_status_cmd(&cfg, json).await
        }
        Command::Mcp => crate::integrations::mcp::run_stdio(cfg)
            .await
            .map(|()| ExitCode::SUCCESS),
        Command::Stream(StreamCommand::Sandbox(a)) => {
            Ok(ExitCode::from(sandbox_cmd(&cfg, a).await))
        }
        Command::Stream(StreamCommand::Check(a)) => Ok(ExitCode::from(check_cmd(&cfg, a).await)),
        Command::Review(ReviewCommand::Tag(a)) => Ok(ExitCode::from(review_tag_cmd(&cfg, a).await)),
        Command::Prep(c) => Ok(crate::prep::cli::run(&cfg, c).await),
        Command::Voice(c) => Ok(ExitCode::from(crate::voice::cli::run(&cfg, c).await)),
        Command::Record(c) => Ok(ExitCode::from(crate::record::cli::run(&cfg, c).await)),
    }
}

// ── stream sandbox ──────────────────────────────────────────────────────

/// Exit code: the session ended and failed its gate.
pub const EXIT_GATE_FAIL: u8 = 6;
/// Exit code: a usage error, or no daemon to talk to.
pub const EXIT_USAGE: u8 = 2;

const SANDBOX_POLL: std::time::Duration = std::time::Duration::from_secs(1);
const PROGRESS_EVERY: std::time::Duration = std::time::Duration::from_secs(10);
/// Status polls in a row that may fail before the daemon counts as gone.
const UNREACHABLE_AFTER: u32 = 5;

/// Why a daemon call failed.
#[derive(Debug, Clone, PartialEq)]
pub enum CallError {
    /// No answer (nothing listening, a timeout, a body that is not a daemon reply).
    Unreachable(String),
    /// The daemon answered with an error.
    Rpc { code: String, message: String },
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(m) => write!(f, "daemon not reachable: {m}"),
            Self::Rpc { code, message } => write!(f, "{code}: {message}"),
        }
    }
}

/// A thin `/rpc` client of the daemon on 127.0.0.1.
pub struct DaemonClient {
    http: reqwest::Client,
    url: String,
}

impl DaemonClient {
    pub fn new(port: u16) -> Self {
        Self::with_timeout(port, std::time::Duration::from_secs(120))
    }

    /// A client whose calls may take up to `timeout` (a `review.tag` runs for minutes).
    pub fn with_timeout(port: u16, timeout: std::time::Duration) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .unwrap_or_default(),
            url: format!("http://127.0.0.1:{port}/rpc"),
        }
    }

    pub async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T, CallError> {
        let body = serde_json::json!({ "method": method, "params": params });
        let resp = self
            .http
            .post(&self.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| CallError::Unreachable(e.to_string()))?;
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| CallError::Unreachable(e.to_string()))?;
        rpc_reply(&bytes)
    }
}

/// `{"result"}` as `T`, or `{"error"}` as [`CallError::Rpc`].
pub fn rpc_reply<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, CallError> {
    let v: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| CallError::Unreachable(format!("not a daemon reply: {e}")))?;
    if let Some(err) = v.get("error") {
        return Err(CallError::Rpc {
            code: err["code"].as_str().unwrap_or("internal").to_string(),
            message: err["message"].as_str().unwrap_or("").to_string(),
        });
    }
    serde_json::from_value(v.get("result").cloned().unwrap_or_default())
        .map_err(|e| CallError::Unreachable(format!("unexpected reply: {e}")))
}

/// 2 for a usage error or no daemon, 1 for any other refusal.
pub fn exit_for_error(e: &CallError) -> u8 {
    match e {
        CallError::Unreachable(_) => EXIT_USAGE,
        CallError::Rpc { code, .. } if code == "invalid_params" => EXIT_USAGE,
        CallError::Rpc { .. } => 1,
    }
}

/// 0 when the ended session passed its gate, else 6.
pub fn exit_for_status(st: &SandboxStatus) -> u8 {
    match &st.report {
        Some(r) if r.pass => 0,
        _ => EXIT_GATE_FAIL,
    }
}

fn mmss(s: u32) -> String {
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// `mm:ss/mm:ss · say-gap max X s · errors N · ack seq K/E`.
pub fn progress_line(st: &SandboxStatus) -> String {
    let ack = st
        .stage_ack_seq
        .map_or_else(|| "-".to_string(), |k| k.to_string());
    let line = format!(
        "{}/{} · say-gap max {:.1} s · errors {} · ack seq {ack}/{}",
        mmss(st.elapsed_s),
        mmss(st.planned_s),
        st.max_say_gap_s,
        st.core_errors,
        st.events
    );
    match st.state {
        SandboxState::Closing => format!("{line} · closing"),
        SandboxState::Judging => format!("{line} · judging"),
        _ => line,
    }
}

/// The checks table and the verdict.
pub fn report_table(st: &SandboxStatus) -> String {
    let Some(r) = &st.report else {
        return format!(
            "session {}: {} with no report\n",
            st.session_id,
            wire(&st.state)
        );
    };
    let mut out = format!("{:<12} {:<11} detail\n", "check", "state");
    for c in &r.checks {
        let state = match c.state {
            SandboxCheckState::Pass => "pass",
            SandboxCheckState::Fail => "FAIL",
            SandboxCheckState::Unmeasured => "unmeasured",
        };
        let note = if check_gates(&r.spec, c.id) {
            ""
        } else {
            " (not gating: unrecorded)"
        };
        out.push_str(&format!("{:<12} {:<11} {}{note}\n", c.id, state, c.detail));
    }
    out.push_str(&format!(
        "session {}: {}, {}\n",
        st.session_id,
        wire(&st.state),
        if r.pass { "PASS" } else { "gate FAILED" }
    ));
    out
}

fn sandbox_ended(s: SandboxState) -> bool {
    matches!(
        s,
        SandboxState::Done | SandboxState::Failed | SandboxState::Stopped
    )
}

/// The session is off the air: its last event is out (judging, or ended).
fn sandbox_off_air(s: SandboxState) -> bool {
    !matches!(s, SandboxState::Running | SandboxState::Closing)
}

fn no_daemon(port: u16) {
    eprintln!("no daemon on 127.0.0.1:{port}: start one with `shadowrunr daemon` (or pass --port)");
}

/// `--record`: the daemon answers, then OBS is set up and recording. `Err` is the exit code.
async fn arm_recording(
    cfg: &Config,
    client: &DaemonClient,
    port: u16,
) -> Result<crate::record::cli::Armed, u8> {
    // Any reply (a "no session yet" error included) means the daemon is there.
    if let Err(e @ CallError::Unreachable(_)) = client
        .call::<serde_json::Value>("sandbox.status", serde_json::json!({}))
        .await
    {
        eprintln!("error: {e}");
        no_daemon(port);
        return Err(exit_for_error(&e));
    }
    match crate::record::cli::arm(cfg, port).await {
        Ok(r) => {
            println!("recording: OBS is recording {}", r.stage_url);
            Ok(r)
        }
        Err(e) => {
            eprintln!("error: not recording, so the session does not start: {e}");
            Err(1)
        }
    }
}

/// Move the recording into the session's dir with recording.json.
async fn finish_recording(rec: &mut crate::record::cli::Armed, st: &SandboxStatus, dir: &str) {
    // A session that failed has no report: its clock is the time it was started.
    let started_ms = st
        .report
        .as_ref()
        .map_or(rec.started_wall_ms, |r| r.started_ms);
    match rec.finish(Path::new(dir), started_ms).await {
        Ok(r) => println!(
            "recording: {} (stream offset {} ms)",
            r.file, r.stream_offset_ms
        ),
        Err(e) => eprintln!("recording: {e}"),
    }
}

/// Recompute the watched checks of the session in `dir` and rewrite its session.json.
async fn refresh_watched(cfg: &Config, dir: &Path) -> Result<SandboxReport, String> {
    let (dir, ffmpeg) = (dir.to_path_buf(), cfg.clips.ffmpeg.clone());
    tokio::task::spawn_blocking(move || crate::record::watch::refresh(&dir, &ffmpeg, true))
        .await
        .map_err(|e| e.to_string())?
}

/// `stream check <dir>`: the watched checks recomputed offline, the checks table and the gate.
async fn check_cmd(cfg: &Config, a: CheckArgs) -> u8 {
    let (dir, ffmpeg, write) = (a.dir.clone(), cfg.clips.ffmpeg.clone(), a.write);
    let r =
        tokio::task::spawn_blocking(move || crate::record::watch::refresh(&dir, &ffmpeg, write))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
    let report = match r {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_USAGE;
        }
    };
    let st = SandboxStatus {
        session_id: report.session_id.clone(),
        state: SandboxState::Done,
        elapsed_s: u32::try_from((report.ended_ms - report.started_ms).max(0) / 1000)
            .unwrap_or(u32::MAX),
        planned_s: report.spec.minutes * 60,
        events: 0,
        max_say_gap_s: report.max_say_gap_s,
        core_errors: report.core_errors,
        stage_ack_seq: None,
        report: Some(report),
    };
    print!("{}", report_table(&st));
    if a.write {
        println!("written: {}", a.dir.join("session.json").display());
    }
    exit_for_status(&st)
}

async fn sandbox_cmd(cfg: &Config, a: SandboxArgs) -> u8 {
    let port = a.port.unwrap_or(cfg.daemon.port);
    let client = DaemonClient::new(port);
    let mut rec = None;
    if a.record {
        match arm_recording(cfg, &client, port).await {
            Ok(r) => rec = Some(r),
            Err(code) => return code,
        }
    }
    let started: SandboxStarted = match client
        .call("sandbox.start", serde_json::json!(a.params()))
        .await
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            if matches!(e, CallError::Unreachable(_)) {
                no_daemon(port);
            }
            if let Some(r) = rec {
                r.abort().await;
            }
            return exit_for_error(&e);
        }
    };
    let id = started.session_id.clone();
    println!("session {id}");
    println!("stage: {}", started.stage_url);
    println!("(serve the stage with `pnpm -C apps/stage dev`; Ctrl-C stops the session)");

    // One listener for the whole wait, so a Ctrl-C during a status call is not lost.
    let (sig_tx, mut sig_rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
    tokio::spawn(async move {
        loop {
            let ok = tokio::signal::ctrl_c().await.is_ok();
            if sig_tx.send(ok).is_err() || !ok {
                return;
            }
        }
    });
    let mut stopping = false;
    let mut failures = 0u32;
    let mut last_line = tokio::time::Instant::now();
    loop {
        tokio::select! {
            () = tokio::time::sleep(SANDBOX_POLL) => {}
            Some(ok) = sig_rx.recv() => {
                if !ok || stopping {
                    eprintln!("detached: session {id} keeps running in the daemon");
                    if let Some(r) = rec.as_mut() {
                        if let Err(e) = r.stop().await {
                            eprintln!("recording: could not stop OBS: {e}");
                        }
                    }
                    return 1;
                }
                stopping = true;
                eprintln!("stopping {id}: the host signs off, then the report (Ctrl-C again to detach)");
                if let Err(e) = client
                    .call::<SandboxStatus>("sandbox.stop", serde_json::json!({ "session_id": id }))
                    .await
                {
                    eprintln!("sandbox.stop: {e}");
                }
            }
        }
        let st: SandboxStatus = match client
            .call("sandbox.status", serde_json::json!({ "session_id": id }))
            .await
        {
            Ok(st) => {
                failures = 0;
                st
            }
            Err(e) => {
                failures += 1;
                if failures >= UNREACHABLE_AFTER || matches!(e, CallError::Rpc { .. }) {
                    eprintln!("error: {e}");
                    if let Some(r) = rec.as_mut() {
                        if let Err(e) = r.stop().await {
                            eprintln!("recording: could not stop OBS: {e}");
                        }
                    }
                    return exit_for_error(&e);
                }
                continue;
            }
        };
        if let Some(r) = rec.as_mut().filter(|r| !r.is_stopped()) {
            if sandbox_off_air(st.state) {
                // The end card is on the stage: the voice plays out, a short tail, then the
                // recording stops.
                match r.stop_after_show(Path::new(&started.dir)).await {
                    Ok(_) => println!("recording: stopped"),
                    Err(e) => eprintln!("recording: could not stop OBS: {e}"),
                }
            }
        }
        if sandbox_ended(st.state) {
            let mut st = st;
            if let Some(r) = rec.as_mut() {
                finish_recording(r, &st, &started.dir).await;
                // The recording is in: its watched checks join session.json.
                if st.report.is_some() {
                    match refresh_watched(cfg, Path::new(&started.dir)).await {
                        Ok(report) => st.report = Some(report),
                        Err(e) => eprintln!("watched checks: {e}"),
                    }
                }
            }
            println!("{}", progress_line(&st));
            print!("{}", report_table(&st));
            println!("dir: {}", started.dir);
            return exit_for_status(&st);
        }
        if last_line.elapsed() >= PROGRESS_EVERY {
            last_line = tokio::time::Instant::now();
            println!("{}", progress_line(&st));
        }
    }
}

// ── review tag ──────────────────────────────────────────────────────────

/// How long one `review.tag` call may take (a 30-minute session is about 15 batches).
const REVIEW_TAG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// `<id>: tagged N, untagged M, calls K`. `calls` counts this call's tagger rows in the
/// session's ledger (`?` when the ledger is not readable from here).
pub fn review_tag_line(id: &str, tags: &SessionTags, calls: Option<u32>) -> String {
    let calls = calls.map_or_else(|| "?".to_string(), |c| c.to_string());
    format!(
        "{id}: tagged {}, untagged {}, calls {calls}",
        tags.lines.len(),
        tags.untagged
    )
}

async fn review_tag_cmd(cfg: &Config, a: ReviewTagArgs) -> u8 {
    let port = a.port.unwrap_or(cfg.daemon.port);
    let client = DaemonClient::with_timeout(port, REVIEW_TAG_TIMEOUT);
    let unreachable = |e: &CallError| {
        eprintln!("error: {e}");
        if matches!(e, CallError::Unreachable(_)) {
            eprintln!(
                "no daemon on 127.0.0.1:{port}: start one with `shadowrunr daemon` (or pass --port)"
            );
        }
        exit_for_error(e)
    };
    let ids: Vec<String> = match (&a.session_id, a.all) {
        (Some(id), _) => vec![id.clone()],
        (None, _) => match client
            .call::<Vec<ReviewSessionRow>>("review.sessions", serde_json::json!({}))
            .await
        {
            Ok(rows) => rows.into_iter().rev().map(|r| r.session_id).collect(),
            Err(e) => return unreachable(&e),
        },
    };
    if ids.is_empty() {
        println!("no sessions under {}", cfg.paths.sandbox_runs().display());
        return 0;
    }
    let mut failed = 0usize;
    for id in &ids {
        let since = runs::now_ms();
        let params = ReviewTagParams {
            session_id: id.clone(),
            force: a.force,
        };
        match client
            .call::<SessionTags>("review.tag", serde_json::json!(params))
            .await
        {
            Ok(tags) => {
                let calls = crate::review::rows::calls_since(
                    &cfg.paths.sandbox_runs().join(id),
                    shadowrunr_types::Role::Tagger,
                    since,
                );
                println!("{}", review_tag_line(id, &tags, calls));
            }
            Err(e @ CallError::Unreachable(_)) => return unreachable(&e),
            Err(e) if a.session_id.is_some() => return unreachable(&e).max(1),
            Err(e) => {
                eprintln!("{id}: {e}");
                failed += 1;
            }
        }
    }
    if failed > 0 {
        eprintln!("{failed} of {} sessions failed", ids.len());
        1
    } else {
        0
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
    opts.case = a.case.clone();
    opts.stream_prep = crate::prep::for_sim(cfg, &spec.character);
    eprintln!(
        "prep: {}",
        shadowrunr_sim::prep_label(opts.stream_prep.as_ref())
    );
    warn_unapproved_case(&opts)?;
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
    /// Experiments the matrix turns on (`rate_cap_exemptions`); they are in the hash.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    experiments: Vec<&'static str>,
    /// The staged real case the runs audit (its content hash is in the instrument).
    #[serde(skip_serializing_if = "Option::is_none")]
    case: Option<PlanCase>,
}

#[derive(Serialize)]
struct PlanCase {
    case_id: String,
    content_sha256: String,
    /// The owner has not approved it for a live stream: the runs are private and marked.
    unapproved_real_case: bool,
}

/// Name an unapproved real case on stderr before a run uses it; fail early on a bad case.
fn warn_unapproved_case(opts: &shadowrunr_sim::SimOptions) -> Result<Option<PlanCase>, String> {
    let Some(staged) = opts.staged_case().map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let unapproved = !staged.approved_for_stream();
    if unapproved {
        eprintln!(
            "case {}: a REAL case NOT approved for stream. This private simulation may run it; every run is marked unapproved_real_case and nothing from it may be published.",
            staged.case.id
        );
    }
    Ok(Some(PlanCase {
        case_id: staged.case.id.clone(),
        content_sha256: staged.content_sha256.clone(),
        unapproved_real_case: unapproved,
    }))
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
    sim.stream_prep = crate::prep::for_sim(cfg, &cfg.sim.character);
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
        let effective = shadowrunr_sim::matrix::Matrix::load(&matrix)
            .map(|m| shadowrunr_sim::tournament::effective_options(&m, &opts.sim))
            .map_err(|e| e.to_string())?;
        let experiments = effective.experiments();
        let case = warn_unapproved_case(&effective)?;
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
            experiments,
            case,
        });
        return Ok(ExitCode::SUCCESS);
    }
    if let Ok(m) = shadowrunr_sim::matrix::Matrix::load(&matrix) {
        warn_unapproved_case(&shadowrunr_sim::tournament::effective_options(&m, &sim))?;
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

async fn daemon_cmd(
    mut cfg: Config,
    port: Option<u16>,
    scripted: bool,
) -> Result<ExitCode, String> {
    let port = port.unwrap_or(cfg.daemon.port);
    // The port it serves on, so the stage URLs it hands out point back at it.
    cfg.daemon.port = port;
    let engine = EngineHandle::build(&cfg, scripted);
    let scripted = engine.is_scripted();
    let state = server::AppState::new(cfg, engine);
    crate::integrations::activity::start_for_daemon(&state);
    let voice = crate::voice::start(&state);
    let (addr, serve) = server::bind(state.clone(), port)
        .await
        .map_err(|e| format!("bind 127.0.0.1:{port}: {e}"))?;
    println!(
        "shadowrunr daemon on http://{addr} ({} engine): POST /rpc, GET /events, GET /voice{}",
        if scripted { "scripted" } else { "live" },
        if voice.is_some() { " (voice on)" } else { "" }
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
    // The voice sidecar stops with the daemon: stdin closed, then killed after its grace.
    if let Some(voice) = voice {
        let _ = tokio::time::timeout(SHUTDOWN_GRACE, voice).await;
    }
    state.voice.stop();
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::sandbox::{SandboxCheck, SandboxCheckId, SandboxReport};

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("shadowrunr").chain(args.iter().copied()))
    }

    fn sandbox_args(args: &[&str]) -> SandboxArgs {
        match parse(args).unwrap().command {
            Command::Stream(StreamCommand::Sandbox(a)) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn stream_sandbox_parses_its_flags() {
        let a = sandbox_args(&[
            "stream",
            "sandbox",
            "--minutes",
            "10",
            "--seed",
            "3",
            "--chaos",
            "lost",
            "--no-judge",
            "--tone",
            "theatrical",
            "--port",
            "9000",
        ]);
        assert_eq!(a.port, Some(9000));
        assert_eq!(
            a.params(),
            SandboxStartParams {
                minutes: 10,
                seed: Some(3),
                chaos: Some(SandboxChaos::Lost),
                judge: false,
                tone: Some("theatrical".into()),
                record: false,
            }
        );
        let a = sandbox_args(&["stream", "sandbox", "--minutes", "5"]);
        assert_eq!(
            serde_json::to_string(&a.params()).unwrap(),
            r#"{"minutes":5}"#,
            "absent flags are omitted, judging is the default"
        );
        assert_eq!(a.port, None);
        let a = sandbox_args(&["stream", "sandbox", "--minutes", "30", "--chaos", "OUTAGE"]);
        assert_eq!(a.chaos, Some(SandboxChaos::Outage));
    }

    #[test]
    fn stream_sandbox_usage_errors_exit_2() {
        for bad in [
            &["stream", "sandbox"][..],
            &["stream", "sandbox", "--minutes", "7"],
            &["stream", "sandbox", "--minutes", "five"],
            &["stream", "sandbox", "--minutes", "5", "--chaos", "meteor"],
            &["stream", "sandbox", "--minutes", "5", "--port", "x"],
        ] {
            let e = parse(bad).unwrap_err();
            assert_eq!(e.exit_code(), 2, "{bad:?}: {e}");
        }
    }

    fn status(state: SandboxState, pass: Option<bool>) -> SandboxStatus {
        SandboxStatus {
            session_id: "sbx-20261020-120000-5m".into(),
            state,
            elapsed_s: 125,
            planned_s: 300,
            events: 240,
            max_say_gap_s: 9.25,
            core_errors: 1,
            stage_ack_seq: Some(230),
            report: pass.map(|pass| SandboxReport {
                session_id: "sbx-20261020-120000-5m".into(),
                spec: SandboxStartParams {
                    minutes: 5,
                    seed: None,
                    chaos: None,
                    judge: true,
                    tone: None,
                    record: false,
                },
                started_ms: 0,
                ended_ms: 300_000,
                pass,
                checks: vec![SandboxCheck {
                    id: SandboxCheckId::SayGap,
                    state: if pass {
                        SandboxCheckState::Pass
                    } else {
                        SandboxCheckState::Fail
                    },
                    detail: "max 9.3 s".into(),
                }],
                max_say_gap_s: 9.25,
                say_gap_p95_s: 5.0,
                beat_ms_p50: 800,
                beat_ms_p95: 6_500,
                core_errors: 1,
                stall_lines: 0,
                health_transitions: 0,
                judge: None,
            }),
        }
    }

    #[test]
    fn exit_codes_map_the_gate_and_the_errors() {
        assert_eq!(exit_for_status(&status(SandboxState::Done, Some(true))), 0);
        assert_eq!(
            exit_for_status(&status(SandboxState::Stopped, Some(true))),
            0
        );
        assert_eq!(exit_for_status(&status(SandboxState::Done, Some(false))), 6);
        assert_eq!(exit_for_status(&status(SandboxState::Failed, None)), 6);
        assert_eq!(exit_for_error(&CallError::Unreachable("refused".into())), 2);
        let rpc = |code: &str| CallError::Rpc {
            code: code.into(),
            message: "m".into(),
        };
        assert_eq!(exit_for_error(&rpc("invalid_params")), 2);
        assert_eq!(exit_for_error(&rpc("preflight_red")), 1);
        assert_eq!(exit_for_error(&rpc("conflict")), 1);
    }

    #[test]
    fn replies_progress_and_the_table_read_as_specified() {
        let body = serde_json::json!({ "result": status(SandboxState::Running, None) });
        let ok: SandboxStatus = rpc_reply(body.to_string().as_bytes()).unwrap();
        assert_eq!(ok.events, 240);
        let err = rpc_reply::<SandboxStatus>(br#"{"error":{"code":"conflict","message":"busy"}}"#)
            .unwrap_err();
        assert_eq!(
            err,
            CallError::Rpc {
                code: "conflict".into(),
                message: "busy".into()
            }
        );
        assert!(matches!(
            rpc_reply::<SandboxStatus>(b"<html>"),
            Err(CallError::Unreachable(_))
        ));

        assert_eq!(
            progress_line(&status(SandboxState::Running, None)),
            "02:05/05:00 · say-gap max 9.2 s · errors 1 · ack seq 230/240"
        );
        let mut st = status(SandboxState::Judging, None);
        st.stage_ack_seq = None;
        assert_eq!(
            progress_line(&st),
            "02:05/05:00 · say-gap max 9.2 s · errors 1 · ack seq -/240 · judging"
        );
        let table = report_table(&status(SandboxState::Done, Some(false)));
        assert!(
            table.contains("say_gap      FAIL        max 9.3 s"),
            "{table}"
        );
        assert!(table.contains("gate FAILED"), "{table}");
    }

    /// The command against a daemon that is not there: exit 2.
    #[tokio::test]
    async fn no_daemon_is_a_usage_exit() {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = free.local_addr().unwrap().port();
        drop(free);
        let tmp = tempfile::tempdir().unwrap();
        let cfg = Config::from_file_config(crate::config::FileConfig::default(), tmp.path());
        let a = sandbox_args(&["stream", "sandbox", "--minutes", "5"]);
        let a = SandboxArgs {
            port: Some(port),
            ..a
        };
        assert_eq!(sandbox_cmd(&cfg, a).await, EXIT_USAGE);
    }

    fn review_tag_args(args: &[&str]) -> ReviewTagArgs {
        match parse(args).unwrap().command {
            Command::Review(ReviewCommand::Tag(a)) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn review_tag_takes_one_session_or_all_with_force() {
        let a = review_tag_args(&["review", "tag", "sbx-20261020-120000-5m"]);
        assert_eq!(a.session_id.as_deref(), Some("sbx-20261020-120000-5m"));
        assert!(!a.all && !a.force);
        let a = review_tag_args(&["review", "tag", "--all", "--force", "--port", "9000"]);
        assert!(a.all && a.force && a.session_id.is_none());
        assert_eq!(a.port, Some(9000));
        for bad in [
            &["review", "tag"][..],
            &["review", "tag", "sbx-20261020-120000-5m", "--all"],
            &["review", "tag", "--all", "--port", "x"],
        ] {
            let e = parse(bad).unwrap_err();
            assert_eq!(e.exit_code(), 2, "{bad:?}: {e}");
        }
    }

    /// Against a daemon whose engine tags: single, all and force exit 0; an unknown session
    /// exits 1; a malformed id or no daemon exits 2.
    #[tokio::test]
    async fn review_tag_exit_codes() {
        use crate::review::tests::{write_session, Mode, TagEngine, SID};
        let tmp = tempfile::tempdir().unwrap();
        let cfg = crate::e2e::test_config(tmp.path(), crate::e2e::repo().join("characters"));
        write_session(&cfg.paths.sandbox_runs(), SID, 1_000);
        write_session(&cfg.paths.sandbox_runs(), "sbx-20261021-120000-10m", 2_000);
        let engine = TagEngine::new(Mode::Good);
        let state = server::AppState::new(
            cfg.clone(),
            EngineHandle {
                engine: engine.clone(),
                fleet: None,
            },
        );
        let addr = crate::e2e::start_state(state).await;
        let port: u16 = addr.rsplit(':').next().unwrap().parse().unwrap();
        let with_port = |args: &[&str]| ReviewTagArgs {
            port: Some(port),
            ..review_tag_args(args)
        };
        assert_eq!(
            review_tag_cmd(&cfg, with_port(&["review", "tag", SID])).await,
            0
        );
        assert_eq!(engine.calls(), 1);
        assert_eq!(
            review_tag_cmd(&cfg, with_port(&["review", "tag", "--all"])).await,
            0
        );
        assert_eq!(engine.calls(), 2, "the tagged session is kept");
        assert_eq!(
            review_tag_cmd(&cfg, with_port(&["review", "tag", "--all", "--force"])).await,
            0
        );
        assert_eq!(engine.calls(), 4);
        let unknown = with_port(&["review", "tag", "sbx-19700101-000000-5m"]);
        assert_eq!(review_tag_cmd(&cfg, unknown).await, 1);
        let malformed = with_port(&["review", "tag", "../x"]);
        assert_eq!(review_tag_cmd(&cfg, malformed).await, EXIT_USAGE);

        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let gone = free.local_addr().unwrap().port();
        drop(free);
        let a = ReviewTagArgs {
            port: Some(gone),
            ..review_tag_args(&["review", "tag", "--all"])
        };
        assert_eq!(review_tag_cmd(&cfg, a).await, EXIT_USAGE);
    }

    #[test]
    fn review_tag_prints_tagged_untagged_and_calls() {
        let tags: SessionTags = serde_json::from_value(serde_json::json!({
            "session_id": "s", "tagger_version": 1, "model": "haiku", "created_ms": 0,
            "lines": [{"line": 4, "kind": "joke", "target": "self", "edge": "none"}],
            "untagged": 2
        }))
        .unwrap();
        assert_eq!(
            review_tag_line("s", &tags, Some(3)),
            "s: tagged 1, untagged 2, calls 3"
        );
        assert_eq!(
            review_tag_line("s", &tags, None),
            "s: tagged 1, untagged 2, calls ?"
        );
    }
}
