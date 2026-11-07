//! Deterministic miners over runs that already exist: no engine, no judge, no quota.
//!
//! [`humor_by_source`] (`shadowrunr sim mine humor-by-source <dir>`): which kinds of beats fail
//! humour. Each judged `humor_landed` fact cites a transcript line (`turn_ref` `<run>#<line>`,
//! the 1-based event index the judge saw); the line's beat is the last `beat_plan` before it,
//! and the beat's source is its typed `source` or, in a transcript written before beats carried
//! one, the source its reason string names ([`source_from_reason`]). Facts are tallied per
//! source, per instrument hash: runs of two hashes are never pooled, so a directory holding
//! several is refused unless one hash is chosen. A table with more than
//! [`MAX_UNCLASSIFIED_SHARE`] of its facts unclassified is refused too: it would point at the
//! wrong source.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use shadowrunr_brain::orchestrate::source_from_reason;
use shadowrunr_types::{JudgeVerdict, StreamEvent};

use crate::transcript;

/// The rubric dimension mined.
pub const HUMOR_DIMENSION: &str = "humor_landed";
/// The most of a table's facts that may be unclassified (no beat before the line, or a reason
/// that names no known duty) before the table is refused.
pub const MAX_UNCLASSIFIED_SHARE: f64 = 0.10;
/// The key a fact with no classifiable beat is counted under.
pub const UNCLASSIFIED: &str = "unclassified";

/// One source's tally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRow {
    /// The beat source's wire name (`case_move`), or [`UNCLASSIFIED`].
    pub source: String,
    pub landed: u32,
    pub n: u32,
}

/// `humor_landed` by beat source, for one instrument hash.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HumorBySource {
    pub instrument_hash: String,
    /// Judged runs read.
    pub runs: u32,
    /// `humor_landed` facts read.
    pub facts: u32,
    /// Of those, classified from a typed `source`, and from the reason string.
    pub typed: u32,
    pub from_reason: u32,
    pub unclassified: u32,
    /// Most facts first, then by name; the unclassified row last when there is one.
    pub rows: Vec<SourceRow>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MineError {
    #[error("io: {0}")]
    Io(String),
    #[error("no judged run under {0}")]
    NoRuns(String),
    #[error(
        "runs of {} instrument hashes ({}): results from different hashes are never pooled; \
         choose one with --hash",
        .0.len(),
        .0.join(", ")
    )]
    MixedHashes(Vec<String>),
    #[error("no judged run with instrument hash {0}")]
    NoSuchHash(String),
    #[error(
        "{unclassified} of {facts} humor facts are unclassified (over {}%): the table would \
         point at the wrong source",
        (MAX_UNCLASSIFIED_SHARE * 100.0) as u32
    )]
    TooUnclassified { unclassified: u32, facts: u32 },
}

/// The judged run directories under `dir` (it, or any directory below it, holding a
/// `verdict.json` and a `transcript.jsonl`), sorted.
fn judged_runs(dir: &Path) -> Result<Vec<PathBuf>, MineError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if d.join("verdict.json").is_file() && d.join("transcript.jsonl").is_file() {
            out.push(d.clone());
        }
        let entries =
            std::fs::read_dir(&d).map_err(|e| MineError::Io(format!("{}: {e}", d.display())))?;
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// A run's instrument hash: `instrument.json`, else `run.json`.
fn run_hash(dir: &Path) -> Option<String> {
    ["instrument.json", "run.json"].iter().find_map(|f| {
        let text = std::fs::read_to_string(dir.join(f)).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        v.get("instrument_hash")?.as_str().map(str::to_string)
    })
}

/// How a fact's beat was classified.
enum Class {
    Typed(String),
    Reason(String),
    None,
}

/// The source of the beat line `line` (1-based) belongs to: the last `beat_plan` before it.
fn classify(events: &[StreamEvent], line: usize) -> Class {
    let Some(at) = line.checked_sub(1).filter(|i| *i < events.len()) else {
        return Class::None;
    };
    for e in events[..at].iter().rev() {
        if let StreamEvent::BeatPlan {
            mode,
            picks,
            reason,
            source,
            ..
        } = e
        {
            return match source {
                Some(s) => Class::Typed(s.as_str().to_string()),
                None => match source_from_reason(*mode, *picks, reason) {
                    Some(s) => Class::Reason(s.as_str().to_string()),
                    None => Class::None,
                },
            };
        }
    }
    Class::None
}

