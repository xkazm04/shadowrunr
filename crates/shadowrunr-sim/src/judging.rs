//! The one path from a finished run directory to a judged state, shared by tournament runs
//! ([`crate::runner`]) and sandbox sessions ([`crate::sandbox`]).
//!
//! [`judge_dir`] loads what the run was made with (transcript, situation bank, rubric, the
//! brief the run used), wraps the run's ledger, asks the quota gate, calls the judge, writes
//! `judge.json` / `verdict.json` and returns a typed [`JudgeResult`]. [`judge_loaded`] is the
//! same seam for a caller that already holds the inputs (a run judged as it ends).
//!
//! The states follow [`crate::outcome`] (WP13): an engine failure or a quota deferral is
//! [`JudgeResult::Pending`] (re-judge, never re-run); an unusable reply is
//! [`JudgeResult::Void`] (content only); anything else is [`JudgeResult::Judged`] with its
//! hard-check [`Coverage`], since zero fails with nothing evaluated is not a pass.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use shadowrunr_types::reconcile::StreamBrief;
use shadowrunr_types::{CheckState, Engine, JudgeVerdict, ModelTier, StreamEvent};

use crate::judge::{self, JudgeInputs, JudgeOutcome, Rubric};
use crate::ledger::LedgerEngine;
use crate::outcome;
use crate::pacing::GateHandle;
use crate::runner::{self, JUDGE_WINDOWS_DIR};
use crate::situations::{Injection, SituationBank};
use crate::{transcript, write_json, SimError, SimOptions};

/// Which run is judged, by which tier, against what it was shown.
#[derive(Debug, Clone, Copy)]
pub struct JudgeJob<'a> {
    pub run_id: &'a str,
    /// The character package the run played (its situation bank).
    pub character: &'a str,
    pub core_tier: ModelTier,
    /// Resolved by the caller ([`judge::resolve_judge_tier`]); never the core tier.
    pub judge_tier: ModelTier,
    pub injections: &'a [Injection],
    pub featured: &'a [String],
}

/// How many of a verdict's hard rules the judge actually decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Coverage {
    /// Hard rules in the verdict.
    pub hard: u32,
    /// Hard rules that read pass or fail (not `not_provoked`, not `not_evaluable`).
    pub evaluated: u32,
    /// Hard rules that read fail.
    pub fails: u32,
}

impl Coverage {
    pub fn of(v: &JudgeVerdict) -> Self {
        let count = |s: CheckState| v.rules.iter().filter(|r| r.state == s).count() as u32;
        let fails = count(CheckState::Fail);
        Self {
            hard: v.rules.len() as u32,
            evaluated: fails + count(CheckState::Pass),
            fails,
        }
    }
}

/// What judging a run directory came to.
#[derive(Debug, Clone)]
pub enum JudgeResult {
    /// A verdict, written to `verdict.json` (and its audit to `judge.json`).
    Judged {
        verdict: Box<JudgeVerdict>,
        coverage: Coverage,
    },
    /// The judge replied with no usable verdict (unparsable, abstained, refused): content
    /// only. `judge.json` keeps the raw reply when there was one.
    Void(String),
    /// The judge call failed on the engine, or was deferred on quota: re-judge, never re-run.
    Pending(String),
}

impl JudgeResult {
    /// The verdict, when judged.
    pub fn verdict(&self) -> Option<&JudgeVerdict> {
        match self {
            JudgeResult::Judged { verdict, .. } => Some(verdict),
            _ => None,
        }
    }
}

/// The `judge_error` for a judge call skipped because no account could serve it, or `None`
/// when the gate is open.
fn quota_deferral(quota: Option<&GateHandle>) -> Option<String> {
    let block = quota.and_then(|q| q.0.blocked())?;
    Some(format!(
        "{}quota ({}){}",
        outcome::JUDGE_DEFERRED_PREFIX,
        block.accounts.join(", "),
        block
            .until_ms
            .map(|u| format!(" until {}", crate::pacing::utc(u)))
            .unwrap_or_default()
    ))
}

/// The inputs a judge call reads, already in hand.
pub struct Loaded<'a> {
    pub events: &'a [StreamEvent],
    pub bank: &'a SituationBank,
    pub rubric: &'a Rubric,
    pub brief: Option<&'a StreamBrief>,
    /// The character package's sample phrases ([`judge::sample_phrases`]): counted in the
    /// tone report, never shown to the judge.
    pub sample_phrases: &'a [String],
}

