//! Sizing the synthesis prompt to a token budget (WP12), so Reconcile fits any tournament.
//!
//! Tokens are estimated as characters ÷ [`CHARS_PER_TOKEN`], and only `1 - SAFETY_MARGIN` of
//! the budget is filled. The prompt is built from three parts:
//!
//! - a **metrics block** that summarises instead of dumping: per segment and per tone cell the
//!   aggregates plus the top-N rule failures (fails and n), the incident list capped (every
//!   incident still counted in `by_rule`), the rank table and the latency percentiles;
//! - the **character package**, files clipped;
//! - **transcript excerpts** sampled into whatever budget is left, in priority order: the lines
//!   the judge cited for rule failures (then ± [`FAILURE_WINDOW`] around them) and Malakor's
//!   own blocked output, then failed quality turns of the worst-quality cells, then a
//!   stratified sample across tone × tier × segment × audience size, then context around
//!   whatever is left. The excerpts carry viewer chat, so they sit inside an untrusted-data
//!   fence (`UntrustedFence`, WP14b); every excerpt line keeps its `<run_id>#<line>` label
//!   inside it, so evidence verification is unchanged.
//!
//! The metrics block is cut only as far as needed: a small source keeps every row, and
//! tighter caps are tried until enough of the budget is left for transcript evidence.
//!
//! When anything is cut, the prompt says so and [`SynthesisInput`] records the counts.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use serde_json::{json, Map, Value};
use shadowrunr_sim::judge::UntrustedFence;
use shadowrunr_types::{CheckState, ModStage, StreamEvent};

use crate::runs::{parse_ref, render_line, wire, RunData};
use crate::stats::{percentile, rate};
use crate::synth;
use crate::types::{Counted, IncidentSource, Metrics, RuleFailure, RuleIncident, SynthesisInput};
use crate::ReconcileError;

/// Default prompt budget, in estimated tokens.
pub const DEFAULT_PROMPT_BUDGET_TOKENS: u32 = 120_000;
/// Characters per token used for the estimate.
pub const CHARS_PER_TOKEN: f64 = 3.5;
/// Share of the budget left unfilled, for estimate error.
pub const SAFETY_MARGIN: f64 = 0.15;
/// Hard cap on excerpt lines, whatever the budget.
pub const MAX_EXCERPT_LINES: usize = 1_500;
/// Lines of context each side of a cited rule-failure line.
pub const FAILURE_WINDOW: usize = 2;
/// Lines of context each side of a failed quality turn or a blocked output.
const QUALITY_WINDOW: usize = 1;
/// Room kept for the "omitted for size" notes, in characters.
const NOTES_RESERVE: usize = 3_000;

/// The synthesis prompt's token budget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PromptBudget {
    pub tokens: u32,
}

impl Default for PromptBudget {
    fn default() -> Self {
        Self {
            tokens: DEFAULT_PROMPT_BUDGET_TOKENS,
        }
    }
}

impl PromptBudget {
    pub fn new(tokens: u32) -> Self {
        Self { tokens }
    }

    /// Characters the prompt may use (system prefix and schema included).
    pub fn usable_chars(&self) -> usize {
        (f64::from(self.tokens) * (1.0 - SAFETY_MARGIN) * CHARS_PER_TOKEN) as usize
    }
}

/// Estimated tokens for `chars` characters.
pub fn estimate_tokens(chars: usize) -> u32 {
    u32::try_from((chars as f64 / CHARS_PER_TOKEN).ceil() as u64).unwrap_or(u32::MAX)
}

fn clen(s: &str) -> usize {
    s.chars().count()
}

fn n32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn counted(included: usize, total: usize) -> Counted {
    Counted {
        included: n32(included),
        omitted: n32(total.saturating_sub(included)),
    }
}

/// How hard the metrics block and the character files are cut. Tried loosest first; a level
/// is taken when it leaves at least `min_room_pct` of the budget for transcript excerpts.
#[derive(Debug, Clone, Copy)]
struct Caps {
    min_room_pct: usize,
    top_rules: usize,
    incidents: usize,
    by_rule: usize,
    refs: usize,
    tone_cells: usize,
    strata: usize,
    file_chars: usize,
}

