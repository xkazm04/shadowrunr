//! The show's recorder: OBS Studio renders the stage itself through a Browser Source and records
//! picture and voice on one clock (contract § run-directory artifacts, `recording.json`).
//!
//! - `obs`: a small obs-websocket v5 client.
//! - [`prepare`]: 1920x1080 at 30 fps, the scene [`SCENE`] holding the browser source [`INPUT`]
//!   on the live stage URL ([`stage_url`]), freshly loaded, the scene on program, OBS's global
//!   desktop and mic inputs muted (the recording carries the stage's audio only).
//! - [`start`] / [`stop`]: record into a directory, and the file OBS wrote.
//! - [`wait_for_voice`]: the voice runs seconds behind the events, so after the session ends
//!   the recorder waits until its last line has played (from the run's `voice.jsonl`).
//! - [`Recording`]: `recording.json`, written beside the recording in the run dir.
//! - `cli`: `shadowrunr record check`, and `shadowrunr stream sandbox --record`, which drives
//!   all of the above around one session.
//! - `watch`: the watched checks (`voiced`, `face`, `av_sync`, `dead_air`), measured from
//!   `voice.jsonl` and the recording into the session's report.
//!
//! **The password is a secret.** It comes from the environment variable `[record]
//! obs_password_env` names, else from OBS's own obs-websocket config file; it is never printed,
//! logged or passed on a command line.

pub mod cli;
pub mod obs;
pub mod watch;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::FaceVariant;
use crate::runs;
use obs::{code, Obs, ObsError};

/// The scene the recorder records.
pub const SCENE: &str = "Shadowrunr Stage";
/// The browser source in it. OBS names are unique across scenes and inputs, so it cannot share
/// the scene's name.
pub const INPUT: &str = "Shadowrunr Stage Browser";
pub const WIDTH: u32 = 1920;
pub const HEIGHT: u32 = 1080;
pub const FPS: u32 = 30;
/// How long the recording runs on after the session has ended, so the end card is in it.
pub const TAIL: Duration = Duration::from_secs(3);
/// How long the stage gets to load in the browser source before recording starts.
pub const STAGE_WARMUP: Duration = Duration::from_secs(4);
/// How long OBS may take to report that recording started or stopped.
pub const STATE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the recorder waits, at most, for the voice to finish after the session ends.
pub const VOICE_DRAIN_MAX: Duration = Duration::from_secs(45);
/// The voice is done once everything rendered has played out and nothing new has been
/// rendered or played for this long.
pub const VOICE_QUIET: Duration = Duration::from_secs(2);
const VOICE_POLL: Duration = Duration::from_millis(250);
/// The recording's base name in the run dir (`recording.mkv`, `recording.mp4`, ...).
pub const FILE_STEM: &str = "recording";
/// `recording.json`.
pub const META_FILE: &str = "recording.json";

/// `[record]` in `shadowrunr.toml`. Every key is optional.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordConfig {
    /// obs-websocket's address. OBS listens on every interface; the recorder only ever talks
    /// to this machine.
    pub obs_host: String,
    pub obs_port: u16,
    /// The environment variable holding obs-websocket's password. Unset: the password is read
    /// from OBS's own `plugin_config/obs-websocket/config.json`.
    pub obs_password_env: String,
    /// Where the built stage is served. Its origin must be `http://localhost:<port>`: the
    /// daemon only accepts browser origins on `localhost`.
    pub stage_url: String,
}

impl Default for RecordConfig {
    fn default() -> Self {
        Self {
            obs_host: "127.0.0.1".into(),
            obs_port: 4455,
            obs_password_env: "SHADOWRUNR_OBS_PASSWORD".into(),
            stage_url: "http://localhost:5178".into(),
        }
    }
}

impl RecordConfig {
    pub fn obs_url(&self) -> String {
        format!("ws://{}:{}", self.obs_host, self.obs_port)
    }
}

/// The live stage in broadcast mode with the voice on, pointed at the daemon on `daemon_port`,
/// drawing the face with `avatar` (`[stage] avatar`).
pub fn stage_url(base: &str, daemon_port: u16, avatar: FaceVariant) -> String {
    format!(
        "{}/?run=live&broadcast=1&voice=1&ws=ws://127.0.0.1:{daemon_port}/events{}",
        base.trim_end_matches('/'),
        avatar.query()
    )
}

