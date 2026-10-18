//! `reconcile.report` and `reconcile.incidents` on the wire. A stored report keeps every
//! red-line incident (the weary tournament's holds 13,324, 3.2 MB); the wire carries the
//! aggregates (`red_line_counts`), the first [`REPORT_INCIDENTS`] incidents, the total when it
//! cut any, and `synthesis_input`. The rest pages through `reconcile.incidents`.

use std::collections::{BTreeMap, BTreeSet};

use shadowrunr_reconcile::StoredReport;
use shadowrunr_types::reconcile::{IncidentCount, IncidentSource, ReconcileReport, RuleIncident};
use shadowrunr_types::rpc::{IncidentsPage, ReconcileIncidentsParams};

use crate::paging::{decode_cursor, encode_cursor, page_limit};
use crate::rpc::RpcError;

/// Incidents a `reconcile.report` reply carries inline.
pub const REPORT_INCIDENTS: usize = 100;

fn source_key(s: IncidentSource) -> u8 {
    match s {
        IncidentSource::Verdict => 0,
        IncidentSource::Moderation => 1,
    }
}

/// Every incident counted by `(rule, source)`, most first, then by rule.
pub fn incident_counts(incidents: &[RuleIncident]) -> Vec<IncidentCount> {
    let mut by: BTreeMap<(String, u8), (IncidentSource, u32, BTreeSet<&str>)> = BTreeMap::new();
    for i in incidents {
        let e = by
            .entry((i.rule.clone(), source_key(i.source)))
            .or_insert_with(|| (i.source, 0, BTreeSet::new()));
        e.1 += 1;
        e.2.insert(i.run_id.as_str());
    }
    let mut out: Vec<IncidentCount> = by
        .into_iter()
        .map(|((rule, _), (source, count, runs))| IncidentCount {
            rule,
            source,
            count,
            runs: u32::try_from(runs.len()).unwrap_or(u32::MAX),
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.rule.cmp(&b.rule)));
    out
}

/// The report as `reconcile.report` sends it.
pub fn report_reply(stored: StoredReport) -> ReconcileReport {
    let StoredReport {
        mut report,
        synthesis_input,
        ..
    } = stored;
    let total = report.red_lines.len();
    report.red_line_counts = incident_counts(&report.red_lines);
    if total > REPORT_INCIDENTS {
        report.red_lines.truncate(REPORT_INCIDENTS);
        report.red_lines_total = Some(u32::try_from(total).unwrap_or(u32::MAX));
    }
    report.synthesis_input = synthesis_input;
    report
}

/// `reconcile.incidents`: incidents in report order, optionally of one rule. The cursor is the
/// offset into the filtered list (reports are immutable once stored).
pub fn incidents_page(
    report: &ReconcileReport,
    p: &ReconcileIncidentsParams,
) -> Result<IncidentsPage, RpcError> {
    let limit = page_limit(p.limit)?;
    let start = match &p.cursor {
        Some(c) => decode_cursor(c, 1)?[0]
            .parse::<usize>()
            .map_err(|_| RpcError::invalid(format!("invalid cursor {c:?}")))?,
        None => 0,
    };
    let filtered: Vec<&RuleIncident> = report
        .red_lines
        .iter()
        .filter(|i| p.rule.as_ref().is_none_or(|r| *r == i.rule))
        .collect();
    let items: Vec<RuleIncident> = filtered
        .iter()
        .skip(start)
        .take(limit)
        .map(|i| (*i).clone())
        .collect();
    let end = start + items.len();
    Ok(IncidentsPage {
        total: u32::try_from(filtered.len()).unwrap_or(u32::MAX),
        next_cursor: (end < filtered.len()).then(|| encode_cursor(&[&end.to_string()])),
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::reconcile::{LatencyStats, RankEngagement, Source, SynthesisInput};

    fn incident(run: &str, rule: &str, source: IncidentSource) -> RuleIncident {
        RuleIncident {
            run_id: run.into(),
            rule: rule.into(),
            source,
            stage: None,
            action: None,
            refs: vec![format!("{run}#1")],
        }
    }

    fn stored(n: usize) -> StoredReport {
        let red_lines = (0..n)
            .map(|k| {
                let rule = if k % 3 == 0 {
                    "red-line:doxxing"
                } else {
                    "red-line:x"
                };
                let source = if k % 2 == 0 {
                    IncidentSource::Verdict
                } else {
                    IncidentSource::Moderation
                };
                incident(&format!("r{}", k % 7), rule, source)
            })
            .collect();
        StoredReport {
            report: ReconcileReport {
                report_id: "rec-1".into(),
                created_ms: 1,
                source: Source::Runs {
                    run_ids: vec!["r0".into()],
                },
                segments: vec![],
                tone: vec![],
                ranks: RankEngagement::default(),
                red_lines,
                latency: LatencyStats::default(),
                summary: "s".into(),
                dropped: vec![],
                red_line_counts: vec![],
                red_lines_total: None,
                synthesis_input: None,
            },
            proposals: vec![],
            runs: vec![],
            plans: vec![],
            synthesis_input: Some(SynthesisInput {
                budget_tokens: 120_000,
                ..SynthesisInput::default()
            }),
        }
    }

    #[test]
    fn a_large_report_is_cut_to_aggregates_and_a_page() {
        let r = report_reply(stored(1_000));
        assert_eq!(r.red_lines.len(), REPORT_INCIDENTS);
        assert_eq!(r.red_lines_total, Some(1_000));
        let counted: u32 = r.red_line_counts.iter().map(|c| c.count).sum();
        assert_eq!(counted, 1_000, "the aggregates cover every incident");
        assert!(r
            .red_line_counts
            .windows(2)
            .all(|w| w[0].count >= w[1].count));
        assert!(r.red_line_counts.iter().all(|c| c.runs == 7));
        assert_eq!(r.synthesis_input.map(|s| s.budget_tokens), Some(120_000));
        let text = serde_json::to_string(&report_reply(stored(1_000))).unwrap();
        assert!(text.len() < 40_000, "{} bytes", text.len());
        assert!(!text.contains("null"));
    }

    #[test]
    fn a_small_report_keeps_every_incident_and_no_total() {
        let r = report_reply(stored(10));
        assert_eq!(r.red_lines.len(), 10);
        assert_eq!(r.red_lines_total, None);
        let v = serde_json::to_value(&r).unwrap();
        assert!(v.get("red_lines_total").is_none());
    }

    #[test]
    fn incidents_page_through_every_incident_once() {
        let report = stored(1_234).report;
        let mut seen = 0;
        let mut cursor = None;
        loop {
            let p = ReconcileIncidentsParams {
                report_id: "rec-1".into(),
                rule: None,
                cursor: cursor.clone(),
                limit: 500,
            };
            let page = incidents_page(&report, &p).unwrap();
            assert_eq!(page.total, 1_234);
            seen += page.items.len();
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        assert_eq!(seen, 1_234);

        let one = ReconcileIncidentsParams {
            report_id: "rec-1".into(),
            rule: Some("red-line:doxxing".into()),
            cursor: None,
            limit: 10,
        };
        let page = incidents_page(&report, &one).unwrap();
        assert_eq!(page.total, 412);
        assert!(page.items.iter().all(|i| i.rule == "red-line:doxxing"));
        let bad = ReconcileIncidentsParams {
            cursor: Some("zz".into()),
            ..one
        };
        assert!(incidents_page(&report, &bad).is_err());
    }
}
