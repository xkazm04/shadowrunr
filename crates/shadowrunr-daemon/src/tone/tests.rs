//! The Tone Lab over a temp copy of Malakor's package (never the repo's files) and a fake core
//! model (no live model).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;
use shadowrunr_engine::ScriptedEngine;
use shadowrunr_types::review::{FeedbackTag, LineFeedback};
use shadowrunr_types::tone::*;
use shadowrunr_types::{
    Engine, EngineError, EngineKind, EngineReply, EngineRequest, ErrorCode, HealthReport, Role,
    Usage, WarmSession,
};

use super::*;
use crate::e2e::{copy_dir, repo, test_config};
use crate::review::tests::{write_session, SID};

const TONE: &str = "theatrical";

/// A config whose characters dir is a temp copy holding `malakor` and the `keeper` fixture.
fn setup(tmp: &Path) -> (Config, PathBuf) {
    let chars = tmp.join("characters");
    copy_dir(&repo().join("characters/malakor"), &chars.join("malakor"));
    copy_dir(
        &repo().join("crates/shadowrunr-brain/tests/fixtures/characters/keeper"),
        &chars.join("keeper"),
    );
    let cfg = test_config(tmp, chars.clone());
    (cfg, chars.join("malakor"))
}

fn bank_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("lines").join(format!("{TONE}.toml"))).unwrap()
}

fn add(text: &str, tags: Vec<LineSituation>) -> ToneLinesAddParams {
    ToneLinesAddParams {
        character: "malakor".into(),
        tone: TONE.into(),
        text: text.into(),
        tags,
        status: None,
    }
}

fn list(cfg: &Config) -> ToneLines {
    lines_list(
        cfg,
        &ToneLinesParams {
            character: "malakor".into(),
            tone: TONE.into(),
        },
    )
    .unwrap()
}

#[test]
fn the_contract_situations_are_the_brains_tags() {
    let wire: Vec<&str> = LineSituation::ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(wire, shadowrunr_brain::character::SITUATION_TAGS);
    for s in LineSituation::ALL {
        assert_eq!(serde_json::to_value(s).unwrap(), json!(s.as_str()));
    }
}

#[test]
fn a_line_is_appended_approved_then_retired_by_text_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, dir) = setup(tmp.path());
    let before = bank_text(&dir);
    let seeds = list(&cfg);
    assert_eq!(seeds.file, "lines/theatrical.toml");
    assert_eq!(seeds.lines.len(), 25);
    assert!(seeds.lines.iter().all(|l| l.status == LineStatus::Seed));

    let w = lines_add(
        &cfg,
        add(
            "  Kneel, \"moth_toad\":\n your roast is   ready. ",
            vec![LineSituation::RoastRequest, LineSituation::RoastRequest],
        ),
    )
    .unwrap();
    assert_eq!(w.file, "lines/theatrical.toml");
    assert_eq!(w.line.id, "th-26");
    assert_eq!(w.line.status, LineStatus::Approved);
    assert_eq!(w.line.tags, vec![LineSituation::RoastRequest]);
    assert_eq!(w.line.text, "Kneel, \"moth_toad\": your roast is ready.");
    let after = bank_text(&dir);
    assert!(
        after.starts_with(&before),
        "the owner's text is kept as written"
    );
    assert!(after.ends_with(
        "\n\n[[line]]\nid = \"th-26\"\nstatus = \"approved\"\ntags = [\"roast_request\"]\ntext = \"Kneel, \\\"moth_toad\\\": your roast is ready.\"\n"
    ), "{after}");
    assert!(crate::character::validate(&dir).is_empty());

    // Approve a seed: one line of the file changes.
    let w = lines_set_status(
        &cfg,
        &ToneLinesSetStatusParams {
            character: "malakor".into(),
            tone: TONE.into(),
            id: "th-02".into(),
            status: LineStatus::Approved,
        },
    )
    .unwrap();
    assert_eq!(w.line.status, LineStatus::Approved);
    let approved = bank_text(&dir);
    let changed: Vec<(&str, &str)> = after
        .lines()
        .zip(approved.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        changed,
        vec![("status = \"seed\"", "status = \"approved\"")]
    );
    // Retire the new line; setting the same status again writes nothing.
    let p = ToneLinesSetStatusParams {
        character: "malakor".into(),
        tone: TONE.into(),
        id: "th-26".into(),
        status: LineStatus::Retired,
    };
    lines_set_status(&cfg, &p).unwrap();
    let retired = bank_text(&dir);
    lines_set_status(&cfg, &p).unwrap();
    assert_eq!(bank_text(&dir), retired);
    let l = list(&cfg);
    assert_eq!(l.lines.len(), 26);
    assert_eq!(l.lines[1].status, LineStatus::Approved);
    assert_eq!(l.lines[25].status, LineStatus::Retired);
    let e = lines_set_status(
        &cfg,
        &ToneLinesSetStatusParams {
            id: "th-99".into(),
            ..p
        },
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    assert!(crate::character::validate(&dir).is_empty());
}

