//! SEG-03 Table Read through the run loop, on the keyless `ScriptedEngine`: SEG-01 handing over
//! to SEG-03 with a prep's table read and playing its three rounds, a `--segment SEG-03` run on
//! the fictional sample, and the refusal without a playable scene.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use shadowrunr_engine::ScriptedEngine;
use shadowrunr_segments::scenes::{SAMPLE_GUEST, SAMPLE_SCENE_SOURCES};
use shadowrunr_segments::table::ColdOpenRule;
use shadowrunr_segments::{HandoverRule, RoundTimes, SegmentConfig, SegmentRuntime};
use shadowrunr_types::prep::BeatMode;
use shadowrunr_types::scene::TableReadPlan;
use shadowrunr_types::{ModelTier, Op, SegmentId, Speaker, StreamEvent};

use super::tests::{repo, spec, test_opts};
use super::*;
use crate::transcript;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

/// A copy of Malakor's package that declares the sample's guest as a foil (a test fixture;
/// the real foil file is the character package's own).
fn characters_with_guest(root: &Path) -> PathBuf {
    let chars = root.join("characters");
    let dir = chars.join("malakor");
    copy_dir(&repo().join("characters/malakor"), &dir);
    let manifest = dir.join("character.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    if !text.contains(&format!("\"{SAMPLE_GUEST}\"")) {
        let edited = text.replacen("foils = [", &format!("foils = [\"{SAMPLE_GUEST}\", "), 1);
        assert_ne!(edited, text);
        std::fs::write(&manifest, edited).unwrap();
    }
    let foil = dir.join("foils").join(format!("{SAMPLE_GUEST}.md"));
    if !foil.is_file() {
        std::fs::write(
            foil,
            "# Foil: The Painter\n\nA fictional test guest: an original parody archetype, not \
             any real person.\n",
        )
        .unwrap();
    }
    chars
}

/// `<data>/scenes/the-painter/` holding the shipped sample's files.
fn data_with_scenes(root: &Path) -> PathBuf {
    let data = root.join("data");
    let dir = data.join("scenes").join(SAMPLE_GUEST);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, text) in SAMPLE_SCENE_SOURCES {
        std::fs::write(dir.join(file), text).unwrap();
    }
    data
}

fn plan() -> TableReadPlan {
    TableReadPlan {
        guest: SAMPLE_GUEST.into(),
        scenes: SAMPLE_SCENE_SOURCES
            .iter()
            .map(|(f, _)| f.trim_end_matches(".toml").to_string())
            .collect(),
    }
}

fn sample_prep_with(plan: TableReadPlan) -> shadowrunr_types::prep::StreamPrep {
    let text = std::fs::read_to_string(repo().join("characters/malakor/prep.sample.json")).unwrap();
    let mut p: shadowrunr_types::prep::StreamPrep = serde_json::from_str(&text).unwrap();
    p.table_read = Some(plan);
    p
}

/// What the transcript says of each scene, in order: its id and its lines (`G` for the guest,
/// `H` for the host).
struct Performed {
    id: String,
    lines: Vec<char>,
}

fn performed(events: &[StreamEvent]) -> Vec<Performed> {
    let mut out: Vec<Performed> = Vec::new();
    let mut open: Option<Performed> = None;
    for e in events {
        match e {
            StreamEvent::SceneStart {
                scene_id, guest, ..
            } => {
                assert!(open.is_none(), "scene_start inside a scene");
                assert_eq!(guest, SAMPLE_GUEST);
                open = Some(Performed {
                    id: scene_id.clone(),
                    lines: Vec::new(),
                });
            }
            StreamEvent::Say { speaker, foil, .. } => {
                if let Some(p) = open.as_mut() {
                    p.lines.push(match speaker {
                        Speaker::Foil => {
                            assert_eq!(foil.as_deref(), Some(SAMPLE_GUEST));
                            'G'
                        }
                        Speaker::Host => {
                            assert!(foil.is_none());
                            'H'
                        }
                    });
                }
            }
            StreamEvent::SceneEnd { scene_id, .. } => {
                let p = open.take().expect("scene_end without scene_start");
                assert_eq!(&p.id, scene_id);
                out.push(p);
            }
            _ => {}
        }
    }
    assert!(open.is_none(), "a scene never ended");
    out
}

