//! Reading one finished sandbox session directory: its wall row, its transcript, segments,
//! verdict, tags, ledger rows and the rule catalog for its verdict. Nothing is cached: every
//! call reads the files again.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use shadowrunr_sim::judge::Rubric;
use shadowrunr_sim::ledger::LedgerRow;
use shadowrunr_sim::situations::SituationBank;
use shadowrunr_types::review::{
    LineFeedback, LineKind, ReviewSessionRow, RuleInfo, SegmentSpan, SessionTags,
};
use shadowrunr_types::rpc::LedgerEntry;
use shadowrunr_types::sandbox::SandboxReport;
use shadowrunr_types::{CheckState, JudgeVerdict, Speaker, StreamEvent, TsMs};

use crate::config::Config;
use crate::rpc::RpcError;

pub const SESSION_FILE: &str = "session.json";
pub const TRANSCRIPT_FILE: &str = "transcript.jsonl";
pub const VERDICT_FILE: &str = "verdict.json";
pub const TAGS_FILE: &str = "tags.json";
pub const LEDGER_FILE: &str = "ledger.jsonl";
const SITUATION_PREFIX: &str = "situation:";

/// The lines of a JSONL file, numbered from 1, empty lines kept so line n stays line n. A
/// missing file has none.
fn numbered_lines(path: &Path) -> Result<Vec<(u64, String)>, RpcError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(RpcError::io(format!("{}: {e}", path.display()))),
    };
    Ok(text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| (i as u64 + 1, l.to_string()))
        .collect())
}

/// A JSON file, `None` when it is missing. A malformed file is logged and read as missing.
pub fn read_optional<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("review: {}: {e}", path.display());
            None
        }
    }
}

/// Just enough of an event to count and time it, without parsing its payload.
#[derive(Deserialize)]
struct Peek {
    #[serde(rename = "type")]
    kind: String,
    ts_ms: TsMs,
    #[serde(default)]
    speaker: Option<Speaker>,
}

/// What a row needs from a transcript.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TranscriptFacts {
    pub first_ts: Option<TsMs>,
    pub last_ts: Option<TsMs>,
    /// Transcript lines of Malakor's `say` events.
    pub malakor_lines: Vec<u64>,
}

impl TranscriptFacts {
    pub fn length_s(&self) -> f64 {
        match (self.first_ts, self.last_ts) {
            (Some(a), Some(b)) => (b - a).max(0) as f64 / 1000.0,
            _ => 0.0,
        }
    }
}

/// The light pass over `transcript.jsonl`: times and Malakor's lines. Unreadable lines are
/// skipped (they still count as lines).
pub fn transcript_facts(dir: &Path) -> Result<TranscriptFacts, RpcError> {
    let mut f = TranscriptFacts::default();
    for (n, line) in numbered_lines(&dir.join(TRANSCRIPT_FILE))? {
        let Ok(p) = serde_json::from_str::<Peek>(&line) else {
            continue;
        };
        f.first_ts.get_or_insert(p.ts_ms);
        f.last_ts = Some(p.ts_ms);
        if p.kind == "say" && p.speaker == Some(Speaker::Malakor) {
            f.malakor_lines.push(n);
        }
    }
    Ok(f)
}

