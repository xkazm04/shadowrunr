//! Reconcile & Plan (WP6). The owner, the Lord of the Darkness, has no live control over the
//! stream. Between streams this crate:
//!
//! - reconciles past (for now simulated) streams: [`analyze`] computes deterministic metrics
//!   from transcripts, verdicts and ledgers, then `Role::Reconciler` synthesises what worked
//!   and what failed plus [`PlanOption`]s, and every evidence ref and diff is verified
//!   mechanically before it is stored;
//! - composes the next stream's plan from the options the owner picks ([`Reconciler::choose`]),
//!   including the in-world `lord_dispatch`;
//! - keeps the owner's prevent/try [`Directive`]s in `<data>/directives.json`.
//!
//! Storage: `<data>/reconcile/<report_id>.json` (report + proposals + plans) and
//! `<data>/next_plan.json` (the latest chosen plan, read at the next stream start).
//!
//! Types and RPC names: docs/spec/contract.md § Daemon protocol.

pub mod directives;
pub mod metrics;
pub mod runs;
pub mod stats;
pub mod synth;
pub mod types;
pub mod verify;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use shadowrunr_types::{Engine, EngineError, SegmentId, TsMs};

pub use directives::DirectiveStore;
pub use metrics::analyze;
pub use runs::{load_source, RunData};
pub use types::*;

#[derive(Debug, thiserror::Error)]
pub enum ReconcileError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("io: {0}")]
    Io(String),
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// The Reconciler's reply was unusable (not JSON, wrong shape, nothing valid left).
    #[error("synthesis: {0}")]
    Synthesis(String),
}

impl ReconcileError {
    /// Error code for the daemon's `{"error": {"code", "message"}}`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::InvalidInput(_) => "invalid_params",
            Self::Io(_) => "io",
            Self::Engine(_) => "engine",
            Self::Synthesis(_) => "synthesis",
        }
    }
}

/// Write pretty JSON via a temp file and a rename, creating parent directories.
pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, v: &T) -> Result<(), ReconcileError> {
    let io = |e: std::io::Error| ReconcileError::Io(format!("{}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
    }
    let text = serde_json::to_string_pretty(v)
        .map_err(|e| ReconcileError::Io(format!("serialise {}: {e}", path.display())))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text + "\n").map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, what: &str) -> Result<T, ReconcileError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ReconcileError::NotFound(what.to_string())
        } else {
            ReconcileError::Io(format!("{}: {e}", path.display()))
        }
    })?;
    serde_json::from_str(&text).map_err(|e| ReconcileError::Io(format!("{}: {e}", path.display())))
}

pub type Clock = Arc<dyn Fn() -> TsMs + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as TsMs)
            .unwrap_or(0)
    })
}

#[derive(Debug, Clone)]
pub struct ReconcileConfig {
    /// `<data>`: holds `reconcile/`, `directives.json`, `next_plan.json`, `lord_chat.jsonl`.
    pub data_dir: PathBuf,
    /// `runs/sim`: holds `<tournament_id>/<run_id>/`.
    pub runs_root: PathBuf,
    /// `characters/`: lets the Reconciler see the bible and tones, and lets diffs be checked
    /// against the current files. Without it, diffs get structural checks only.
    pub characters_root: Option<PathBuf>,
}

/// What `<data>/reconcile/<report_id>.json` holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredReport {
    pub report: ReconcileReport,
    #[serde(default)]
    pub proposals: Vec<PlanOption>,
    #[serde(default)]
    pub runs: Vec<RunSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plans: Vec<NextStreamPlan>,
}

/// One `lord.chat` turn, appended to `<data>/lord_chat.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub ts_ms: TsMs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    pub message: String,
    pub reply: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directives_added: Vec<String>,
}

/// Turns of earlier conversation sent with each `lord_chat`.
const CHAT_HISTORY_TURNS: usize = 8;
/// Longest dispatch accepted, in characters.
const MAX_DISPATCH_CHARS: usize = 4_000;

/// The public API the daemon (WP5) and the console call.
pub struct Reconciler {
    cfg: ReconcileConfig,
    engine: Arc<dyn Engine>,
    clock: Clock,
    directives: DirectiveStore,
}

impl Reconciler {
    pub fn new(cfg: ReconcileConfig, engine: Arc<dyn Engine>) -> Self {
        Self::with_clock(cfg, engine, system_clock())
    }