/// Why `base` cannot be the stage's origin, if it cannot.
pub fn stage_origin_problem(base: &str) -> Option<String> {
    let rest = base.strip_prefix("http://localhost")?;
    let port_ok = rest.is_empty()
        || rest == "/"
        || rest
            .strip_prefix(':')
            .map(|p| p.trim_end_matches('/'))
            .is_some_and(|p| !p.is_empty() && p.parse::<u16>().is_ok());
    (!port_ok).then(|| format!("[record] stage_url {base:?} is not http://localhost:<port>"))
}

/// The stage URL check, also when it is not on `http://localhost` at all.
pub fn check_stage_base(base: &str) -> Result<(), String> {
    if !base.starts_with("http://localhost") {
        return Err(format!(
            "[record] stage_url {base:?} must be on http://localhost:<port> (the daemon only \
             accepts browser origins on localhost)"
        ));
    }
    stage_origin_problem(base).map_or(Ok(()), Err)
}

/// OBS's own obs-websocket config file.
pub fn obs_websocket_config() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?)
    } else if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(x)
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".config")
    };
    Some(
        base.join("obs-studio")
            .join("plugin_config")
            .join("obs-websocket")
            .join("config.json"),
    )
}

/// Where the password came from (never the password itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordSource {
    Env(String),
    ObsConfig,
    None,
}

impl std::fmt::Display for PasswordSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Env(name) => write!(f, "${name}"),
            Self::ObsConfig => f.write_str("OBS's obs-websocket config"),
            Self::None => f.write_str("none"),
        }
    }
}

/// The password from `env_value` (the configured variable's value), else from OBS's config
/// file at `obs_config`, when its server has authentication on.
pub fn password_from(
    env_name: &str,
    env_value: Option<String>,
    obs_config: Option<&Path>,
) -> (Option<String>, PasswordSource) {
    if let Some(p) = env_value.filter(|p| !p.is_empty()) {
        return (Some(p), PasswordSource::Env(env_name.to_string()));
    }
    let from_file = obs_config
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v["auth_required"].as_bool() != Some(false))
        .and_then(|v| v["server_password"].as_str().map(String::from))
        .filter(|p| !p.is_empty());
    match from_file {
        Some(p) => (Some(p), PasswordSource::ObsConfig),
        None => (None, PasswordSource::None),
    }
}

/// The configured password and where it came from.
pub fn password(cfg: &RecordConfig) -> (Option<String>, PasswordSource) {
    let env = std::env::var(&cfg.obs_password_env).ok();
    password_from(
        &cfg.obs_password_env,
        env,
        obs_websocket_config().as_deref(),
    )
}

/// Connect and identify, with output events (record start and stop).
pub async fn connect(cfg: &RecordConfig) -> Result<Obs, ObsError> {
    let (pw, _) = password(cfg);
    Obs::connect(&cfg.obs_url(), pw.as_deref(), obs::EVENTS_OUTPUTS).await
}

/// `Ok` for a success, and for `ok_code` (e.g. "already exists").
fn allow(r: Result<Value, ObsError>, ok_code: i64) -> Result<Value, ObsError> {
    match r {
        Err(e) if e.code() == Some(ok_code) => Ok(Value::Null),
        r => r,
    }
}

