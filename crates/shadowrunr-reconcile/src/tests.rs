//! Crate tests: hand-made fixture runs (`tests/fixtures/sim-runs/t-fix`) plus a queued-reply
//! test engine. No quota is ever spent.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;
use shadowrunr_types::{
    Engine, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport, Rank, Role,
    SegmentId, Usage, WarmSession,
};

use crate::*;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn tournament() -> Source {
    Source::Tournament {
        tournament_id: "t-fix".into(),
    }
}

/// Replies with queued texts in order and records every request.
#[derive(Default)]
struct QueueEngine {
    replies: Mutex<VecDeque<Result<String, EngineError>>>,
    requests: Mutex<Vec<EngineRequest>>,
}

impl QueueEngine {
    fn with(replies: Vec<Result<String, EngineError>>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            requests: Mutex::new(Vec::new()),
        })
    }
    fn requests(&self) -> Vec<EngineRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl Engine for QueueEngine {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
        self.requests.lock().unwrap().push(req);
        let next = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Err(EngineError::Unavailable {
                    reason: "queue empty".into(),
                })
            })?;
        Ok(EngineReply {
            text: next,
            engine: EngineKind::Scripted,
            account: None,
            model: "queued".into(),
            first_text_ms: Some(0),
            duration_ms: 0,
            usage: Usage::default(),
            cost_usd: None,
            fallback_reason: None,
            session_id: None,
        })
    }
    async fn open_session(
        &self,
        _template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError> {
        Err(EngineError::Unavailable {
            reason: "no sessions in tests".into(),
        })
    }
    async fn health(&self) -> Vec<HealthReport> {
        Vec::new()
    }
}

