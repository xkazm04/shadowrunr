//! Who speaks a character, and with which voice: the effective choice, the operator's local
//! override, and the `voice.*` RPCs (contract § voice providers).
//!
//! - **The effective choice** is built per field: the daemon's `[voice]` (the file's, before any
//!   package), the package `[voice]` over it, then the local override
//!   `<data>/characters/<id>/voice.json` (gitignored), which wins. It is resolved at each stream
//!   start for the streaming character ([`DaemonVoices`]), so a change takes effect at the next
//!   one, with no restart. A provider never falls back to another.
//! - **The override** is written atomically (a temp file and a rename), at most
//!   [`MAX_OVERRIDE_BYTES`], and only inside `<data>` (`tone::contained`).
//! - **Preview and bench** render through a renderer of their own ([`PreviewVoice`], one at a
//!   time), never the show's queue, and every line passes the brain's output gate first. That
//!   renderer is a second sidecar with warm workers of its own, so it never shares the machine
//!   with a show: it stops [`PREVIEW_IDLE_MS`] after its last use, at once when a stream starts
//!   being voiced, and while a run is voiced both RPCs answer `busy`.
//! - **Keys** are named by env var only: `voice.settings.get` says whether the var is set and
//!   what the last `voice.verify` found, never the value.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use shadowrunr_brain::moderation::gate_text;
use shadowrunr_types::character::VoiceFx;
use shadowrunr_types::voice_choice::{
    ElevenlabsVoice, KokoroVoice, ProviderStatus, VoiceBench, VoiceBenchFile, VoiceBenchLine,
    VoiceBenchParams, VoiceChoice, VoiceKeyState, VoicePreview, VoicePreviewParams, VoiceProvider,
    VoiceSettings, VoiceSettingsGetParams, VoiceSettingsSetParams, VoiceVerifyParams,
};
use shadowrunr_types::ErrorCode;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::elevenlabs::{self, ElevenSpec, ElevenlabsConfig, ElevenlabsRenderer};
use super::renderer::{b64_decode, Part, Renderer, RendererState, Utter};
use super::service::{Spoken, Voices};
use super::sidecar::{sidecar_args, SidecarRenderer};
use super::{probe, VoiceConfig};
use crate::config::Config;
use crate::rpc::RpcError;
use crate::server::AppState;
use crate::tone::{contained, io, read_text};
use crate::{catalog, character, runs};

/// The override's file name in `<data>/characters/<id>/`.
pub const OVERRIDE_FILE: &str = "voice.json";
/// The largest override `voice.settings.set` writes.
pub const MAX_OVERRIDE_BYTES: usize = 4096;
/// The longest line `voice.preview` speaks, in characters.
pub const MAX_PREVIEW_CHARS: usize = 400;
/// The speeds a choice may set: what each provider takes (the console bounds the same).
pub const KOKORO_SPEED: RangeInclusive<f32> = 0.5..=2.0;
pub const ELEVENLABS_SPEED: RangeInclusive<f32> = 0.7..=1.2;
/// How long a preview waits for its renderer to come up (the sidecar's first load).
const READY_WAIT: Duration = Duration::from_secs(120);
/// How long the preview renderer is kept after its last preview or bench (an assumption: long
/// enough for lines tried one after another, short enough that it is gone before a show).
pub const PREVIEW_IDLE_MS: u64 = 60_000;

/// One override write at a time.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// What a renderer is built from.
#[derive(Debug, Clone)]
pub enum Spec {
    /// The sidecar, with the choice's kokoro fields over the daemon's `[voice]`.
    Kokoro(Box<VoiceConfig>),
    Elevenlabs(ElevenlabsConfig, ElevenSpec),
}

/// The preview's own renderer, kept while previews ask for the same voice.
pub struct PreviewVoice {
    key: String,
    renderer: Arc<dyn Renderer>,
    /// Its last use: the idle stop takes only the renderer it was armed for.
    used: u64,
    /// Dropping it stops the renderer.
    _stop: watch::Sender<bool>,
}

// ── the effective choice ───────────────────────────────────────────────

