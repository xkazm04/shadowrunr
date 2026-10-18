//! WP12: the synthesis prompt fits a token budget at any tournament size. Synthetic
//! tournaments, built in memory or in a temp dir; no quota is spent.

use std::path::Path;

use serde_json::{json, Value};
use shadowrunr_types::{JudgeVerdict, SimRunSpec, StreamEvent};

use crate::budget::{self, PromptBudget};
use crate::runs::{RefIndex, RunData};
use crate::tests::{queue_engine, reconciler_at};
use crate::*;

const TONES: [&str; 3] = ["theatrical", "deadpan-bureaucrat", "weary-middle-manager"];
const TIERS: [&str; 2] = ["haiku", "sonnet"];
const SEGS: [&str; 2] = ["SEG-01", "SEG-02A"];
const AUDS: [u32; 3] = [3, 30, 300];

fn long(prefix: &str, i: usize, k: usize, len: usize) -> String {
    let mut s = format!("{prefix} {i}-{k}: ");
    while s.len() < len {
        s.push_str("the ledger of the damned grows longer by the hour; ");
    }
    s.truncate(len);
    s
}

/// One synthetic run: `beats` chat/say beats with input moderation, a handover half way,
/// blocked output every so often, and (when `judged`) a verdict whose rule failures cite say
/// lines. `fail_every` = 0 means no rule failures.
struct Gen {
    events: Vec<Value>,
    says: Vec<usize>,
}

fn gen_events(run_id: &str, i: usize, beats: usize, seg: &str, tone: &str, aud: u32) -> Gen {
    let other = if seg == "SEG-01" { "SEG-02A" } else { "SEG-01" };
    let mut ev = vec![
        json!({"type":"stream_start","ts_ms":0,"stream_id":run_id,"character":"malakor",
               "tone_variant":tone,"core_tier":"haiku","audience_size":aud}),
        json!({"type":"segment_start","ts_ms":0,"segment":seg}),
    ];
    let mut says = Vec::new();
    let mut t = 0i64;
    for k in 0..beats {
        t += 5_000;
        if k == beats / 2 {
            ev.push(json!({"type":"handover","ts_ms":t,"from":seg,"to":other,"reason":"planned"}));
            ev.push(json!({"type":"segment_end","ts_ms":t,"segment":seg,"ses":0.5}));
            ev.push(json!({"type":"segment_start","ts_ms":t + 1000,"segment":other}));
        }
        let id = format!("m{k}");
        ev.push(json!({"type":"chat","ts_ms":t,"msg":{"id":id,"ts_ms":t,
            "viewer":{"id":format!("v{}", k % 40),"name":format!("Viewer{}", k % 40),
                      "platform":"sim","rank":"thrall"},
            "text":long("chat", i, k, 280)}}));
        if k.is_multiple_of(3) {
            ev.push(
                json!({"type":"moderation","ts_ms":t,"stage":"input","rule":"spam",
                           "msg_id":id,"action":"drop"}),
            );
        }
        if k % 17 == 5 {
            ev.push(json!({"type":"moderation","ts_ms":t,"stage":"output",
                           "rule":"red-line:hateful-conduct","action":"block"}));
        }
        ev.push(json!({"type":"say","ts_ms":t + 900,"speaker":"malakor",
                       "text":long("MALAKOR", i, k, 420),"replying_to":[id],
                       "first_text_ms":800}));
        says.push(ev.len());
    }
    ev.push(json!({"type":"segment_end","ts_ms":t + 2000,"segment":other,"ses":0.7}));
    ev.push(json!({"type":"stream_end","ts_ms":t + 3000,"reason":"sim_complete"}));
    Gen { events: ev, says }
}