#[test]
fn a_bad_line_is_refused_and_nothing_is_written() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, dir) = setup(tmp.path());
    let before = bank_text(&dir);
    let seed = list(&cfg).lines[0].text.clone();
    let cases: Vec<(ToneLinesAddParams, ErrorCode, &str)> = vec![
        (
            add("   ", vec![LineSituation::Bait]),
            ErrorCode::InvalidParams,
            "empty",
        ),
        (
            add("A line.", vec![]),
            ErrorCode::InvalidParams,
            "situation",
        ),
        (
            add(&"x".repeat(401), vec![LineSituation::Bait]),
            ErrorCode::InvalidParams,
            "longer",
        ),
        (
            add("Hail {nobody}.", vec![LineSituation::Bait]),
            ErrorCode::InvalidParams,
            "placeholder",
        ),
        (
            ToneLinesAddParams {
                status: Some(LineStatus::Retired),
                ..add("A line.", vec![LineSituation::Bait])
            },
            ErrorCode::InvalidParams,
            "never retired",
        ),
        (
            add(&seed.to_uppercase(), vec![LineSituation::Bait]),
            ErrorCode::Conflict,
            "th-01",
        ),
    ];
    for (p, code, says) in cases {
        let e = lines_add(&cfg, p).unwrap_err();
        assert_eq!(e.code, code, "{e}");
        assert!(e.message.contains(says), "{e}");
    }
    assert_eq!(bank_text(&dir), before);
}