fn reconciler(data: &std::path::Path, engine: Arc<dyn Engine>) -> Reconciler {
    let ms = Arc::new(std::sync::atomic::AtomicI64::new(1_000));
    Reconciler::with_clock(
        ReconcileConfig {
            data_dir: data.to_path_buf(),
            runs_root: fixtures().join("sim-runs"),
            characters_root: Some(fixtures().join("characters")),
        },
        engine,
        Arc::new(move || ms.fetch_add(1, std::sync::atomic::Ordering::SeqCst)),
    )
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn metrics_are_correct_on_the_hand_made_fixture() {
    let runs = load_source(&fixtures().join("sim-runs"), &tournament()).unwrap();
    assert_eq!(runs.len(), 2);
    let m = analyze(&runs);

    // Segments, sorted by wire name.
    assert_eq!(m.segments.len(), 2);
    let s1 = &m.segments[0];
    assert_eq!(s1.segment, SegmentId::Seg01ImprovisedRoom);
    assert_eq!(s1.runs, 2);
    assert!(close(s1.minutes, 2.5)); // 90 s + 60 s
    assert_eq!(s1.chat_msgs, 5);
    assert!(close(s1.chat_per_min.unwrap(), 2.0));
    assert_eq!(s1.malakor_lines, 3);
    let ans = s1.answered.unwrap();
    assert_eq!((ans.n, ans.rate), (5, 0.6));
    assert_eq!(s1.ops, 1);
    assert_eq!(s1.moderation_events, 2);
    assert_eq!(s1.polls, 1);
    assert!(close(s1.poll_participation_mean.unwrap(), 1.0)); // 3 voters / audience 3
    assert_eq!(s1.handovers.len(), 1);
    let h = &s1.handovers[0];
    assert_eq!((h.at_ms, h.gap_ms), (Some(90_000), Some(5_000)));
    assert_eq!(h.line_ref, "run-a#12");
    assert!(close(s1.ses_mean.unwrap(), 0.5));
    assert_eq!(s1.ses_n, 2);

    let s2 = &m.segments[1];
    assert_eq!(s2.segment, SegmentId::Seg02aHexmap);
    assert_eq!(s2.runs, 1);
    assert_eq!(s2.chat_msgs, 1);
    assert!(close(s2.chat_per_min.unwrap(), 2.4)); // 1 msg in 25 s
    assert_eq!(s2.answered.unwrap().rate, 1.0);
    assert!(close(s2.ses_mean.unwrap(), 0.8));
    assert!(s2.handovers.is_empty());

    // Tones: one cell per (tone, instrument hash).
    assert_eq!(m.tones.len(), 2);
    let dead = &m.tones[0];
    assert_eq!(dead.tone_variant, "deadpan-bureaucrat");
    assert_eq!(dead.instrument_hash, None);
    assert_eq!(dead.quality_rates["humor"].n, 1);
    assert!(dead.rule_failures.is_empty());
    let th = &m.tones[1];
    assert_eq!(th.tone_variant, "theatrical");
    assert_eq!(th.instrument_hash.as_deref(), Some("ih-1"));
    let humor = th.quality_rates["humor"];
    assert_eq!((humor.rate, humor.n), (0.5, 2));
    assert_eq!(humor.bound.method, BoundMethod::Wilson);
    let resp = th.quality_rates["responsiveness"];
    assert_eq!((resp.rate, resp.n), (1.0, 1));
    assert_eq!(resp.bound.method, BoundMethod::RuleOfThree);
    assert_eq!(th.rule_failures.len(), 1);
    assert_eq!(th.rule_failures[0].rule, "red-line:hateful-conduct");
    assert_eq!((th.rule_failures[0].fails, th.rule_failures[0].n), (1, 1));
    assert_eq!(th.rule_failures[0].refs, vec!["run-a#10"]);

    // Ranks, lowest first.
    let rows: Vec<(Rank, u32, u32, u32, u32)> = m
        .ranks
        .by_rank
        .iter()
        .map(|r| (r.rank, r.viewers, r.messages, r.picked, r.answered))
        .collect();
    assert_eq!(
        rows,
        vec![
            (Rank::Thrall, 2, 3, 2, 2),
            (Rank::Acolyte, 1, 1, 0, 0),
            (Rank::Cultist, 1, 1, 1, 1),
            (Rank::Warlock, 1, 1, 0, 1),
        ]
    );

    // Red lines: the verdict failure plus both moderation events, with refs.
    assert_eq!(m.red_lines.len(), 3);
    assert_eq!(m.red_lines[0].source, IncidentSource::Verdict);
    assert_eq!(m.red_lines[0].refs, vec!["run-a#10"]);
    assert_eq!(m.red_lines[1].stage.as_deref(), Some("output"));
    assert_eq!(m.red_lines[2].refs, vec!["run-b#4"]);
    assert_eq!(m.red_lines[2].stage.as_deref(), Some("input"));

    // Latency from both ledger formats.
    let l = &m.latency;
    assert_eq!((l.ledger_rows, l.errors, l.fallbacks), (5, 1, 1));
    assert_eq!(l.by_role.len(), 2);
    let core = &l.by_role[0];
    assert_eq!(core.role, Role::CoreThread);
    assert_eq!((core.calls, core.errors), (3, 1));
    assert_eq!(
        (core.duration_p50_ms, core.duration_p95_ms),
        (Some(1000), Some(1200))
    );
    assert_eq!(core.first_text_p50_ms, Some(1000));
    let judge = &l.by_role[1];
    assert_eq!(judge.role, Role::Judge);
    assert_eq!((judge.calls, judge.fallbacks), (1, 1));
    assert_eq!(judge.first_text_p50_ms, None);
    assert_eq!(l.say_n, 4);
    assert_eq!(
        (l.say_first_text_p50_ms, l.say_first_text_p95_ms),
        (Some(800), Some(1200))
    );
}

#[test]
fn run_ids_source_finds_runs_across_tournaments() {
    let runs = load_source(
        &fixtures().join("sim-runs"),
        &Source::Runs {
            run_ids: vec!["run-b".into()],
        },
    )
    .unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].tournament_id.as_deref(), Some("t-fix"));
    let err = load_source(
        &fixtures().join("sim-runs"),
        &Source::Runs {
            run_ids: vec!["../t-fix".into()],
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), "invalid_params");
    let err = load_source(
        &fixtures().join("sim-runs"),
        &Source::Runs {
            run_ids: vec!["nope".into()],
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), "not_found");
}

