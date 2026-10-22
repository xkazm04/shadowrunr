//! The ElevenLabs renderer behind [`Renderer`]: the cloud voice, one HTTP request per sentence
//! (`POST /v1/text-to-speech/{voice_id}/with-timestamps?output_format=pcm_24000`), so a line's
//! parts flow like the sidecar's (the same sentence rules, [`super::text::split_line`]).
//!
//! - **The key** is read from the env var `[voice.elevenlabs] api_key_env` names at each call
//!   and sent only in the `xi-api-key` header. Nothing here stores it: it is never logged, never
//!   in an error, never in `Debug`.
//! - **Delivery** becomes a short inline audio tag before each sentence (`[sarcastic] ...`,
//!   [`audio_tag`]); an intensity under [`TAG_MIN_INTENSITY`] or a neutral line gets none, and
//!   the pace has no tag (the brain writes pauses as punctuation).
//! - **Speed**: the voice's `speed` goes in `voice_settings`; a model that refuses it is said
//!   once on stderr and asked without it from then on.
//! - **No visemes**: the mouth follows loudness. A part's `timings` is `provider` when the
//!   character alignment came back, else `none`; `chars` is what the request billed.
//! - **Failures**: 401/403 is `degraded` ("elevenlabs: key rejected"); a network error or a
//!   timeout fails the line from its part with a short reason. It never falls back to another
//!   provider: the voice says why it is not speaking, and captions carry the show.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use shadowrunr_types::delivery::{Delivery, Emotion};
use shadowrunr_types::voice::VoiceTimings;
use shadowrunr_types::voice_choice::{ProviderStatus, VoiceKeyState, VoiceProvider};
use tokio::sync::{mpsc, watch};
use tokio::task::AbortHandle;

use super::renderer::{Part, Rendered, Renderer, RendererState, Utter};
use super::text;

/// The sample rate asked for (`output_format=pcm_24000`), the sidecar's own.
pub const SAMPLE_RATE: u32 = 24_000;
/// A delivery below this intensity gets no audio tag.
pub const TAG_MIN_INTENSITY: f32 = 0.4;
/// The speed a voice speaks at when its choice sets none: the owner's "slightly quicker".
pub const DEFAULT_SPEED: f32 = 1.08;
/// The reason a refused key leaves.
pub const KEY_REJECTED: &str = "elevenlabs: key rejected";

/// `[voice.elevenlabs]` in `shadowrunr.toml`. The key itself is never here: only the name of
/// the env var that holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ElevenlabsConfig {
    pub api_key_env: String,
    /// The model a choice that names none speaks with.
    pub model: String,
    pub base_url: String,
    /// One request, at most.
    pub timeout_ms: u64,
}

impl Default for ElevenlabsConfig {
    fn default() -> Self {
        Self {
            api_key_env: "ELEVENLABS_API_KEY".into(),
            model: "eleven_v4_turbo".into(),
            base_url: "https://api.elevenlabs.io".into(),
            timeout_ms: 20_000,
        }
    }
}

impl ElevenlabsConfig {
    /// Whether the key's env var holds a value (never the value).
    pub fn key_set(&self) -> bool {
        self.key().is_some()
    }

    fn key(&self) -> Option<String> {
        std::env::var(&self.api_key_env)
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url.trim_end_matches('/'))
    }

    fn client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_millis(self.timeout_ms.max(1)))
            .build()
            .unwrap_or_default()
    }

    fn missing(&self) -> String {
        format!("elevenlabs: {} is not set", self.api_key_env)
    }
}

/// The voice one renderer speaks: resolved from the character's effective choice.
#[derive(Debug, Clone, PartialEq)]
pub struct ElevenSpec {
    pub voice_id: Option<String>,
    pub model: String,
    pub speed: f32,
}

/// The inline audio tag a delivery opens each sentence with; `None` for neutral, or under
/// [`TAG_MIN_INTENSITY`].
pub fn audio_tag(d: Option<&Delivery>) -> Option<&'static str> {
    let d = d?;
    if d.intensity < TAG_MIN_INTENSITY {
        return None;
    }
    Some(match d.emotion {
        Emotion::Neutral => return None,
        Emotion::Sarcasm => "[sarcastic]",
        Emotion::Hushed => "[whispers]",
        Emotion::Menace => "[menacing]",
        Emotion::Glee => "[excited]",
        Emotion::Deadpan => "[deadpan]",
        Emotion::Outrage => "[outraged]",
        Emotion::Weary => "[sighs]",
        Emotion::Triumph => "[proclaiming]",
        Emotion::Tender => "[softly]",
    })
}