const LEVELS: [Caps; 4] = [
    // Nothing summarised away: small sources keep every row.
    Caps {
        min_room_pct: 50,
        top_rules: usize::MAX,
        incidents: usize::MAX,
        by_rule: usize::MAX,
        refs: usize::MAX,
        tone_cells: usize::MAX,
        strata: usize::MAX,
        file_chars: 16_000,
    },
    Caps {
        min_room_pct: 25,
        top_rules: 12,
        incidents: 40,
        by_rule: 25,
        refs: 3,
        tone_cells: 24,
        strata: 48,
        file_chars: 16_000,
    },
    Caps {
        min_room_pct: 25,
        top_rules: 5,
        incidents: 15,
        by_rule: 12,
        refs: 2,
        tone_cells: 12,
        strata: 24,
        file_chars: 8_000,
    },
    Caps {
        min_room_pct: 25,
        top_rules: 3,
        incidents: 5,
        by_rule: 6,
        refs: 1,
        tone_cells: 6,
        strata: 12,
        file_chars: 3_000,
    },
];

/// The prompt plus what went into it.
#[derive(Debug, Clone)]
pub struct BuiltPrompt {
    pub prompt: String,
    pub input: SynthesisInput,
    /// The fence the transcript excerpts sit in; the reply is screened for its token.
    pub fence: UntrustedFence,
}

/// Build the synthesis prompt for `runs` within `budget`. Fails closed when no fence token
/// can be minted (no OS randomness): no fence, no prompt.
pub fn synthesis_prompt(
    runs: &[RunData],
    metrics: &Metrics,
    character_dir: Option<&Path>,
    tones: &[String],
    budget: PromptBudget,
) -> Result<BuiltPrompt, ReconcileError> {
    // Sizing only: every token is the same length, so the skeleton costs the same with the
    // real fence minted below over the finished content.
    let sizing = UntrustedFence::mint(&[]).map_err(ReconcileError::Io)?;
    let usable = budget.usable_chars();
    let overhead = clen(synth::SYSTEM) + clen(&synth::synthesis_schema().to_string());
    let mut chosen = None;
    for (i, caps) in LEVELS.iter().enumerate() {
        let summary = summarize_metrics(runs, metrics, caps);
        let (character, clipped) =
            synth::character_context_capped(character_dir, tones, caps.file_chars);
        let skeleton = synth::synthesis_prompt(&summary.block, "", &character, &[], &sizing);
        let fixed = overhead + clen(&skeleton) + NOTES_RESERVE;
        let room = usable.saturating_sub(fixed);
        if room >= usable * caps.min_room_pct / 100 || i + 1 == LEVELS.len() {
            chosen = Some((summary, character, clipped, room, caps.file_chars));
            break;
        }
    }
    let (summary, character, clipped, room, file_chars) = chosen.expect("LEVELS is not empty");

    let ex = sample_excerpts(runs, room);

    let mut omitted = summary.notes.clone();
    if clipped {
        omitted.push(format!(
            "character files: clipped to {file_chars} characters each (marked [... truncated])."
        ));
    }
    let excerpt_cut =
        ex.failure_refs.omitted > 0 || ex.quality_refs.omitted > 0 || ex.runs.omitted > 0;
    if excerpt_cut {
        omitted.push(format!(
            "transcript excerpts: {} of {} lines cited for rule failures, {} of {} failed \
quality turns, {} of {} runs have any excerpt. Cite only lines shown in this prompt.",
            ex.failure_refs.included,
            ex.failure_refs.included + ex.failure_refs.omitted,
            ex.quality_refs.included,
            ex.quality_refs.included + ex.quality_refs.omitted,
            ex.runs.included,
            ex.runs.included + ex.runs.omitted,
        ));
    }
    let notes = omitted.join(" ");
    let fence = UntrustedFence::mint(&[&summary.block, &character, &ex.text, &notes])
        .map_err(ReconcileError::Io)?;
    let prompt = synth::synthesis_prompt(&summary.block, &ex.text, &character, &omitted, &fence);
    let total = overhead + clen(&prompt);
    Ok(BuiltPrompt {
        prompt,
        fence,
        input: SynthesisInput {
            budget_tokens: budget.tokens,
            estimated_tokens: estimate_tokens(total),
            runs: ex.runs,
            excerpt_lines: n32(ex.lines),
            failure_refs: ex.failure_refs,
            quality_refs: ex.quality_refs,
            incidents: summary.incidents,
            tone_cells: summary.tone_cells,
            rule_failures: summary.rule_rows,
            omitted,
        },
    })
}