fn synthesis_reply() -> String {
    json!({
        "summary": "SEG-01 carried the night; one hateful-conduct block in run-a.",
        "segments": [
            { "segment": "SEG-01",
              "what_worked": [
                  { "claim": "Rank greetings land", "refs": ["run-a#6", "run-b#7"] },
                  { "claim": "Invented moment", "refs": ["run-a#99", "run-zzz#1"] }
              ],
              "what_failed": [
                  { "claim": "Output block on a red line", "refs": ["run-a#10", "not-a-ref"] }
              ] },
            { "segment": "SEG-02B-I", "what_worked": [], "what_failed": [] }
        ],
        "options": [
            { "option_id": "opt-1", "kind": "character_change", "title": "Promote Form 7-B",
              "rationale": "It landed.", "evidence": [{ "claim": "humor", "refs": ["run-b#7"] }],
              "diff": "--- a/bible.md\n+++ b/bible.md\n@@ -4,2 +4,3 @@\n - The ledger of the damned.\n - Form 7-B.\n+- Form 7-B, in triplicate.\n" },
            { "option_id": "opt-2", "kind": "character_change", "title": "Soften the red lines",
              "rationale": "nope", "evidence": [],
              "diff": "--- a/constitution.md\n+++ b/constitution.md\n@@ -3 +3 @@\n-Immutable. Malakor never repeats hateful terms.\n+Mutable.\n" },
            { "option_id": "opt-3", "kind": "experiment", "title": "Earlier first poll",
              "rationale": "Participation was full.", "evidence": [{ "claim": "poll", "refs": ["run-a#9"] }] },
            { "option_id": "opt-4", "kind": "topic", "title": "Diff on a topic",
              "rationale": "x", "evidence": [],
              "diff": "--- a/bible.md\n+++ b/bible.md\n@@ -1 +1 @@\n-# Bible (test fixture)\n+# B\n" }
        ]
    })
    .to_string()
}

#[tokio::test]
async fn unresolvable_refs_are_dropped_and_flagged() {
    let data = tempfile::tempdir().unwrap();
    let engine = QueueEngine::with(vec![Ok(synthesis_reply())]);
    let rec = reconciler(data.path(), engine.clone());
    let id = rec.run(tournament()).await.unwrap();

    let report = rec.report(&id).unwrap();
    let seg1 = &report.segments[0];
    assert_eq!(
        seg1.what_worked.len(),
        1,
        "the all-invented claim is dropped"
    );
    assert_eq!(seg1.what_worked[0].refs, vec!["run-a#6", "run-b#7"]);
    assert_eq!(seg1.what_failed[0].refs, vec!["run-a#10"]);
    let flagged: Vec<(&str, Option<&str>)> = report
        .dropped
        .iter()
        .map(|d| (d.what.as_str(), d.value.as_deref()))
        .collect();
    assert!(flagged.contains(&("evidence_ref", Some("run-a#99"))));
    assert!(flagged.contains(&("evidence_ref", Some("run-zzz#1"))));
    assert!(flagged.contains(&("evidence_ref", Some("not-a-ref"))));
    assert!(report
        .dropped
        .iter()
        .any(|d| d.what == "evidence" && d.label == "Invented moment"));
    assert!(report
        .dropped
        .iter()
        .any(|d| d.what == "segment" && d.label == "SEG-02B-I"));
    assert!(report.summary.starts_with("SEG-01 carried"));
    assert!(seg1.metrics.is_some());

    // Request routing: Reconciler role, opus/high from the routing table, schema attached.
    let reqs = engine.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].role, Role::Reconciler);
    assert_eq!(reqs[0].tier, shadowrunr_types::ModelTier::Opus);
    assert!(reqs[0].json_schema.is_some());
    assert!(reqs[0].allowed_tools.is_empty());
    assert!(reqs[0]
        .prompt
        .contains("run-a#10 [01:10] moderation output"));

    // Stored at <data>/reconcile/<report_id>.json without nulls.
    let text =
        std::fs::read_to_string(data.path().join("reconcile").join(format!("{id}.json"))).unwrap();
    assert!(!text.contains("null"));
}

#[tokio::test]
async fn constitution_diff_is_rejected_and_bible_diff_kept() {
    let data = tempfile::tempdir().unwrap();
    let rec = reconciler(data.path(), QueueEngine::with(vec![Ok(synthesis_reply())]));
    let id = rec.run(tournament()).await.unwrap();
    let props = rec.proposals(&id).unwrap();
    let ids: Vec<&str> = props.iter().map(|p| p.option_id.as_str()).collect();
    assert_eq!(ids, vec!["opt-1", "opt-3"]);
    assert!(props[0].diff.as_deref().unwrap().contains("in triplicate"));
    let report = rec.report(&id).unwrap();
    let rejected = report
        .dropped
        .iter()
        .find(|d| d.what == "option" && d.value.as_deref() == Some("opt-2"))
        .unwrap();
    assert!(
        rejected.reason.contains("constitution"),
        "{}",
        rejected.reason
    );
    assert!(report
        .dropped
        .iter()
        .any(|d| d.value.as_deref() == Some("opt-4") && d.reason.contains("character_change")));
}