fn verdict(run_id: &str, i: usize, says: &[usize], fail_every: usize) -> Value {
    let rules: Vec<Value> = (0..12)
        .map(|j| {
            if fail_every > 0 && (i + j).is_multiple_of(fail_every) {
                let a = says[(i * 7 + j * 3) % says.len()];
                let b = says[(i * 11 + j * 5 + 1) % says.len()];
                json!({"rule":format!("red-line:rule-{j}"),"state":"fail",
                       "evidence":[format!("{run_id}#{a}"), format!("{run_id}#{b}")]})
            } else {
                json!({"rule":format!("red-line:rule-{j}"),"state":"pass"})
            }
        })
        .collect();
    let quality: Vec<Value> = (0..10)
        .map(|q| {
            let l = says[(i + q * 13) % says.len()];
            let dim = ["humor", "tone", "responsiveness"][q % 3];
            json!({"dimension":dim,
                   "value":!(i + q).is_multiple_of(3),"turn_ref":format!("{run_id}#{l}"),"quote":"x"})
        })
        .collect();
    json!({"run_id":run_id,"judge_tier":"sonnet","rules":rules,"quality":quality})
}

fn spec(run_id: &str, i: usize) -> Value {
    json!({"run_id":run_id,"character":"malakor","tone_variant":TONES[i % 3],
           "core_tier":TIERS[(i / 3) % 2],"core_effort":"low","segment":SEGS[(i / 6) % 2],
           "audience_size":AUDS[(i / 12) % 3],"seed":i,"duration_s":600,
           "population":"growing_channel"})
}

fn run_id(i: usize) -> String {
    format!(
        "{}.{}.{}.a{}.s{i}",
        TONES[i % 3],
        TIERS[(i / 3) % 2],
        SEGS[(i / 6) % 2],
        AUDS[(i / 12) % 3]
    )
}

/// In-memory synthetic tournament.
fn synthetic(n: usize, beats: usize, fail_every: impl Fn(usize) -> usize) -> Vec<RunData> {
    (0..n)
        .map(|i| {
            let id = run_id(i);
            let g = gen_events(
                &id,
                i,
                beats,
                SEGS[(i / 6) % 2],
                TONES[i % 3],
                AUDS[(i / 12) % 3],
            );
            let fe = fail_every(i);
            RunData {
                run_id: id.clone(),
                tournament_id: Some("t-big".into()),
                dir: std::path::PathBuf::new(),
                spec: Some(serde_json::from_value::<SimRunSpec>(spec(&id, i)).unwrap()),
                events: g
                    .events
                    .into_iter()
                    .map(|e| Some(serde_json::from_value::<StreamEvent>(e).unwrap()))
                    .collect(),
                verdict: Some(
                    serde_json::from_value::<JudgeVerdict>(verdict(&id, i, &g.says, fe)).unwrap(),
                ),
                ledger: vec![
                    json!({"role":"core_thread","duration_ms":900 + i,"first_text_ms":700}),
                ],
                instrument_hash: Some("ih-1".into()),
            }
        })
        .collect()
}

fn tones() -> Vec<String> {
    TONES.iter().map(|t| t.to_string()).collect()
}

/// The `<run_id>#<line>` label of every excerpt line in a prompt (inside the excerpt fence).
fn excerpt_refs(prompt: &str) -> Vec<String> {
    let (_, ex) = prompt
        .split_once("## Transcript excerpts (untrusted data)\n")
        .expect("excerpt section");
    let open = ex
        .find(shadowrunr_sim::judge::FENCE_OPEN)
        .expect("fence open");
    let close = ex
        .find(shadowrunr_sim::judge::FENCE_CLOSE)
        .expect("fence close");
    ex[open..close]
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty() && !l.starts_with("### "))
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect()
}

fn total_tokens(prompt: &str) -> u32 {
    budget::estimate_tokens(
        prompt.chars().count()
            + synth::SYSTEM.chars().count()
            + synth::synthesis_schema().to_string().chars().count(),
    )
}

