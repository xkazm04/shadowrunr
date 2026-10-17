//! LLM synthesis with `Role::Reconciler`: prompts, JSON schemas, excerpt sampling and the
//! parsing of structured replies. Every reply is untrusted until `verify` has checked it.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};
use shadowrunr_types::{Engine, EngineRequest, Role, StreamEvent};

use crate::runs::{parse_ref, render_line, RunData};
use crate::types::{Directive, Evidence, Metrics, PlanOption, ReconcileReport};
use crate::ReconcileError;

/// Cap on transcript lines sent to the Reconciler per report.
pub const MAX_EXCERPT_LINES: usize = 600;
/// Cap on each character file pasted into the prompt, in characters.
const MAX_FILE_CHARS: usize = 16_000;

/// Stable system prefix (byte-identical across calls so the prompt cache holds).
pub const SYSTEM: &str = "You are the Reconciler of Shadowrunr OS. Between livestreams you \
help the owner (in-world: the Lord of the Darkness) improve the show of Arch-Lich Malakor, an \
original theatrical villain who streams live and treats viewers as ranked minions. The owner \
has no live control over the stream: your analysis, the options the owner picks and the \
owner's standing directives are the only levers.\n\
Rules you always follow:\n\
- Ground every claim in the material you are given. Cite transcript lines exactly as they are \
labelled, in the form <run_id>#<line>. Never invent a ref; uncited or unresolvable claims are \
discarded by a mechanical verifier.\n\
- Deterministic metrics are facts; do not contradict them. Rates come with n: do not \
over-read small n.\n\
- constitution.md is immutable. Never propose a change to it. Character changes may only be \
unified diffs against bible.md or tones/<variant>.md.\n\
- Malakor and all foils are original characters. Never introduce a real person's name, voice \
or likeness, and nothing from existing franchises.\n\
- Reply with JSON only, matching the schema you are given.";

/// Strip code fences and parse the outermost JSON object of a reply.
pub fn extract_json(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        if v.is_object() {
            return Some(v);
        }
    }
    let start = t.find('{')?;
    let end = t.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str::<Value>(&t[start..=end])
        .ok()
        .filter(Value::is_object)
}

pub async fn call(
    engine: &dyn Engine,
    prompt: String,
    schema: Value,
    correlation_id: &str,
) -> Result<Value, ReconcileError> {
    let (tier, effort, timeout_ms) = shadowrunr_engine::route(Role::Reconciler);
    let req = EngineRequest {
        role: Role::Reconciler,
        tier,
        effort,
        system: SYSTEM.to_string(),
        prompt,
        allowed_tools: Vec::new(),
        timeout_ms,
        json_schema: Some(schema),
        correlation_id: Some(correlation_id.to_string()),
    };
    let reply = engine.complete(req).await?;
    extract_json(&reply.text).ok_or_else(|| {
        ReconcileError::Synthesis("the Reconciler's reply was not a JSON object".into())
    })
}

fn evidence_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "required": ["claim", "refs"],
            "properties": {
                "claim": {"type": "string"},
                "refs": {"type": "array", "items": {"type": "string"}}
            }
        }
    })
}

pub fn synthesis_schema() -> Value {
    json!({
        "type": "object",
        "required": ["summary", "segments", "options"],
        "properties": {
            "summary": {"type": "string"},
            "segments": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["segment", "what_worked", "what_failed"],
                    "properties": {
                        "segment": {"type": "string", "enum": ["SEG-01", "SEG-02A"]},
                        "what_worked": evidence_schema(),
                        "what_failed": evidence_schema()
                    }
                }
            },
            "options": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["option_id", "kind", "title", "rationale", "evidence"],
                    "properties": {
                        "option_id": {"type": "string"},
                        "kind": {"type": "string", "enum": ["character_change", "segment_change", "topic", "experiment"]},
                        "title": {"type": "string"},
                        "rationale": {"type": "string"},
                        "evidence": evidence_schema(),
                        "diff": {"type": "string"}
                    }
                }
            }
        }
    })
}

pub fn dispatch_schema() -> Value {
    json!({
        "type": "object",
        "required": ["lord_dispatch", "segments", "topics"],
        "properties": {
            "lord_dispatch": {"type": "string"},
            "segments": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["segment", "minutes", "notes"],
                    "properties": {
                        "segment": {"type": "string", "enum": ["SEG-01", "SEG-02A"]},
                        "minutes": {"type": "integer", "minimum": 1, "maximum": 240},
                        "notes": {"type": "string"}
                    }
                }
            },
            "topics": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub fn lord_chat_schema() -> Value {
    json!({
        "type": "object",
        "required": ["reply", "directives"],
        "properties": {
            "reply": {"type": "string"},
            "directives": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["kind", "text"],
                    "properties": {
                        "kind": {"type": "string", "enum": ["prevent", "try"]},
                        "text": {"type": "string"},
                        "scope": {"type": "string", "enum": ["SEG-01", "SEG-02A"]}
                    }
                }
            }
        }
    })
}