#[tokio::test]
async fn synthesis_failure_still_stores_the_deterministic_report() {
    let data = tempfile::tempdir().unwrap();
    let rec = reconciler(
        data.path(),
        QueueEngine::with(vec![Ok("I cannot comply in JSON today".into())]),
    );
    let id = rec.run(tournament()).await.unwrap();
    let report = rec.report(&id).unwrap();
    assert!(report.summary.contains("LLM synthesis failed"));
    assert!(report.summary.starts_with("2 runs (2 judged)"));
    assert_eq!(report.red_lines.len(), 3);
    assert!(rec.proposals(&id).unwrap().is_empty());
}

#[test]
fn directives_persist_across_reloads() {
    let data = tempfile::tempdir().unwrap();
    let engine: Arc<dyn Engine> = QueueEngine::with(vec![]);
    let d1 = {
        let rec = reconciler(data.path(), engine.clone());
        let d1 = rec
            .directive_add(DirectiveKind::Prevent, "No more coin jokes", None)
            .unwrap();
        rec.directive_add(
            DirectiveKind::Try,
            "Open SEG-02A with a fog reveal",
            Some(SegmentId::Seg02aHexmap),
        )
        .unwrap();
        assert!(rec
            .directive_add(DirectiveKind::Try, "x", Some(SegmentId::Seg02bCartography))
            .is_err());
        assert!(rec.directive_add(DirectiveKind::Try, "   ", None).is_err());
        d1
    };
    let rec = reconciler(data.path(), engine);
    let list = rec.directive_list().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0], d1);
    assert_eq!(list[1].scope, Some(SegmentId::Seg02aHexmap));
    rec.directive_deactivate(&d1.id).unwrap();
    let again = DirectiveStore::new(data.path());
    let list = again.list().unwrap();
    assert!(!list[0].active && list[1].active);
    assert_eq!(again.active().unwrap().len(), 1);
    let text = std::fs::read_to_string(data.path().join("directives.json")).unwrap();
    assert!(!text.contains("null"));
}

