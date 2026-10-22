//! Malakor's voice and face assets (contract appendix (voice)).
//!
//! - `GET /voice` (`socket`): a WebSocket that pushes `VoiceFrame`s. The voice service
//!   (`service`, one [`VoiceHub`] per daemon) gives each Malakor `say` of a followed live run to
//!   the voice sidecar (`sidecar`: `tools/voice`, JSON lines on stdio, behind the
//!   [`renderer::Renderer`] seam) and forwards every rendered sentence as an `utterance`.
//!   Bounded: the queue holds `max_queue` lines, up to `lines_in_flight` more are rendering, a
//!   line whose audio cannot start within `max_lag_ms` of its `say` is `skipped`, and a slow
//!   client loses frames, never the service.
//! - `GET /avatar/{file}` (`avatar`): the face rig the stage draws (`layers.json` plus its
//!   PNGs) from `<data>/characters/<id>/avatar/rig/` for the character on air, else the shared
//!   `<data>/avatar/rig/`; 404 when there is none, and the stage draws its bundled placeholder.
//! - `<run dir>/voice.jsonl` (`log`): what the voice rendered and skipped, and what the stage
//!   reported playing, on the session's stream clock.
//! - `shadowrunr voice check|say|replay` (`cli`, with `probe` and `replay`).
//!
//! Optional end to end: with `[voice] enabled = false` (the default) `/voice` says `off` and
//! nothing is spawned; a missing engine or a dead sidecar is `degraded` with the reason, and the
//! stream itself never waits on the voice.

pub mod avatar;
pub mod cli;
pub mod elevenlabs;
pub mod log;
pub mod probe;
pub mod renderer;
pub mod replay;
pub mod service;
pub mod settings;
pub mod sidecar;
pub mod socket;
pub mod text;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use shadowrunr_types::character::VoiceFx;

pub use avatar::avatar_handler;
pub use service::VoiceHub;

use crate::server::AppState;

/// `[voice]` in `shadowrunr.toml`. Every key is optional; paths resolve against the file's
/// directory. No path here is ever committed: the example file only names the keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    pub enabled: bool,
    /// Python interpreter that runs the sidecar (`python -m shadowrunr_voice`). The mouth shapes
    /// need numpy and onnxruntime in it; without them the voice speaks with no visemes.
    pub python: String,
    /// The sidecar package's parent directory (`tools/voice` in the repo).
    pub sidecar: String,
    /// The sherpa-onnx TTS home: `bin/sherpa-onnx-offline-tts[.exe]` and `kokoro/` (model,
    /// voices, tokens, espeak-ng data, lexicons).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_dir: Option<String>,
    /// Kokoro's durations subgraph (`.onnx`), for mouth shapes. Absent = no visemes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durations_model: Option<String>,
    /// The voice id and its verified speaker id (`af_heart` is sid 3 of kokoro-multi-lang-v1_0).
    pub voice: String,
    pub sid: u32,
    /// The voices file `sid` is read from (`--voices`): a name in `voices_dir`, or a path.
    /// Absent: the model's own `kokoro/voices.bin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voices_file: Option<String>,
    /// Where a `voices_file` name is looked for. Absent: `~/.shadowrunr/voices`, where
    /// `python -m shadowrunr_voice.blend` writes by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voices_dir: Option<String>,
    /// The effect chain over each sentence (`--fx`). Absent: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fx: Option<VoiceFx>,
    /// Lines waiting to render, at most; the oldest is skipped (`queue_full`) past it.
    pub max_queue: usize,
    /// A line whose first audio would start later than this after its `say` is skipped (`late`).
    pub max_lag_ms: u64,
    /// One sentence's render, at most.
    pub render_timeout_ms: u64,
    /// Lines handed to the sidecar at once. Their audio still goes out one line after another,
    /// in order; the sidecar renders ahead into the later ones.
    pub lines_in_flight: usize,
    /// Engine runs the sidecar renders at once (`--workers`).
    pub render_workers: usize,
    /// Each engine run's threads (`--threads`). Absent = the sidecar's default (2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_threads: Option<u32>,
    /// `[voice.elevenlabs]`: the cloud provider (a character's choice picks it).
    pub elevenlabs: elevenlabs::ElevenlabsConfig,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            python: "python".into(),
            sidecar: "tools/voice".into(),
            engine_dir: None,
            durations_model: None,
            voice: "af_heart".into(),
            sid: 3,
            voices_file: None,
            voices_dir: None,
            fx: None,
            max_queue: 6,
            max_lag_ms: 15_000,
            render_timeout_ms: 30_000,
            lines_in_flight: 3,
            render_workers: 3,
            render_threads: None,
            elevenlabs: elevenlabs::ElevenlabsConfig::default(),
        }
    }
}

