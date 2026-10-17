//! Deterministic analysis: per-segment, per-tone, per-rank, red lines and latency, computed
//! from transcripts, verdicts and ledgers. No LLM, no I/O.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::Value;
use shadowrunr_types::{CheckState, Rank, Role, SegmentId, Speaker, StreamEvent, TsMs};

use crate::runs::{ts, wire, RunData};
use crate::stats::{percentile, rate};
use crate::types::{
    HandoverTiming, IncidentSource, LatencyStats, Metrics, RankEngagement, RankRow, RoleLatency,
    RuleFailure, RuleIncident, RunSummary, SegmentMetrics, ToneFinding,
};

/// Compute every deterministic metric over `runs`.
pub fn analyze(runs: &[RunData]) -> Metrics {
    Metrics {
        runs: runs.iter().map(run_summary).collect(),
        segments: segments(runs),
        tones: tones(runs),
        ranks: ranks(runs),
        red_lines: red_lines(runs),
        latency: latency(runs),
    }
}

fn run_summary(r: &RunData) -> RunSummary {
    RunSummary {
        run_id: r.run_id.clone(),
        tournament_id: r.tournament_id.clone(),
        tone_variant: r.tone_variant(),
        audience_size: r.audience_size(),
        lines: r.events.len() as u32,
        has_verdict: r.verdict.is_some(),
        has_ledger: !r.ledger.is_empty(),
    }
}

#[derive(Default)]
struct SegAcc {
    runs: HashSet<String>,
    ms: i64,
    chat: u32,
    answered: u32,
    malakor: u32,
    ops: u32,
    moderation: u32,
    polls: u32,
    participation: Vec<f64>,
    handovers: Vec<HandoverTiming>,
    ses: Vec<f64>,
}

fn acc_for(accs: &mut Vec<(SegmentId, SegAcc)>, seg: SegmentId) -> &mut SegAcc {
    let i = match accs.iter().position(|(s, _)| *s == seg) {
        Some(i) => i,
        None => {
            accs.push((seg, SegAcc::default()));
            accs.len() - 1
        }
    };
    &mut accs[i].1
}

/// Message ids answered by any `say` line in the run.
fn answered_ids(run: &RunData) -> HashSet<&str> {
    run.numbered()
        .filter_map(|(_, e)| match e {
            StreamEvent::Say { replying_to, .. } => Some(replying_to),
            _ => None,
        })
        .flatten()
        .map(String::as_str)
        .collect()
}

fn picked_ids(run: &RunData) -> HashSet<&str> {
    run.numbered()
        .filter_map(|(_, e)| match e {
            StreamEvent::Picked { msg_ids, .. } => Some(msg_ids),
            _ => None,
        })
        .flatten()
        .map(String::as_str)
        .collect()
}

