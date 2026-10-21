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
//!   PNGs) from `<data>/avatar/rig/`; 404 when there is none, and the stage draws its bundled
//!   placeholder.
//! - `shadowrunr voice check|say|replay` (`cli`, with `probe` and `replay`).
//!
//! Optional end to end: with `[voice] enabled = false` (the default) `/voice` says `off` and
//! nothing is spawned; a missing engine or a dead sidecar is `degraded` with the reason, and the
//! stream itself never waits on the voice.

pub mod avatar;
pub mod cli;
pub mod probe;
pub mod renderer;
pub mod replay;
pub mod service;
pub mod sidecar;
pub mod socket;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

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
            max_queue: 6,
            max_lag_ms: 8_000,
            render_timeout_ms: 30_000,
            lines_in_flight: 3,
            render_workers: 3,
            render_threads: None,
        }
    }
}

impl VoiceConfig {
    /// The same config with its paths resolved against `root`.
    pub fn resolved(mut self, root: &Path) -> Self {
        self.sidecar = resolve(root, &self.sidecar);
        self.engine_dir = self.engine_dir.map(|p| resolve(root, &p));
        self.durations_model = self.durations_model.map(|p| resolve(root, &p));
        self
    }
}

fn resolve(root: &Path, p: &str) -> String {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        p.to_string()
    } else {
        root.join(path).to_string_lossy().into_owned()
    }
}

/// `<data>/avatar/rig`: where `/avatar/{file}` serves from.
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

/// Daemon hook: with `[voice] enabled`, start the sidecar and speak through it until the
/// daemon shuts down. Returns the supervisor, which ends once the sidecar has stopped.
pub fn start(state: &Arc<AppState>) -> Option<tokio::task::JoinHandle<()>> {
    if !state.voice.enabled() {
        return None;
    }
    let (renderer, task) =
        sidecar::SidecarRenderer::start(state.voice.config().clone(), state.shutdown_signal());
    state.voice.attach(renderer, state.shutdown_signal());
    Some(task)
}