#[test]
fn a_write_that_breaks_the_package_is_undone() {
    let tmp = tempfile::tempdir().unwrap();
    let (_cfg, dir) = setup(tmp.path());
    let path = dir.join("lines").join(format!("{TONE}.toml"));
    let before = bank_text(&dir);
    let broken = before.replacen("tags = [\"bait\"]", "tags = [\"nonsense\"]", 1);
    let e = write_validated(&dir, &path, "lines/theatrical.toml", &broken).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert!(e.message.contains("unchanged"), "{e}");
    assert!(e.detail.unwrap()["issues"][0]["file"] == "lines/theatrical.toml");
    assert_eq!(bank_text(&dir), before);
    // A new file that breaks it is removed again.
    let fresh = dir.join("lines").join("calm.toml");
    let e = write_validated(&dir, &fresh, "lines/calm.toml", &bank::new_file("calm"));
    assert!(e.is_err());
    assert!(!fresh.exists());
    let leftovers: Vec<_> = std::fs::read_dir(dir.join("lines"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("tone-lab"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn writes_stay_inside_the_characters_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, _) = setup(tmp.path());
    let snapshot = || {
        let mut v = Vec::new();
        fn walk(p: &Path, v: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(p).unwrap().filter_map(|e| e.ok()) {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, v);
                } else {
                    v.push(p);
                }
            }
        }
        walk(tmp.path(), &mut v);
        v.sort();
        v
    };
    let files = snapshot();
    for character in [
        "..",
        "../characters/malakor",
        "malakor/..",
        "",
        "..\\malakor",
        "/etc",
    ] {
        let mut p = add("A line.", vec![LineSituation::Bait]);
        p.character = character.into();
        let e = lines_add(&cfg, p).unwrap_err();
        assert!(
            matches!(e.code, ErrorCode::InvalidParams | ErrorCode::NotFound),
            "{character}: {e}"
        );
        let e = directions_set(
            &cfg,
            &ToneDirectionsSetParams {
                character: character.into(),
                text: "x".into(),
            },
        )
        .unwrap_err();
        assert!(
            matches!(e.code, ErrorCode::InvalidParams | ErrorCode::NotFound),
            "{character}: {e}"
        );
    }
    for tone in [
        "../constitution",
        "..\\constitution",
        "../../../evil",
        "theatrical/../x",
        "..",
        "",
        "C:\\evil",
    ] {
        let mut p = add("A line.", vec![LineSituation::Bait]);
        p.tone = tone.into();
        assert_eq!(
            lines_add(&cfg, p).unwrap_err().code,
            ErrorCode::InvalidParams,
            "{tone}"
        );
    }
    let mut p = add("A line.", vec![LineSituation::Bait]);
    p.tone = "no-such-tone".into();
    assert_eq!(lines_add(&cfg, p).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(snapshot(), files, "no file was written or removed");
}

#[test]
fn directions_round_trip_as_plain_text() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, dir) = setup(tmp.path());
    let d = directions_get(&cfg, "malakor").unwrap();
    assert_eq!(d.file, "directions.md");
    assert!(d.text.starts_with("# Directions"));
    let set = directions_set(
        &cfg,
        &ToneDirectionsSetParams {
            character: "malakor".into(),
            text: "# Directions\r\n\r\n## Do\r\n\r\n- Bite harder.\r\n\r\n\r\n".into(),
        },
    )
    .unwrap();
    assert_eq!(set.text, "# Directions\n\n## Do\n\n- Bite harder.\n");
    assert_eq!(
        std::fs::read_to_string(dir.join("directions.md")).unwrap(),
        set.text
    );
    assert_eq!(directions_get(&cfg, "malakor").unwrap().text, set.text);
    // The keeper fixture has no directions: empty, then written.
    assert_eq!(directions_get(&cfg, "keeper").unwrap().text, "");
    let e = directions_set(
        &cfg,
        &ToneDirectionsSetParams {
            character: "malakor".into(),
            text: "x".repeat(MAX_DIRECTIONS_CHARS + 1),
        },
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
}

// ── tone.variants ──────────────────────────────────────────────────────

/// Plays the core model with a fixed reply; keeps the requests.
struct FakeCore {
    reply: Result<String, ()>,
    requests: Mutex<Vec<EngineRequest>>,
}