/// Set OBS up to record the stage at `url`. Refused while OBS is already recording.
pub async fn prepare(obs: &mut Obs, url: &str) -> Result<(), ObsError> {
    if recording_active(obs).await? {
        return Err(ObsError::Request {
            request: "StartRecord".into(),
            code: code::OUTPUT_RUNNING,
            comment: "OBS is already recording; stop it first".into(),
        });
    }
    obs.request(
        "SetVideoSettings",
        json!({
            "fpsNumerator": FPS, "fpsDenominator": 1,
            "baseWidth": WIDTH, "baseHeight": HEIGHT,
            "outputWidth": WIDTH, "outputHeight": HEIGHT,
        }),
    )
    .await?;
    allow(
        obs.request("CreateScene", json!({ "sceneName": SCENE }))
            .await,
        code::RESOURCE_ALREADY_EXISTS,
    )?;
    let settings = json!({
        "url": url,
        "width": WIDTH,
        "height": HEIGHT,
        "fps": FPS,
        "fps_custom": true,
        "reroute_audio": true,
        "restart_when_active": true,
        "shutdown": false,
    });
    let created = obs
        .request(
            "CreateInput",
            json!({
                "sceneName": SCENE,
                "inputName": INPUT,
                "inputKind": "browser_source",
                "inputSettings": settings,
                "sceneItemEnabled": true,
            }),
        )
        .await;
    match created {
        Ok(_) => {}
        Err(e) if e.code() == Some(code::RESOURCE_ALREADY_EXISTS) => {
            // It exists: point it at this URL, reload the page without its cache (the same URL
            // does not reload it, and a page left from the last session keeps the face it drew
            // then: sbx-20261021-095610-5m kept a placeholder from before the rig was served),
            // and make sure it is in the scene.
            obs.request(
                "SetInputSettings",
                json!({ "inputName": INPUT, "inputSettings": settings, "overlay": true }),
            )
            .await?;
            obs.request(
                "PressInputPropertiesButton",
                json!({ "inputName": INPUT, "propertyName": "refreshnocache" }),
            )
            .await?;
            let item = obs
                .request(
                    "GetSceneItemId",
                    json!({ "sceneName": SCENE, "sourceName": INPUT }),
                )
                .await;
            if let Err(e) = item {
                if e.code() != Some(code::RESOURCE_NOT_FOUND) {
                    return Err(e);
                }
                obs.request(
                    "CreateSceneItem",
                    json!({ "sceneName": SCENE, "sourceName": INPUT, "sceneItemEnabled": true }),
                )
                .await?;
            }
        }
        Err(e) => return Err(e),
    }
    obs.request("SetCurrentProgramScene", json!({ "sceneName": SCENE }))
        .await?;
    mute_global_audio(obs).await
}

/// Mute OBS's global desktop and mic/aux inputs: the stage's audio comes through the browser
/// source, and nothing else on the machine belongs in the recording.
async fn mute_global_audio(obs: &mut Obs) -> Result<(), ObsError> {
    let special = obs.request("GetSpecialInputs", Value::Null).await?;
    let Some(map) = special.as_object() else {
        return Ok(());
    };
    for name in map.values().filter_map(Value::as_str) {
        allow(
            obs.request(
                "SetInputMute",
                json!({ "inputName": name, "inputMuted": true }),
            )
            .await,
            code::RESOURCE_NOT_FOUND,
        )?;
    }
    Ok(())
}

pub async fn recording_active(obs: &mut Obs) -> Result<bool, ObsError> {
    let st = obs.request("GetRecordStatus", Value::Null).await?;
    Ok(st["outputActive"].as_bool().unwrap_or(false))
}

/// Whether the recorder's scene exists.
pub async fn scene_present(obs: &mut Obs) -> Result<bool, ObsError> {
    let list = obs.request("GetSceneList", Value::Null).await?;
    Ok(list["scenes"]
        .as_array()
        .is_some_and(|s| s.iter().any(|x| x["sceneName"].as_str() == Some(SCENE))))
}

fn is_state(d: &Value, state: &str) -> bool {
    d["outputState"].as_str() == Some(state)
}

/// Record into `dir`; the wall time OBS reported the recording started.
pub async fn start(obs: &mut Obs, dir: &Path) -> Result<i64, ObsError> {
    obs.request(
        "SetRecordDirectory",
        json!({ "recordDirectory": dir.to_string_lossy() }),
    )
    .await?;
    obs.request("StartRecord", Value::Null).await?;
    obs.wait_event(
        "RecordStateChanged",
        |d| is_state(d, "OBS_WEBSOCKET_OUTPUT_STARTED"),
        STATE_TIMEOUT,
    )
    .await?;
    Ok(runs::now_ms())
}

