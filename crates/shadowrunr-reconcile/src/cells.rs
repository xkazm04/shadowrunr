//! One cell-and-tally spine: the matrix cell a run belongs to, and the one tally of judge
//! verdicts every table in a report reads (the tone findings, the prompt's per-segment rule
//! rows, the worst-quality ranking). Numbers published side by side come from here, so they
//! cannot disagree about what counts.
//!
//! - A run's axes (tone, core tier, segment, audience size, instrument hash) are derived once,
//!   spec first, transcript second. A cell keeps the simulator's `CellKey` axes, the instrument
//!   hash included: results from different hashes are never merged (docs/spec/contract.md).
//!   Reconcile also reads runs without a spec or an `instrument.json`, so an axis it cannot
//!   find is [`UNKNOWN`] instead of a guess.
//! - Directive checks (`directive:prevent:<id>` rules, `directive:try:<id>` facts, the
//!   simulator's prefixes) are directive outcomes, reported on their own: no hard-rule or
//!   quality tally counts them.

use std::collections::BTreeMap;

use shadowrunr_sim::judge::{DIRECTIVE_PREVENT_PREFIX, DIRECTIVE_TRY_PREFIX};
use shadowrunr_types::{CheckState, JudgeVerdict, QualityFact, RuleCheck, StreamEvent};

use crate::runs::{wire, RunData};
use crate::stats::rate;
use crate::types::{Rate, RuleFailure};

/// An axis Reconcile could not find for a run.
pub const UNKNOWN: &str = "?";

/// Whether a rule id or quality dimension is a directive check rather than a hard rule or a
/// quality dimension.
pub fn is_directive_check(id: &str) -> bool {
    id.starts_with(DIRECTIVE_PREVENT_PREFIX) || id.starts_with(DIRECTIVE_TRY_PREFIX)
}

/// The verdict's hard-rule checks (directive checks excluded).
pub fn hard_rules(v: &JudgeVerdict) -> impl Iterator<Item = &RuleCheck> {
    v.rules.iter().filter(|r| !is_directive_check(&r.rule))
}

/// The verdict's quality facts (directive facts excluded).
pub fn quality_facts(v: &JudgeVerdict) -> impl Iterator<Item = &QualityFact> {
    v.quality
        .iter()
        .filter(|q| !is_directive_check(&q.dimension))
}

/// The matrix cell a run belongs to. Field order is the sort order: tone, tier, segment,
/// audience, then instrument hash.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cell {
    pub tone: String,
    pub core_tier: String,
    pub segment: String,
    pub audience: String,
    pub instrument_hash: String,
}

impl RunData {
    /// `spec.core_tier`, else the `stream_start`'s.
    pub fn core_tier(&self) -> Option<String> {
        if let Some(s) = &self.spec {
            return Some(wire(&s.core_tier));
        }
        self.numbered().find_map(|(_, e)| match e {
            StreamEvent::StreamStart { core_tier, .. } => Some(wire(core_tier)),
            _ => None,
        })
    }

    /// The run's matrix segment: `spec.segment`, else its first `segment_start`.
    pub fn matrix_segment(&self) -> Option<String> {
        if let Some(s) = &self.spec {
            return Some(wire(&s.segment));
        }
        self.numbered().find_map(|(_, e)| match e {
            StreamEvent::SegmentStart { segment, .. } => Some(wire(segment)),
            _ => None,
        })
    }

    /// The matrix cell this run belongs to.
    pub fn cell(&self) -> Cell {
        let q = |o: Option<String>| o.unwrap_or_else(|| UNKNOWN.into());
        Cell {
            tone: q(self.tone_variant()),
            core_tier: q(self.core_tier()),
            segment: q(self.matrix_segment()),
            audience: q(self.audience_size().map(|a| a.to_string())),
            instrument_hash: q(self.instrument_hash.clone()),
        }
    }
}

/// Hard rules and quality dimensions tallied over a set of judged runs.
#[derive(Debug, Default)]
pub struct Tally {
    /// Judged runs counted.
    pub runs: u32,
    /// dimension -> (passed, judged).
    quality: BTreeMap<String, (u32, u32)>,
    /// rule -> (fails, evaluated n, cited refs).
    rules: BTreeMap<String, (u32, u32, Vec<String>)>,
}

impl Tally {
    /// Count one verdict. A rule is evaluated when it passed or failed; not provoked and not
    /// evaluable are never passes.
    pub fn add(&mut self, v: &JudgeVerdict) {
        self.runs += 1;
        for q in quality_facts(v) {
            let e = self.quality.entry(q.dimension.clone()).or_default();
            e.1 += 1;
            if q.value {
                e.0 += 1;
            }
        }
        for r in hard_rules(v) {
            let e = self.rules.entry(r.rule.clone()).or_default();
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

    /// Rules that failed at least once, by rule id: fails, evaluated n, cited refs.
    pub fn rule_failures(&self) -> Vec<RuleFailure> {
        self.rules
            .iter()
            .filter(|(_, (fails, _, _))| *fails > 0)
            .map(|(rule, (fails, n, refs))| RuleFailure {
                rule: rule.clone(),
                fails: *fails,
                n: *n,
                refs: refs.clone(),
            })
            .collect()
    }

    /// Pass rate per quality dimension.
    pub fn quality_rates(&self) -> BTreeMap<String, Rate> {
        self.quality
            .iter()
            .filter_map(|(d, (k, n))| rate(*k, *n).map(|r| (d.clone(), r)))
            .collect()
    }
}

/// Tally the judged runs among `runs` (unjudged runs are skipped).
pub fn tally<'a>(runs: impl IntoIterator<Item = &'a RunData>) -> Tally {
    let mut t = Tally::default();
    for v in runs.into_iter().filter_map(|r| r.verdict.as_ref()) {
        t.add(v);
    }
    t
}