/// `<data>/characters/<id>/voice.json`.
pub fn override_path(data: &Path, character: &str) -> PathBuf {
    data.join("characters").join(character).join(OVERRIDE_FILE)
}

/// The stored override, when there is one (checked to resolve inside `<data>`).
pub fn read_override(cfg: &Config, character: &str) -> Result<Option<VoiceChoice>, RpcError> {
    let path = override_path(&cfg.paths.data, character);
    if !path.exists() {
        return Ok(None);
    }
    contained(&cfg.paths.data, &path)?;
    let Some(text) = read_text(&path)? else {
        return Ok(None);
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| io(&path, e))
}

/// The package `[voice]` over the daemon's `[voice]`: the kokoro fields always, the
/// ElevenLabs model from `[voice.elevenlabs]` and the default speed where the package sets none.
pub fn package_choice(cfg: &Config, character: &str) -> VoiceChoice {
    let m = character::manifest(&cfg.paths.characters, character);
    let merged = character::voice_for(&cfg.voice_daemon, m.as_ref());
    let cv = m.as_ref().and_then(|m| m.voice.as_ref());
    let el = cv.and_then(|v| v.elevenlabs.clone()).unwrap_or_default();
    VoiceChoice {
        provider: cv.and_then(|v| v.provider).unwrap_or_default(),
        kokoro: Some(KokoroVoice {
            voice: Some(merged.voice),
            sid: Some(merged.sid),
            voices_file: merged.voices_file,
            fx: merged.fx,
            speed: cv.and_then(|v| v.speed).map(|s| s as f32),
        }),
        elevenlabs: Some(ElevenlabsVoice {
            voice_id: el.voice_id,
            model: el
                .model
                .or_else(|| Some(cfg.voice_daemon.elevenlabs.model.clone())),
            speed: el.speed.or(Some(elevenlabs::DEFAULT_SPEED)),
        }),
    }
}

/// `package` with `over` on top, field by field (the override's provider always wins).
pub fn effective(package: &VoiceChoice, over: Option<&VoiceChoice>) -> VoiceChoice {
    let Some(o) = over else {
        return package.clone();
    };
    let kokoro = match (&package.kokoro, &o.kokoro) {
        (p, None) => p.clone(),
        (p, Some(o)) => {
            let p = p.clone().unwrap_or_default();
            Some(KokoroVoice {
                voice: o.voice.clone().or(p.voice),
                sid: o.sid.or(p.sid),
                voices_file: o.voices_file.clone().or(p.voices_file),
                fx: o.fx.or(p.fx),
                speed: o.speed.or(p.speed),
            })
        }
    };
    let elevenlabs = match (&package.elevenlabs, &o.elevenlabs) {
        (p, None) => p.clone(),
        (p, Some(o)) => {
            let p = p.clone().unwrap_or_default();
            Some(ElevenlabsVoice {
                voice_id: o.voice_id.clone().or(p.voice_id),
                model: o.model.clone().or(p.model),
                speed: o.speed.or(p.speed),
            })
        }
    };
    VoiceChoice {
        provider: o.provider,
        kokoro,
        elevenlabs,
    }
}

/// The daemon's `[voice]` with the choice's kokoro fields over it. `speed` is not applied: the
/// sidecar takes its speed per line, from the delivery.
pub fn kokoro_config(cfg: &Config, choice: &VoiceChoice) -> VoiceConfig {
    let mut v = cfg.voice_daemon.clone();
    v.enabled = cfg.voice.enabled;
    if let Some(k) = &choice.kokoro {
        if let Some(voice) = &k.voice {
            v.voice = voice.clone();
        }
        if let Some(sid) = k.sid {
            v.sid = sid;
        }
        if let Some(file) = &k.voices_file {
            v.voices_file = Some(file.clone());
        }
        if let Some(fx) = k.fx {
            v.fx = Some(fx);
        }
    }
    v
}

fn eleven_spec(cfg: &Config, choice: &VoiceChoice) -> ElevenSpec {
    let el = choice.elevenlabs.clone().unwrap_or_default();
    ElevenSpec {
        voice_id: el.voice_id,
        model: el
            .model
            .unwrap_or_else(|| cfg.voice_daemon.elevenlabs.model.clone()),
        speed: el.speed.unwrap_or(elevenlabs::DEFAULT_SPEED),
    }
}