impl VoiceConfig {
    /// The same config with its paths resolved against `root`.
    pub fn resolved(mut self, root: &Path) -> Self {
        self.sidecar = resolve(root, &self.sidecar);
        self.engine_dir = self.engine_dir.map(|p| resolve(root, &p));
        self.durations_model = self.durations_model.map(|p| resolve(root, &p));
        self.voices_dir = self.voices_dir.map(|p| match home_relative(&p) {
            Some(home) => home.to_string_lossy().into_owned(),
            None => resolve(root, &p),
        });
        self
    }

    /// The voices file the sidecar reads: `voices_file` as a path when it is absolute, else in
    /// `voices_dir` (else `~/.shadowrunr/voices`). `None` is the model's own.
    pub fn voices_path(&self) -> Option<PathBuf> {
        let file = self.voices_file.as_deref()?;
        if Path::new(file).is_absolute() {
            return Some(PathBuf::from(file));
        }
        let dir = match &self.voices_dir {
            Some(d) => PathBuf::from(d),
            None => default_voices_dir()?,
        };
        Some(dir.join(file))
    }
}

/// `~/<rest>` under the home directory; `None` for any other path.
fn home_relative(p: &str) -> Option<PathBuf> {
    let rest = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\"))?;
    crate::integrations::settings::home_dir().map(|h| h.join(rest))
}

/// `~/.shadowrunr/voices`: where `python -m shadowrunr_voice.blend` writes by default.
pub fn default_voices_dir() -> Option<PathBuf> {
    crate::integrations::settings::home_dir().map(|h| h.join(".shadowrunr").join("voices"))
}

fn resolve(root: &Path, p: &str) -> String {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        p.to_string()
    } else {
        root.join(path).to_string_lossy().into_owned()
    }
}

/// `<data>/avatar/rig`: the shared rig `/avatar/{file}` falls back to when the character on
/// air has none of its own (`crate::character::rig_dir`).
pub fn rig_dir(data: &Path) -> PathBuf {
    data.join("avatar").join("rig")
}

/// `GET /voice`: the origin rule of `/events`, then one session (`socket`).
pub async fn voice_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !crate::server::origin_ok(&headers, &state.cfg.daemon.extra_origins) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| socket::session(state, socket))
}

/// Daemon hook: with `[voice] enabled`, speak with the configured character's (`[sim]
/// character`) voice until the daemon shuts down, and with each streaming character's from its
/// stream start on (`settings::DaemonVoices`). Returns a task that ends once the renderer
/// speaking at shutdown has stopped.
pub fn start(state: &Arc<AppState>) -> Option<tokio::task::JoinHandle<()>> {
    if !state.voice.enabled() {
        return None;
    }
    let voices = Arc::new(settings::DaemonVoices::new(state));
    let first = settings::spoken_for(&state.cfg, &state.cfg.sim.character);
    state
        .voice
        .attach_voices(voices, &first, state.shutdown_signal());
    let hub = state.voice.clone();
    let down = state.shutdown_signal();
    Some(tokio::spawn(async move {
        down.await;
        if let Some(task) = hub.stop_renderer() {
            let _ = task.await;
        }
    }))
}