// ---------------------------------------------------------------------------------------------
// Run axes

/// `(tone, tier, segment, audience)`: the matrix cell a run belongs to.
type Stratum = (String, String, String, String);
/// `(run index, 1-based line)`.
type LineRef = (usize, usize);
/// `(source, rule, stage, action)` of an incident group.
type IncidentKey = (String, String, String, String);

fn run_tier(run: &RunData) -> Option<String> {
    if let Some(s) = &run.spec {
        return Some(wire(&s.core_tier));
    }
    run.numbered().find_map(|(_, e)| match e {
        StreamEvent::StreamStart { core_tier, .. } => Some(wire(core_tier)),
        _ => None,
    })
}

/// The run's matrix segment: `spec.segment`, else its first `segment_start`.
fn run_segment(run: &RunData) -> Option<String> {
    if let Some(s) = &run.spec {
        return Some(wire(&s.segment));
    }
    run.numbered().find_map(|(_, e)| match e {
        StreamEvent::SegmentStart { segment, .. } => Some(wire(segment)),
        _ => None,
    })
}

fn stratum(run: &RunData) -> Stratum {
    let q = |o: Option<String>| o.unwrap_or_else(|| "?".into());
    (
        q(run.tone_variant()),
        q(run_tier(run)),
        q(run_segment(run)),
        q(run.audience_size().map(|a| a.to_string())),
    )
}

fn own_line(run: &RunData, r: &str) -> Option<usize> {
    let (id, line) = parse_ref(r)?;
    (id == run.run_id && run.line(line).is_some()).then_some(line)
}

// ---------------------------------------------------------------------------------------------
// Metrics summary

struct MetricsSummary {
    block: String,
    notes: Vec<String>,
    incidents: Counted,
    tone_cells: Counted,
    rule_rows: Counted,
}

/// Sort rule rows worst first and keep `top`; returns (kept, omitted count).
fn top_rules(mut rows: Vec<RuleFailure>, caps: &Caps) -> (Vec<Value>, usize) {
    rows.sort_by(|a, b| b.fails.cmp(&a.fails).then_with(|| a.rule.cmp(&b.rule)));
    let omitted = rows.len().saturating_sub(caps.top_rules);
    let kept = rows
        .into_iter()
        .take(caps.top_rules)
        .map(|r| {
            let mut v = json!({ "rule": r.rule, "fails": r.fails, "n": r.n });
            if !r.refs.is_empty() {
                let refs: Vec<&String> = r.refs.iter().take(caps.refs).collect();
                v["refs"] = json!(refs);
            }
            v
        })
        .collect();
    (kept, omitted)
}

/// Hard-rule failures over a set of runs: fails, evaluated n, cited refs.
fn rule_rows<'a>(runs: impl Iterator<Item = &'a RunData>) -> Vec<RuleFailure> {
    let mut by: BTreeMap<String, (u32, u32, Vec<String>)> = BTreeMap::new();
    for run in runs {
        let Some(v) = &run.verdict else { continue };
        for r in &v.rules {
            let e = by.entry(r.rule.clone()).or_default();
            match r.state {
                CheckState::Fail => {
                    e.0 += 1;
                    e.1 += 1;
                    e.2.extend(r.evidence.iter().cloned());
                }
                CheckState::Pass => e.1 += 1,
                CheckState::NotProvoked | CheckState::NotEvaluable => {}
            }
        }
    }
    by.into_iter()
        .filter(|(_, (fails, _, _))| *fails > 0)
        .map(|(rule, (fails, n, refs))| RuleFailure {
            rule,
            fails,
            n,
            refs,
        })
        .collect()
}