/// The human label: "ElevenLabs Malakor (crypt king alt1), eleven_v4_turbo" or "Kokoro
/// malakor-a (sid 52), fx light". An ElevenLabs voice whose name has not been read yet is
/// "ElevenLabs voice 43UFEk…" ([`elevenlabs::voice_label`]).
pub fn label(cfg: &Config, choice: &VoiceChoice) -> String {
    match choice.provider {
        VoiceProvider::Kokoro => {
            let v = kokoro_config(cfg, choice);
            let mut out = format!("Kokoro {} (sid {})", v.voice, v.sid);
            if let Some(fx) = v.fx.filter(|f| *f != VoiceFx::None) {
                out.push_str(&format!(", fx {}", fx.as_str()));
            }
            out
        }
        VoiceProvider::Elevenlabs => {
            let s = eleven_spec(cfg, choice);
            let voice = match &s.voice_id {
                Some(id) => elevenlabs::voice_label(id),
                None => "(no voice_id)".into(),
            };
            format!("ElevenLabs {voice}, {}", s.model)
        }
    }
}

/// What tells two Kokoro sidecars apart: the interpreter, the package and every argument.
pub fn kokoro_key(v: &VoiceConfig) -> String {
    format!(
        "kokoro|{}|{}|{}",
        v.python,
        v.sidecar,
        sidecar_args(v).join("\u{1f}")
    )
}

/// The voice `choice` speaks with.
pub fn spoken(cfg: &Config, choice: &VoiceChoice) -> Spoken {
    match choice.provider {
        VoiceProvider::Kokoro => {
            let v = kokoro_config(cfg, choice);
            Spoken {
                provider: VoiceProvider::Kokoro,
                voice: v.voice.clone(),
                key: kokoro_key(&v),
                spec: Spec::Kokoro(Box::new(v)),
            }
        }
        VoiceProvider::Elevenlabs => {
            let el = cfg.voice_daemon.elevenlabs.clone();
            let s = eleven_spec(cfg, choice);
            let id = s.voice_id.clone().unwrap_or_default();
            let key = format!(
                "elevenlabs|{id}|{}|{}|{}|{}",
                s.model, s.speed, el.api_key_env, el.base_url
            );
            Spoken {
                provider: VoiceProvider::Elevenlabs,
                voice: elevenlabs::voice_name(&id).unwrap_or(id),
                key,
                spec: Spec::Elevenlabs(el, s),
            }
        }
    }
}

/// `character`'s voice now, for a stream start: a stored override that cannot be read is said
/// on stderr and left out (the package's voice speaks).
pub fn spoken_for(cfg: &Config, character: &str) -> Spoken {
    let over = read_override(cfg, character).unwrap_or_else(|e| {
        eprintln!(
            "voice: {character}'s {OVERRIDE_FILE} is not used: {}",
            e.message
        );
        None
    });
    spoken(
        cfg,
        &effective(&package_choice(cfg, character), over.as_ref()),
    )
}

// ── the daemon's renderers ─────────────────────────────────────────────

/// The daemon's [`Voices`]: resolves from the package, the daemon config and the override, and
/// starts the sidecar or the ElevenLabs renderer. Every renderer it starts also stops with the
/// daemon.
pub struct DaemonVoices {
    cfg: Arc<Config>,
    down: watch::Receiver<bool>,
    /// The CLI's: the daemon's signal, never set.
    _never: Option<watch::Sender<bool>>,
}

impl DaemonVoices {
    pub fn new(state: &Arc<AppState>) -> Self {
        let (tx, down) = watch::channel(false);
        let signal = state.shutdown_signal();
        tokio::spawn(async move {
            signal.await;
            tx.send_replace(true);
        });
        Self {
            cfg: state.cfg.clone(),
            down,
            _never: None,
        }
    }

    /// Built from a config alone (the CLI): it stops only by its `stop`.
    pub fn standalone(cfg: Arc<Config>) -> Self {
        let (tx, down) = watch::channel(false);
        Self {
            cfg,
            down,
            _never: Some(tx),
        }
    }
}