/// Stop recording: the file OBS wrote (once OBS says it is closed) and the wall time the
/// recording stopped.
pub async fn stop(obs: &mut Obs) -> Result<(PathBuf, i64), ObsError> {
    let resp = obs.request("StopRecord", Value::Null).await?;
    let ended = runs::now_ms();
    let stopped = obs
        .wait_event(
            "RecordStateChanged",
            |d| is_state(d, "OBS_WEBSOCKET_OUTPUT_STOPPED"),
            STATE_TIMEOUT,
        )
        .await;
    let path = stopped
        .ok()
        .and_then(|d| d["outputPath"].as_str().map(String::from))
        .or_else(|| resp["outputPath"].as_str().map(String::from))
        .ok_or_else(|| ObsError::Protocol("StopRecord named no output file".into()))?;
    Ok((PathBuf::from(path), ended))
}

/// Where the voice stands, from a `voice.jsonl`'s lines.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VoiceProgress {
    /// Rendered parts the stage has neither played nor dropped (nor the service skipped).
    pub unplayed: usize,
    /// Each played part, with its length.
    pub played: Vec<((u64, u32), u64)>,
}

/// Read `voice.jsonl` text: what has been played, and what is still to play.
pub fn voice_progress(text: &str) -> VoiceProgress {
    let mut rendered: HashMap<(u64, u32), u64> = HashMap::new();
    let mut played = Vec::new();
    let mut played_keys = HashSet::new();
    let mut gone: HashSet<u64> = HashSet::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let seq = v["seq"].as_u64().unwrap_or(0);
        let part = v["part"].as_u64().and_then(|p| u32::try_from(p).ok());
        match v["event"].as_str() {
            Some("rendered") => {
                if let Some(p) = part {
                    rendered.insert((seq, p), v["duration_ms"].as_u64().unwrap_or(0));
                }
            }
            Some("played") => {
                if let Some(p) = part {
                    if played_keys.insert((seq, p)) {
                        played.push((seq, p));
                    }
                }
            }
            Some("stage_dropped") | Some("skipped") => {
                gone.insert(seq);
            }
            _ => {}
        }
    }
    VoiceProgress {
        unplayed: rendered
            .keys()
            .filter(|k| !played_keys.contains(k) && !gone.contains(&k.0))
            .count(),
        played: played
            .into_iter()
            .map(|k| (k, rendered.get(&k).copied().unwrap_or(0)))
            .collect(),
    }
}

/// Wait until the voice in `dir`'s `voice.jsonl` has played out: every rendered part played
/// (or dropped), the last one's audio over, and [`VOICE_QUIET`] with nothing new; at most
/// [`VOICE_DRAIN_MAX`]. A run with no voice log returns at once. A part's audio is taken to
/// end its length after its `played` line first appears here.
pub async fn wait_for_voice(dir: &Path) -> Duration {
    wait_for_voice_within(dir, VOICE_QUIET, VOICE_DRAIN_MAX).await
}

async fn wait_for_voice_within(dir: &Path, quiet: Duration, max: Duration) -> Duration {
    let path = dir.join(crate::voice::log::FILE);
    let t0 = tokio::time::Instant::now();
    let mut seen: HashMap<(u64, u32), tokio::time::Instant> = HashMap::new();
    let mut last_text = String::new();
    let mut changed_at = t0;
    let mut speaking_until = t0;
    let mut unplayed = 0;
    loop {
        let now = tokio::time::Instant::now();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return now - t0;
        };
        if text != last_text {
            changed_at = now;
            let p = voice_progress(&text);
            for (key, ms) in &p.played {
                let at = *seen.entry(*key).or_insert(now);
                speaking_until = speaking_until.max(at + Duration::from_millis(*ms));
            }
            unplayed = p.unplayed;
            last_text = text;
        }
        let done = unplayed == 0 && now >= speaking_until && now >= changed_at + quiet;
        if done || now >= t0 + max {
            return now - t0;
        }
        tokio::time::sleep(VOICE_POLL).await;
    }
}

/// `recording.json` (contract § run-directory artifacts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    pub tool: String,
    /// The recording's absolute path.
    pub file: String,
    pub started_wall_ms: i64,
    /// The stream-clock time of the recording's first frame: `started_wall_ms` minus the
    /// session's `started_ms` (negative when recording started before the session). A
    /// stream-clock time `t` sits at `t - stream_offset_ms` in the file.
    pub stream_offset_ms: i64,
    pub ended_wall_ms: i64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub stage_url: String,
}