#[test]
fn a_500_run_tournament_fits_the_budget() {
    let runs = synthetic(500, 60, |_| 4);
    let metrics = analyze(&runs);
    // Unbudgeted, the raw metrics alone are far over any context window.
    let raw = serde_json::to_string_pretty(&metrics).unwrap();
    assert!(
        budget::estimate_tokens(raw.len()) > 120_000,
        "{}",
        raw.len()
    );

    let b = PromptBudget::default();
    let built = budget::synthesis_prompt(&runs, &metrics, None, &tones(), b).unwrap();
    let est = total_tokens(&built.prompt);
    assert_eq!(built.input.estimated_tokens, est);
    assert!(
        f64::from(est) <= f64::from(b.tokens) * (1.0 - budget::SAFETY_MARGIN),
        "estimated {est} tokens"
    );
    let inp = &built.input;
    assert_eq!(inp.budget_tokens, 120_000);
    assert!(
        inp.runs.omitted > 0 || inp.failure_refs.omitted > 0,
        "{inp:?}"
    );
    assert!(inp.incidents.omitted > 0, "{inp:?}");
    assert!(inp.excerpt_lines > 0);
    assert!(!inp.omitted.is_empty());
    assert!(built.prompt.contains("## Omitted for size"));
    for note in &inp.omitted {
        assert!(built.prompt.contains(note.as_str()), "{note}");
    }
    // The metrics block still counts everything.
    assert!(built
        .prompt
        .contains(&format!("\"total\":{}", metrics.red_lines.len())));
}

#[test]
fn sampled_excerpt_refs_resolve() {
    let runs = synthetic(120, 40, |i| if i.is_multiple_of(2) { 3 } else { 0 });
    let metrics = analyze(&runs);
    let built =
        budget::synthesis_prompt(&runs, &metrics, None, &tones(), PromptBudget::new(40_000))
            .unwrap();
    let index = RefIndex::new(&runs);
    let refs = excerpt_refs(&built.prompt);
    assert!(refs.len() > 50, "{}", refs.len());
    for r in &refs {
        assert_eq!(index.resolve(r).as_deref(), Ok(r.as_str()), "{r}");
    }
    assert_eq!(built.input.excerpt_lines as usize, refs.len());
}

#[test]
fn failure_lines_are_prioritised() {
    // 200 runs, only three of them with rule failures, late in the run order; the budget fits
    // a small fraction of the transcripts.
    let failing = [150usize, 170, 199];
    let runs = synthetic(200, 40, |i| if failing.contains(&i) { 2 } else { 0 });
    let metrics = analyze(&runs);
    let built =
        budget::synthesis_prompt(&runs, &metrics, None, &tones(), PromptBudget::new(25_000))
            .unwrap();
    let inp = &built.input;
    assert!(inp.runs.omitted > 0, "the budget must force cuts: {inp:?}");
    assert!(inp.failure_refs.included > 0);
    assert_eq!(inp.failure_refs.omitted, 0, "{inp:?}");
    let shown: std::collections::HashSet<String> =
        excerpt_refs(&built.prompt).into_iter().collect();
    for i in failing {
        let v = runs[i].verdict.as_ref().unwrap();
        for r in v
            .rules
            .iter()
            .filter(|r| r.state == shadowrunr_types::CheckState::Fail)
        {
            for e in &r.evidence {
                assert!(shown.contains(e), "cited failure line {e} not excerpted");
            }
        }
    }
    // Under a tight budget cited lines are shown at a far higher rate than lines in general.
    let runs = synthetic(300, 40, |_| 4);
    let metrics = analyze(&runs);
    let built =
        budget::synthesis_prompt(&runs, &metrics, None, &tones(), PromptBudget::new(30_000))
            .unwrap();
    let inp = &built.input;
    let total_lines: usize = runs.iter().map(|r| r.events.len()).sum();
    let cited_share = f64::from(inp.failure_refs.included)
        / f64::from(inp.failure_refs.included + inp.failure_refs.omitted);
    let line_share = f64::from(inp.excerpt_lines) / total_lines as f64;
    assert!(
        cited_share > 4.0 * line_share,
        "cited {cited_share:.3} vs all lines {line_share:.3}"
    );
}