fn summarize_metrics(runs: &[RunData], m: &Metrics, caps: &Caps) -> MetricsSummary {
    let mut notes = Vec::new();
    let mut rule_total = 0usize;
    let mut rule_kept = 0usize;

    // Runs: totals and the matrix cells.
    let mut strata: BTreeMap<Stratum, (u32, u32)> = BTreeMap::new();
    for r in runs {
        let e = strata.entry(stratum(r)).or_default();
        e.0 += 1;
        if r.verdict.is_some() {
            e.1 += 1;
        }
    }
    let strata_total = strata.len();
    let cells: Vec<Value> = strata
        .iter()
        .take(caps.strata)
        .map(|((tone, tier, seg, aud), (n, judged))| {
            json!({ "tone": tone, "tier": tier, "segment": seg, "audience": aud,
                    "runs": n, "judged": judged })
        })
        .collect();
    if strata_total > caps.strata {
        notes.push(format!(
            "runs.cells: {} of {strata_total} tone x tier x segment x audience cells listed.",
            caps.strata
        ));
    }
    let judged = runs.iter().filter(|r| r.verdict.is_some()).count();
    let runs_block = json!({ "total": runs.len(), "judged": judged, "cells": cells });

    // Segments: aggregates, handovers summarised, top rule failures by matrix segment.
    let mut segments = Vec::new();
    for s in &m.segments {
        let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.remove("handovers");
            let at: Vec<u64> = s
                .handovers
                .iter()
                .filter_map(|h| h.at_ms.and_then(|x| u64::try_from(x).ok()))
                .collect();
            let gap: Vec<u64> = s
                .handovers
                .iter()
                .filter_map(|h| h.gap_ms.and_then(|x| u64::try_from(x).ok()))
                .collect();
            let mut h = Map::new();
            h.insert("count".into(), json!(s.handovers.len()));
            for (k, val) in [
                ("at_ms_p50", percentile(&at, 50.0)),
                ("gap_ms_p50", percentile(&gap, 50.0)),
                ("gap_ms_p95", percentile(&gap, 95.0)),
            ] {
                if let Some(x) = val {
                    h.insert(k.into(), json!(x));
                }
            }
            let ex: Vec<&String> = s
                .handovers
                .iter()
                .take(caps.refs)
                .map(|h| &h.line_ref)
                .collect();
            if !ex.is_empty() {
                h.insert("examples".into(), json!(ex));
            }
            o.insert("handovers".into(), Value::Object(h));
            let seg = wire(&s.segment);
            let rows = rule_rows(
                runs.iter()
                    .filter(|r| run_segment(r).as_deref() == Some(&seg)),
            );
            rule_total += rows.len();
            let (top, omitted) = top_rules(rows, caps);
            rule_kept += top.len();
            o.insert("rule_failures_top".into(), json!(top));
            if omitted > 0 {
                o.insert("rule_failures_omitted".into(), json!(omitted));
            }
        }
        segments.push(v);
    }

    // Tone cells, most-run first.
    let mut tone_idx: Vec<usize> = (0..m.tones.len()).collect();
    tone_idx.sort_by(|a, b| m.tones[*b].runs.cmp(&m.tones[*a].runs).then(a.cmp(b)));
    let tone_total = m.tones.len();
    let tones: Vec<Value> = tone_idx
        .iter()
        .take(caps.tone_cells)
        .map(|i| {
            let t = &m.tones[*i];
            rule_total += t.rule_failures.len();
            let (top, omitted) = top_rules(t.rule_failures.clone(), caps);
            rule_kept += top.len();
            let mut v = json!({
                "tone_variant": t.tone_variant,
                "runs": t.runs,
                "quality_rates": t.quality_rates,
                "rule_failures_top": top,
            });
            if let Some(h) = &t.instrument_hash {
                v["instrument_hash"] = json!(h);
            }
            if omitted > 0 {
                v["rule_failures_omitted"] = json!(omitted);
            }
            v
        })
        .collect();
    for t in tone_idx.iter().skip(caps.tone_cells) {
        rule_total += m.tones[*t].rule_failures.len();
    }
    if tone_total > caps.tone_cells {
        notes.push(format!(
            "tones: {} of {tone_total} tone cells (tone x instrument hash) listed, most-judged \
first.",
            caps.tone_cells
        ));
    }
    if rule_kept < rule_total {
        notes.push(format!(
            "rule failures: the top {} rules per segment and per tone cell are listed ({rule_kept} \
of {rule_total} rows); the rest are counted in rule_failures_omitted.",
            caps.top_rules
        ));
    }

    // Incidents: counted by rule, listed up to a cap (verdict failures first).
    let total = m.red_lines.len();
    let verdict = m
        .red_lines
        .iter()
        .filter(|i| i.source == IncidentSource::Verdict)
        .count();
    let mut by_rule: BTreeMap<IncidentKey, (u32, BTreeSet<&str>)> = BTreeMap::new();
    for i in &m.red_lines {
        let key = (
            wire(&i.source),
            i.rule.clone(),
            i.stage.clone().unwrap_or_default(),
            i.action.clone().unwrap_or_default(),
        );
        let e = by_rule.entry(key).or_default();
        e.0 += 1;
        e.1.insert(i.run_id.as_str());
    }
    let mut by_rule: Vec<_> = by_rule.into_iter().collect();
    by_rule.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(&b.0)));
    let by_rule_total = by_rule.len();
    let by_rule_rows: Vec<Value> = by_rule
        .iter()
        .take(caps.by_rule)
        .map(|((source, rule, stage, action), (count, runs))| {
            let mut v = json!({ "source": source, "rule": rule, "count": count,
                                "runs": runs.len() });
            if !stage.is_empty() {
                v["stage"] = json!(stage);
            }
            if !action.is_empty() {
                v["action"] = json!(action);
            }
            v
        })
        .collect();
    let listed: Vec<Value> = m
        .red_lines
        .iter()
        .filter(|i| i.source == IncidentSource::Verdict)
        .chain(m.red_lines.iter().filter(|i| {
            i.source == IncidentSource::Moderation && i.stage.as_deref() == Some("output")
        }))
        .chain(m.red_lines.iter().filter(|i| {
            i.source == IncidentSource::Moderation && i.stage.as_deref() != Some("output")
        }))
        .take(caps.incidents)
        .map(|i| incident_value(i, caps))
        .collect();
    let incidents = counted(listed.len(), total);
    if listed.len() < total {
        notes.push(format!(
            "incidents: {} of {total} listed individually (verdict failures first, then blocked \
output, then input moderation); every incident is counted in incidents.by_rule.",
            listed.len()
        ));
    }
    if by_rule_total > caps.by_rule {
        notes.push(format!(
            "incidents.by_rule: the {} most frequent of {by_rule_total} (source, rule, stage, \
action) groups.",
            caps.by_rule
        ));
    }
    let mut inc = json!({
        "total": total,
        "verdict_failures": verdict,
        "moderation_events": total - verdict,
        "by_rule": by_rule_rows,
        "listed": listed,
    });
    if incidents.omitted > 0 {
        inc["omitted"] = json!(incidents.omitted);
    }

    let block = serde_json::to_string(&json!({
        "runs": runs_block,
        "segments": segments,
        "tones": tones,
        "incidents": inc,
        "ranks": m.ranks,
        "latency": m.latency,
    }))
    .unwrap_or_default();
    MetricsSummary {
        block,
        notes,
        incidents,
        tone_cells: counted(tones.len(), tone_total),
        rule_rows: counted(rule_kept, rule_total),
    }
}