impl Recording {
    pub fn new(
        file: &Path,
        started_wall_ms: i64,
        ended_wall_ms: i64,
        session_started_ms: i64,
        stage_url: &str,
    ) -> Self {
        Self {
            tool: "obs".into(),
            file: file.to_string_lossy().into_owned(),
            started_wall_ms,
            stream_offset_ms: started_wall_ms - session_started_ms,
            ended_wall_ms,
            width: WIDTH,
            height: HEIGHT,
            fps: FPS,
            stage_url: stage_url.to_string(),
        }
    }

    pub fn write(&self, dir: &Path) -> std::io::Result<PathBuf> {
        let path = dir.join(META_FILE);
        let mut text = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        text.push(b'\n');
        std::fs::write(&path, text)?;
        Ok(path)
    }
}

/// Move OBS's file into `dir` as `recording.<ext>`. OBS may hold the file for a moment after it
/// says it stopped, so the move is retried; across volumes it is a copy and a delete.
pub async fn move_into(src: &Path, dir: &Path) -> std::io::Result<PathBuf> {
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mkv".into());
    let dst = dir.join(format!("{FILE_STEM}.{ext}"));
    let mut last = None;
    for _ in 0..20 {
        match std::fs::rename(src, &dst) {
            Ok(()) => return Ok(dst),
            Err(e) => last = Some(e),
        }
        if std::fs::copy(src, &dst).is_ok() {
            let _ = std::fs::remove_file(src);
            return Ok(dst);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("move failed")))
}

#[cfg(test)]
mod tests {
    use super::obs::fake::serve;
    use super::*;

    #[test]
    fn the_stage_url_is_the_live_broadcast_with_voice() {
        assert_eq!(
            stage_url("http://localhost:5178/", 7800, FaceVariant::Glow),
            "http://localhost:5178/?run=live&broadcast=1&voice=1&ws=ws://127.0.0.1:7800/events"
        );
        assert_eq!(
            stage_url("http://localhost:5178", 7800, FaceVariant::Cut),
            "http://localhost:5178/?run=live&broadcast=1&voice=1&ws=ws://127.0.0.1:7800/events&avatar=cut"
        );
        assert!(
            stage_url("http://localhost:5178", 7800, FaceVariant::Depth).ends_with("&avatar=depth")
        );
        assert!(check_stage_base("http://localhost:5178").is_ok());
        assert!(check_stage_base("http://localhost:5178/").is_ok());
        assert!(check_stage_base("http://127.0.0.1:5178").is_err());
        assert!(check_stage_base("http://localhost.evil:5178").is_err());
        assert!(check_stage_base("http://localhost:x").is_err());
    }

    #[test]
    fn the_password_comes_from_the_env_then_from_obs_config() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("config.json");
        std::fs::write(
            &file,
            r#"{"auth_required":true,"server_enabled":true,"server_password":"from-file","server_port":4455}"#,
        )
        .unwrap();
        let (p, src) = password_from("PW", Some("from-env".into()), Some(&file));
        assert_eq!(
            (p.as_deref(), src),
            (Some("from-env"), PasswordSource::Env("PW".into()))
        );
        let (p, src) = password_from("PW", Some(String::new()), Some(&file));
        assert_eq!(
            (p.as_deref(), src.clone()),
            (Some("from-file"), PasswordSource::ObsConfig)
        );
        // The source names where the password came from, never the password.
        assert!(!src.to_string().contains("from-file"));
        std::fs::write(&file, r#"{"auth_required":false,"server_password":"x"}"#).unwrap();
        assert_eq!(
            password_from("PW", None, Some(&file)),
            (None, PasswordSource::None)
        );
        assert_eq!(
            password_from("PW", None, None),
            (None, PasswordSource::None)
        );
    }