impl FakeCore {
    fn new(reply: Result<String, ()>) -> Self {
        Self {
            reply,
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Engine for FakeCore {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
        self.requests.lock().unwrap().push(req);
        let text = self.reply.clone().map_err(|()| EngineError::Unavailable {
            reason: "fake fleet down".into(),
        })?;
        Ok(EngineReply {
            text,
            engine: EngineKind::Scripted,
            account: None,
            model: "fake-core".into(),
            first_text_ms: None,
            duration_ms: 1,
            usage: Usage::default(),
            cost_usd: None,
            fallback_reason: None,
            session_id: None,
        })
    }
    async fn open_session(&self, t: EngineRequest) -> Result<Box<dyn WarmSession>, EngineError> {
        ScriptedEngine.open_session(t).await
    }
    async fn health(&self) -> Vec<HealthReport> {
        Vec::new()
    }
}

fn variants_params(n: u32) -> ToneVariantsParams {
    ToneVariantsParams {
        character: "malakor".into(),
        tone: TONE.into(),
        situation: LineSituation::RoastRequest,
        line: Some("Moth, you are a rounding error.".into()),
        note: Some("Meaner, and name the vote he lost.".into()),
        n,
    }
}

#[tokio::test]
async fn variants_ask_the_core_model_and_keep_only_new_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, dir) = setup(tmp.path());
    let before = bank_text(&dir);
    let seed = list(&cfg).lines[0].text.clone();
    let reply = json!({ "lines": [
        "Moth, you lost the vote to a swamp.",
        seed,
        "moth, you are a rounding error.",
        "",
        "Moth, you lost the vote to a swamp.",
        "Second  angle,\nnew line.",
        "Third.",
        "Fourth, over the limit."
    ]})
    .to_string();
    let core = FakeCore::new(Ok(reply));
    let v = variants_with(Arc::new(cfg.clone()), &core, variants_params(3))
        .await
        .unwrap();
    assert_eq!(
        v.candidates,
        vec![
            "Moth, you lost the vote to a swamp.",
            "Second angle, new line.",
            "Third."
        ]
    );
    assert_eq!(v.gated, 0);
    let reqs = core.requests.lock().unwrap();
    let r = &reqs[0];
    assert_eq!(r.role, Role::CoreThread);
    assert_eq!(
        (r.tier, r.effort, r.timeout_ms),
        cfg.engine.route(Role::CoreThread)
    );
    assert!(r.json_schema.is_some());
    assert_eq!(r.correlation_id.as_deref(), Some("tone.variants:malakor"));
    assert!(r.system.contains("# The tone"));
    assert!(r.system.contains("# Directions from the owner"));
    assert!(r.prompt.contains("a viewer asks to be roasted"));
    assert!(r.prompt.contains("Write 3 new lines"));
    assert!(r
        .prompt
        .contains("<<<tone-lab\nMoth, you are a rounding error.\ntone-lab>>>"));
    assert!(r.prompt.contains("Meaner, and name the vote he lost."));
    assert!(
        r.prompt.contains("- "),
        "bank lines of the situation are shown"
    );
    assert_eq!(bank_text(&dir), before, "variants write nothing");
}

#[tokio::test]
async fn variants_refuse_bad_params_and_report_the_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, _) = setup(tmp.path());
    let cfg = Arc::new(cfg);
    let core = FakeCore::new(Ok("{}".into()));
    for n in [0, 6] {
        let e = variants_with(cfg.clone(), &core, variants_params(n))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams);
    }
    let mut p = variants_params(2);
    p.note = Some("x".repeat(MAX_INPUT_CHARS + 1));
    assert_eq!(
        variants_with(cfg.clone(), &core, p).await.unwrap_err().code,
        ErrorCode::InvalidParams
    );
    let mut p = variants_params(2);
    p.tone = "../tones/theatrical".into();
    assert_eq!(
        variants_with(cfg.clone(), &core, p).await.unwrap_err().code,
        ErrorCode::InvalidParams
    );
    assert!(core.requests.lock().unwrap().is_empty());
    let down = FakeCore::new(Err(()));
    let e = variants_with(cfg.clone(), &down, variants_params(2))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Engine);
    // The scripted engine's echo is no JSON: its plain lines come back as candidates.
    let v = variants_with(cfg, &ScriptedEngine, variants_params(2))
        .await
        .unwrap();
    assert!(!v.candidates.is_empty() && v.candidates.len() <= 2);
    assert!(v.candidates[0].starts_with("[scripted CoreThread]"));
}

// ── tone.inbox / tone.review_line ──────────────────────────────────────

fn mark(
    cfg: &Config,
    session_id: &str,
    line: u64,
    tags: Vec<FeedbackTag>,
    note: Option<&str>,
    at_ms: i64,
) {
    let row = LineFeedback {
        session_id: session_id.into(),
        line,
        tags,
        note: note.map(String::from),
        at_ms,
    };
    crate::review::store::append(
        &crate::review::store::feedback_path(&cfg.paths.review_dir()),
        &row,
    )
    .unwrap();
}