    pub fn with_clock(cfg: ReconcileConfig, engine: Arc<dyn Engine>, clock: Clock) -> Self {
        let directives = DirectiveStore::new(&cfg.data_dir);
        Self {
            cfg,
            engine,
            clock,
            directives,
        }
    }

    fn report_path(&self, report_id: &str) -> Result<PathBuf, ReconcileError> {
        runs::check_id("report_id", report_id)?;
        Ok(self
            .cfg
            .data_dir
            .join("reconcile")
            .join(format!("{report_id}.json")))
    }

    fn load(&self, report_id: &str) -> Result<StoredReport, ReconcileError> {
        read_json(
            &self.report_path(report_id)?,
            &format!("report {report_id}"),
        )
    }

    fn save(&self, stored: &StoredReport) -> Result<(), ReconcileError> {
        write_json_atomic(&self.report_path(&stored.report.report_id)?, stored)
    }

    /// Deterministic metrics for a source, without the LLM.
    pub fn analyze_source(&self, source: &Source) -> Result<Metrics, ReconcileError> {
        Ok(analyze(&load_source(&self.cfg.runs_root, source)?))
    }

    fn character_dir(&self, runs: &[RunData]) -> Option<PathBuf> {
        let root = self.cfg.characters_root.as_ref()?;
        let id = runs.iter().find_map(RunData::character)?;
        runs::check_id("character", &id).ok()?;
        let dir = root.join(id);
        dir.is_dir().then_some(dir)
    }

    /// `reconcile.run`: analyse, synthesise, verify and store. Returns the report id. When
    /// the LLM synthesis fails the deterministic report is still stored, with the failure in
    /// its summary and no proposals.
    pub async fn run(&self, source: Source) -> Result<String, ReconcileError> {
        let runs = load_source(&self.cfg.runs_root, &source)?;
        let metrics = analyze(&runs);
        let created_ms = (self.clock)();
        let mut report_id = format!("rec-{created_ms}");
        let mut n = 2;
        while self.report_path(&report_id)?.exists() {
            report_id = format!("rec-{created_ms}-{n}");
            n += 1;
        }

        let char_dir = self.character_dir(&runs);
        let mut tones: Vec<String> = runs.iter().filter_map(RunData::tone_variant).collect();
        tones.sort();
        tones.dedup();
        let prompt = synth::synthesis_prompt(
            &metrics,
            &synth::excerpts(&runs),
            &synth::character_context(char_dir.as_deref(), &tones),
        );
        let synthesis = match synth::call(
            self.engine.as_ref(),
            prompt,
            synth::synthesis_schema(),
            &report_id,
        )
        .await
        {
            Ok(v) => synth::parse::<synth::RawSynthesis>(v),
            Err(e) => Err(e),
        };

        let mut dropped = Vec::new();
        let index = runs::RefIndex::new(&runs);
        let mut segments: Vec<SegmentFinding> = metrics
            .segments
            .iter()
            .map(|m| SegmentFinding {
                segment: m.segment,
                runs: m.runs,
                ses_mean: m.ses_mean,
                what_worked: Vec::new(),
                what_failed: Vec::new(),
                metrics: Some(m.clone()),
            })
            .collect();
        let (summary, proposals) = match synthesis {
            Ok(raw) => {
                for s in raw.segments {
                    let found = segments
                        .iter_mut()
                        .find(|f| runs::wire(&f.segment) == s.segment.trim());
                    let Some(f) = found else {
                        dropped.push(DroppedItem {
                            what: "segment".into(),
                            label: s.segment.clone(),
                            value: None,
                            reason: "segment did not run in this source".into(),
                        });
                        continue;
                    };
                    let worked = verify::verify_evidence(&index, s.what_worked, &mut dropped);
                    let failed = verify::verify_evidence(&index, s.what_failed, &mut dropped);
                    f.what_worked.extend(worked);
                    f.what_failed.extend(failed);
                }
                let proposals =
                    self.verify_options(&index, raw.options, char_dir.as_deref(), &mut dropped);
                let summary = if raw.summary.trim().is_empty() {
                    deterministic_summary(&metrics)
                } else {
                    raw.summary.trim().to_string()
                };
                (summary, proposals)
            }
            Err(e) => (
                format!(
                    "{} LLM synthesis failed ({e}); deterministic metrics only, no proposals.",
                    deterministic_summary(&metrics)
                ),
                Vec::new(),
            ),
        };

        let report = ReconcileReport {
            report_id: report_id.clone(),
            created_ms,
            source,
            segments,
            tone: metrics.tones,
            ranks: metrics.ranks,
            red_lines: metrics.red_lines,
            latency: metrics.latency,
            summary,
            dropped,
        };
        self.save(&StoredReport {
            report,
            proposals,
            runs: metrics.runs,
            plans: Vec::new(),
        })?;
        Ok(report_id)
    }