    #[test]
    fn voice_progress_counts_what_is_still_to_play() {
        let text = [
            r#"{"ts_ms":1,"seq":0,"event":"face","reason":"rig"}"#,
            r#"{"ts_ms":2,"seq":4,"event":"rendered","part":0,"duration_ms":1500,"text":"a"}"#,
            r#"{"ts_ms":3,"seq":4,"event":"rendered","part":1,"duration_ms":900,"text":"b"}"#,
            r#"{"ts_ms":4,"seq":4,"event":"played","part":0}"#,
            r#"{"ts_ms":5,"seq":6,"event":"rendered","part":0,"duration_ms":700,"text":"c"}"#,
            r#"{"ts_ms":6,"seq":6,"event":"stage_dropped","reason":"stage_cap"}"#,
            "not json",
        ]
        .join("\n");
        let p = voice_progress(&text);
        assert_eq!(
            p.unplayed, 1,
            "seq 4 part 1 is still to play; seq 6 was dropped"
        );
        assert_eq!(p.played, [((4, 0), 1500)]);
        let all = format!(
            "{text}\n{}",
            r#"{"ts_ms":7,"seq":4,"event":"played","part":1}"#
        );
        let p = voice_progress(&all);
        assert_eq!(p.unplayed, 0);
        assert_eq!(p.played, [((4, 0), 1500), ((4, 1), 900)]);
    }

    #[tokio::test]
    async fn the_recorder_waits_for_the_last_line_to_play_out() {
        let (quiet, max) = (Duration::from_millis(300), Duration::from_secs(3));
        let d = tempfile::tempdir().unwrap();
        // No voice log: nothing to wait for.
        assert!(wait_for_voice_within(d.path(), quiet, max).await < Duration::from_millis(50));
        let log = d.path().join("voice.jsonl");
        std::fs::write(
            &log,
            concat!(
                r#"{"ts_ms":2,"seq":4,"event":"rendered","part":0,"duration_ms":1000,"text":"a"}"#,
                "
",
                r#"{"ts_ms":4,"seq":4,"event":"played","part":0}"#,
                "
"
            ),
        )
        .unwrap();
        // The line's 1 s of audio, then done (the quiet window is shorter).
        let waited = wait_for_voice_within(d.path(), quiet, max).await;
        assert!(waited >= Duration::from_secs(1), "{waited:?}");
        assert!(waited < Duration::from_millis(1800), "{waited:?}");
        // A part that never plays holds the recorder only up to the cap.
        std::fs::write(
            &log,
            r#"{"ts_ms":2,"seq":9,"event":"rendered","part":0,"duration_ms":10,"text":"a"}"#,
        )
        .unwrap();
        let waited = wait_for_voice_within(d.path(), quiet, max).await;
        assert!(waited >= max, "{waited:?}");
        assert!(waited < max + Duration::from_secs(1), "{waited:?}");
    }

    #[test]
    fn recording_json_carries_the_stream_offset() {
        let d = tempfile::tempdir().unwrap();
        let r = Recording::new(
            &d.path().join("recording.mkv"),
            9_000,
            320_000,
            10_000,
            "http://localhost:5178/?run=live",
        );
        assert_eq!(r.stream_offset_ms, -1_000);
        let path = r.write(d.path()).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(v["tool"], "obs");
        assert_eq!(
            (v["width"].as_u64(), v["fps"].as_u64()),
            (Some(1920), Some(30))
        );
        assert_eq!(v["stream_offset_ms"], -1_000);
    }