#[test]
fn review_feedback_becomes_candidates_and_suggestions() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, _) = setup(tmp.path());
    write_session(&cfg.paths.sandbox_runs(), SID, 1_000);
    let other = "sbx-20261020-130000-5m";
    let odir = write_session(&cfg.paths.sandbox_runs(), other, 2_000);
    // The second session is the keeper's: its marks never reach Malakor's inbox.
    let t = std::fs::read_to_string(odir.join("transcript.jsonl"))
        .unwrap()
        .replace("\"character\":\"malakor\"", "\"character\":\"keeper\"");
    std::fs::write(odir.join("transcript.jsonl"), t).unwrap();
    mark(&cfg, SID, 4, vec![FeedbackTag::GreatRoast], None, 10);
    mark(
        &cfg,
        SID,
        6,
        vec![FeedbackTag::Flat],
        Some("Say why the vote matters."),
        20,
    );
    mark(
        &cfg,
        SID,
        9,
        vec![FeedbackTag::Landed, FeedbackTag::TooMean],
        None,
        30,
    );
    mark(&cfg, other, 4, vec![FeedbackTag::GreatRoast], None, 40);
    // A mark that feeds neither list, and a missing session, are skipped.
    mark(
        &cfg,
        SID,
        6,
        vec![FeedbackTag::Flat, FeedbackTag::WrongTag],
        Some("Say why the vote matters."),
        21,
    );
    mark(
        &cfg,
        "sbx-20261020-140000-5m",
        4,
        vec![FeedbackTag::Landed],
        None,
        50,
    );

    let ib = inbox(&cfg, "malakor").unwrap();
    let c: Vec<(u64, Option<LineSituation>)> = ib
        .candidates
        .iter()
        .map(|i| (i.line, i.situation))
        .collect();
    assert_eq!(c, vec![(9, None), (4, Some(LineSituation::RoastRequest))]);
    let first = &ib.candidates[1];
    assert_eq!(first.text, "Moth, you are a rounding error.");
    assert_eq!(first.tone, "theatrical");
    assert_eq!(first.tags, vec![FeedbackTag::GreatRoast]);
    let s: Vec<(u64, Option<&str>)> = ib
        .suggestions
        .iter()
        .map(|i| (i.line, i.note.as_deref()))
        .collect();
    assert_eq!(s, vec![(9, None), (6, Some("Say why the vote matters."))]);
    assert!(!serde_json::to_string(&ib).unwrap().contains("null"));

    // Banked, a candidate leaves the inbox.
    lines_add(
        &cfg,
        add(
            "Moth, you are a rounding error.",
            vec![LineSituation::RoastRequest],
        ),
    )
    .unwrap();
    let ib = inbox(&cfg, "malakor").unwrap();
    assert_eq!(
        ib.candidates.iter().map(|i| i.line).collect::<Vec<_>>(),
        vec![9]
    );
    assert_eq!(inbox(&cfg, "keeper").unwrap().candidates.len(), 1);
}

#[test]
fn a_review_line_is_sent_with_its_situation() {
    let tmp = tempfile::tempdir().unwrap();
    let (cfg, _) = setup(tmp.path());
    write_session(&cfg.paths.sandbox_runs(), SID, 1_000);
    mark(&cfg, SID, 4, vec![FeedbackTag::Flat], Some("Sharper."), 10);
    let p = |line: u64, character: &str| ToneReviewLineParams {
        character: character.into(),
        session_id: SID.into(),
        line,
    };
    let i = review_line(&cfg, &p(4, "malakor")).unwrap();
    assert_eq!(i.situation, Some(LineSituation::RoastRequest));
    assert_eq!(i.note.as_deref(), Some("Sharper."));
    assert_eq!(i.at_ms, Some(10));
    let i = review_line(&cfg, &p(6, "malakor")).unwrap();
    assert_eq!((i.situation, i.tags.len(), i.at_ms), (None, 0, None));
    assert_eq!(
        review_line(&cfg, &p(5, "malakor")).unwrap_err().code,
        ErrorCode::InvalidParams
    );
    assert_eq!(
        review_line(&cfg, &p(4, "keeper")).unwrap_err().code,
        ErrorCode::InvalidParams
    );
    let mut bad = p(4, "malakor");
    bad.session_id = "../sbx-20261020-120000-5m".into();
    assert_eq!(
        review_line(&cfg, &bad).unwrap_err().code,
        ErrorCode::InvalidParams
    );
    bad.session_id = "sbx-20261020-170000-5m".into();
    assert_eq!(
        review_line(&cfg, &bad).unwrap_err().code,
        ErrorCode::NotFound
    );
}
