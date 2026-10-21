//! `shadowrunr record check`, and the recorder half of `shadowrunr stream sandbox --record`.
//!
//! A recorded sandbox ([`Armed`]):
//! 1. before the session: OBS must answer (else the run refuses to start, exit 1), the scene and
//!    browser source are set up on the live stage URL, the page gets [`STAGE_WARMUP`] to load,
//!    and recording starts into `<runs>/sandbox/.recording/`;
//! 2. when the session leaves the air (judging, or ended), the recorder waits for the voice to
//!    play out (it runs seconds behind the events; `wait_for_voice`), then [`TAIL`] longer so
//!    the end card is in it, then stops;
//! 3. once the session has its report, the file moves into the run dir as `recording.<ext>`
//!    beside `recording.json`, whose `stream_offset_ms` comes from the report's `started_ms`.

use std::path::{Path, PathBuf};

use clap::Subcommand;

use super::obs::ObsError;
use super::{
    check_stage_base, connect, move_into, password, prepare, recording_active, scene_present,
    stage_url, start, stop, wait_for_voice, RecordConfig, Recording, SCENE, STAGE_WARMUP, TAIL,
};
use crate::config::Config;

#[derive(Debug, Subcommand)]
pub enum RecordCommand {
    /// Probe OBS without recording: reachable, version, password accepted, the recorder's scene
    /// present. Exit 0 when OBS answers and accepts the password, else 1.
    Check,
}

pub async fn run(cfg: &Config, cmd: RecordCommand) -> u8 {
    match cmd {
        RecordCommand::Check => check_cmd(&cfg.record).await,
    }
}

/// What to do when OBS does not answer (shared by `check` and `--record`).
fn hint(e: &ObsError) -> &'static str {
    match e {
        ObsError::Unreachable(_) | ObsError::Timeout(_) => {
            "start OBS with its WebSocket server on (Tools > WebSocket Server Settings); see docs/guide/recording.md"
        }
        ObsError::Auth(_) => {
            "set the password in the variable [record] obs_password_env names, or leave it unset to use OBS's own config"
        }
        _ => "see docs/guide/recording.md",
    }
}

async fn check_cmd(cfg: &RecordConfig) -> u8 {
    let (_, source) = password(cfg);
    println!("obs-websocket  {}", cfg.obs_url());
    println!("password from  {source}");
    let mut obs = match connect(cfg).await {
        Ok(o) => o,
        Err(e) => {
            println!("reachable      no: {e}");
            eprintln!("{}", hint(&e));
            return 1;
        }
    };
    println!(
        "reachable      yes (obs-websocket {})",
        obs.websocket_version
    );
    println!(
        "auth           {}",
        if obs.authenticated {
            "ok"
        } else {
            "not required by OBS"
        }
    );
    let mut code = 0;
    match obs.request("GetVersion", serde_json::Value::Null).await {
        Ok(v) => println!(
            "version        OBS {} (rpc {})",
            v["obsVersion"].as_str().unwrap_or("?"),
            v["rpcVersion"].as_u64().unwrap_or(0)
        ),
        Err(e) => {
            println!("version        {e}");
            code = 1;
        }
    }
    match scene_present(&mut obs).await {
        Ok(true) => println!("scene          {SCENE:?} present"),
        Ok(false) => println!("scene          {SCENE:?} not yet (a recorded run creates it)"),
        Err(e) => println!("scene          {e}"),
    }
    match recording_active(&mut obs).await {
        Ok(a) => println!("recording      {}", if a { "ACTIVE" } else { "idle" }),
        Err(e) => println!("recording      {e}"),
    }
    match check_stage_base(&cfg.stage_url) {
        Ok(()) => println!("stage          {}", cfg.stage_url),
        Err(e) => {
            println!("stage          {e}");
            code = 1;
        }
    }
    obs.close().await;
    code
}

/// A recording running around one sandbox session.
pub struct Armed {
    cfg: RecordConfig,
    pub stage_url: String,
    pub started_wall_ms: i64,
    /// What OBS wrote and when it stopped, once stopped.
    stopped: Option<(PathBuf, i64)>,
}

/// Where OBS records while the session's directory does not exist yet.
pub fn pending_dir(cfg: &Config) -> PathBuf {
    cfg.paths.sandbox_runs().join(".recording")
}

/// Set OBS up for the stage on `daemon_port` and start recording. Any failure is a clear
/// message, and nothing is left recording.
pub async fn arm(cfg: &Config, daemon_port: u16) -> Result<Armed, String> {
    let rc = cfg.record.clone();
    check_stage_base(&rc.stage_url)?;
    let url = stage_url(&rc.stage_url, daemon_port);
    let mut obs = connect(&rc)
        .await
        .map_err(|e| format!("{e}\n{}", hint(&e)))?;
    prepare(&mut obs, &url).await.map_err(|e| e.to_string())?;
    let dir = pending_dir(cfg);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // The page loads (and connects to the daemon) before anything is recorded.
    tokio::time::sleep(STAGE_WARMUP).await;
    let started = start(&mut obs, &dir).await.map_err(|e| e.to_string())?;
    obs.close().await;
    Ok(Armed {
        cfg: rc,
        stage_url: url,
        started_wall_ms: started,
        stopped: None,
    })
}

impl Armed {
    pub fn is_stopped(&self) -> bool {
        self.stopped.is_some()
    }

    /// The session in `dir` is off the air: wait for its voice to play out and the tail, then
    /// stop.
    pub async fn stop_after_show(&mut self, dir: &Path) -> Result<PathBuf, String> {
        let waited = wait_for_voice(dir).await;
        if waited.as_secs() >= 1 {
            println!(
                "recording: the voice played out {:.1} s after the end",
                waited.as_secs_f64()
            );
        }
        tokio::time::sleep(TAIL).await;
        self.stop().await
    }

    /// Stop recording (once); the file OBS wrote.
    pub async fn stop(&mut self) -> Result<PathBuf, String> {
        if let Some((p, _)) = &self.stopped {
            return Ok(p.clone());
        }
        let mut obs = connect(&self.cfg).await.map_err(|e| e.to_string())?;
        let r = stop(&mut obs).await.map_err(|e| e.to_string());
        obs.close().await;
        let (path, ended) = r?;
        self.stopped = Some((path.clone(), ended));
        Ok(path)
    }

    /// The session never ran: stop and throw the recording away.
    pub async fn abort(mut self) {
        match self.stop().await {
            Ok(p) => {
                let _ = std::fs::remove_file(p);
            }
            Err(e) => eprintln!("recording: could not stop OBS: {e}"),
        }
    }

    /// Stop if still recording, move the file into `dir` and write `recording.json` with the
    /// session's `started_ms`.
    pub async fn finish(
        &mut self,
        dir: &Path,
        session_started_ms: i64,
    ) -> Result<Recording, String> {
        self.stop().await?;
        let (src, ended) = self.stopped.clone().expect("stopped above");
        let file = move_into(&src, dir)
            .await
            .map_err(|e| format!("move {} into {}: {e}", src.display(), dir.display()))?;
        let rec = Recording::new(
            &file,
            self.started_wall_ms,
            ended,
            session_started_ms,
            &self.stage_url,
        );
        rec.write(dir)
            .map_err(|e| format!("{}: {e}", dir.join(super::META_FILE).display()))?;
        Ok(rec)
    }
}