/// `humor_landed` by beat source over the judged runs under `dir`, for `hash` (required when
/// the runs carry more than one instrument hash).
pub fn humor_by_source(dir: &Path, hash: Option<&str>) -> Result<HumorBySource, MineError> {
    let runs = judged_runs(dir)?;
    if runs.is_empty() {
        return Err(MineError::NoRuns(dir.display().to_string()));
    }
    let mut by_hash: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for r in runs {
        let h = run_hash(&r).unwrap_or_else(|| "unknown".into());
        by_hash.entry(h).or_default().push(r);
    }
    let (instrument_hash, runs) = match hash {
        Some(h) => {
            let hit = by_hash
                .iter()
                .find(|(k, _)| k.as_str() == h || (h.len() >= 8 && k.starts_with(h)));
            match hit {
                Some((k, v)) => (k.clone(), v.clone()),
                None => return Err(MineError::NoSuchHash(h.to_string())),
            }
        }
        None if by_hash.len() > 1 => {
            return Err(MineError::MixedHashes(by_hash.keys().cloned().collect()))
        }
        None => by_hash.into_iter().next().expect("one hash"),
    };
    let mut tally: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let (mut facts, mut typed, mut from_reason, mut unclassified) = (0u32, 0u32, 0u32, 0u32);
    for r in &runs {
        let text = std::fs::read_to_string(r.join("verdict.json"))
            .map_err(|e| MineError::Io(format!("{}: {e}", r.display())))?;
        let verdict: JudgeVerdict = serde_json::from_str(&text)
            .map_err(|e| MineError::Io(format!("{}/verdict.json: {e}", r.display())))?;
        let events = transcript::read(&r.join("transcript.jsonl"))
            .map_err(|e| MineError::Io(e.to_string()))?;
        for f in verdict
            .quality
            .iter()
            .filter(|f| f.dimension == HUMOR_DIMENSION)
        {
            facts += 1;
            let line = f
                .turn_ref
                .rsplit_once('#')
                .and_then(|(_, l)| l.parse::<usize>().ok());
            let key = match line.map_or(Class::None, |l| classify(&events, l)) {
                Class::Typed(s) => {
                    typed += 1;
                    s
                }
                Class::Reason(s) => {
                    from_reason += 1;
                    s
                }
                Class::None => {
                    unclassified += 1;
                    UNCLASSIFIED.to_string()
                }
            };
            let t = tally.entry(key).or_default();
            t.0 += u32::from(f.value);
            t.1 += 1;
        }
    }
    if facts > 0 && f64::from(unclassified) / f64::from(facts) > MAX_UNCLASSIFIED_SHARE {
        return Err(MineError::TooUnclassified {
            unclassified,
            facts,
        });
    }
    let mut rows: Vec<SourceRow> = tally
        .into_iter()
        .map(|(source, (landed, n))| SourceRow { source, landed, n })
        .collect();
    rows.sort_by(|a, b| {
        (a.source == UNCLASSIFIED)
            .cmp(&(b.source == UNCLASSIFIED))
            .then(b.n.cmp(&a.n))
            .then(a.source.cmp(&b.source))
    });
    Ok(HumorBySource {
        instrument_hash,
        runs: u32::try_from(runs.len()).unwrap_or(u32::MAX),
        facts,
        typed,
        from_reason,
        unclassified,
        rows,
    })
}