/// The options of every scene ballot, in order.
fn ballots(events: &[StreamEvent]) -> Vec<Vec<String>> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Op {
                op: Op::StartPoll {
                    poll_id, options, ..
                },
                ..
            } if poll_id.starts_with("scene-ballot-") => Some(options.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn seg01_hands_over_to_the_table_read_and_plays_three_rounds() {
    let tmp = tempfile::tempdir().unwrap();
    let mut opts = test_opts(&tmp.path().join("sim"));
    opts.judge = false;
    opts.characters_root = characters_with_guest(tmp.path());
    opts.data_dir = data_with_scenes(tmp.path());
    opts.stream_prep = Some(sample_prep_with(plan()));
    let sp = spec("table-read", ModelTier::Haiku, 10, 600);
    let out = tmp.path().join("out");
    let prep = prepare(&sp, &out, Arc::new(ScriptedEngine), &opts).unwrap();
    assert_eq!(
        prep.table_read.as_ref().map(|t| t.scenes.len()),
        Some(3),
        "the plan's three scenes are playable"
    );
    let stage = SegmentRuntime::with_config(
        SegmentId::Seg01ImprovisedRoom,
        SegmentConfig {
            start_ms: Some(0),
            cold_open: Some(ColdOpenRule {
                to: "opening".into(),
                min_s: 0,
                max_s: 0,
                active_chatters: 1,
                window_s: 60,
                sustain_s: 0,
            }),
            handover: Some(HandoverRule {
                to: SegmentId::Seg02aHexmap,
                ccv_min: u32::MAX,
                sustain_s: 1,
                max_elapsed_s: 30,
                grace_s: 5,
            }),
            round_times: Some(RoundTimes {
                intro_s: 10,
                ballot_secs: 20,
                talk_s: 30,
                floor_s: 200,
                ceiling_s: 1_000,
                scene_max_s: 120,
            }),
            ..segment_config(&prep).unwrap()
        },
    )
    .unwrap();
    let memory = tmp.path().join("memory");
    std::fs::create_dir_all(&memory).unwrap();
    let brain = build_brain(
        &prep,
        &opts,
        &memory,
        prep.engine.clone(),
        &stage,
        brain_tuning(&opts, &prep),
    )
    .await
    .unwrap();
    let res = execute(prep, brain, stage, &opts).await.unwrap();
    let events = transcript::read(&res.transcript).unwrap();

    // SEG-01, then SEG-03 (not the investigation).
    let segments: Vec<SegmentId> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::SegmentStart { segment, .. } => Some(*segment),
            _ => None,
        })
        .collect();
    assert_eq!(
        segments,
        [SegmentId::Seg01ImprovisedRoom, SegmentId::Seg03TableRead]
    );
    // A ballot of three, then of the two left, then the last scene unvoted.
    let b = ballots(&events);
    assert_eq!(b.len(), 2, "{b:?}");
    assert_eq!(b[0].len(), 3);
    assert_eq!(b[1].len(), 2);
    let played = performed(&events);
    let ids: Vec<&str> = played.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(played.len(), 3, "{ids:?}");
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 3, "each scene once: {ids:?}");
    // The second ballot offers only the two not played; the third scene is the one left.
    let title = |id: &str| {
        events
            .iter()
            .find_map(|e| match e {
                StreamEvent::SceneStart {
                    scene_id, title, ..
                } if scene_id == id => Some(title.clone()),
                _ => None,
            })
            .unwrap()
    };
    assert!(!b[1].contains(&title(ids[0])), "{b:?}");
    assert!(b[1].contains(&title(ids[1])) && b[1].contains(&title(ids[2])));
    for p in &played {
        assert!(p.lines.contains(&'G'), "{}: the guest speaks", p.id);
        assert_eq!(p.lines.last(), Some(&'H'), "{}: the host's last word", p.id);
    }
    // Free talk between the scenes, then the closing.
    let rec: RunRecord = read_json(&out.join("table-read/run.json")).unwrap();
    assert!(rec.tick_errors.is_empty(), "{:?}", rec.tick_errors);
    let plans: Vec<BeatMode> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::BeatPlan { mode, .. } => Some(*mode),
            _ => None,
        })
        .collect();
    assert!(plans.contains(&BeatMode::Scene));
}

#[tokio::test]
async fn a_seg03_run_plays_the_fictional_sample_and_refuses_without_a_playable_scene() {
    let tmp = tempfile::tempdir().unwrap();
    let mut opts = test_opts(&tmp.path().join("sim"));
    opts.judge = false;
    opts.data_dir = tmp.path().join("data");
    let mut sp = spec("seg03", ModelTier::Haiku, 10, 240);
    sp.segment = SegmentId::Seg03TableRead;
    let out = tmp.path().join("out");

    // The guest is not one of the package's foils: nothing is playable, no run.
    if !std::fs::read_to_string(repo().join("characters/malakor/character.toml"))
        .unwrap()
        .contains(&format!("\"{SAMPLE_GUEST}\""))
    {
        let e = run_one_with(sp.clone(), out.clone(), Arc::new(ScriptedEngine), &opts)
            .await
            .unwrap_err();
        assert!(e.to_string().contains("no playable scene"), "{e}");
        assert!(out.join("seg03/error.json").is_file());
    }
    // A prep whose plan names only scenes that do not exist refuses too.
    let mut bad = opts.clone();
    bad.characters_root = characters_with_guest(tmp.path());
    bad.stream_prep = Some(sample_prep_with(TableReadPlan {
        guest: SAMPLE_GUEST.into(),
        scenes: vec!["no-such-scene".into()],
    }));
    let mut sp_bad = sp.clone();
    sp_bad.run_id = "seg03-bad".into();
    let e = run_one_with(sp_bad, out.clone(), Arc::new(ScriptedEngine), &bad)
        .await
        .unwrap_err();
    assert!(
        e.to_string().contains("no-such-scene: no such scene"),
        "{e}"
    );

    // With the guest declared, `--segment SEG-03` plays the compiled-in sample.
    opts.characters_root = characters_with_guest(tmp.path());
    let res = run_one_with(sp, out.clone(), Arc::new(ScriptedEngine), &opts)
        .await
        .unwrap();
    let events = transcript::read(&res.transcript).unwrap();
    assert!(events.iter().any(|e| matches!(
        e,
        StreamEvent::SegmentStart {
            segment: SegmentId::Seg03TableRead,
            ..
        }
    )));
    assert_eq!(ballots(&events).first().map(Vec::len), Some(3));
    assert!(!performed(&events).is_empty());
    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::StreamEnd { .. })));
}