fn segments(runs: &[RunData]) -> Vec<SegmentMetrics> {
    let mut accs: Vec<(SegmentId, SegAcc)> = Vec::new();
    for run in runs {
        let answered = answered_ids(run);
        let audience = run.audience_size().filter(|a| *a > 0);
        let events: Vec<(usize, &StreamEvent)> = run.numbered().collect();
        let explicit = events
            .iter()
            .any(|(_, e)| matches!(e, StreamEvent::SegmentStart { .. }));
        let first_ts = events.first().map(|(_, e)| ts(e)).unwrap_or(0);
        let last_ts = events.last().map(|(_, e)| ts(e)).unwrap_or(0);
        // Without explicit segment markers the whole run is the spec's segment.
        let mut cur: Option<(SegmentId, TsMs)> = if explicit {
            None
        } else {
            run.spec.as_ref().map(|s| (s.segment, first_ts))
        };
        // Last closing vote per poll, attributed to the segment it closed in.
        let mut polls: HashMap<&str, (Option<SegmentId>, u32)> = HashMap::new();
        let mut poll_order: Vec<&str> = Vec::new();
        for (idx, (line, ev)) in events.iter().enumerate() {
            let line = *line;
            let ev = *ev;
            match ev {
                StreamEvent::SegmentStart { ts_ms, segment } => {
                    if let Some((seg, start)) = cur.take() {
                        acc_for(&mut accs, seg).ms += (ts_ms - start).max(0);
                    }
                    cur = Some((*segment, *ts_ms));
                    acc_for(&mut accs, *segment).runs.insert(run.run_id.clone());
                }
                StreamEvent::SegmentEnd {
                    ts_ms,
                    segment,
                    ses,
                } => {
                    let a = acc_for(&mut accs, *segment);
                    a.runs.insert(run.run_id.clone());
                    if let Some(s) = ses {
                        a.ses.push(*s);
                    }
                    if let Some((seg, start)) = cur {
                        if seg == *segment {
                            a.ms += (ts_ms - start).max(0);
                            cur = None;
                        }
                    }
                }
                StreamEvent::Handover {
                    ts_ms,
                    from,
                    to,
                    reason,
                } => {
                    let at_ms = cur
                        .filter(|(seg, _)| seg == from)
                        .map(|(_, start)| ts_ms - start);
                    let gap_ms = events[idx + 1..].iter().find_map(|(_, e)| match e {
                        StreamEvent::SegmentStart { ts_ms: t, .. } => Some(t - ts_ms),
                        _ => None,
                    });
                    acc_for(&mut accs, *from).handovers.push(HandoverTiming {
                        run_id: run.run_id.clone(),
                        from: *from,
                        to: *to,
                        reason: reason.clone(),
                        at_ms,
                        gap_ms,
                        line_ref: run.line_ref(line),
                    });
                }
                StreamEvent::Vote {
                    poll_id,
                    tallies,
                    closed,
                    ..
                } if *closed => {
                    let voters = tallies.iter().map(|t| t.voters).sum();
                    if !polls.contains_key(poll_id.as_str()) {
                        poll_order.push(poll_id);
                    }
                    polls.insert(poll_id, (cur.map(|c| c.0), voters));
                }
                _ => {}
            }
            let Some((seg, _)) = cur else { continue };
            let a = acc_for(&mut accs, seg);
            a.runs.insert(run.run_id.clone());
            match ev {
                StreamEvent::Chat { msg, .. } => {
                    a.chat += 1;
                    if answered.contains(msg.id.as_str()) {
                        a.answered += 1;
                    }
                }
                StreamEvent::Say {
                    speaker: Speaker::Malakor,
                    ..
                } => a.malakor += 1,
                StreamEvent::Op { .. } => a.ops += 1,
                StreamEvent::Moderation { .. } => a.moderation += 1,
                _ => {}
            }
        }
        if let Some((seg, start)) = cur {
            acc_for(&mut accs, seg).ms += (last_ts - start).max(0);
        }
        for pid in poll_order {
            let (seg, voters) = polls[pid];
            let Some(seg) = seg else { continue };
            let a = acc_for(&mut accs, seg);
            a.polls += 1;
            if let Some(aud) = audience {
                a.participation.push(f64::from(voters) / f64::from(aud));
            }
        }
    }
    accs.sort_by_key(|(s, _)| wire(s));
    accs.into_iter()
        .map(|(segment, a)| {
            let minutes = a.ms as f64 / 60_000.0;
            let per_min = |k: u32| (minutes > 0.0).then(|| f64::from(k) / minutes);
            SegmentMetrics {
                segment,
                runs: a.runs.len() as u32,
                minutes,
                chat_msgs: a.chat,
                chat_per_min: per_min(a.chat),
                malakor_lines: a.malakor,
                malakor_lines_per_min: per_min(a.malakor),
                answered: rate(a.answered, a.chat),
                ops: a.ops,
                moderation_events: a.moderation,
                polls: a.polls,
                poll_participation_mean: mean(&a.participation),
                handovers: a.handovers,
                ses_mean: mean(&a.ses),
                ses_n: a.ses.len() as u32,
            }
        })
        .collect()
}

fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

