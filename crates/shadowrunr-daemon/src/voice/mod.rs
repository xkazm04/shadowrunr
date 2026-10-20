//! Malakor's voice and face assets (contract appendix (voice)).
//!
//! - `GET /voice`: a WebSocket that pushes [`VoiceFrame`]s. The daemon gives each Malakor `say`
//!   of the followed live run to the voice sidecar (`tools/voice`, JSON lines on stdio) and
//!   forwards every rendered sentence as an `utterance`. Bounded: a line whose audio cannot
//!   start within `max_lag_ms` of its `say` is `skipped`, and the queue holds `max_queue` lines.
//! - `GET /avatar/{file}`: the face rig the stage draws (`layers.json` plus its PNGs) from
//!   `<data>/avatar/rig/`; 404 when there is none, and the stage draws its bundled placeholder.
//!
//! Optional end to end: with `[voice] enabled = false` (the default) `/voice` says `off` and
//! nothing is spawned; a missing engine or a dead sidecar is `degraded` with the reason, and the
//! stream itself never waits on the voice.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use shadowrunr_types::voice::{VoiceFrame, VoiceState};

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

/// `GET /voice`. Stub until AV-2: says the voice is off and holds the socket open.
pub async fn voice_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !crate::server::origin_ok(&headers, &state.cfg.daemon.extra_origins) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| session(state, socket))
}

async fn session(_state: Arc<AppState>, mut socket: WebSocket) {
    let off = VoiceFrame::Status {
        state: VoiceState::Off,
        reason: Some("voice not built yet".into()),
        voice: None,
        visemes: false,
    };
    let text = serde_json::to_string(&off).unwrap_or_default();
    if socket.send(Message::Text(text.into())).await.is_err() {
        return;
    }
    while let Some(Ok(msg)) = socket.recv().await {
        if matches!(msg, Message::Close(_)) {
            break;
        }
    }
}

/// `GET /avatar/{file}`. Stub until AV-2: no rig.
pub async fn avatar_handler(
    State(_state): State<Arc<AppState>>,
    UrlPath(_file): UrlPath<String>,
) -> Response {
    StatusCode::NOT_FOUND.into_response()
}