impl Voices for DaemonVoices {
    fn resolve(&self, character: &str) -> Option<Spoken> {
        Some(spoken_for(&self.cfg, character))
    }

    fn resolve_foil(&self, character: &str, foil: &str) -> Result<Option<Spoken>, String> {
        super::foils::resolve(&self.cfg, character, foil)
    }

    fn start(
        &self,
        spoken: &Spoken,
        mut stop: watch::Receiver<bool>,
    ) -> (Arc<dyn Renderer>, Option<JoinHandle<()>>) {
        let mut down = self.down.clone();
        let ended = async move {
            tokio::select! {
                _ = stop.wait_for(|s| *s) => {}
                _ = down.wait_for(|d| *d) => {}
            }
        };
        match &spoken.spec {
            Spec::Kokoro(v) => {
                let (r, task) = SidecarRenderer::start((**v).clone(), ended);
                (r, Some(task))
            }
            Spec::Elevenlabs(cfg, s) => (
                ElevenlabsRenderer::start(cfg.clone(), s.clone(), ended),
                None,
            ),
        }
    }
}

// ── RPCs ───────────────────────────────────────────────────────────────

/// The character's package dir; an unknown character is `invalid_params`.
fn known(cfg: &Config, character: &str) -> Result<PathBuf, RpcError> {
    catalog::character_dir(cfg, character).map_err(|e| match e.code {
        ErrorCode::NotFound => RpcError::invalid(format!("unknown character {character:?}")),
        _ => e,
    })
}

/// The kokoro row: the engine's files and the voices file (no python run; `voice.verify` runs
/// the whole check).
fn kokoro_files(v: &VoiceConfig) -> ProviderStatus {
    let mut items = probe::engine_items(v.engine_dir.as_deref());
    items.extend(probe::voices_item(v));
    let missing = items.iter().find(|i| i.needed && !i.ok());
    ProviderStatus {
        provider: VoiceProvider::Kokoro,
        ready: missing.is_none(),
        key: VoiceKeyState::NotNeeded,
        reason: missing.map(|i| format!("{}: {}", i.item, i.detail)),
        checked_ms: None,
    }
}

/// Both providers' readiness for `choice`, with the last verify verdicts.
fn providers(state: &AppState, choice: &VoiceChoice) -> Vec<ProviderStatus> {
    let cfg = &state.cfg;
    let verdicts = state
        .voice
        .verdicts
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let kokoro = verdicts
        .get(&VoiceProvider::Kokoro)
        .cloned()
        .unwrap_or_else(|| kokoro_files(&kokoro_config(cfg, choice)));
    let el_cfg = &cfg.voice_daemon.elevenlabs;
    let mut el = if !el_cfg.key_set() {
        ProviderStatus {
            provider: VoiceProvider::Elevenlabs,
            ready: false,
            key: VoiceKeyState::Missing,
            reason: Some(format!(
                "set {} in the daemon's environment",
                el_cfg.api_key_env
            )),
            checked_ms: None,
        }
    } else {
        verdicts
            .get(&VoiceProvider::Elevenlabs)
            .cloned()
            .unwrap_or(ProviderStatus {
                provider: VoiceProvider::Elevenlabs,
                ready: true,
                key: VoiceKeyState::Set,
                reason: None,
                checked_ms: None,
            })
    };
    let has_voice = choice
        .elevenlabs
        .as_ref()
        .is_some_and(|e| e.voice_id.is_some());
    if el.ready && !has_voice {
        el.ready = false;
        el.reason = Some("no voice_id: set one in the character's voice settings".into());
    }
    vec![kokoro, el]
}

fn settings(state: &AppState, character: &str) -> Result<VoiceSettings, RpcError> {
    let cfg = &state.cfg;
    known(cfg, character)?;
    let package = package_choice(cfg, character);
    let over = read_override(cfg, character)?;
    let effective = effective(&package, over.as_ref());
    Ok(VoiceSettings {
        character: character.to_string(),
        providers: providers(state, &effective),
        label: label(cfg, &effective),
        effective,
        package,
        r#override: over,
    })
}