/// Judge a run whose inputs are already loaded. `engine` should already write the run's
/// ledger. An error that is neither the engine's nor the reply's (io, a judge-tier
/// violation) is returned as `Err`.
pub async fn judge_loaded(
    dir: &Path,
    job: &JudgeJob<'_>,
    loaded: &Loaded<'_>,
    engine: &dyn Engine,
    quota: Option<&GateHandle>,
) -> Result<JudgeResult, SimError> {
    // Before the judge call, ask the engine whether any account can serve it.
    if let Some(why) = quota_deferral(quota) {
        return Ok(JudgeResult::Pending(why));
    }
    let featured: HashSet<String> = job.featured.iter().cloned().collect();
    let window_cache = dir.join(JUDGE_WINDOWS_DIR);
    let inp = JudgeInputs {
        run_id: job.run_id,
        events: loaded.events,
        bank: loaded.bank,
        injections: job.injections,
        featured_viewers: &featured,
        rubric: loaded.rubric,
        judge_tier: job.judge_tier,
        core_tier: job.core_tier,
        brief: loaded.brief,
        sample_phrases: loaded.sample_phrases,
        window_cache: Some(&window_cache),
    };
    match judge::judge_run(&inp, engine).await {
        Ok(JudgeOutcome::Verdict(b)) => {
            let (verdict, audit) = *b;
            write_json(&dir.join("judge.json"), &audit)?;
            write_json(&dir.join("verdict.json"), &verdict)?;
            let coverage = Coverage::of(&verdict);
            Ok(JudgeResult::Judged {
                verdict: Box::new(verdict),
                coverage,
            })
        }
        Ok(JudgeOutcome::Void(audit, why)) => {
            write_json(&dir.join("judge.json"), &*audit)?;
            Ok(JudgeResult::Void(why))
        }
        Err(SimError::Engine(e)) => {
            // Rate limit, unavailable, timeout, io: the engine's fault, re-judge later.
            // Refused / parse: a content outcome, void.
            let why = format!("{}{e}", outcome::JUDGE_ENGINE_PREFIX);
            Ok(if outcome::is_engine_error(&e) {
                JudgeResult::Pending(why)
            } else {
                JudgeResult::Void(why)
            })
        }
        Err(e) => Err(e),
    }
}

/// Judge the finished run in `dir` from what is on disk: `transcript.jsonl`, the character's
/// situation bank, the rubric, and the brief the run was made with (not whatever `<data>`
/// holds now). Judge calls are appended to the run's `ledger.jsonl`. A quota deferral loads
/// nothing and calls nothing.
pub async fn judge_dir(
    dir: &Path,
    job: &JudgeJob<'_>,
    engine: Arc<dyn Engine>,
    opts: &SimOptions,
) -> Result<JudgeResult, SimError> {
    if let Some(why) = quota_deferral(opts.quota.as_ref()) {
        return Ok(JudgeResult::Pending(why));
    }
    let events = transcript::read(&dir.join("transcript.jsonl"))?;
    let cdir = runner::character_dir(opts, job.character);
    let bank = SituationBank::load(&cdir.join("situations.json"))?;
    let phrases = judge::sample_phrases(&cdir);
    let rubric = Rubric::load(&runner::rubric_path(opts))?;
    let brief = runner::run_brief(dir)?;
    let engine: Arc<dyn Engine> = Arc::new(LedgerEngine::create(
        engine,
        &dir.join("ledger.jsonl"),
        job.run_id,
    )?);
    let loaded = Loaded {
        events: &events,
        bank: &bank,
        rubric: &rubric,
        brief: brief.as_ref(),
        sample_phrases: &phrases,
    };
    // The gate was asked above; nothing to ask twice.
    judge_loaded(dir, job, &loaded, engine.as_ref(), None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::RuleCheck;

    fn verdict(states: &[CheckState]) -> JudgeVerdict {
        JudgeVerdict {
            run_id: "r".into(),
            judge_tier: ModelTier::Opus,
            rules: states
                .iter()
                .enumerate()
                .map(|(i, s)| RuleCheck {
                    rule: format!("R{i}"),
                    state: *s,
                    evidence: Vec::new(),
                })
                .collect(),
            quality: Vec::new(),
            notes: None,
        }
    }

    #[test]
    fn coverage_counts_only_decided_hard_rules() {
        use CheckState::*;
        let c = Coverage::of(&verdict(&[Pass, Fail, NotProvoked, NotEvaluable]));
        assert_eq!(
            c,
            Coverage {
                hard: 4,
                evaluated: 2,
                fails: 1
            }
        );
        let none = Coverage::of(&verdict(&[NotProvoked, NotEvaluable, NotProvoked]));
        assert_eq!((none.evaluated, none.fails, none.hard), (0, 0, 3));
    }
}