/// Voice names by id, as the API last gave them (for status labels).
fn names() -> &'static Mutex<HashMap<String, String>> {
    static NAMES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    NAMES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The voice's name, when a check has read it.
pub fn voice_name(voice_id: &str) -> Option<String> {
    names()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(voice_id)
        .cloned()
}

fn remember_name(voice_id: &str, name: &str) {
    names()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(voice_id.to_string(), name.to_string());
}

/// Why a request failed. Every variant's text is safe to show: none carries the key.
#[derive(Debug, Clone, PartialEq)]
enum Fail {
    Missing(String),
    Rejected,
    /// An HTTP status the API answered with, and its `detail.status` when it gave one.
    Status(u16, String),
    Timeout(u64),
    Network,
    Body(String),
}

impl Fail {
    fn reason(&self) -> String {
        match self {
            Fail::Missing(why) => why.clone(),
            Fail::Rejected => KEY_REJECTED.into(),
            Fail::Status(429, _) => "elevenlabs: rate limited (429)".into(),
            Fail::Status(code, detail) if detail.is_empty() => format!("elevenlabs: HTTP {code}"),
            Fail::Status(code, detail) => format!("elevenlabs: HTTP {code} ({detail})"),
            Fail::Timeout(ms) => format!("elevenlabs: no answer within {ms} ms"),
            Fail::Network => "elevenlabs: cannot reach the API".into(),
            Fail::Body(why) => format!("elevenlabs: {why}"),
        }
    }
}

/// The API's `detail.status` (a short code such as `voice_not_found`), never its message.
fn detail_status(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["detail"]["status"].as_str().map(str::to_string))
        .filter(|s| s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or_default()
}

fn transport(e: &reqwest::Error, timeout_ms: u64) -> Fail {
    if e.is_timeout() {
        Fail::Timeout(timeout_ms)
    } else {
        Fail::Network
    }
}

/// One authenticated GET: the JSON body on 200.
async fn get_json(cfg: &ElevenlabsConfig, path: &str) -> Result<serde_json::Value, Fail> {
    let key = cfg.key().ok_or_else(|| Fail::Missing(cfg.missing()))?;
    let resp = cfg
        .client()
        .get(cfg.url(path))
        .header("xi-api-key", key)
        .send()
        .await
        .map_err(|e| transport(&e, cfg.timeout_ms))?;
    let status = resp.status().as_u16();
    let body = resp
        .bytes()
        .await
        .map_err(|e| transport(&e, cfg.timeout_ms))?;
    match status {
        200 => serde_json::from_slice(&body).map_err(|_| Fail::Body("not JSON".into())),
        401 | 403 if detail_status(&body).contains("missing_permission") => {
            Err(Fail::Status(status, "missing_permissions".into()))
        }
        401 | 403 => Err(Fail::Rejected),
        _ => Err(Fail::Status(status, detail_status(&body))),
    }
}

/// `voice.verify` for ElevenLabs: one cheap authenticated call (`GET /v1/user`). A refused
/// key is `rejected`; a call that could not say (network, 429, a key without account read
/// permission) leaves it `set` with the reason: could not verify is not broken.
pub async fn verify(cfg: &ElevenlabsConfig, now_ms: i64) -> ProviderStatus {
    let status = |ready: bool, key: VoiceKeyState, reason: Option<String>| ProviderStatus {
        provider: VoiceProvider::Elevenlabs,
        ready,
        key,
        reason,
        checked_ms: Some(now_ms),
    };
    match get_json(cfg, "/v1/user").await {
        Ok(_) => status(true, VoiceKeyState::Verified, None),
        Err(Fail::Missing(why)) => status(false, VoiceKeyState::Missing, Some(why)),
        Err(Fail::Rejected) => status(false, VoiceKeyState::Rejected, Some(KEY_REJECTED.into())),
        Err(Fail::Status(_, d)) if d == "missing_permissions" => status(
            true,
            VoiceKeyState::Set,
            Some(
                "the key cannot read the account (no user_read permission), so it was not verified"
                    .into(),
            ),
        ),
        Err(f) => status(
            true,
            VoiceKeyState::Set,
            Some(format!("could not verify: {}", f.reason())),
        ),
    }
}

/// What one sentence's request returned.
#[derive(Debug, Deserialize)]
struct Tts {
    #[serde(default)]
    audio_base64: String,
    #[serde(default)]
    alignment: Option<Alignment>,
}

#[derive(Debug, Deserialize)]
struct Alignment {
    #[serde(default)]
    characters: Vec<String>,
}

