//! A second character through the daemon: `sim.start` and pre-flight with the keeper fixture
//! (an original test persona) run on the keeper's words, tuning, flavour and rank ladder, and
//! none of Malakor's.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use shadowrunr_engine::ScriptedEngine;
use shadowrunr_types::character::Tuning;
use shadowrunr_types::{
    Engine, EngineError, EngineReply, EngineRequest, HealthReport, Role, StreamEvent, WarmSession,
};

use super::catalog::keeper_root;
use super::{rpc, spec, start_state, test_config};
use crate::engine::EngineHandle;
use crate::server::AppState;

/// The scripted engine, keeping every request (and every warm session's template).
#[derive(Default)]
struct Recording {
    seen: Mutex<Vec<EngineRequest>>,
}

#[async_trait]
impl Engine for Recording {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
        self.seen.lock().unwrap().push(req.clone());
        ScriptedEngine.complete(req).await
    }

    async fn open_session(
        &self,
        template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError> {
        self.seen.lock().unwrap().push(template.clone());
        ScriptedEngine.open_session(template).await
    }

    async fn health(&self) -> Vec<HealthReport> {
        ScriptedEngine.health().await
    }
}

const MALAKOR_WORDS: &[&str] = &[
    "Malakor",
    "Arch-Lich",
    "minion",
    "Lord of the Darkness",
    "bone-clerk",
];

fn keeper_config(tmp: &std::path::Path) -> crate::config::Config {
    let mut cfg = test_config(tmp, keeper_root(tmp));
    cfg.sim.character = "keeper".into();
    cfg
}

#[tokio::test]
async fn a_daemon_session_runs_on_the_keeper_s_words() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recording::default());
    let state = AppState::new(
        keeper_config(tmp.path()),
        EngineHandle {
            engine: rec.clone(),
            fleet: None,
        },
    );
    let addr = start_state(state.clone()).await;
    let mut s = spec("keeper-live");
    s["character"] = json!("keeper");
    s["tone_variant"] = json!("calm");
    let started = rpc(&addr, "sim.start", json!({ "spec": s, "judge": false })).await;
    assert_eq!(started["result"]["run_id"], "keeper-live", "{started}");
    let transcript = tmp
        .path()
        .join("runs/sim/adhoc/keeper-live/transcript.jsonl");
    let done = async {
        while state.live.any_in_flight() || !transcript.is_file() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(120), done)
        .await
        .expect("the run finished in time");

    let text = std::fs::read_to_string(&transcript).unwrap();
    let first: StreamEvent = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    match first {
        StreamEvent::StreamStart {
            character,
            host_name,
            ..
        } => {
            assert_eq!(character, "keeper");
            assert_eq!(host_name.as_deref(), Some("Odile"));
        }
        other => panic!("{other:?}"),
    }
    assert!(text.contains("\"speaker\":\"host\""), "the host spoke");
    for w in MALAKOR_WORDS {
        assert!(!text.contains(w), "{w:?} in the keeper transcript");
    }
    let seen = rec.seen.lock().unwrap().clone();
    let core: Vec<&EngineRequest> = seen.iter().filter(|r| r.role == Role::CoreThread).collect();
    assert!(!core.is_empty());
    assert!(core
        .iter()
        .all(|r| r.system.starts_with("# Constitution: Keeper Odile Marsh")));
    assert!(seen
        .iter()
        .any(|r| r.system.contains("deckhands") || r.prompt.contains("deckhands")));
    // The simulated chatters are left out: the repo's `sim/calibration.json` describes its
    // archetypes in Malakor's words (a known gap of the calibration, not of the package).
    for r in seen.iter().filter(|r| r.role != Role::Chatter) {
        for w in MALAKOR_WORDS {
            let all = format!(
                "{}
---
{}",
                r.system, r.prompt
            );
            if let Some(at) = all.find(w) {
                let from = all.floor_char_boundary(at.saturating_sub(300));
                let to = all.ceil_char_boundary((at + 200).min(all.len()));
                panic!("{w:?} reached a {:?} prompt: ...{}", r.role, &all[from..to]);
            }
        }
    }
}

#[tokio::test]
async fn the_daemon_s_segment_config_carries_the_keeper_s_tuning_and_ladder() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = keeper_config(tmp.path());
    let dir = cfg.paths.characters.join("keeper");
    let pkg = shadowrunr_brain::load_character(&dir, "").unwrap();
    let sc = crate::character::segment_config(&cfg, &pkg).unwrap();
    assert_eq!(sc.persona.followers, "deckhands");
    assert_eq!(sc.tuning, pkg.tuning);
    assert_eq!(sc.tuning.carry_ms, 30_000);
    assert_ne!(sc.tuning, Tuning::DEFAULT);
    assert!(sc.flavour.values().any(|f| f
        .rules_prompt
        .as_deref()
        .is_some_and(|r| r.contains("night watch"))));
    // Pre-flight starts every segment on that config: green for the keeper ...
    let pf = crate::preflight::run_all_checked(&cfg, &EngineHandle::scripted()).await;
    let pf04 = pf.iter().find(|c| c.report.check == "PF-04").unwrap();
    assert_eq!(
        pf04.report.state,
        shadowrunr_types::HealthState::Green,
        "{pf04:?}"
    );
    // ... and red once its rank overrides break the ladder (weights must not fall up it).
    let manifest = dir.join("character.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        text.replace("[ranks]\n", "[ranks]\nweights = { dread_council = 0.1 }\n"),
    )
    .unwrap();
    let pf = crate::preflight::run_all_checked(&cfg, &EngineHandle::scripted()).await;
    let pf04 = pf.iter().find(|c| c.report.check == "PF-04").unwrap();
    assert_eq!(
        pf04.report.state,
        shadowrunr_types::HealthState::Red,
        "{pf04:?}"
    );
    assert!(pf04.report.detail.contains("[ranks]"), "{pf04:?}");
    // The ranks table the console shows is the same ladder.
    let state = AppState::new(cfg, EngineHandle::scripted());
    let addr = start_state(state).await;
    let v = rpc(&addr, "ranks.table", json!({ "character": "keeper" })).await;
    assert_eq!(v["error"]["code"], "io", "{v}");
}
