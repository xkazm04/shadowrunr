//! `sim run --nights N`: consecutive streams ("nights") that share one memory directory and one
//! population, so night two can be measured.
//!
//! Each night is a run of its own (`<out>/<base>/<base>-n<k>/`, judged like any run when
//! judging is on) with a fresh brain and segment runtime, as a real restart between streams
//! would have. What carries over is what carries over live: the memory directory, and the
//! audience (the population is drawn from the base seed every night; the chat is drawn from a
//! fresh seed per night). The brain's clock moves [`NIGHT_GAP_MS`] per night, past the sleep
//! cycle's minimum interval, and the sleep cycle is forced between nights (with a live engine
//! that is one consolidator call per night; the scripted engine spends nothing).
//!
//! `nights.json` reports, per returning viewer, how often a moment from a past night reached
//! the prompt (the recall receipts' `moments`) and how often his line in that beat used it
//! (two or more distinctive words of the moment: a diagnostic, never a verdict).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use shadowrunr_brain::moments::line_uses_moment;
use shadowrunr_brain::{Brain, MemoryStore};
use shadowrunr_types::{
    ChatMsg, Engine, SegmentContext, SimRunSpec, Speaker, StreamEvent, TsMs, VoteTally,
};

use crate::runner::{
    brain_tuning, build_brain, execute, prepare, run_stage, temp_memory_dir, Mind,
};
use crate::transcript::{self, RECALL_FILE};
use crate::{write_json, RunOutput, SimError, SimOptions};

/// How far the brain's clock moves from one night to the next: a day, past the sleep cycle's
/// six-hour minimum interval.
pub const NIGHT_GAP_MS: TsMs = 24 * 3_600_000;
/// The report a multi-night run writes into `<out>/<base>/`.
pub const NIGHTS_FILE: &str = "nights.json";

/// The brain of one night, on the multi-night clock: the run's clock starts at 0 every night,
/// the brain's at `offset_ms`, and every event comes back on the run's clock.
pub struct NightMind {
    /// Boxed: a brain moved by value through the runner's frames runs a debug build's main
    /// thread out of stack.
    pub brain: Box<Brain>,
    pub offset_ms: TsMs,
    /// Run the sleep cycle at the end of the stream even without pressure (between nights).
    pub force_sleep: bool,
}

fn shift(ev: &mut StreamEvent, by: TsMs) {
    let t = transcript::ts(ev);
    transcript::set_ts(ev, t.saturating_add(by));
}

#[async_trait]
impl Mind for NightMind {
    async fn tick(
        &mut self,
        now_ms: TsMs,
        mut inbox: Vec<ChatMsg>,
        ctx: &SegmentContext,
    ) -> Result<Vec<StreamEvent>, SimError> {
        for m in &mut inbox {
            m.ts_ms = m.ts_ms.saturating_add(self.offset_ms);
        }
        let mut evs = self
            .brain
            .tick(now_ms.saturating_add(self.offset_ms), inbox, ctx)
            .await?;
        for e in &mut evs {
            shift(e, -self.offset_ms);
        }
        Ok(evs)
    }

    async fn end_stream(&mut self, now_ms: TsMs) -> Result<(), SimError> {
        let at = now_ms.saturating_add(self.offset_ms);
        self.brain.end_stream(at).await?;
        if self.force_sleep && self.brain.last_consolidation().is_none() {
            // A failed cycle is recorded (cycles/<id>.md) and never fails the night.
            let _ = self.brain.consolidate_now(at).await;
        }
        Ok(())
    }

    fn vote_closed(&mut self, poll_id: &str, tallies: &[VoteTally], now_ms: TsMs) {
        let _ = self
            .brain
            .confirm_vote(poll_id, tallies, now_ms.saturating_add(self.offset_ms));
    }
}

/// One night of a multi-night run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NightRow {
    pub night: u32,
    pub run_id: String,
    pub run_dir: PathBuf,
    /// Recall receipts written, and how many offered a moment.
    pub receipts: usize,
    pub moments_offered: usize,
}

/// One returning viewer across nights two and later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReturningViewer {
    pub viewer_id: String,
    /// Beats whose prompt carried one of their moments.
    pub shown: u32,
    /// Of those, beats where one of his lines shared two or more distinctive words with it.
    pub used: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NightsReport {
    pub base_run_id: String,
    pub nights: Vec<NightRow>,
    pub returning: Vec<ReturningViewer>,
    /// The kept memory directory (`keep_memory`), else `None` (deleted after the last night).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_dir: Option<PathBuf>,
}