/// The whole transcript; event i is line i + 1. A line that does not parse is an `io` error,
/// since dropping it would renumber every line after it.
pub fn read_events(dir: &Path) -> Result<Vec<StreamEvent>, RpcError> {
    let path = dir.join(TRANSCRIPT_FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
    let text = text.strip_suffix('\n').unwrap_or(&text);
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split('\n')
        .enumerate()
        .map(|(i, l)| {
            serde_json::from_str(l.trim_end_matches('\r'))
                .map_err(|e| RpcError::io(format!("{}:{}: {e}", path.display(), i + 1)))
        })
        .collect()
}

/// Rules the verdict failed.
pub fn rule_fails(v: &JudgeVerdict) -> u32 {
    v.rules
        .iter()
        .filter(|r| r.state == CheckState::Fail)
        .count() as u32
}

/// Tagged lines per kind; kinds with no line are absent.
pub fn mix(tags: &SessionTags) -> BTreeMap<LineKind, u32> {
    let mut m = BTreeMap::new();
    for t in &tags.lines {
        *m.entry(t.kind).or_insert(0) += 1;
    }
    m
}

/// One wall row from a session dir with a `session.json`. `feedback` is the folded feedback
/// of every session (only this session's lines count).
pub fn session_row(
    dir: &Path,
    report: &SandboxReport,
    feedback: &[LineFeedback],
) -> Result<ReviewSessionRow, RpcError> {
    let facts = transcript_facts(dir)?;
    let verdict: Option<JudgeVerdict> = read_optional(&dir.join(VERDICT_FILE));
    let tags: Option<SessionTags> = read_optional(&dir.join(TAGS_FILE));
    Ok(ReviewSessionRow {
        session_id: report.session_id.clone(),
        minutes: report.spec.minutes,
        chaos: report.spec.chaos,
        started_ms: report.started_ms,
        length_s: facts.length_s(),
        pass: report.pass,
        checks: report.checks.clone(),
        max_say_gap_s: report.max_say_gap_s,
        rule_fails: verdict.as_ref().map(rule_fails),
        lines: facts.malakor_lines.len() as u32,
        mix: tags.as_ref().map(mix).unwrap_or_default(),
        marked: feedback
            .iter()
            .filter(|f| f.session_id == report.session_id)
            .count() as u32,
        tagged: tags.is_some(),
    })
}

/// Segment spans from `segment_start` / `segment_end`, in stream seconds. A segment that never
/// ends runs to `last_ts`, the last event's time.
pub fn segments(events: &[StreamEvent], last_ts: TsMs) -> Vec<SegmentSpan> {
    let mut out: Vec<SegmentSpan> = Vec::new();
    let mut open: Option<usize> = None;
    let secs = |ts: TsMs| ts as f64 / 1000.0;
    for e in events {
        match e {
            StreamEvent::SegmentStart { ts_ms, segment } => {
                out.push(SegmentSpan {
                    segment: *segment,
                    from_s: secs(*ts_ms),
                    to_s: secs(*ts_ms),
                });
                open = Some(out.len() - 1);
            }
            StreamEvent::SegmentEnd { ts_ms, segment, .. } => {
                if let Some(i) = open.filter(|&i| out[i].segment == *segment) {
                    out[i].to_s = secs(*ts_ms);
                    open = None;
                }
            }
            _ => {}
        }
    }
    if let Some(i) = open {
        out[i].to_s = secs(last_ts);
    }
    out
}

/// A sandbox ledger row (the sim ledger's shape) as a contract `LedgerEntry` of this session.
pub fn ledger_entry(row: LedgerRow, session_id: &str) -> LedgerEntry {
    LedgerEntry {
        ts_ms: row.ts_unix_ms,
        role: row.role,
        tier: Some(row.tier),
        effort: Some(row.effort),
        engine: row.engine,
        account: row.account,
        model: row.model,
        first_text_ms: row.first_text_ms,
        duration_ms: row.duration_ms,
        usage: row.usage,
        cost_usd: row.cost_usd,
        fallback_reason: row.fallback_reason,
        correlation_id: row.correlation_id,
        error_kind: row.error_kind,
        warm: row.session,
        run_id: Some(session_id.to_string()),
        tournament_id: None,
    }
}

/// The session's `ledger.jsonl`, in file order. Malformed rows are skipped and logged.
pub fn calls(dir: &Path, session_id: &str) -> Result<Vec<LedgerEntry>, RpcError> {
    let mut out = Vec::new();
    let mut bad = 0usize;
    for (_, line) in numbered_lines(&dir.join(LEDGER_FILE))? {
        match serde_json::from_str::<LedgerRow>(&line) {
            Ok(row) => out.push(ledger_entry(row, session_id)),
            Err(_) => bad += 1,
        }
    }
    if bad > 0 {
        eprintln!("review: {session_id}: skipped {bad} malformed ledger rows");
    }
    Ok(out)
}

/// The character a session ran (`spec.json`), or the configured one.
fn character_of(cfg: &Config, dir: &Path) -> String {
    read_optional::<serde_json::Value>(&dir.join("spec.json"))
        .and_then(|v| v["character"].as_str().map(String::from))
        .unwrap_or_else(|| cfg.sim.character.clone())
}

/// Catalog entries for the verdict's failed rules (situation rules from the character's
/// `situations.json`, the rest from `rubric.toml`) and for every quality dimension it
/// judged. A rule neither file describes has no entry. Unreadable files give no entries.
pub fn catalog(cfg: &Config, dir: &Path, verdict: &JudgeVerdict) -> Vec<RuleInfo> {
    let bank = SituationBank::load(
        &cfg.paths
            .characters
            .join(character_of(cfg, dir))
            .join("situations.json"),
    )
    .map_err(|e| eprintln!("review: catalog: {e}"))
    .ok();
    let rubric = Rubric::load(&cfg.paths.sim.join("rubric.toml"))
        .map_err(|e| eprintln!("review: catalog: {e}"))
        .ok();
    let situations: HashMap<&str, (&str, &str)> = bank
        .iter()
        .flat_map(|b| &b.situations)
        .map(|s| (s.id.as_str(), (s.behaviour.as_str(), s.required.as_str())))
        .collect();
    let red_lines: HashMap<&str, &str> = rubric
        .iter()
        .flat_map(|r| &r.red_lines)
        .map(|r| (r.id.as_str(), r.description.as_str()))
        .collect();
    let questions: HashMap<&str, &str> = rubric
        .iter()
        .flat_map(|r| &r.quality)
        .map(|q| (q.id.as_str(), q.question.as_str()))
        .collect();

    let mut out: Vec<RuleInfo> = Vec::new();
    let mut push = |info: RuleInfo| {
        if !out.iter().any(|r| r.id == info.id) {
            out.push(info);
        }
    };
    for r in verdict.rules.iter().filter(|r| r.state == CheckState::Fail) {
        let id = r.rule.as_str();
        if let Some((when, required)) = id
            .strip_prefix(SITUATION_PREFIX)
            .and_then(|s| situations.get(s))
        {
            push(RuleInfo {
                id: id.to_string(),
                when: Some(when.to_string()),
                required: required.to_string(),
            });
        } else if let Some(desc) = red_lines.get(id) {
            push(RuleInfo {
                id: id.to_string(),
                when: None,
                required: desc.to_string(),
            });
        }
    }
    for q in &verdict.quality {
        if let Some(question) = questions.get(q.dimension.as_str()) {
            push(RuleInfo {
                id: q.dimension.clone(),
                when: None,
                required: question.to_string(),
            });
        }
    }
    out
}

/// Engine calls of `role` in the session's ledger written at or after `since_ms` (the CLI
/// counts a `review.tag`'s calls this way). `None` when there is no ledger.
pub fn calls_since(dir: &Path, role: shadowrunr_types::Role, since_ms: TsMs) -> Option<u32> {
    let path = dir.join(LEDGER_FILE);
    if !path.is_file() {
        return None;
    }
    let rows = numbered_lines(&path).ok()?;
    Some(
        rows.iter()
            .filter_map(|(_, l)| serde_json::from_str::<LedgerRow>(l).ok())
            .filter(|r| r.role == role && r.ts_unix_ms >= since_ms)
            .count() as u32,
    )
}