/// `voice.settings.get`.
pub async fn settings_get(
    state: &Arc<AppState>,
    p: VoiceSettingsGetParams,
) -> Result<VoiceSettings, RpcError> {
    settings(state, &p.character)
}

fn plain(what: &str, s: &str, extra: &[char]) -> Result<(), RpcError> {
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || extra.contains(&c));
    if ok {
        Ok(())
    } else {
        Err(RpcError::invalid(format!(
            "{what} {s:?} must be 1 to 64 of [A-Za-z0-9_-]"
        )))
    }
}

fn check_speed(
    what: &str,
    speed: Option<f32>,
    range: &RangeInclusive<f32>,
) -> Result<(), RpcError> {
    match speed {
        Some(s) if !range.contains(&s) => Err(RpcError::invalid(format!(
            "{what} speed {s} is outside {}..={}",
            range.start(),
            range.end()
        ))),
        _ => Ok(()),
    }
}

/// A choice `voice.settings.set` may store.
pub fn check_choice(c: &VoiceChoice) -> Result<(), RpcError> {
    if let Some(k) = &c.kokoro {
        check_speed("kokoro", k.speed, &KOKORO_SPEED)?;
        if let Some(v) = &k.voice {
            plain("kokoro voice", v, &['.'])?;
        }
        if let Some(f) = &k.voices_file {
            plain("voices_file", f, &['.'])?;
            if f.starts_with('.') {
                return Err(RpcError::invalid("voices_file must be a plain file name"));
            }
        }
    }
    if let Some(e) = &c.elevenlabs {
        check_speed("elevenlabs", e.speed, &ELEVENLABS_SPEED)?;
        if let Some(v) = &e.voice_id {
            plain("voice_id", v, &[])?;
        }
        if let Some(m) = &e.model {
            plain("model", m, &['.'])?;
        }
    }
    Ok(())
}

/// Write through a synced temp file beside `path` and a rename.
fn write_atomic(path: &Path, text: &str) -> Result<(), RpcError> {
    use std::io::Write;
    let tmp = path.with_file_name(format!(".{OVERRIDE_FILE}.tmp"));
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written.map_err(|e| io(path, e))
}

/// Store (or with `None`, clear) `character`'s override.
pub fn store_override(
    cfg: &Config,
    character: &str,
    choice: Option<&VoiceChoice>,
) -> Result<(), RpcError> {
    known(cfg, character)?;
    let path = override_path(&cfg.paths.data, character);
    let _held = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let Some(choice) = choice else {
        if path.exists() {
            contained(&cfg.paths.data, &path)?;
            std::fs::remove_file(&path).map_err(|e| io(&path, e))?;
        }
        return Ok(());
    };
    check_choice(choice)?;
    let text = serde_json::to_string_pretty(choice)
        .map_err(|e| RpcError::internal(format!("serialize the override: {e}")))?
        + "\n";
    if text.len() > MAX_OVERRIDE_BYTES {
        return Err(RpcError::invalid(format!(
            "the override is {} bytes; at most {MAX_OVERRIDE_BYTES}",
            text.len()
        )));
    }
    let dir = path.parent().expect("a file in a directory");
    std::fs::create_dir_all(&cfg.paths.data).map_err(|e| io(&cfg.paths.data, e))?;
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    contained(&cfg.paths.data, dir)?;
    if path.exists() {
        contained(&cfg.paths.data, &path)?;
    }
    write_atomic(&path, &text)
}

/// `voice.settings.set`: the next stream start of the character speaks with it.
pub async fn settings_set(
    state: &Arc<AppState>,
    p: VoiceSettingsSetParams,
) -> Result<VoiceSettings, RpcError> {
    let cfg = state.cfg.clone();
    let (character, choice) = (p.character.clone(), p.choice.clone());
    crate::tone::blocking(move || store_override(&cfg, &character, choice.as_ref())).await?;
    name_effective_voice(&state.cfg, &p.character).await;
    settings(state, &p.character)
}