#[derive(Debug)]
struct Inner {
    cfg: ElevenlabsConfig,
    spec: ElevenSpec,
    http: reqwest::Client,
    state: watch::Sender<RendererState>,
    lines: Mutex<HashMap<String, AbortHandle>>,
    /// The key was refused: every line fails at once until the renderer is replaced.
    rejected: AtomicBool,
    /// The model takes `speed` (until it refuses it once).
    speed_ok: AtomicBool,
}

/// The ElevenLabs voice: see the module doc.
#[derive(Debug, Clone)]
pub struct ElevenlabsRenderer {
    inner: Arc<Inner>,
}

impl ElevenlabsRenderer {
    /// A renderer for `spec`: `starting` while one `GET /v1/voices/{voice_id}` checks the key
    /// and reads the voice's name, then `ready` (no visemes) or `degraded` with the reason. Its
    /// lines stop when `shutdown` resolves.
    pub fn start(
        cfg: ElevenlabsConfig,
        spec: ElevenSpec,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Arc<Self> {
        let me = Arc::new(Self {
            inner: Arc::new(Inner {
                http: cfg.client(),
                cfg,
                spec,
                state: watch::Sender::new(RendererState::Starting),
                lines: Mutex::new(HashMap::new()),
                rejected: AtomicBool::new(false),
                speed_ok: AtomicBool::new(true),
            }),
        });
        let inner = me.inner.clone();
        tokio::spawn(async move {
            let st = inner.check().await;
            inner.state.send_replace(st);
        });
        let inner = me.inner.clone();
        tokio::spawn(async move {
            shutdown.await;
            for (_, h) in inner.lines().drain() {
                h.abort();
            }
        });
        me
    }

    /// Whether the model still takes `speed`.
    pub fn speed_accepted(&self) -> bool {
        self.inner.speed_ok.load(Ordering::Relaxed)
    }
}

impl Inner {
    fn lines(&self) -> std::sync::MutexGuard<'_, HashMap<String, AbortHandle>> {
        self.lines.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn label(&self) -> String {
        let id = self.spec.voice_id.as_deref().unwrap_or("-");
        let name = voice_name(id).unwrap_or_else(|| id.to_string());
        format!("{name}, {}", self.spec.model)
    }

    async fn check(&self) -> RendererState {
        if !self.cfg.key_set() {
            return RendererState::Degraded(self.cfg.missing());
        }
        let Some(id) = self.spec.voice_id.clone() else {
            return RendererState::Degraded(
                "elevenlabs: no voice_id (set it in the character's voice.json)".into(),
            );
        };
        match get_json(&self.cfg, &format!("/v1/voices/{id}")).await {
            Ok(v) => {
                if let Some(name) = v["name"].as_str() {
                    remember_name(&id, name);
                }
                RendererState::Ready {
                    visemes: false,
                    reason: Some(self.label()),
                }
            }
            Err(Fail::Rejected) => {
                self.rejected.store(true, Ordering::Relaxed);
                RendererState::Degraded(KEY_REJECTED.into())
            }
            Err(Fail::Status(404, _)) => {
                RendererState::Degraded(format!("elevenlabs: voice {id} is not in the account"))
            }
            Err(Fail::Missing(why)) => RendererState::Degraded(why),
            Err(f) => RendererState::Ready {
                visemes: false,
                reason: Some(format!(
                    "{}; the voice was not checked: {}",
                    self.label(),
                    f.reason()
                )),
            },
        }
    }

    /// One sentence's request, with `speed` while the model takes it.
    async fn tts(&self, key: &str, text: &str) -> Result<Tts, Fail> {
        let with_speed = self.speed_ok.load(Ordering::Relaxed);
        match self.request(key, text, with_speed).await {
            Err(Fail::Status(400 | 422, body)) if with_speed && body.contains("speed") => {
                if self.speed_ok.swap(false, Ordering::Relaxed) {
                    eprintln!(
                        "voice: elevenlabs model {} does not take speed; asking without it",
                        self.spec.model
                    );
                }
                self.request(key, text, false).await
            }
            other => other,
        }
    }