    fn verify_options(
        &self,
        index: &runs::RefIndex<'_>,
        raw: Vec<synth::RawOption>,
        char_dir: Option<&Path>,
        dropped: &mut Vec<DroppedItem>,
    ) -> Vec<PlanOption> {
        let mut out: Vec<PlanOption> = Vec::new();
        for (i, o) in raw.into_iter().enumerate() {
            let title = o.title.trim().to_string();
            let reject = |reason: String, dropped: &mut Vec<DroppedItem>| {
                dropped.push(DroppedItem {
                    what: "option".into(),
                    label: title.clone(),
                    value: o.option_id.clone(),
                    reason,
                });
            };
            if title.is_empty() {
                reject("option has no title".into(), dropped);
                continue;
            }
            let kind = match serde_json::from_value::<OptionKind>(serde_json::Value::String(
                o.kind.trim().to_string(),
            )) {
                Ok(k) => k,
                Err(_) => {
                    reject(format!("unknown option kind {:?}", o.kind), dropped);
                    continue;
                }
            };
            let diff = o.diff.as_deref().map(str::trim).filter(|d| !d.is_empty());
            if let Some(d) = diff {
                if kind != OptionKind::CharacterChange {
                    reject(
                        "only a character_change option may carry a diff".into(),
                        dropped,
                    );
                    continue;
                }
                if let Err(reason) = verify::check_diff(d, char_dir) {
                    reject(format!("diff rejected: {reason}"), dropped);
                    continue;
                }
            }
            let mut option_id = o
                .option_id
                .as_deref()
                .map(str::trim)
                .filter(|id| runs::check_id("option_id", id).is_ok())
                .map(str::to_string)
                .unwrap_or_else(|| format!("opt-{}", i + 1));
            let mut n = 2;
            while out.iter().any(|p| p.option_id == option_id) {
                option_id = format!("opt-{}-{n}", i + 1);
                n += 1;
            }
            let evidence = verify::verify_evidence(index, o.evidence, dropped);
            out.push(PlanOption {
                option_id,
                kind,
                title,
                rationale: o.rationale.trim().to_string(),
                evidence,
                diff: diff.map(|d| format!("{d}\n")),
            });
        }
        out
    }

    /// `reconcile.report`.
    pub fn report(&self, report_id: &str) -> Result<ReconcileReport, ReconcileError> {
        Ok(self.load(report_id)?.report)
    }