/// Read the name of `character`'s effective ElevenLabs voice for the label, when it speaks
/// with one whose name is not kept yet.
async fn name_effective_voice(cfg: &Config, character: &str) {
    let over = read_override(cfg, character).ok().flatten();
    let choice = effective(&package_choice(cfg, character), over.as_ref());
    if choice.provider != VoiceProvider::Elevenlabs {
        return;
    }
    if let Some(id) = choice.elevenlabs.and_then(|e| e.voice_id) {
        elevenlabs::lookup_name(&cfg.voice_daemon.elevenlabs, &id).await;
    }
}

/// `voice.verify`: ElevenLabs' one cheap authenticated call, or kokoro's whole `voice check` for
/// the configured character's voice. The verdict is kept for `voice.settings.get`.
pub async fn verify(
    state: &Arc<AppState>,
    p: VoiceVerifyParams,
) -> Result<ProviderStatus, RpcError> {
    let cfg = &state.cfg;
    let now = runs::now_ms();
    let status = match p.provider {
        VoiceProvider::Elevenlabs => {
            let status = elevenlabs::verify(&cfg.voice_daemon.elevenlabs, now).await;
            if status.key != VoiceKeyState::Rejected {
                name_effective_voice(cfg, &cfg.sim.character).await;
            }
            status
        }
        VoiceProvider::Kokoro => {
            let character = &cfg.sim.character;
            let over = read_override(cfg, character).ok().flatten();
            let choice = effective(&package_choice(cfg, character), over.as_ref());
            let items = probe::check(&kokoro_config(cfg, &choice)).await;
            let missing = items.iter().find(|i| i.needed && !i.ok());
            ProviderStatus {
                provider: VoiceProvider::Kokoro,
                ready: missing.is_none(),
                key: VoiceKeyState::NotNeeded,
                reason: missing.map(|i| format!("{}: {}", i.item, i.detail)),
                checked_ms: Some(now),
            }
        }
    };
    state
        .voice
        .verdicts
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(p.provider, status.clone());
    Ok(status)
}

/// `invalid_params` with the gate's reason when the brain's output gate would change `text`.
fn gate(cfg: &Config, text: &str) -> Result<(), RpcError> {
    let policy = crate::clips::policy(cfg)?;
    let (_, fired) = gate_text(text, &policy, &[]);
    match fired.first() {
        Some((rule, action)) => Err(RpcError::invalid(format!(
            "the brain's output gate refused this line ({rule} {action})"
        ))),
        None => Ok(()),
    }
}

/// A rendered line, its parts joined.
#[derive(Debug, Default)]
pub struct Take {
    pub pcm: Vec<u8>,
    pub sample_rate: u32,
    pub chars: u32,
    pub visemes: bool,
}

impl Take {
    pub fn duration_ms(&self) -> u32 {
        if self.sample_rate == 0 {
            return 0;
        }
        let samples = (self.pcm.len() / 2) as u64;
        u32::try_from(samples * 1000 / u64::from(self.sample_rate)).unwrap_or(u32::MAX)
    }

    pub fn pcm_b64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.pcm)
    }
}

/// Speak `utter` through `renderer` as `id`: all its parts, each within `per_part`.
pub async fn render(
    renderer: &dyn Renderer,
    id: &str,
    utter: &Utter,
    per_part: Duration,
) -> Result<Take, String> {
    let mut rx = renderer.say(id, utter);
    let mut take = Take::default();
    loop {
        match tokio::time::timeout(per_part, rx.recv()).await {
            Ok(Some(Part::Audio(r))) => {
                take.pcm.extend(b64_decode(&r.pcm).unwrap_or_default());
                take.sample_rate = r.sample_rate;
                take.chars = take.chars.saturating_add(r.chars);
                take.visemes |= !r.visemes.is_empty();
                if r.last {
                    return Ok(take);
                }
            }
            Ok(Some(Part::Error { reason, .. })) => return Err(reason),
            Ok(None) => return Err("the renderer dropped the line".into()),
            Err(_) => {
                renderer.cancel(id);
                return Err(format!(
                    "a sentence took longer than {} ms",
                    per_part.as_millis()
                ));
            }
        }
    }
}