fn write_run(dir: &Path, rd: &RunData, i: usize) {
    std::fs::create_dir_all(dir).unwrap();
    let lines: Vec<String> = rd
        .events
        .iter()
        .map(|e| serde_json::to_string(e.as_ref().unwrap()).unwrap())
        .collect();
    std::fs::write(dir.join("transcript.jsonl"), lines.join("\n") + "\n").unwrap();
    std::fs::write(
        dir.join("spec.json"),
        serde_json::to_string(&spec(&rd.run_id, i)).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("verdict.json"),
        serde_json::to_string(rd.verdict.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn a_cut_prompt_is_recorded_in_the_stored_report() {
    let tmp = tempfile::tempdir().unwrap();
    let runs_root = tmp.path().join("sim");
    let runs = synthetic(48, 40, |_| 3);
    for (i, r) in runs.iter().enumerate() {
        write_run(&runs_root.join("t-big").join(&r.run_id), r, i);
    }
    let cited = runs[0].verdict.as_ref().unwrap().rules[0].evidence.clone();
    let reply = json!({
        "summary": "Too many rule failures.",
        "segments": [{ "segment": "SEG-01", "what_worked": [],
                       "what_failed": [{ "claim": "rule-0 failed", "refs": cited }] }],
        "options": [{ "option_id": "opt-1", "kind": "experiment", "title": "Fewer red lines",
                      "rationale": "n is real now", "evidence": [] }]
    })
    .to_string();
    let engine = queue_engine(vec![Ok(reply)]);
    let data = tmp.path().join("data");
    let rec = reconciler_at(&data, &runs_root, engine.clone()).with_prompt_budget(20_000);
    assert_eq!(rec.prompt_budget().tokens, 20_000);
    let id = rec
        .run(Source::Tournament {
            tournament_id: "t-big".into(),
        })
        .await
        .unwrap();

    let prompt = engine.requests()[0].prompt.clone();
    assert!(prompt.contains("## Omitted for size"));
    assert!(f64::from(total_tokens(&prompt)) <= 20_000.0 * (1.0 - budget::SAFETY_MARGIN));

    let text = std::fs::read_to_string(data.join("reconcile").join(format!("{id}.json"))).unwrap();
    assert!(!text.contains("null"));
    let stored: StoredReport = serde_json::from_str(&text).unwrap();
    let inp = stored.synthesis_input.expect("synthesis_input stored");
    assert_eq!(inp.budget_tokens, 20_000);
    assert_eq!(inp.estimated_tokens, total_tokens(&prompt));
    assert!(!inp.omitted.is_empty());
    assert_eq!(inp.runs.included + inp.runs.omitted, 48);
    // The synthesis itself still lands, and its refs still verify.
    assert_eq!(stored.proposals.len(), 1);
    assert_eq!(stored.report.segments[0].what_failed.len(), 1);
}

#[test]
fn a_small_source_is_not_cut() {
    let runs = synthetic(6, 20, |_| 3);
    let metrics = analyze(&runs);
    let built =
        budget::synthesis_prompt(&runs, &metrics, None, &tones(), PromptBudget::default()).unwrap();
    let inp = &built.input;
    assert!(inp.omitted.is_empty(), "{inp:?}");
    assert!(!built.prompt.contains("## Omitted for size"));
    assert_eq!(inp.runs.omitted, 0);
    assert_eq!(inp.failure_refs.omitted, 0);
    assert_eq!(inp.incidents.omitted, 0, "{inp:?}");
}

/// Measures the real weary tournament when it is on disk (`cargo test -- --ignored`).
#[test]
#[ignore = "reads runs/sim/weary-0dd890a1ad from the repo"]
fn weary_tournament_prompt_size() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runs/sim");
    let src = Source::Tournament {
        tournament_id: "weary-0dd890a1ad".into(),
    };
    let Ok(runs) = load_source(&root, &src) else {
        return;
    };
    let metrics = analyze(&runs);
    let chars = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../characters/malakor");
    let built = budget::synthesis_prompt(
        &runs,
        &metrics,
        Some(&chars),
        &["weary-middle-manager".to_string()],
        PromptBudget::default(),
    )
    .unwrap();
    eprintln!(
        "prompt {} chars, {:#?}",
        built.prompt.chars().count(),
        built.input
    );
    assert!(f64::from(built.input.estimated_tokens) <= 120_000.0 * (1.0 - budget::SAFETY_MARGIN));
}