    async fn request(&self, key: &str, text: &str, with_speed: bool) -> Result<Tts, Fail> {
        let voice = self.spec.voice_id.as_deref().unwrap_or_default();
        let mut body = serde_json::json!({ "text": text, "model_id": self.spec.model });
        if with_speed {
            body["voice_settings"] = serde_json::json!({ "speed": self.spec.speed });
        }
        let url = self.cfg.url(&format!(
            "/v1/text-to-speech/{voice}/with-timestamps?output_format=pcm_{SAMPLE_RATE}"
        ));
        let resp = self
            .http
            .post(url)
            .header("xi-api-key", key)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| transport(&e, self.cfg.timeout_ms))?;
        let status = resp.status().as_u16();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| transport(&e, self.cfg.timeout_ms))?;
        match status {
            200 => serde_json::from_slice(&bytes)
                .map_err(|_| Fail::Body("the answer is not JSON".into())),
            401 | 403 => Err(Fail::Rejected),
            400 | 422 => {
                // Kept whole only to spot a refused `speed`; the reason shows the status code.
                let text = String::from_utf8_lossy(&bytes).to_lowercase();
                let detail = detail_status(&bytes);
                Err(Fail::Status(
                    status,
                    if text.contains("speed") {
                        format!("{detail} speed")
                    } else {
                        detail
                    },
                ))
            }
            _ => Err(Fail::Status(status, detail_status(&bytes))),
        }
    }

    async fn speak(self: Arc<Self>, id: String, utter: Utter, tx: mpsc::UnboundedSender<Part>) {
        let parts = text::split_line(&utter.text);
        let tag = audio_tag(utter.delivery.as_ref());
        let n = parts.len();
        if n == 0 {
            let _ = tx.send(Part::Error {
                part: 0,
                reason: "nothing to say".into(),
            });
        }
        for (i, sentence) in parts.into_iter().enumerate() {
            let part = i as u32;
            let Some(key) = self.cfg.key() else {
                let why = self.cfg.missing();
                self.state
                    .send_replace(RendererState::Degraded(why.clone()));
                let _ = tx.send(Part::Error { part, reason: why });
                break;
            };
            let sent = match tag {
                Some(t) => format!("{t} {sentence}"),
                None => sentence.clone(),
            };
            match self.tts(&key, &sent).await {
                Ok(t) if !t.audio_base64.is_empty() => {
                    let timings = if t.alignment.is_some_and(|a| !a.characters.is_empty()) {
                        VoiceTimings::Provider
                    } else {
                        VoiceTimings::None
                    };
                    self.state.send_if_modified(|s| {
                        if matches!(s, RendererState::Degraded(_)) {
                            *s = RendererState::Ready {
                                visemes: false,
                                reason: Some(self.label()),
                            };
                            return true;
                        }
                        false
                    });
                    let ok = tx
                        .send(Part::Audio(Rendered {
                            part,
                            last: i + 1 == n,
                            text: sentence,
                            sample_rate: SAMPLE_RATE,
                            pcm: t.audio_base64,
                            visemes: Vec::new(),
                            chars: u32::try_from(sent.chars().count()).unwrap_or(u32::MAX),
                            timings,
                        }))
                        .is_ok();
                    if !ok {
                        break;
                    }
                }
                Ok(_) => {
                    let _ = tx.send(Part::Error {
                        part,
                        reason: "elevenlabs: the answer has no audio".into(),
                    });
                    break;
                }
                Err(f) => {
                    if f == Fail::Rejected {
                        self.rejected.store(true, Ordering::Relaxed);
                        self.state
                            .send_replace(RendererState::Degraded(KEY_REJECTED.into()));
                    }
                    let _ = tx.send(Part::Error {
                        part,
                        reason: f.reason(),
                    });
                    break;
                }
            }
        }
        self.lines().remove(&id);
    }
}

impl Renderer for ElevenlabsRenderer {
    fn state(&self) -> watch::Receiver<RendererState> {
        self.inner.state.subscribe()
    }

    fn say(&self, id: &str, line: &Utter) -> mpsc::UnboundedReceiver<Part> {
        let (tx, rx) = mpsc::unbounded_channel();
        let inner = &self.inner;
        let refused = if inner.rejected.load(Ordering::Relaxed) {
            Some(KEY_REJECTED.to_string())
        } else if !inner.cfg.key_set() {
            let why = inner.cfg.missing();
            inner
                .state
                .send_replace(RendererState::Degraded(why.clone()));
            Some(why)
        } else if inner.spec.voice_id.is_none() {
            Some("elevenlabs: no voice_id".into())
        } else {
            None
        };
        if let Some(reason) = refused {
            let _ = tx.send(Part::Error { part: 0, reason });
            return rx;
        }
        let task = tokio::spawn(inner.clone().speak(id.to_string(), line.clone(), tx));
        inner.lines().insert(id.to_string(), task.abort_handle());
        rx
    }

    fn cancel(&self, id: &str) {
        if let Some(h) = self.inner.lines().remove(id) {
            h.abort();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