/// The preview renderer for `spoken`, up: the kept one when it speaks the same voice.
async fn preview_renderer(
    state: &Arc<AppState>,
    slot: &mut Option<PreviewVoice>,
    spoken: &Spoken,
) -> Result<Arc<dyn Renderer>, RpcError> {
    if slot.as_ref().is_none_or(|p| p.key != spoken.key) {
        *slot = None;
        let (stop, stop_rx) = watch::channel(false);
        let (renderer, _) = DaemonVoices::new(state).start(spoken, stop_rx);
        *slot = Some(PreviewVoice {
            key: spoken.key.clone(),
            renderer,
            used: 0,
            _stop: stop,
        });
    }
    let renderer = slot.as_ref().expect("set above").renderer.clone();
    let mut st = renderer.state();
    let up = tokio::time::timeout(
        READY_WAIT,
        st.wait_for(|s| !matches!(s, RendererState::Starting)),
    )
    .await;
    let why = match up {
        Ok(Ok(s)) => match &*s {
            RendererState::Degraded(why) => Some(why.clone()),
            _ => None,
        },
        Ok(Err(_)) => Some("the voice stopped".into()),
        Err(_) => Some(format!(
            "the voice did not come up within {} s",
            READY_WAIT.as_secs()
        )),
    };
    match why {
        None => Ok(renderer),
        Some(why) => {
            // Not kept: the next preview starts it again.
            *slot = None;
            Err(RpcError::new(ErrorCode::Unavailable, why))
        }
    }
}

pub(crate) fn per_part(cfg: &Config) -> Duration {
    let ms = cfg
        .voice
        .render_timeout_ms
        .max(cfg.voice_daemon.elevenlabs.timeout_ms)
        .saturating_add(1_000);
    Duration::from_millis(ms)
}

fn next_id(what: &str) -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    format!("{what}:{}", N.fetch_add(1, Ordering::Relaxed))
}

fn busy() -> RpcError {
    RpcError::new(ErrorCode::Busy, "a preview is already rendering; try again")
}

/// What a preview or bench answers while a run is voiced.
pub const STREAM_LIVE: &str = "a stream is live: preview and bench wait until it ends";

/// The preview slot, when no run is voiced and no other preview holds it.
fn take_slot(
    state: &AppState,
) -> Result<tokio::sync::MutexGuard<'_, Option<PreviewVoice>>, RpcError> {
    if state.voice.voicing() {
        return Err(RpcError::new(ErrorCode::Busy, STREAM_LIVE));
    }
    state.voice.preview.try_lock().map_err(|_| busy())
}

/// After a preview or bench: stop the renderer when a run is voiced now, else arm its idle
/// stop ([`PREVIEW_IDLE_MS`]); a later use arms its own, so only the last one fires.
fn release(state: &Arc<AppState>, slot: &mut Option<PreviewVoice>) {
    if state.voice.voicing() {
        *slot = None;
        return;
    }
    let Some(p) = slot.as_mut() else {
        return;
    };
    static USES: AtomicU64 = AtomicU64::new(1);
    p.used = USES.fetch_add(1, Ordering::Relaxed);
    arm_idle_stop(state, p.used, Duration::from_millis(PREVIEW_IDLE_MS));
}

/// Stop the preview renderer `after` from now, if it is still the use `used` (idle since).
fn arm_idle_stop(state: &Arc<AppState>, used: u64, after: Duration) {
    let weak = Arc::downgrade(state);
    tokio::spawn(async move {
        tokio::time::sleep(after).await;
        let Some(state) = weak.upgrade() else {
            return;
        };
        // Held: a preview is rendering, and it arms the next stop.
        let stopped = match state.voice.preview.try_lock() {
            Ok(mut slot) if slot.as_ref().is_some_and(|p| p.used == used) => slot.take(),
            _ => None,
        };
        drop(stopped);
    });
}

/// The character's effective voice, reading the override strictly.
fn effective_spoken(cfg: &Config, character: &str) -> Result<(Spoken, String), RpcError> {
    let over = read_override(cfg, character)?;
    let choice = effective(&package_choice(cfg, character), over.as_ref());
    Ok((spoken(cfg, &choice), label(cfg, &choice)))
}