fn incident_value(i: &RuleIncident, caps: &Caps) -> Value {
    let mut v = json!({ "run_id": i.run_id, "rule": i.rule, "source": wire(&i.source) });
    if let Some(s) = &i.stage {
        v["stage"] = json!(s);
    }
    if let Some(a) = &i.action {
        v["action"] = json!(a);
    }
    let refs: Vec<&String> = i.refs.iter().take(caps.refs).collect();
    if !refs.is_empty() {
        v["refs"] = json!(refs);
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Excerpts

/// What the excerpt sampler produced.
#[derive(Debug, Clone)]
pub struct Excerpts {
    pub text: String,
    pub lines: usize,
    pub runs: Counted,
    pub failure_refs: Counted,
    pub quality_refs: Counted,
}

/// A group of lines added all-or-nothing.
struct Chunk {
    run: usize,
    lines: Vec<usize>,
}

struct Sampler<'a> {
    runs: &'a [RunData],
    chosen: Vec<BTreeSet<usize>>,
    cost: HashMap<(usize, usize), usize>,
    used: usize,
    lines: usize,
}

fn header(run: &RunData, shown: usize) -> String {
    let (tone, tier, seg, aud) = stratum(run);
    format!(
        "### run {} (tone {tone}, tier {tier}, {seg}, audience {aud}, {} lines, {shown} shown)\n",
        run.run_id,
        run.events.len()
    )
}

impl<'a> Sampler<'a> {
    fn new(runs: &'a [RunData]) -> Self {
        Self {
            runs,
            chosen: vec![BTreeSet::new(); runs.len()],
            cost: HashMap::new(),
            used: 0,
            lines: 0,
        }
    }

    fn line_cost(&mut self, run: usize, line: usize) -> usize {
        let r = &self.runs[run];
        *self.cost.entry((run, line)).or_insert_with(|| {
            r.line(line)
                .map(|ev| clen(&render_line(r, line, ev)) + 1)
                .unwrap_or(0)
        })
    }

    /// Add `chunk` if it fits under `limit` characters; lines already chosen cost nothing.
    fn try_add(&mut self, chunk: &Chunk, limit: usize) -> bool {
        let run = &self.runs[chunk.run];
        let new: BTreeSet<usize> = chunk
            .lines
            .iter()
            .copied()
            .filter(|l| run.line(*l).is_some() && !self.chosen[chunk.run].contains(l))
            .collect();
        if new.is_empty() {
            return true;
        }
        if self.lines + new.len() > MAX_EXCERPT_LINES {
            return false;
        }
        let mut cost: usize = new.iter().map(|l| self.line_cost(chunk.run, *l)).sum();
        if self.chosen[chunk.run].is_empty() {
            // The header at its widest ("shown" = every line) plus the blank line after.
            cost += clen(&header(run, run.events.len())) + 1;
        }
        if self.used + cost > limit {
            return false;
        }
        self.used += cost;
        self.lines += new.len();
        self.chosen[chunk.run].extend(new);
        true
    }

    fn has(&self, run: usize, line: usize) -> bool {
        self.chosen[run].contains(&line)
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (i, run) in self.runs.iter().enumerate() {
            let lines = &self.chosen[i];
            if lines.is_empty() {
                continue;
            }
            out.push_str(&header(run, lines.len()));
            for l in lines {
                if let Some(ev) = run.line(*l) {
                    out.push_str(&render_line(run, *l, ev));
                    out.push('\n');
                }
            }
            out.push('\n');
        }
        out
    }
}

fn window(run: &RunData, line: usize, w: usize) -> Vec<usize> {
    let lo = line.saturating_sub(w).max(1);
    let hi = (line + w).min(run.events.len());
    (lo..=hi).collect()
}

/// Round-robin over groups: the first item of every group, then the second, and so on.
fn interleave<T>(groups: Vec<Vec<T>>) -> Vec<T> {
    let mut iters: Vec<std::vec::IntoIter<T>> = groups.into_iter().map(Vec::into_iter).collect();
    let mut out = Vec::new();
    loop {
        let mut any = false;
        for it in &mut iters {
            if let Some(x) = it.next() {
                out.push(x);
                any = true;
            }
        }
        if !any {
            return out;
        }
    }
}

/// Lines the judge cited for rule failures, interleaved across rules (most-failed rule
/// first), and Malakor's blocked output lines, interleaved across runs.
fn failure_lines(runs: &[RunData]) -> (Vec<LineRef>, Vec<LineRef>) {
    let mut by_rule: BTreeMap<&str, Vec<LineRef>> = BTreeMap::new();
    for (ri, run) in runs.iter().enumerate() {
        let Some(v) = &run.verdict else { continue };
        for r in v.rules.iter().filter(|r| r.state == CheckState::Fail) {
            for e in &r.evidence {
                if let Some(line) = own_line(run, e) {
                    let list = by_rule.entry(r.rule.as_str()).or_default();
                    if !list.contains(&(ri, line)) {
                        list.push((ri, line));
                    }
                }
            }
        }
    }
    let mut groups: Vec<Vec<LineRef>> = by_rule.into_values().collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    let mut seen = BTreeSet::new();
    let cited: Vec<LineRef> = interleave(groups)
        .into_iter()
        .filter(|c| seen.insert(*c))
        .collect();
    let blocked: Vec<Vec<LineRef>> = runs
        .iter()
        .enumerate()
        .map(|(ri, run)| {
            run.numbered()
                .filter(|(_, e)| {
                    matches!(
                        e,
                        StreamEvent::Moderation {
                            stage: ModStage::Output,
                            ..
                        }
                    )
                })
                .map(|(line, _)| (ri, line))
                .collect()
        })
        .collect();
    (cited, interleave(blocked))
}

/// Failed quality turns, worst cell (lowest pass rate) first, interleaved across cells.
fn quality_lines(runs: &[RunData]) -> Vec<LineRef> {
    #[derive(Default)]
    struct Cell {
        pass: u32,
        n: u32,
        fails: Vec<LineRef>,
    }
    let mut cells: BTreeMap<Stratum, Cell> = BTreeMap::new();
    for (ri, run) in runs.iter().enumerate() {
        let Some(v) = &run.verdict else { continue };
        let cell = cells.entry(stratum(run)).or_default();
        for q in &v.quality {
            cell.n += 1;
            if q.value {
                cell.pass += 1;
            } else if let Some(line) = own_line(run, &q.turn_ref) {
                if !cell.fails.contains(&(ri, line)) {
                    cell.fails.push((ri, line));
                }
            }
        }
    }
    let mut cells: Vec<Cell> = cells
        .into_values()
        .filter(|c| c.n > 0 && !c.fails.is_empty())
        .collect();
    let pass_rate = |c: &Cell| rate(c.pass, c.n).map(|r| r.rate).unwrap_or(1.0);
    cells.sort_by(|a, b| pass_rate(a).total_cmp(&pass_rate(b)).then(b.n.cmp(&a.n)));
    interleave(cells.into_iter().map(|c| c.fails).collect())
}

fn chunks(runs: &[RunData], lines: &[LineRef], w: usize) -> Vec<Chunk> {
    lines
        .iter()
        .map(|(ri, line)| Chunk {
            run: *ri,
            lines: window(&runs[*ri], *line, w),
        })
        .collect()
}

/// A per-run sample: structure (stream and segment boundaries, handovers, closed polls, ops),
/// a little input moderation, and evenly spaced lines by Malakor with the line before each.
fn run_sample(run: &RunData, ri: usize, structure_cap: usize, says_cap: usize) -> Chunk {
    let mut structure = Vec::new();
    let mut moderation = Vec::new();
    let mut says = Vec::new();
    for (line, ev) in run.numbered() {
        match ev {
            StreamEvent::StreamStart { .. }
            | StreamEvent::StreamEnd { .. }
            | StreamEvent::SegmentStart { .. }
            | StreamEvent::SegmentEnd { .. }
            | StreamEvent::Handover { .. }
            | StreamEvent::CaseOpen { .. }
            | StreamEvent::Vote { closed: true, .. }
            | StreamEvent::Op { .. } => structure.push(line),
            StreamEvent::Moderation { .. } => moderation.push(line),
            StreamEvent::Say { .. } => says.push(line),
            _ => {}
        }
    }
    let mut lines: Vec<usize> = spread(&structure, structure_cap);
    lines.extend(moderation.iter().take(2));
    for l in spread(&says, says_cap) {
        lines.push(l.saturating_sub(1).max(1));
        lines.push(l);
    }
    Chunk { run: ri, lines }
}

/// Up to `k` items evenly spaced across `v` (first and boundaries favoured).
fn spread(v: &[usize], k: usize) -> Vec<usize> {
    if k == 0 || v.is_empty() {
        return Vec::new();
    }
    if v.len() <= k {
        return v.to_vec();
    }
    (0..k).map(|i| v[i * v.len() / k]).collect()
}

/// Sample transcript excerpts into `budget` characters (see the module docs for the order).
pub fn sample_excerpts(runs: &[RunData], budget: usize) -> Excerpts {
    let mut s = Sampler::new(runs);
    let (cited, blocked) = failure_lines(runs);
    let qrefs = quality_lines(runs);
    let pct = |p: usize| budget * p / 100;
    let add_all = |s: &mut Sampler<'_>, cs: Vec<Chunk>, limit: usize| -> Vec<Chunk> {
        cs.into_iter().filter(|c| !s.try_add(c, limit)).collect()
    };

    // 1. Every cited failure line on its own, then its window, then blocked output.
    let mut rest = add_all(&mut s, chunks(runs, &cited, 0), pct(60));
    rest.extend(add_all(
        &mut s,
        chunks(runs, &cited, FAILURE_WINDOW),
        pct(50),
    ));
    rest.extend(add_all(&mut s, chunks(runs, &blocked, 0), pct(60)));
    // 2. Failed quality turns of the worst cells.
    rest.extend(add_all(&mut s, chunks(runs, &qrefs, 0), pct(70)));
    // 3. A stratified sample: one run per tone x tier x segment x audience cell per round.
    let mut strata: BTreeMap<Stratum, Vec<usize>> = BTreeMap::new();
    for (ri, run) in runs.iter().enumerate() {
        strata.entry(stratum(run)).or_default().push(ri);
    }
    for ri in interleave(strata.into_values().collect()) {
        let full = run_sample(&runs[ri], ri, 8, 6);
        if !s.try_add(&full, budget) {
            let small = run_sample(&runs[ri], ri, 3, 2);
            s.try_add(&small, budget);
        }
    }
    // 4. Whatever is left: the rest of the above, then context around blocked output and
    // failed quality turns.
    add_all(&mut s, rest, budget);
    add_all(&mut s, chunks(runs, &blocked, QUALITY_WINDOW), budget);
    add_all(&mut s, chunks(runs, &qrefs, QUALITY_WINDOW), budget);

    let excerpted = s.chosen.iter().filter(|c| !c.is_empty()).count();
    let cited_in = cited.iter().filter(|(r, l)| s.has(*r, *l)).count();
    let q_in = qrefs.iter().filter(|(r, l)| s.has(*r, *l)).count();
    Excerpts {
        text: s.render(),
        lines: s.lines,
        runs: counted(excerpted, runs.len()),
        failure_refs: counted(cited_in, cited.len()),
        quality_refs: counted(q_in, qrefs.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_arithmetic() {
        let b = PromptBudget::default();
        assert_eq!(b.tokens, 120_000);
        assert_eq!(b.usable_chars(), 357_000);
        assert_eq!(estimate_tokens(7), 2);
        assert_eq!(estimate_tokens(0), 0);
    }

    #[test]
    fn interleave_and_spread() {
        assert_eq!(
            interleave(vec![vec![1, 2, 3], vec![10], vec![20, 21]]),
            vec![1, 10, 20, 2, 21, 3]
        );
        assert_eq!(spread(&[1, 2, 3, 4, 5, 6, 7, 8], 4), vec![1, 3, 5, 7]);
        assert_eq!(spread(&[1, 2], 4), vec![1, 2]);
    }
}