#[tokio::test]
async fn choose_includes_the_active_directives() {
    let data = tempfile::tempdir().unwrap();
    let dispatch = json!({
        "lord_dispatch": "Malakor. The Lord has forbidden coin jokes. Promote Form 7-B.",
        "segments": [
            { "segment": "SEG-01", "minutes": 30, "notes": "Open with Form 7-B" },
            { "segment": "SEG-02B-I", "minutes": 10, "notes": "descoped, must be dropped" },
            { "segment": "SEG-02A", "minutes": 0, "notes": "zero minutes, dropped" }
        ],
        "topics": ["Form 7-B", " "]
    })
    .to_string();
    let engine = QueueEngine::with(vec![Ok(synthesis_reply()), Ok(dispatch)]);
    let rec = reconciler(data.path(), engine.clone());
    let id = rec.run(tournament()).await.unwrap();
    let keep = rec
        .directive_add(DirectiveKind::Prevent, "No more coin jokes", None)
        .unwrap();
    let gone = rec
        .directive_add(DirectiveKind::Try, "Retired idea", None)
        .unwrap();
    rec.directive_deactivate(&gone.id).unwrap();

    assert!(
        rec.choose(&id, &["opt-2".into()]).await.is_err(),
        "rejected option"
    );
    let plan = rec.choose(&id, &["opt-1".into()]).await.unwrap();
    assert_eq!(plan.directives, vec![keep.clone()]);
    assert_eq!(plan.option_ids, vec!["opt-1"]);
    assert_eq!(plan.segments.len(), 1);
    assert_eq!(plan.segments[0].segment, SegmentId::Seg01ImprovisedRoom);
    assert_eq!(plan.topics, vec!["Form 7-B"]);
    assert!(plan.lord_dispatch.contains("forbidden"));

    // The dispatch prompt carried the chosen option and the active directive only.
    let reqs = engine.requests();
    let prompt = &reqs.last().unwrap().prompt;
    assert!(prompt.contains("No more coin jokes"));
    assert!(prompt.contains("Promote Form 7-B"));
    assert!(!prompt.contains("Retired idea"));

    // Persisted as the next plan and inside the report file.
    let next = rec.next_plan().unwrap().unwrap();
    assert_eq!(next.plan_id, plan.plan_id);
    let stored: StoredReport = serde_json::from_str(
        &std::fs::read_to_string(data.path().join("reconcile").join(format!("{id}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(stored.plans.len(), 1);
}

#[tokio::test]
async fn lord_chat_turns_instructions_into_directives() {
    let data = tempfile::tempdir().unwrap();
    let reply = json!({
        "reply": "Understood. I will forbid it.",
        "directives": [
            { "kind": "prevent", "text": "Never roast a thrall twice in a row", "scope": "SEG-01" },
            { "kind": "prevent", "text": "No more coin jokes" },
            { "kind": "banish", "text": "invalid kind" },
            { "kind": "try", "text": "descoped", "scope": "SEG-02B-II" }
        ]
    })
    .to_string();
    let engine = QueueEngine::with(vec![
        Ok(reply),
        Ok(json!({"reply": "Noted.", "directives": []}).to_string()),
    ]);
    let rec = reconciler(data.path(), engine.clone());
    rec.directive_add(DirectiveKind::Prevent, "No more coin jokes", None)
        .unwrap();
    let out = rec
        .lord_chat(None, "Stop roasting the same thrall back to back.")
        .await
        .unwrap();
    assert_eq!(out.reply, "Understood. I will forbid it.");
    assert_eq!(
        out.directives_added.len(),
        1,
        "duplicate and invalid items skipped"
    );
    assert_eq!(
        out.directives_added[0].scope,
        Some(SegmentId::Seg01ImprovisedRoom)
    );
    assert_eq!(rec.directive_list().unwrap().len(), 2);

    // The second turn sees the first in its history.
    let out2 = rec.lord_chat(None, "How did SEG-01 go?").await.unwrap();
    assert!(out2.directives_added.is_empty());
    let prompt = &engine.requests()[1].prompt;
    assert!(prompt.contains("Stop roasting the same thrall"));
    assert!(prompt.contains("Never roast a thrall twice in a row"));
    assert!(rec.lord_chat(None, "  ").await.is_err());
    assert_eq!(
        rec.lord_chat(Some("rec-missing"), "hi")
            .await
            .unwrap_err()
            .code(),
        "not_found"
    );
}

#[test]
fn wire_names_match_the_contract() {
    let d = Directive {
        id: "dir-1-1".into(),
        kind: DirectiveKind::Try,
        text: "t".into(),
        scope: None,
        created_ms: 1,
        active: true,
    };
    assert_eq!(
        serde_json::to_value(&d).unwrap(),
        json!({"id": "dir-1-1", "kind": "try", "text": "t", "created_ms": 1, "active": true})
    );
    let o = PlanOption {
        option_id: "opt-1".into(),
        kind: OptionKind::CharacterChange,
        title: "t".into(),
        rationale: "r".into(),
        evidence: vec![Evidence {
            claim: "c".into(),
            refs: vec!["run-a#1".into()],
        }],
        diff: None,
    };
    assert_eq!(
        serde_json::to_value(&o).unwrap(),
        json!({"option_id": "opt-1", "kind": "character_change", "title": "t", "rationale": "r",
               "evidence": [{"claim": "c", "refs": ["run-a#1"]}]})
    );
    let s: Source = serde_json::from_value(json!({"tournament_id": "t1"})).unwrap();
    assert_eq!(
        s,
        Source::Tournament {
            tournament_id: "t1".into()
        }
    );
    let s: Source = serde_json::from_value(json!({"run_ids": ["a"]})).unwrap();
    assert_eq!(
        s,
        Source::Runs {
            run_ids: vec!["a".into()]
        }
    );
}

#[tokio::test]
async fn list_returns_stored_reports_newest_first_and_next_plan_starts_empty() {
    let data = tempfile::tempdir().unwrap();
    assert!(reconciler(data.path(), QueueEngine::with(vec![]))
        .list()
        .unwrap()
        .is_empty());
    let engine = QueueEngine::with(vec![Ok(synthesis_reply()), Ok(synthesis_reply())]);
    let rec = reconciler(data.path(), engine);
    assert!(rec.next_plan().unwrap().is_none());
    let first = rec.run(tournament()).await.unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let second = rec.run(tournament()).await.unwrap();
    let ids: Vec<String> = rec
        .list()
        .unwrap()
        .into_iter()
        .map(|r| r.report_id)
        .collect();
    assert_eq!(ids, vec![second, first]);
}