    /// `reconcile.list`: every stored report, newest first. Unreadable files are skipped.
    pub fn list(&self) -> Result<Vec<shadowrunr_types::rpc::ReportSummary>, ReconcileError> {
        let dir = self.cfg.data_dir.join("reconcile");
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(ReconcileError::Io(format!("{}: {e}", dir.display()))),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|_| path.extension().is_some_and(|x| x == "json"))
            else {
                continue;
            };
            if let Ok(stored) = self.load(id) {
                out.push(shadowrunr_types::rpc::ReportSummary {
                    report_id: stored.report.report_id.clone(),
                    created_ms: stored.report.created_ms,
                    source: stored.report.source.clone(),
                    proposals: u32::try_from(stored.proposals.len()).unwrap_or(u32::MAX),
                });
            }
        }
        out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms));
        Ok(out)
    }

    /// `plan.proposals`.
    pub fn proposals(&self, report_id: &str) -> Result<Vec<PlanOption>, ReconcileError> {
        Ok(self.load(report_id)?.proposals)
    }

    /// `plan.choose`: compose the next stream's plan from the chosen options and every
    /// active directive. The plan's `directives` are the active directives, verbatim; the
    /// LLM writes `lord_dispatch`, the running order and topics.
    pub async fn choose(
        &self,
        report_id: &str,
        option_ids: &[String],
    ) -> Result<NextStreamPlan, ReconcileError> {
        let mut stored = self.load(report_id)?;
        let mut chosen = Vec::new();
        for id in option_ids {
            match stored.proposals.iter().find(|p| &p.option_id == id) {
                Some(p) => {
                    if !chosen.iter().any(|c: &PlanOption| &c.option_id == id) {
                        chosen.push(p.clone());
                    }
                }
                None => {
                    return Err(ReconcileError::InvalidInput(format!(
                        "report {report_id} has no option {id}"
                    )))
                }
            }
        }
        let directives = self.directives.active()?;
        let created_ms = (self.clock)();
        let plan_id = format!("plan-{created_ms}");
        let v = synth::call(
            self.engine.as_ref(),
            synth::dispatch_prompt(&stored.report, &chosen, &directives),
            synth::dispatch_schema(),
            &plan_id,
        )
        .await?;
        let raw: synth::RawDispatch = synth::parse(v)?;
        let lord_dispatch = raw.lord_dispatch.trim().to_string();
        if lord_dispatch.is_empty() {
            return Err(ReconcileError::Synthesis("empty lord_dispatch".into()));
        }
        if lord_dispatch.chars().count() > MAX_DISPATCH_CHARS {
            return Err(ReconcileError::Synthesis(format!(
                "lord_dispatch longer than {MAX_DISPATCH_CHARS} characters"
            )));
        }
        let segments: Vec<PlanSegment> = raw
            .segments
            .into_iter()
            .filter_map(|s| {
                let segment = parse_active_segment(&s.segment)?;
                let minutes = u32::try_from(s.minutes)
                    .ok()
                    .filter(|m| (1..=240).contains(m))?;
                Some(PlanSegment {
                    segment,
                    minutes,
                    notes: s.notes.trim().to_string(),
                })
            })
            .collect();
        if segments.is_empty() {
            return Err(ReconcileError::Synthesis(
                "the plan names no valid segment (SEG-01 or SEG-02A, 1..=240 minutes)".into(),
            ));
        }
        let plan = NextStreamPlan {
            plan_id,
            created_ms,
            lord_dispatch,
            segments,
            topics: raw
                .topics
                .into_iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
            directives,
            report_id: Some(report_id.to_string()),
            option_ids: chosen.iter().map(|c| c.option_id.clone()).collect(),
        };
        stored.plans.push(plan.clone());
        self.save(&stored)?;
        write_json_atomic(&self.cfg.data_dir.join("next_plan.json"), &plan)?;
        Ok(plan)
    }

    /// The latest chosen plan (what the next stream starts with), if any.
    pub fn next_plan(&self) -> Result<Option<NextStreamPlan>, ReconcileError> {
        match read_json(&self.cfg.data_dir.join("next_plan.json"), "next plan") {
            Ok(p) => Ok(Some(p)),
            Err(ReconcileError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// `directive.add`.
    pub fn directive_add(
        &self,
        kind: DirectiveKind,
        text: &str,
        scope: Option<SegmentId>,
    ) -> Result<Directive, ReconcileError> {
        if let Some(s) = scope {
            if !is_active_segment(s) {
                return Err(ReconcileError::InvalidInput(format!(
                    "segment {} is descoped",
                    runs::wire(&s)
                )));
            }
        }
        self.directives.add(kind, text, scope, (self.clock)())
    }

    /// `directive.list`: all directives, active and inactive, oldest first.
    pub fn directive_list(&self) -> Result<Vec<Directive>, ReconcileError> {
        self.directives.list()
    }

    pub fn directive_deactivate(&self, id: &str) -> Result<Directive, ReconcileError> {
        self.directives.deactivate(id)
    }

    fn chat_log(&self) -> PathBuf {
        self.cfg.data_dir.join("lord_chat.jsonl")
    }

    fn chat_history(&self) -> Vec<(String, String)> {
        let Ok(text) = std::fs::read_to_string(self.chat_log()) else {
            return Vec::new();
        };
        let turns: Vec<ChatTurn> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        turns
            .iter()
            .skip(turns.len().saturating_sub(CHAT_HISTORY_TURNS))
            .map(|t| (t.message.clone(), t.reply.clone()))
            .collect()
    }

    /// `lord.chat`: a conversation with the Reconciler, which may turn the owner's
    /// instructions into directives (structured output). Invalid directive items are
    /// skipped; duplicates of an active directive are not added twice.
    pub async fn lord_chat(
        &self,
        report_id: Option<&str>,
        message: &str,
    ) -> Result<LordChatReply, ReconcileError> {
        let message = message.trim();
        if message.is_empty() {
            return Err(ReconcileError::InvalidInput("message is empty".into()));
        }
        let report = report_id.map(|id| self.report(id)).transpose()?;
        let active = self.directives.active()?;
        let now = (self.clock)();
        let v = synth::call(
            self.engine.as_ref(),
            synth::lord_chat_prompt(report.as_ref(), &active, &self.chat_history(), message),
            synth::lord_chat_schema(),
            &format!("lord-chat-{now}"),
        )
        .await?;
        let raw: synth::RawLordChat = synth::parse(v)?;
        let reply = raw.reply.trim().to_string();
        if reply.is_empty() {
            return Err(ReconcileError::Synthesis("empty reply".into()));
        }
        let mut added: Vec<Directive> = Vec::new();
        for d in raw.directives {
            let kind = match d.kind.trim() {
                "prevent" => DirectiveKind::Prevent,
                "try" => DirectiveKind::Try,
                _ => continue,
            };
            let scope = match d.scope.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                None => None,
                Some(s) => match parse_active_segment(s) {
                    Some(seg) => Some(seg),
                    None => continue,
                },
            };
            let text = d.text.trim();
            let dup = active
                .iter()
                .chain(added.iter())
                .any(|a| a.kind == kind && a.scope == scope && a.text.eq_ignore_ascii_case(text));
            if dup {
                continue;
            }
            if let Ok(dir) = self.directives.add(kind, text, scope, (self.clock)()) {
                added.push(dir);
            }
        }
        let turn = ChatTurn {
            ts_ms: now,
            report_id: report_id.map(str::to_string),
            message: message.to_string(),
            reply: reply.clone(),
            directives_added: added.iter().map(|d| d.id.clone()).collect(),
        };
        append_jsonl(&self.chat_log(), &turn)?;
        Ok(LordChatReply {
            reply,
            directives_added: added,
        })
    }
}

fn append_jsonl<T: Serialize>(path: &Path, v: &T) -> Result<(), ReconcileError> {
    use std::io::Write;
    let io = |e: std::io::Error| ReconcileError::Io(format!("{}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let line = serde_json::to_string(v).map_err(|e| ReconcileError::Io(e.to_string()))?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(io)?;
    writeln!(f, "{line}").map_err(io)
}

fn is_active_segment(s: SegmentId) -> bool {
    matches!(s, SegmentId::Seg01ImprovisedRoom | SegmentId::Seg02aHexmap)
}

/// `SEG-01` / `SEG-02A`; descoped and unknown segments are `None`.
fn parse_active_segment(s: &str) -> Option<SegmentId> {
    serde_json::from_value::<SegmentId>(serde_json::Value::String(s.trim().to_string()))
        .ok()
        .filter(|s| is_active_segment(*s))
}

/// One-paragraph summary from the metrics alone.
pub fn deterministic_summary(m: &Metrics) -> String {
    let judged = m.runs.iter().filter(|r| r.has_verdict).count();
    let verdict_fails = m
        .red_lines
        .iter()
        .filter(|i| i.source == IncidentSource::Verdict)
        .count();
    let moderation = m.red_lines.len() - verdict_fails;
    let segs: Vec<String> = m
        .segments
        .iter()
        .map(|s| {
            let ses = s
                .ses_mean
                .map(|v| format!(", SES {v:.2}"))
                .unwrap_or_default();
            let rate = s
                .chat_per_min
                .map(|v| format!(", {v:.1} chat/min"))
                .unwrap_or_default();
            format!("{} ({} runs{rate}{ses})", runs::wire(&s.segment), s.runs)
        })
        .collect();
    format!(
        "{} runs ({judged} judged). Segments: {}. Hard-rule failures: {verdict_fails}; moderation events: {moderation}.",
        m.runs.len(),
        if segs.is_empty() {
            "none".to_string()
        } else {
            segs.join("; ")
        }
    )
}

#[cfg(test)]
mod tests;
