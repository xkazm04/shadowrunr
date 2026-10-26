//! Scripted sim runs as CI regressions (milestone 4): one keyless `ScriptedEngine` run per live
//! segment (SEG-01, SEG-02A, SEG-03; SEG-02B is descoped), compared with a committed outcome in
//! `tests/expected/<segment>.json`.
//!
//! The outcome holds only what the segment runtime and the run decide: the segment, the
//! segments and handovers in order, the scenes played, the closed votes and their winners, the
//! end reason, the runtime-decided event kinds, that the brain spoke, and that no tick errored.
//! It never holds line text, line counts, which viewer was picked, or the brain's own records
//! (`beat_plan`, `picked`, `moderation`, `job`, `health`, `session`): those belong to the
//! brain's tuning and must not turn this suite red.
//!
//! To accept a deliberate change: `SHADOWRUNR_BLESS=1 cargo test -p shadowrunr-sim --test
//! scripted_regressions`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use shadowrunr_sim::{run_one_with, transcript, Hooks, RunRecord, SimOptions};
use shadowrunr_types::{Effort, ModelTier, Op, SegmentId, SimRunSpec, StreamEvent};

const BLESS_CMD: &str =
    "SHADOWRUNR_BLESS=1 cargo test -p shadowrunr-sim --test scripted_regressions";

/// Event kinds the brain's own choices decide; left out of the pinned set.
const BRAIN_KINDS: [&str; 6] = [
    "beat_plan",
    "picked",
    "moderation",
    "job",
    "health",
    "session",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn kind(ev: &StreamEvent) -> String {
    match serde_json::to_value(ev) {
        Ok(Value::Object(m)) => m["type"].as_str().unwrap_or("?").to_string(),
        _ => "?".into(),
    }
}

/// The segment-level outcome of one run, as the JSON the expectation file holds.
fn outcome(segment: SegmentId, events: &[StreamEvent], rec: &RunRecord) -> Value {
    let segments: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::SegmentStart { segment, .. } => Some(shadowrunr_sim::wire(segment)),
            _ => None,
        })
        .collect();
    let handovers: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Handover { from, to, .. } => Some(format!(
                "{}->{}",
                shadowrunr_sim::wire(from),
                shadowrunr_sim::wire(to)
            )),
            _ => None,
        })
        .collect();
    let scenes_played = events
        .iter()
        .filter(|e| matches!(e, StreamEvent::SceneEnd { .. }))
        .count();
    let polls_opened = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                StreamEvent::Op {
                    op: Op::StartPoll { .. },
                    ..
                }
            )
        })
        .count();
    let mut closed: Vec<Value> = Vec::new();
    for e in events {
        if let StreamEvent::Vote {
            poll_id,
            tallies,
            closed: true,
            ..
        } = e
        {
            let winner = tallies
                .iter()
                .max_by(|a, b| a.weight.total_cmp(&b.weight))
                .map(|t| t.option.clone());
            closed.push(json!({
                "poll_id": poll_id,
                "options": tallies.len(),
                "winner": winner,
            }));
        }
    }
    let kinds: BTreeSet<String> = events
        .iter()
        .map(kind)
        .filter(|k| !BRAIN_KINDS.contains(&k.as_str()))
        .collect();
    let end_reason = events.iter().rev().find_map(|e| match e {
        StreamEvent::StreamEnd { reason, .. } => Some(shadowrunr_sim::wire(reason)),
        _ => None,
    });
    json!({
        "segment": shadowrunr_sim::wire(&segment),
        "segments_in_order": segments,
        "handovers": handovers,
        "scenes_played": scenes_played,
        "polls_opened": polls_opened,
        "closed_votes": closed,
        "end_reason": end_reason,
        "event_kinds": kinds,
        "brain_spoke": events.iter().any(|e| matches!(e, StreamEvent::Say { .. })),
        "tick_errors": rec.tick_errors.len(),
    })
}

async fn run(segment: SegmentId, name: &str, duration_s: u32) -> Value {
    let tmp = tempfile::tempdir().unwrap();
    let sim_root = tmp.path().join("sim");
    std::fs::create_dir_all(&sim_root).unwrap();
    std::fs::copy(repo().join("sim/rubric.toml"), sim_root.join("rubric.toml")).unwrap();
    let opts = SimOptions {
        judge: false,
        characters_root: repo().join("characters"),
        sim_root,
        segments_data_dir: repo().join("crates/shadowrunr-segments/data"),
        data_dir: tmp.path().join("data"),
        hooks: Hooks::default(),
        ..SimOptions::default()
    };
    let spec = SimRunSpec {
        run_id: name.into(),
        character: "malakor".into(),
        tone_variant: "theatrical".into(),
        core_tier: ModelTier::Haiku,
        core_effort: Effort::Low,
        segment,
        audience_size: 10,
        seed: 7,
        duration_s,
        population: "growing_channel".into(),
    };
    let out = tmp.path().join("out");
    let res = run_one_with(
        spec,
        out.clone(),
        Arc::new(shadowrunr_engine::ScriptedEngine),
        &opts,
    )
    .await
    .expect("the scripted run completes");
    let events = transcript::read(&res.transcript).unwrap();
    let text = std::fs::read_to_string(out.join(name).join("run.json")).unwrap();
    let rec: RunRecord = serde_json::from_str(&text).unwrap();
    outcome(segment, &events, &rec)
}

fn check(name: &str, actual: Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/expected")
        .join(format!("{name}.json"));
    if std::env::var_os("SHADOWRUNR_BLESS").is_some_and(|v| v == "1") {
        let text = serde_json::to_string_pretty(&actual).unwrap() + "\n";
        std::fs::write(&path, text).unwrap();
        return;
    }
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}. Create it with: {BLESS_CMD}", path.display()));
    let expected: Value = serde_json::from_str(&text).unwrap();
    let (Some(want), Some(got)) = (expected.as_object(), actual.as_object()) else {
        panic!("{name}: the expectation is not an object. Re-bless with: {BLESS_CMD}");
    };
    let mut diffs = Vec::new();
    for key in want
        .keys()
        .chain(got.keys().filter(|k| !want.contains_key(*k)))
    {
        if want.get(key) != got.get(key) {
            diffs.push(format!(
                "  field `{key}`: expected {}, got {}",
                want.get(key).map_or("(absent)".into(), |v| v.to_string()),
                got.get(key).map_or("(absent)".into(), |v| v.to_string())
            ));
        }
    }
    assert!(
        diffs.is_empty(),
        "scripted {name} run changed against tests/expected/{name}.json:\n{}\n\
         If the change is intended, accept it with: {BLESS_CMD}",
        diffs.join("\n")
    );
}

#[tokio::test]
async fn seg01_improvised_room_outcome_is_unchanged() {
    check(
        "seg01",
        run(SegmentId::Seg01ImprovisedRoom, "seg01", 120).await,
    );
}

#[tokio::test]
async fn seg02a_hexmap_outcome_is_unchanged() {
    check("seg02a", run(SegmentId::Seg02aHexmap, "seg02a", 120).await);
}

#[tokio::test]
async fn seg03_table_read_outcome_is_unchanged() {
    check("seg03", run(SegmentId::Seg03TableRead, "seg03", 240).await);
}