    #[tokio::test]
    async fn prepare_sets_up_the_scene_and_tolerates_what_exists() {
        let (url, server) = serve(None, |ty, _| match ty {
            "GetRecordStatus" => (obs::code::SUCCESS, json!({"outputActive": false}), vec![]),
            "CreateScene" | "CreateInput" => {
                (obs::code::RESOURCE_ALREADY_EXISTS, Value::Null, vec![])
            }
            "GetSceneItemId" => (obs::code::RESOURCE_NOT_FOUND, Value::Null, vec![]),
            "GetSpecialInputs" => (
                obs::code::SUCCESS,
                json!({"desktop1": "Desktop Audio", "desktop2": null, "mic1": "Mic/Aux"}),
                vec![],
            ),
            _ => (obs::code::SUCCESS, Value::Null, vec![]),
        })
        .await;
        let mut o = Obs::connect(&url, None, 0).await.unwrap();
        prepare(&mut o, "http://localhost:5178/?run=live")
            .await
            .unwrap();
        o.close().await;
        let seen = server.await.unwrap();
        let kinds: Vec<&str> = seen
            .requests
            .iter()
            .map(|r| r["requestType"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "GetRecordStatus",
                "SetVideoSettings",
                "CreateScene",
                "CreateInput",
                "SetInputSettings",
                "PressInputPropertiesButton",
                "GetSceneItemId",
                "CreateSceneItem",
                "SetCurrentProgramScene",
                "GetSpecialInputs",
                "SetInputMute",
                "SetInputMute",
            ]
        );
        let video = &seen.requests[1]["requestData"];
        assert_eq!(
            (video["baseWidth"].as_u64(), video["outputHeight"].as_u64()),
            (Some(1920), Some(1080))
        );
        assert_eq!(video["fpsNumerator"], 30);
        let input = &seen.requests[3]["requestData"];
        assert_eq!(input["inputKind"], "browser_source");
        assert_eq!(input["inputSettings"]["reroute_audio"], true);
        assert_eq!(input["inputSettings"]["restart_when_active"], true);
        assert_eq!(input["inputSettings"]["width"], 1920);
        assert_eq!(
            seen.requests[4]["requestData"]["inputSettings"]["url"],
            "http://localhost:5178/?run=live"
        );
    }

    #[tokio::test]
    async fn prepare_refuses_while_obs_is_recording() {
        let (url, server) = serve(None, |ty, _| match ty {
            "GetRecordStatus" => (obs::code::SUCCESS, json!({"outputActive": true}), vec![]),
            _ => (obs::code::SUCCESS, Value::Null, vec![]),
        })
        .await;
        let mut o = Obs::connect(&url, None, 0).await.unwrap();
        let err = prepare(&mut o, "u").await.unwrap_err();
        assert!(err.to_string().contains("already recording"), "{err}");
        o.close().await;
        assert_eq!(server.await.unwrap().requests.len(), 1);
    }

    #[tokio::test]
    async fn start_and_stop_wait_for_obs_to_say_so() {
        let (url, server) = serve(None, |ty, _| match ty {
            "StartRecord" => (
                obs::code::SUCCESS,
                Value::Null,
                vec![json!({"eventType": "RecordStateChanged", "eventIntent": 64,
                    "eventData": {"outputActive": false, "outputState": "OBS_WEBSOCKET_OUTPUT_STARTING"}}),
                    json!({"eventType": "RecordStateChanged", "eventIntent": 64,
                    "eventData": {"outputActive": true, "outputState": "OBS_WEBSOCKET_OUTPUT_STARTED"}})],
            ),
            "StopRecord" => (
                obs::code::SUCCESS,
                json!({"outputPath": "C:/rec/early.mkv"}),
                vec![json!({"eventType": "RecordStateChanged", "eventIntent": 64,
                    "eventData": {"outputActive": false, "outputState": "OBS_WEBSOCKET_OUTPUT_STOPPED",
                    "outputPath": "C:/rec/final.mkv"}})],
            ),
            _ => (obs::code::SUCCESS, Value::Null, vec![]),
        })
        .await;
        let mut o = Obs::connect(&url, None, obs::EVENTS_OUTPUTS).await.unwrap();
        let t0 = runs::now_ms();
        let started = start(&mut o, Path::new("C:/rec")).await.unwrap();
        assert!(started >= t0);
        let (path, ended) = stop(&mut o).await.unwrap();
        assert_eq!(path, PathBuf::from("C:/rec/final.mkv"));
        assert!(ended >= started);
        o.close().await;
        let seen = server.await.unwrap();
        assert_eq!(seen.requests[0]["requestData"]["recordDirectory"], "C:/rec");
    }

    #[tokio::test]
    async fn the_file_moves_into_the_run_dir_keeping_its_extension() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("2026-10-21 10-00-00.mp4");
        std::fs::write(&src, b"media").unwrap();
        let run = d.path().join("sbx-1");
        std::fs::create_dir_all(&run).unwrap();
        let dst = move_into(&src, &run).await.unwrap();
        assert_eq!(dst, run.join("recording.mp4"));
        assert!(!src.exists());
        assert_eq!(std::fs::read(dst).unwrap(), b"media");
    }
}