impl NightsReport {
    /// The shown-to-used table, one line per returning viewer.
    pub fn table(&self) -> String {
        let mut s = String::from("returning viewer      shown  used\n");
        if self.returning.is_empty() {
            s.push_str("(no moment from a past night reached a prompt)\n");
        }
        for r in &self.returning {
            s.push_str(&format!(
                "{:<20} {:>6} {:>5}\n",
                r.viewer_id, r.shown, r.used
            ));
        }
        s
    }
}

/// The recall receipts beside a night's transcript.
fn read_recalls(transcript: &Path) -> Result<Vec<StreamEvent>, SimError> {
    let path = transcript.with_file_name(RECALL_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    transcript::read(&path)
}

/// Shown-to-used per returning viewer, from nights two and later.
fn returning(
    outputs: &[RunOutput],
    store: &MemoryStore,
) -> Result<(Vec<NightRow>, Vec<ReturningViewer>), SimError> {
    let mut rows = Vec::new();
    let mut per: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for (k, out) in outputs.iter().enumerate() {
        let recalls = read_recalls(&out.transcript)?;
        let events = transcript::read(&out.transcript)?;
        let mut offered = 0;
        for r in &recalls {
            let StreamEvent::Recall { ts_ms, moments, .. } = r else {
                continue;
            };
            if moments.is_empty() {
                continue;
            }
            offered += 1;
            if k == 0 {
                continue;
            }
            let lines: Vec<&str> = events
                .iter()
                .filter_map(|e| match e {
                    StreamEvent::Say {
                        ts_ms: t,
                        speaker: Speaker::Host,
                        text,
                        ..
                    } if t == ts_ms => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            for id in moments {
                let Some(f) = store.fact(id)? else {
                    continue;
                };
                let viewer = f.viewer_id.clone().unwrap_or_default();
                let e = per.entry(viewer).or_default();
                e.0 += 1;
                if lines.iter().any(|l| line_uses_moment(l, &f.fact.value)) {
                    e.1 += 1;
                }
            }
        }
        rows.push(NightRow {
            night: k as u32 + 1,
            run_id: out.spec.run_id.clone(),
            run_dir: out
                .transcript
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            receipts: recalls.len(),
            moments_offered: offered,
        });
    }
    let returning = per
        .into_iter()
        .map(|(viewer_id, (shown, used))| ReturningViewer {
            viewer_id,
            shown,
            used,
        })
        .collect();
    Ok((rows, returning))
}

/// The spec of night `k` (1-based) of a run based on `spec`: its own run id, and a fresh chat
/// seed after the first night.
pub fn night_spec(spec: &SimRunSpec, k: u32) -> SimRunSpec {
    SimRunSpec {
        run_id: format!("{}-n{k}", spec.run_id),
        seed: spec
            .seed
            .wrapping_add(u64::from(k - 1).wrapping_mul(0x9e37_79b9)),
        ..spec.clone()
    }
}

/// Run `nights` consecutive streams sharing one memory directory and one population. Each
/// night lands in `<out_dir>/<base>/<base>-n<k>/`; `nights.json` lands in `<out_dir>/<base>/`.
pub async fn run_nights_with(
    spec: SimRunSpec,
    nights: u32,
    out_dir: PathBuf,
    engine: Arc<dyn Engine>,
    opts: &SimOptions,
) -> Result<(Vec<RunOutput>, NightsReport), SimError> {
    if nights == 0 {
        return Err(SimError::Spec("nights must be at least 1".into()));
    }
    let base_dir = out_dir.join(&spec.run_id);
    let memory_dir = temp_memory_dir(&spec.run_id);
    std::fs::create_dir_all(&memory_dir)
        .map_err(|e| SimError::Io(format!("{}: {e}", memory_dir.display())))?;
    let res = async {
        let mut outputs = Vec::new();
        for k in 1..=nights {
            let s = night_spec(&spec, k);
            let mut prep = prepare(&s, &base_dir, engine.clone(), opts)?;
            prep.pop_seed = Some(spec.seed);
            let stage = run_stage(&prep)?;
            let brain = Box::new(
                Box::pin(build_brain(
                    &prep,
                    opts,
                    &memory_dir,
                    prep.engine.clone(),
                    &stage,
                    brain_tuning(opts, &prep),
                ))
                .await?,
            );
            let mind = NightMind {
                brain,
                offset_ms: TsMs::from(k - 1) * NIGHT_GAP_MS,
                force_sleep: k < nights,
            };
            outputs.push(Box::pin(execute(prep, mind, stage, opts)).await?);
        }
        let store = MemoryStore::open(&memory_dir)?;
        let (rows, returning) = returning(&outputs, &store)?;
        let report = NightsReport {
            base_run_id: spec.run_id.clone(),
            nights: rows,
            returning,
            memory_dir: opts.keep_memory.then(|| memory_dir.clone()),
        };
        write_json(&base_dir.join(NIGHTS_FILE), &report)?;
        Ok((outputs, report))
    }
    .await;
    if !opts.keep_memory {
        let _ = std::fs::remove_dir_all(&memory_dir);
    }
    res
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::runner::tests::{spec, test_opts};
    use crate::Hooks;
    use shadowrunr_types::ModelTier;

    fn chat_viewers(events: &[StreamEvent]) -> BTreeSet<String> {
        events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Chat { msg, .. } => Some(msg.viewer.id.clone()),
                _ => None,
            })
            .collect()
    }

    /// The scripted twin of `sim run --nights 2`: one memory, one population, fresh chat, the
    /// clock a day on, the sleep cycle forced between nights, receipts and the report written.
    #[tokio::test]
    async fn two_scripted_nights_share_memory_and_population() {
        let tmp = tempfile::tempdir().unwrap();
        let mut opts = test_opts(&tmp.path().join("sim"));
        opts.hooks = Hooks::default();
        opts.judge = false;
        opts.keep_memory = true;
        let out = tmp.path().join("out");
        let (outputs, report) = run_nights_with(
            spec("nights", ModelTier::Haiku, 30, 120),
            2,
            out.clone(),
            Arc::new(shadowrunr_engine::ScriptedEngine),
            &opts,
        )
        .await
        .expect("two nights complete");
        assert_eq!(outputs.len(), 2);
        assert!(out.join("nights/nights-n1/transcript.jsonl").is_file());
        assert!(out.join("nights/nights-n2/transcript.jsonl").is_file());
        assert!(out.join("nights/nights-n2/recall.jsonl").is_file());
        let back: NightsReport = crate::read_json(&out.join("nights").join(NIGHTS_FILE)).unwrap();
        assert_eq!(back, report);
        assert_eq!(report.nights.len(), 2);
        assert!(report.nights.iter().all(|n| n.receipts > 0), "{report:?}");
        assert!(report.table().starts_with("returning viewer"));

        // One population, fresh chat.
        let n1 = transcript::read(&outputs[0].transcript).unwrap();
        let n2 = transcript::read(&outputs[1].transcript).unwrap();
        // The base population is drawn from the base seed every night (a night's own
        // stimuli may still bring a wave of raiders or a copypasta crowd with them).
        let pop = crate::population::generate(
            &crate::calibration::load(&opts.sim_root).unwrap().0,
            "growing_channel",
            30,
            120,
            7,
            opts.hooks.rank,
        )
        .unwrap();
        let ids: BTreeSet<String> = pop.viewers.iter().map(|v| v.viewer.id.clone()).collect();
        let (s1, s2) = (chat_viewers(&n1), chat_viewers(&n2));
        for seen in [&s1, &s2] {
            let base: BTreeSet<&String> =
                seen.iter().filter(|id| id.starts_with("sim-v")).collect();
            assert!(!base.is_empty());
            assert!(
                base.iter().all(|id| ids.contains(*id)),
                "{seen:?} not in the population"
            );
        }
        assert!(
            s1.intersection(&s2).count() > 0,
            "the same viewers come back"
        );
        let texts = |evs: &[StreamEvent]| -> Vec<String> {
            evs.iter()
                .filter_map(|e| match e {
                    StreamEvent::Chat { msg, .. } => Some(msg.text.clone()),
                    _ => None,
                })
                .collect()
        };
        assert_ne!(texts(&n1), texts(&n2), "each night draws fresh chat");
        // Every transcript stays on the run's own clock.
        assert!(n2.iter().all(|e| transcript::ts(e) < NIGHT_GAP_MS));

        // One memory: night two's episodes sit a day later, after a forced sleep cycle.
        let mem = report.memory_dir.clone().expect("kept");
        let store = MemoryStore::open(&mem).unwrap();
        let eps = store.recent_episodes(500).unwrap();
        assert!(eps
            .iter()
            .any(|e| e.stream_id == "nights-n1" && e.ts_ms < NIGHT_GAP_MS));
        assert!(eps
            .iter()
            .any(|e| e.stream_id == "nights-n2" && e.ts_ms >= NIGHT_GAP_MS));
        let cycles = std::fs::read_dir(mem.join("cycles"))
            .map(|d| d.count())
            .unwrap_or(0);
        assert_eq!(
            cycles, 1,
            "the sleep cycle ran between the nights, not after the last"
        );
        let _ = std::fs::remove_dir_all(&mem);
    }
}