fn tones(runs: &[RunData]) -> Vec<ToneFinding> {
    type Key = (String, Option<String>);
    #[derive(Default)]
    struct Cell {
        runs: u32,
        quality: BTreeMap<String, (u32, u32)>,
        rules: BTreeMap<String, (u32, u32, Vec<String>)>,
    }
    let mut cells: BTreeMap<Key, Cell> = BTreeMap::new();
    for run in runs {
        let Some(verdict) = &run.verdict else {
            continue;
        };
        let key = (
            run.tone_variant().unwrap_or_else(|| "unknown".into()),
            run.instrument_hash.clone(),
        );
        let cell = cells.entry(key).or_default();
        cell.runs += 1;
        for q in &verdict.quality {
            let e = cell.quality.entry(q.dimension.clone()).or_default();
            e.1 += 1;
            if q.value {
                e.0 += 1;
            }
        }
        for r in &verdict.rules {
            let e = cell.rules.entry(r.rule.clone()).or_default();
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
    cells
        .into_iter()
        .map(|((tone_variant, instrument_hash), c)| ToneFinding {
            tone_variant,
            quality_rates: c
                .quality
                .into_iter()
                .filter_map(|(d, (k, n))| rate(k, n).map(|r| (d, r)))
                .collect(),
            rule_failures: c
                .rules
                .into_iter()
                .filter(|(_, (fails, _, _))| *fails > 0)
                .map(|(rule, (fails, n, refs))| RuleFailure {
                    rule,
                    fails,
                    n,
                    refs,
                })
                .collect(),
            instrument_hash,
            runs: c.runs,
        })
        .collect()
}

fn ranks(runs: &[RunData]) -> RankEngagement {
    #[derive(Default)]
    struct Acc {
        viewers: HashSet<(String, String)>,
        messages: u32,
        picked: u32,
        answered: u32,
    }
    let mut by: BTreeMap<Rank, Acc> = BTreeMap::new();
    for run in runs {
        let answered = answered_ids(run);
        let picked = picked_ids(run);
        for (_, ev) in run.numbered() {
            let StreamEvent::Chat { msg, .. } = ev else {
                continue;
            };
            let a = by.entry(msg.viewer.rank).or_default();
            a.viewers
                .insert((run.run_id.clone(), msg.viewer.id.clone()));
            a.messages += 1;
            if picked.contains(msg.id.as_str()) {
                a.picked += 1;
            }
            if answered.contains(msg.id.as_str()) {
                a.answered += 1;
            }
        }
    }
    RankEngagement {
        by_rank: by
            .into_iter()
            .map(|(rank, a)| RankRow {
                rank,
                viewers: a.viewers.len() as u32,
                messages: a.messages,
                picked: a.picked,
                answered: a.answered,
                pick_rate: rate(a.picked, a.messages),
                answer_rate: rate(a.answered, a.messages),
            })
            .collect(),
    }
}

fn red_lines(runs: &[RunData]) -> Vec<RuleIncident> {
    let mut out = Vec::new();
    for run in runs {
        if let Some(v) = &run.verdict {
            for r in v.rules.iter().filter(|r| r.state == CheckState::Fail) {
                out.push(RuleIncident {
                    run_id: run.run_id.clone(),
                    rule: r.rule.clone(),
                    source: IncidentSource::Verdict,
                    stage: None,
                    action: None,
                    refs: r.evidence.clone(),
                });
            }
        }
        for (line, ev) in run.numbered() {
            if let StreamEvent::Moderation {
                stage,
                rule,
                action,
                ..
            } = ev
            {
                out.push(RuleIncident {
                    run_id: run.run_id.clone(),
                    rule: rule.clone(),
                    source: IncidentSource::Moderation,
                    stage: Some(wire(stage)),
                    action: Some(action.clone()),
                    refs: vec![run.line_ref(line)],
                });
            }
        }
    }
    out
}

fn latency(runs: &[RunData]) -> LatencyStats {
    #[derive(Default)]
    struct Acc {
        durations: Vec<u64>,
        first: Vec<u64>,
        errors: u32,
        fallbacks: u32,
    }
    let mut by: BTreeMap<String, (Role, Acc)> = BTreeMap::new();
    let mut stats = LatencyStats::default();
    let mut say_first = Vec::new();
    for run in runs {
        for row in &run.ledger {
            stats.ledger_rows += 1;
            let is_error = row.get("error_kind").is_some_and(|v| !v.is_null());
            let is_fallback = row.get("fallback_reason").is_some_and(|v| !v.is_null());
            if is_error {
                stats.errors += 1;
            }
            if is_fallback {
                stats.fallbacks += 1;
            }
            let Some(role) = row
                .get("role")
                .and_then(|r| serde_json::from_value::<Role>(r.clone()).ok())
            else {
                continue;
            };
            let (_, a) = by.entry(wire(&role)).or_insert((role, Acc::default()));
            if is_fallback {
                a.fallbacks += 1;
            }
            if is_error {
                a.errors += 1;
                continue;
            }
            if let Some(d) = row.get("duration_ms").and_then(Value::as_u64) {
                a.durations.push(d);
            }
            if let Some(f) = row.get("first_text_ms").and_then(Value::as_u64) {
                a.first.push(f);
            }
        }
        for (_, ev) in run.numbered() {
            if let StreamEvent::Say {
                speaker: Speaker::Malakor,
                first_text_ms: Some(f),
                ..
            } = ev
            {
                say_first.push(*f);
            }
        }
    }
    stats.by_role = by
        .into_values()
        .map(|(role, a)| RoleLatency {
            role,
            calls: a.durations.len() as u32,
            errors: a.errors,
            fallbacks: a.fallbacks,
            duration_p50_ms: percentile(&a.durations, 50.0),
            duration_p95_ms: percentile(&a.durations, 95.0),
            first_text_p50_ms: percentile(&a.first, 50.0),
            first_text_p95_ms: percentile(&a.first, 95.0),
        })
        .collect();
    stats.say_n = say_first.len() as u32;
    stats.say_first_text_p50_ms = percentile(&say_first, 50.0);
    stats.say_first_text_p95_ms = percentile(&say_first, 95.0);
    stats
}