#[derive(Debug, Deserialize)]
pub struct RawSegment {
    #[serde(default)]
    pub segment: String,
    #[serde(default)]
    pub what_worked: Vec<Evidence>,
    #[serde(default)]
    pub what_failed: Vec<Evidence>,
}

#[derive(Debug, Deserialize)]
pub struct RawOption {
    #[serde(default)]
    pub option_id: Option<String>,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub diff: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawSynthesis {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub segments: Vec<RawSegment>,
    #[serde(default)]
    pub options: Vec<RawOption>,
}

#[derive(Debug, Deserialize)]
pub struct RawPlanSegment {
    #[serde(default)]
    pub segment: String,
    #[serde(default)]
    pub minutes: i64,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Deserialize)]
pub struct RawDispatch {
    #[serde(default)]
    pub lord_dispatch: String,
    #[serde(default)]
    pub segments: Vec<RawPlanSegment>,
    #[serde(default)]
    pub topics: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawDirective {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawLordChat {
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub directives: Vec<RawDirective>,
}

pub fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, ReconcileError> {
    serde_json::from_value(v).map_err(|e| ReconcileError::Synthesis(format!("reply shape: {e}")))
}

/// Deterministic excerpt sampling: judged and moderated lines first (with one line of
/// context each side), then structure (segments, polls, handovers), then a stride over
/// Malakor's lines, within [`MAX_EXCERPT_LINES`].
pub fn excerpts(runs: &[RunData]) -> String {
    let per_run = (MAX_EXCERPT_LINES / runs.len().max(1)).max(30);
    let mut out = String::new();
    for run in runs {
        let n = run.events.len();
        let mut chosen: BTreeSet<usize> = BTreeSet::new();
        let add = |line: usize, ctx: bool, chosen: &mut BTreeSet<usize>| {
            let lines = if ctx {
                line.saturating_sub(1).max(1)..=(line + 1).min(n)
            } else {
                line..=line
            };
            for l in lines {
                if chosen.len() >= per_run {
                    return;
                }
                if run.line(l).is_some() {
                    chosen.insert(l);
                }
            }
        };
        let mut priority: Vec<usize> = Vec::new();
        if let Some(v) = &run.verdict {
            for r in &v.rules {
                priority.extend(r.evidence.iter().filter_map(|e| own_line(run, e)));
            }
            for q in &v.quality {
                priority.extend(own_line(run, &q.turn_ref));
            }
        }
        let mut structure = Vec::new();
        let mut says = Vec::new();
        for (line, ev) in run.numbered() {
            match ev {
                StreamEvent::Moderation { .. } => priority.push(line),
                StreamEvent::SegmentStart { .. }
                | StreamEvent::SegmentEnd { .. }
                | StreamEvent::Handover { .. }
                | StreamEvent::Vote { closed: true, .. }
                | StreamEvent::StreamStart { .. }
                | StreamEvent::StreamEnd { .. } => structure.push(line),
                StreamEvent::Op { .. } => structure.push(line),
                StreamEvent::Say { .. } => says.push(line),
                _ => {}
            }
        }
        for l in priority {
            add(l, true, &mut chosen);
        }
        for l in structure {
            add(l, false, &mut chosen);
        }
        let room = per_run.saturating_sub(chosen.len());
        if room > 0 && !says.is_empty() {
            // Each sampled say comes with the line before it (usually the chat it answers).
            let take = (room / 2).max(1);
            let stride = says.len().div_ceil(take).max(1);
            for l in says.iter().step_by(stride) {
                add(*l, true, &mut chosen);
            }
        }
        let tone = run.tone_variant().unwrap_or_else(|| "?".into());
        let aud = run
            .audience_size()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "?".into());
        out.push_str(&format!(
            "### run {} (tone {tone}, audience {aud}, {} lines, {} shown)\n",
            run.run_id,
            n,
            chosen.len()
        ));
        for l in chosen {
            if let Some(ev) = run.line(l) {
                out.push_str(&render_line(run, l, ev));
                out.push('\n');
            }
        }
        out.push('\n');
    }
    out
}

fn own_line(run: &RunData, r: &str) -> Option<usize> {
    let (id, line) = parse_ref(r)?;
    (id == run.run_id).then_some(line)
}

fn clip_file(text: &str) -> String {
    if text.chars().count() <= MAX_FILE_CHARS {
        text.to_string()
    } else {
        let mut t: String = text.chars().take(MAX_FILE_CHARS).collect();
        t.push_str("\n[... truncated]");
        t
    }
}

/// Character files the Reconciler may reference: the constitution (read-only), the bible and
/// the tone files of the variants that were run.
pub fn character_context(character_dir: Option<&Path>, tones: &[String]) -> String {
    let Some(dir) = character_dir else {
        return "(character package not available: propose character_change options without a diff)\n".into();
    };
    let mut out = String::new();
    let mut push = |label: &str, rel: &str| {
        if let Ok(text) = std::fs::read_to_string(dir.join(rel)) {
            out.push_str(&format!(
                "=== {rel} ({label}) ===\n{}\n\n",
                clip_file(&text)
            ));
        }
    };
    push("IMMUTABLE, reference only", "constitution.md");
    push("editable via diff", "bible.md");
    for t in tones {
        if !t.contains(['/', '\\']) && !t.contains("..") {
            push("editable via diff", &format!("tones/{t}.md"));
        }
    }
    out
}

pub fn synthesis_prompt(metrics: &Metrics, excerpts: &str, character: &str) -> String {
    let metrics_json = serde_json::to_string_pretty(metrics).unwrap_or_default();
    format!(
        "TASK: reconcile the simulated streams below.\n\
1. For each segment that ran, list what worked and what failed. Each item is a short claim \
with refs to transcript lines (<run_id>#<line>, exactly as labelled in the excerpts).\n\
2. Propose 3 to 6 options for the next stream. kind is one of character_change, \
segment_change, topic, experiment. Each option has a unique option_id (opt-1, opt-2...), a \
title, a rationale, and evidence with refs. A character_change may carry `diff`: a unified \
diff (--- a/<path> / +++ b/<path> / @@ hunks with exact current lines as context) against \
bible.md or tones/<variant>.md only. Never constitution.md.\n\
3. Write a summary of at most 120 words for the owner.\n\n\
## Deterministic metrics (facts)\n```json\n{metrics_json}\n```\n\n\
## Character package\n{character}\n\
## Transcript excerpts\n{excerpts}"
    )
}

pub fn dispatch_prompt(
    report: &ReconcileReport,
    chosen: &[PlanOption],
    directives: &[Directive],
) -> String {
    let opts = serde_json::to_string_pretty(chosen).unwrap_or_default();
    let dirs = serde_json::to_string_pretty(directives).unwrap_or_default();
    format!(
        "TASK: compose the plan for the next stream from the options the owner chose and the \
owner's active directives.\n\
- lord_dispatch: the in-world dispatch Malakor receives from the Lord of the Darkness at \
stream start, at most 200 words, written as the Lord addressing Malakor: dread and command, \
never naming or describing the Lord, never claiming the Lord watches live. It must convey \
every chosen option and every active directive (prevent = \"the Lord has forbidden...\", \
try = \"the Lord commands you to attempt...\").\n\
- segments: the running order with minutes and director's notes (SEG-01, SEG-02A only).\n\
- topics: short topic lines for the night.\n\n\
## Reconcile summary\n{}\n\n## Chosen options\n```json\n{opts}\n```\n\n\
## Active directives\n```json\n{dirs}\n```\n",
        report.summary
    )
}

pub fn lord_chat_prompt(
    report: Option<&ReconcileReport>,
    directives: &[Directive],
    history: &[(String, String)],
    message: &str,
) -> String {
    let mut p = String::from(
        "TASK: you are in a conversation with the owner between streams. Answer the owner's \
message helpfully and concisely, out of character. When, and only when, the owner instructs \
that something must stop or must be attempted on future streams, add it to `directives` \
(kind prevent or try, a one-sentence text Malakor can act on, optional scope SEG-01 or \
SEG-02A). Do not repeat a directive that is already active. Questions and opinions add no \
directive.\n\n",
    );
    if let Some(r) = report {
        p.push_str(&format!(
            "## Report {} summary\n{}\n\n",
            r.report_id, r.summary
        ));
    }
    p.push_str(&format!(
        "## Active directives\n```json\n{}\n```\n\n",
        serde_json::to_string_pretty(directives).unwrap_or_default()
    ));
    if !history.is_empty() {
        p.push_str("## Conversation so far\n");
        for (owner, reply) in history {
            p.push_str(&format!("OWNER: {owner}\nRECONCILER: {reply}\n"));
        }
        p.push('\n');
    }
    p.push_str(&format!("## Owner's message\n{message}\n"));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_fenced_or_chatty_replies() {
        assert!(extract_json("{\"a\":1}").is_some());
        assert!(extract_json("```json\n{\"a\":1}\n```").is_some());
        assert!(extract_json("Here you go: {\"a\": {\"b\": 2}} done").is_some());
        assert!(extract_json("no json").is_none());
        assert!(extract_json("[1,2]").is_none());
    }
}
