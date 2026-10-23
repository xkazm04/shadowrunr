//! `ModVoice`: the package load, the one request per decision, and every way a line falls back.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use shadowrunr_types::moderation::{ModActionKind, ModDecision, ModMode};
use shadowrunr_types::{
    Engine, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport, Platform, Role,
    Usage, WarmSession,
};

use super::*;
use crate::config::{Config, FileConfig, PathsConfig};

/// A string placed where a viewer's message would be: it must never reach the prompt.
const SENTINEL: &str = "SENTINEL-MESSAGE-TEXT-7f3a";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn s(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

fn config(tmp: &Path, characters: PathBuf, character: &str) -> Config {
    let mut fc = FileConfig {
        paths: PathsConfig {
            characters: s(characters),
            runs: s(tmp.join("runs")),
            data: s(tmp.join("data")),
            policy: s(repo().join("policy")),
            sim: s(repo().join("sim")),
            segments: s(repo().join("crates/shadowrunr-segments/data")),
        },
        ..FileConfig::default()
    };
    fc.engine.scripted = true;
    fc.moderator.character = character.into();
    Config::from_file_config(fc, tmp)
}

fn warden(tmp: &Path) -> ModVoice {
    let (v, err) = ModVoice::load(&config(tmp, repo().join("characters"), "moderator"));
    assert!(err.is_none(), "{err:?}");
    v
}

/// A decision whose message would have carried [`SENTINEL`]: the id and viewer fields are all
/// the voice gets.
fn decision(action: ModActionKind, timeout_s: Option<u32>) -> ModDecision {
    ModDecision {
        id: "d-1".into(),
        ts_ms: 1,
        platform: Platform::Twitch,
        viewer_id: "u-42".into(),
        viewer_name: "SootGoblin24".into(),
        message_id: "m-9".into(),
        rule_id: "no-spam".into(),
        action,
        timeout_s,
        mode: ModMode::Armed,
        executed: false,
        line: None,
        error: None,
    }
}

enum Script {
    Reply(&'static str),
    Fail,
}

/// Captures every request and answers with its script.
struct Capture {
    script: Script,
    seen: Mutex<Vec<EngineRequest>>,
}

impl Capture {
    fn new(script: Script) -> Arc<Self> {
        Arc::new(Self {
            script,
            seen: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl Engine for Capture {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
        self.seen.lock().unwrap().push(req);
        match self.script {
            Script::Fail => Err(EngineError::Unavailable {
                reason: "fake fleet down".into(),
            }),
            Script::Reply(text) => Ok(EngineReply {
                text: text.into(),
                engine: EngineKind::Scripted,
                account: None,
                model: "fake-mod-voice".into(),
                first_text_ms: None,
                duration_ms: 1,
                usage: Usage::default(),
                cost_usd: None,
                fallback_reason: None,
                session_id: None,
            }),
        }
    }

    async fn open_session(
        &self,
        _template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError> {
        Err(EngineError::Unavailable {
            reason: "no sessions".into(),
        })
    }

    async fn health(&self) -> Vec<HealthReport> {
        Vec::new()
    }
}

async fn line_with(script: Script, d: &ModDecision) -> (String, Vec<EngineRequest>) {
    let tmp = tempfile::tempdir().unwrap();
    let v = warden(tmp.path());
    let cap = Capture::new(script);
    let engine: Arc<dyn Engine> = cap.clone();
    let line = v.line(&engine, d, "No spam").await;
    let seen = cap.seen.lock().unwrap().clone();
    (line, seen)
}

fn fallback_for(viewer: &str, rule: &str) -> String {
    read_fallback(&repo().join("characters/moderator"))
        .unwrap()
        .replace("{viewer}", viewer)
        .replace("{rule}", rule)
}

#[test]
fn the_package_loads_with_its_identity_constitution_and_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let v = warden(tmp.path());
    assert_eq!(v.name, "Warden");
    assert!(v.constitution.contains("Never repeat, quote"));
    assert!(v.fallback.contains("{viewer}") && v.fallback.contains("{rule}"));
    let filled = fallback_for("SootGoblin24", "No spam");
    assert!(filled.chars().count() < 200, "{filled}");
}

#[test]
fn a_missing_or_host_package_gives_the_neutral_voice_and_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let (v, err) = ModVoice::load(&config(tmp.path(), repo().join("characters"), "nobody"));
    assert_eq!(v.name, "Moderator");
    assert!(err.unwrap().contains("nobody"));
    let (v, err) = ModVoice::load(&config(tmp.path(), repo().join("characters"), "malakor"));
    assert_eq!(v.name, "Moderator");
    assert!(err.unwrap().contains("role = \"moderator\""));
    let (_, err) = ModVoice::load(&config(tmp.path(), repo().join("characters"), "../x"));
    assert!(err.unwrap().contains("not a package id"));
}

#[tokio::test]
async fn one_mod_voice_request_names_the_rule_and_viewer_never_the_message() {
    let d = decision(ModActionKind::Timeout, Some(600));
    let (line, seen) = line_with(
        Script::Reply("Warden: SootGoblin24, a Writ of Silence: ten minutes, rule No spam."),
        &d,
    )
    .await;
    assert_eq!(seen.len(), 1);
    let req = &seen[0];
    assert_eq!(req.role, Role::ModVoice);
    assert!(req.allowed_tools.is_empty());
    assert!(req.json_schema.is_none());
    let (_, _, timeout_ms) = Config::from_file_config(FileConfig::default(), Path::new("."))
        .engine
        .route(Role::ModVoice);
    assert_eq!(req.timeout_ms, timeout_ms);
    assert!(req.prompt.contains("No spam"), "{}", req.prompt);
    assert!(req.prompt.contains("SootGoblin24"), "{}", req.prompt);
    assert!(req.prompt.contains("10 minutes"), "{}", req.prompt);
    assert!(req.prompt.contains("armed"), "{}", req.prompt);
    assert!(req.system.contains("Warden") && req.system.contains("Never repeat"));
    for text in [&req.prompt, &req.system] {
        assert!(!text.contains(SENTINEL));
    }
    // The speaker label is stripped.
    assert_eq!(
        line,
        "SootGoblin24, a Writ of Silence: ten minutes, rule No spam."
    );
}

#[tokio::test]
async fn a_blocked_reply_returns_the_fallback() {
    let d = decision(ModActionKind::Delete, None);
    let (line, _) = line_with(Script::Reply("I cannot continue with this stream."), &d).await;
    assert_eq!(line, fallback_for("SootGoblin24", "No spam"));
    // A mention of anyone but the viewer, and an emoji, break the form.
    let (line, _) = line_with(Script::Reply("@SomeoneElse removed under No spam."), &d).await;
    assert_eq!(line, fallback_for("SootGoblin24", "No spam"));
    let (line, _) = line_with(Script::Reply("Removed under No spam \u{1F480}"), &d).await;
    assert_eq!(line, fallback_for("SootGoblin24", "No spam"));
}

#[tokio::test]
async fn an_engine_error_or_an_empty_reply_returns_the_fallback() {
    let d = decision(ModActionKind::Ban, None);
    let (line, seen) = line_with(Script::Fail, &d).await;
    assert_eq!(seen.len(), 1);
    assert_eq!(line, fallback_for("SootGoblin24", "No spam"));
    let (line, _) = line_with(Script::Reply("  \n\n  "), &d).await;
    assert_eq!(line, fallback_for("SootGoblin24", "No spam"));
}

#[tokio::test]
async fn the_line_is_one_line_of_at_most_300_chars() {
    let d = decision(ModActionKind::Delete, None);
    let long: &'static str = Box::leak(
        format!(
            "\n**Warden:** \"{}\"\nA second line that must go.",
            "Entered in the ledger under No spam. ".repeat(20)
        )
        .into_boxed_str(),
    );
    let (line, _) = line_with(Script::Reply(long), &d).await;
    assert!(!line.contains('\n'));
    assert!(line.chars().count() <= MAX_LINE_CHARS, "{}", line.len());
    assert!(line.starts_with("Entered in the ledger"), "{line}");
}

#[test]
fn the_fallback_file_is_checked() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(read_fallback(tmp.path()).unwrap_err().contains(LINES_FILE));
    std::fs::write(tmp.path().join(LINES_FILE), "fallback = \"  \"\n").unwrap();
    assert!(read_fallback(tmp.path()).unwrap_err().contains("empty"));
    std::fs::write(tmp.path().join(LINES_FILE), "fallback = \"{viewr} out\"\n").unwrap();
    assert!(read_fallback(tmp.path()).unwrap_err().contains("viewr"));
    std::fs::write(
        tmp.path().join(LINES_FILE),
        "fallback = \"{viewer} out ({rule})\"\n",
    )
    .unwrap();
    assert_eq!(read_fallback(tmp.path()).unwrap(), "{viewer} out ({rule})");
}

#[test]
fn a_viewer_name_the_gate_refuses_never_reaches_the_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let v = warden(tmp.path());
    let mut d = decision(ModActionKind::Delete, None);
    // A name that is itself a character break: the follower word stands in for it.
    d.viewer_name = "I will stop here".into();
    let line = v.fallback_line(&d, "No spam");
    assert!(!line.contains("stop here"), "{line}");
    assert!(line.starts_with("minion"), "{line}");
}