impl HumorBySource {
    /// The table as text: one row a source, landed of n and the share.
    pub fn render(&self) -> String {
        let mut s = format!(
            "humor_landed by beat source, instrument {} ({} runs, {} facts: {} typed, {} from the \
             reason, {} unclassified)\n",
            self.instrument_hash,
            self.runs,
            self.facts,
            self.typed,
            self.from_reason,
            self.unclassified
        );
        s.push_str(&format!(
            "{:<12} {:>7} {:>5} {:>6}\n",
            "source", "landed", "n", "share"
        ));
        for r in &self.rows {
            let share = if r.n == 0 {
                0.0
            } else {
                f64::from(r.landed) / f64::from(r.n) * 100.0
            };
            s.push_str(&format!(
                "{:<12} {:>7} {:>5} {:>5.0}%\n",
                r.source, r.landed, r.n, share
            ));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::prep::{BeatMode, BeatSource};
    use shadowrunr_types::{ModelTier, QualityFact, Speaker};

    fn plan(ts: i64, mode: BeatMode, reason: &str, source: Option<BeatSource>) -> StreamEvent {
        StreamEvent::BeatPlan {
            ts_ms: ts,
            mode,
            bit_id: None,
            picks: 0,
            reason: reason.into(),
            source,
        }
    }

    fn say(ts: i64, text: &str) -> StreamEvent {
        StreamEvent::Say {
            ts_ms: ts,
            speaker: Speaker::Host,
            foil: None,
            text: text.into(),
            replying_to: Vec::new(),
            first_text_ms: None,
            delivery: None,
        }
    }

    /// One judged run: its transcript, its `humor_landed` facts as (line, landed), its hash.
    fn run(root: &Path, id: &str, hash: &str, events: &[StreamEvent], facts: &[(usize, bool)]) {
        let d = root.join(id);
        std::fs::create_dir_all(&d).unwrap();
        let lines: Vec<String> = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();
        std::fs::write(d.join("transcript.jsonl"), lines.join("\n") + "\n").unwrap();
        let v = JudgeVerdict {
            run_id: id.into(),
            judge_tier: ModelTier::Sonnet,
            rules: Vec::new(),
            quality: facts
                .iter()
                .map(|(l, v)| QualityFact {
                    dimension: HUMOR_DIMENSION.into(),
                    value: *v,
                    turn_ref: format!("{id}#{l}"),
                    quote: "a quote".into(),
                })
                .chain(std::iter::once(QualityFact {
                    dimension: "in_character".into(),
                    value: false,
                    turn_ref: format!("{id}#2"),
                    quote: "not humour".into(),
                }))
                .collect(),
            notes: None,
        };
        std::fs::write(d.join("verdict.json"), serde_json::to_string(&v).unwrap()).unwrap();
        std::fs::write(
            d.join("instrument.json"),
            format!(r#"{{"instrument_hash":"{hash}"}}"#),
        )
        .unwrap();
    }

    /// Two runs on one hash: a new transcript with typed sources and an old one classified from
    /// its reasons. Hand tally: case_move 2/3 (typed L2, L4; reason L4), purpose 1/2 (typed L6,
    /// reason L2), chat 0/1 (reason L6).
    fn fixture(root: &Path) {
        let new = [
            plan(
                0,
                BeatMode::Improvise,
                "case: dispatch HEX-03",
                Some(BeatSource::CaseMove),
            ),
            say(0, "The clerks ride."),
            say(0, "With quills."),
            say(1, "And ledgers."),
            plan(
                5,
                BeatMode::Improvise,
                "quiet: improvised",
                Some(BeatSource::Purpose),
            ),
            say(5, "Onward with the scheme."),
        ];
        run(
            root,
            "new.s1",
            "aaaaaaaaaaaa",
            &new,
            &[(2, true), (4, false), (6, true)],
        );
        let old = [
            plan(
                0,
                BeatMode::Mixed,
                "segment+chat: topic: the state's purpose",
                None,
            ),
            say(0, "Deal the cards."),
            plan(5, BeatMode::Improvise, "case: reveal HEX-01 DOC", None),
            say(5, "Face up, the syndicate."),
            plan(
                10,
                BeatMode::React,
                "segment+chat: question: no segment material, tied back",
                None,
            ),
            say(10, "Bob, no."),
        ];
        run(
            root,
            "old.s1",
            "aaaaaaaaaaaa",
            &old,
            &[(2, false), (4, true), (6, false)],
        );
    }

    #[test]
    fn the_table_matches_a_hand_tally_and_classifies_old_runs_from_the_reason() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        let t = humor_by_source(tmp.path(), None).unwrap();
        assert_eq!(t.instrument_hash, "aaaaaaaaaaaa");
        assert_eq!(
            (t.runs, t.facts, t.typed, t.from_reason, t.unclassified),
            (2, 6, 3, 3, 0)
        );
        let row = |s: &str| {
            t.rows
                .iter()
                .find(|r| r.source == s)
                .map(|r| (r.landed, r.n))
        };
        assert_eq!(row("case_move"), Some((2, 3)));
        assert_eq!(row("purpose"), Some((1, 2)));
        assert_eq!(row("chat"), Some((0, 1)));
        assert_eq!(t.rows.len(), 3);
        assert!(t.render().contains("case_move"), "{}", t.render());
    }

    #[test]
    fn runs_of_two_hashes_are_refused_unless_one_is_chosen() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        run(
            tmp.path(),
            "other.s1",
            "bbbbbbbbbbbb",
            &[
                plan(0, BeatMode::Improvise, "case: show DOC-01", None),
                say(0, "Up it goes."),
            ],
            &[(2, true)],
        );
        let err = humor_by_source(tmp.path(), None).unwrap_err();
        assert_eq!(
            err,
            MineError::MixedHashes(vec!["aaaaaaaaaaaa".into(), "bbbbbbbbbbbb".into()])
        );
        let b = humor_by_source(tmp.path(), Some("bbbbbbbb")).unwrap();
        assert_eq!((b.runs, b.facts), (1, 1));
        assert_eq!(
            b.rows,
            vec![SourceRow {
                source: "case_move".into(),
                landed: 1,
                n: 1
            }]
        );
        let a = humor_by_source(tmp.path(), Some("aaaaaaaaaaaa")).unwrap();
        assert_eq!(a.facts, 6, "the other hash never joins");
        assert!(matches!(
            humor_by_source(tmp.path(), Some("cccccccc")),
            Err(MineError::NoSuchHash(_))
        ));
    }

    #[test]
    fn a_table_over_ten_percent_unclassified_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path());
        // One fact with no beat before its line, one on a reason naming no duty: 2 of 8.
        run(
            tmp.path(),
            "odd.s1",
            "aaaaaaaaaaaa",
            &[
                say(0, "Before any beat."),
                plan(5, BeatMode::Improvise, "something new entirely", None),
                say(5, "Mystery line."),
            ],
            &[(1, true), (3, false)],
        );
        assert_eq!(
            humor_by_source(tmp.path(), None).unwrap_err(),
            MineError::TooUnclassified {
                unclassified: 2,
                facts: 8
            }
        );
    }
}