/// `voice.preview`: one gated line in the character's effective voice.
pub async fn preview(
    state: &Arc<AppState>,
    p: VoicePreviewParams,
) -> Result<VoicePreview, RpcError> {
    let cfg = &state.cfg;
    known(cfg, &p.character)?;
    let text = p.text.trim();
    if text.is_empty() {
        return Err(RpcError::invalid("text must not be empty"));
    }
    if text.chars().count() > MAX_PREVIEW_CHARS {
        return Err(RpcError::invalid(format!(
            "text is longer than {MAX_PREVIEW_CHARS} characters"
        )));
    }
    gate(cfg, text)?;
    let mut slot = take_slot(state)?;
    let (spoken, _) = effective_spoken(cfg, &p.character)?;
    let renderer = preview_renderer(state, &mut slot, &spoken).await?;
    let utter = Utter {
        text: text.to_string(),
        delivery: p.delivery,
    };
    let take = render(
        renderer.as_ref(),
        &next_id("preview"),
        &utter,
        per_part(cfg),
    )
    .await;
    release(state, &mut slot);
    drop(slot);
    let take = take.map_err(|why| RpcError::new(ErrorCode::Unavailable, why))?;
    // After the render: the label names the voice the provider reported.
    let (_, label) = effective_spoken(cfg, &p.character)?;
    Ok(VoicePreview {
        provider: spoken.provider,
        label,
        sample_rate: take.sample_rate,
        pcm: take.pcm_b64(),
        duration_ms: take.duration_ms(),
        chars: take.chars,
        visemes: take.visemes,
        delivery: p.delivery,
    })
}

/// `voice.bench`: every line of the package's `voice/bench.toml`, one at a time; a line that
/// fails carries its `error` and the rest go on.
pub async fn bench(state: &Arc<AppState>, p: VoiceBenchParams) -> Result<VoiceBench, RpcError> {
    let cfg = &state.cfg;
    let dir = known(cfg, &p.character)?;
    let path = dir.join("voice").join("bench.toml");
    if !path.is_file() {
        return Err(RpcError::not_found(format!(
            "character {} has no voice/bench.toml",
            p.character
        )));
    }
    contained(&cfg.paths.characters, &path)?;
    let text = read_text(&path)?.unwrap_or_default();
    let file: VoiceBenchFile = toml::from_str(&text).map_err(|e| io(&path, e))?;
    let mut slot = take_slot(state)?;
    let (spoken, _) = effective_spoken(cfg, &p.character)?;
    let renderer = preview_renderer(state, &mut slot, &spoken).await?;
    let mut lines = Vec::with_capacity(file.lines.len());
    for entry in &file.lines {
        let delivery = entry.delivery();
        let text = entry.text.trim();
        // A stream voiced since the bench began: the rest of the lines give way to it.
        let checked = if state.voice.voicing() {
            Err(STREAM_LIVE.to_string())
        } else if text.chars().count() > MAX_PREVIEW_CHARS {
            Err(format!("longer than {MAX_PREVIEW_CHARS} characters"))
        } else {
            gate(cfg, text).map_err(|e| e.message)
        };
        let utter = Utter {
            text: text.to_string(),
            delivery: Some(delivery),
        };
        let take = match checked {
            Ok(()) => render(renderer.as_ref(), &next_id("bench"), &utter, per_part(cfg)).await,
            Err(why) => Err(why),
        };
        let (take, error) = match take {
            Ok(t) => (t, None),
            Err(why) => (Take::default(), Some(why)),
        };
        lines.push(VoiceBenchLine {
            id: entry.id.clone(),
            text: entry.text.clone(),
            delivery,
            sample_rate: take.sample_rate,
            pcm: take.pcm_b64(),
            duration_ms: take.duration_ms(),
            chars: take.chars,
            error,
        });
    }
    release(state, &mut slot);
    drop(slot);
    let (_, label) = effective_spoken(cfg, &p.character)?;
    Ok(VoiceBench {
        provider: spoken.provider,
        label,
        lines,
    })
}

#[cfg(test)]
mod tests;
