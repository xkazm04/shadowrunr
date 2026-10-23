//! Twitch: the moderator's bot account (docs/guide/moderator.md).
//!
//! Login is Twitch's device-code flow; the bot's tokens live only in the OS credential store
//! (Windows Credential Manager), never in a file, a log or an error. Inbound chat arrives over
//! EventSub WebSocket (`channel.chat.message`); actions and lines go through Helix.
//!
//! - [`auth`]: device-code login, refresh, validate, revoke (`id.twitch.tv`).
//! - [`helix`]: the Helix calls (moderation actions, chat lines, user lookup) with one
//!   refresh-and-retry on 401 and a fail-fast on 429.
//! - [`eventsub`]: the EventSub WebSocket session and the `channel.chat.message` mapping.
//! - [`store`]: the credential store (keyring outside tests, in-memory in tests).

mod auth;
mod eventsub;
mod helix;
pub mod store;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use shadowrunr_types::moderation::{PlatformState, PlatformStatus, TwitchConnect};
use shadowrunr_types::Platform;
use tokio::task::JoinHandle;

use self::store::{Credential, CredentialStore};
use super::{ChatInbox, ChatPlatform, PlatformError};

/// The scopes the bot asks for: read and write chat as the bot, delete messages, ban and time
/// out (docs/guide/moderator.md).
pub const SCOPES: &str =
    "user:read:chat user:write:chat moderator:manage:banned_users moderator:manage:chat_messages";

/// Twitch caps one chat message at 500 characters (Helix "Send Chat Message").
pub const MAX_MESSAGE_CHARS: usize = 500;
/// Twitch caps a ban or timeout reason at 500 characters (Helix "Ban User").
pub const MAX_REASON_CHARS: usize = 500;
/// The longest timeout Twitch accepts: two weeks, in seconds (Helix "Ban User").
pub const MAX_TIMEOUT_S: u32 = 1_209_600;

/// `[twitch]`. The client id is public (it identifies the app, not a user); an empty
/// `client_id` keeps Twitch off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TwitchConfig {
    /// The Twitch developer application's client id (public client, device-code flow).
    pub client_id: String,
    /// The broadcaster's login: the channel the bot moderates.
    pub channel: String,
}

/// Where the adapter talks to, and how fast its clocks run. The default is Twitch itself;
/// tests point every base at a local mock and shrink the time unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// `https://id.twitch.tv` (OAuth: `/oauth2/device`, `/oauth2/token`, ...).
    pub id_base: String,
    /// `https://api.twitch.tv/helix`.
    pub helix_base: String,
    /// `wss://eventsub.wss.twitch.tv/ws`.
    pub eventsub_url: String,
    /// One "second" of the device-poll interval and the reconnect backoff.
    pub time_unit: Duration,
    /// How often a live session re-validates its token (Twitch requires hourly).
    pub validate_every: Duration,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            id_base: "https://id.twitch.tv".into(),
            helix_base: "https://api.twitch.tv/helix".into(),
            eventsub_url: "wss://eventsub.wss.twitch.tv/ws".into(),
            time_unit: Duration::from_secs(1),
            validate_every: Duration::from_secs(3600),
        }
    }
}

/// What `status()` reports, plus the ids the Helix calls need.
#[derive(Debug, Clone)]
struct Live {
    state: PlatformState,
    detail: Option<String>,
    bot_login: Option<String>,
    bot_id: Option<String>,
    broadcaster_id: Option<String>,
    /// A 429 told us to wait until this Unix second; calls fail fast until then.
    rate_limited_until: Option<i64>,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            state: PlatformState::Disconnected,
            detail: None,
            bot_login: None,
            bot_id: None,
            broadcaster_id: None,
            rate_limited_until: None,
        }
    }
}

/// The Twitch adapter.
pub struct TwitchPlatform {
    cfg: TwitchConfig,
    ep: Endpoints,
    http: reqwest::Client,
    store: Arc<dyn CredentialStore>,
    cred: Mutex<Option<Credential>>,
    live: Mutex<Live>,
    inbox: Mutex<Option<ChatInbox>>,
    session: Mutex<Option<JoinHandle<()>>>,
    login: Mutex<Option<JoinHandle<()>>>,
    /// Public-client refresh tokens are one-time use: only one refresh may be in flight.
    refresh_lock: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for TwitchPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwitchPlatform")
            .field("cfg", &self.cfg)
            .field("ep", &self.ep)
            .field("cred", &self.current_cred())
            .field("live", &self.live.lock().map(|l| l.clone()).ok())
            .finish_non_exhaustive()
    }
}

impl TwitchPlatform {
    /// Builds the adapter; nothing connects until [`TwitchPlatform::start`].
    pub fn new(cfg: TwitchConfig) -> Arc<Self> {
        #[cfg(not(test))]
        let store: Arc<dyn CredentialStore> = Arc::new(store::KeyringStore);
        #[cfg(test)]
        let store: Arc<dyn CredentialStore> = Arc::new(store::MemoryStore::new());
        Self::with_endpoints(cfg, Endpoints::default(), store)
    }

    /// Builds the adapter against other endpoints and another credential store (tests).
    pub fn with_endpoints(
        cfg: TwitchConfig,
        ep: Endpoints,
        store: Arc<dyn CredentialStore>,
    ) -> Arc<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Arc::new(Self {
            cfg,
            ep,
            http,
            store,
            cred: Mutex::new(None),
            live: Mutex::new(Live::default()),
            inbox: Mutex::new(None),
            session: Mutex::new(None),
            login: Mutex::new(None),
            refresh_lock: tokio::sync::Mutex::new(()),
        })
    }

    /// Connects with the stored credential, if there is one, and keeps the EventSub session up
    /// (reconnecting with backoff), pushing every chat message into `inbox`. Returns at once.
    pub fn start(self: &Arc<Self>, inbox: ChatInbox) {
        if let Ok(mut i) = self.inbox.lock() {
            *i = Some(inbox);
        }
        if self.cfg.client_id.is_empty() {
            return;
        }
        self.spawn_session();
    }

    /// Begins a device-code login; the daemon then polls for the token in the background and
    /// connects when the operator has entered the code.
    pub async fn begin_device_login(self: &Arc<Self>) -> Result<TwitchConnect, PlatformError> {
        if self.cfg.client_id.is_empty() {
            return Err(PlatformError("no [twitch] client_id configured".into()));
        }
        let device = self.request_device_code().await?;
        let connect = TwitchConnect {
            verification_uri: device.verification_uri.clone(),
            user_code: device.user_code.clone(),
            expires_s: device.expires_in,
        };
        self.set_live(
            PlatformState::Connecting,
            Some(format!(
                "waiting for the code at {}",
                device.verification_uri
            )),
        );
        let me = self.clone();
        if let Some(handle) = spawn(async move { me.poll_device_login(device).await }) {
            if let Ok(mut l) = self.login.lock() {
                if let Some(old) = l.replace(handle) {
                    old.abort();
                }
            }
        }
        Ok(connect)
    }

    /// Disconnects and deletes the stored credential.
    pub async fn disconnect(&self) {
        for slot in [&self.login, &self.session] {
            if let Ok(mut h) = slot.lock() {
                if let Some(h) = h.take() {
                    h.abort();
                }
            }
        }
        let cred = self.cred.lock().ok().and_then(|mut c| c.take());
        if let Some(cred) = cred {
            // Best effort: a revoked token is dead even if a copy survived somewhere.
            self.revoke(&cred.access_token).await;
        }
        if let Err(e) = self.store.delete() {
            eprintln!("twitch: {e}");
        }
        if let Ok(mut l) = self.live.lock() {
            *l = Live::default();
        }
    }

    // ---- shared helpers --------------------------------------------------------------------

    fn current_cred(&self) -> Option<Credential> {
        self.cred.lock().ok().and_then(|c| c.clone())
    }

    fn set_cred(&self, cred: Option<Credential>) {
        if let Ok(mut c) = self.cred.lock() {
            *c = cred;
        }
    }

    fn set_live(&self, state: PlatformState, detail: Option<String>) {
        if let Ok(mut l) = self.live.lock() {
            l.state = state;
            l.detail = detail;
        }
    }

    fn live(&self) -> Live {
        self.live.lock().map(|l| l.clone()).unwrap_or_default()
    }

    fn inbox(&self) -> Option<ChatInbox> {
        self.inbox.lock().ok().and_then(|i| i.clone())
    }

    /// Removes any token the adapter holds from `text`, so a server that echoes one back
    /// cannot carry it into an error string.
    fn scrub(&self, text: &str) -> String {
        let mut out = text.to_string();
        if let Some(c) = self.current_cred() {
            for s in [&c.access_token, &c.refresh_token] {
                if !s.is_empty() {
                    out = out.replace(s.expose(), "<redacted>");
                }
            }
        }
        out
    }

    /// (Re)starts the session task: validate, resolve the channel, keep EventSub up.
    fn spawn_session(self: &Arc<Self>) {
        let me = self.clone();
        if let Some(handle) = spawn(async move { me.run_session().await }) {
            if let Ok(mut s) = self.session.lock() {
                if let Some(old) = s.replace(handle) {
                    old.abort();
                }
            }
        }
    }
}

/// Spawns on the current tokio runtime, if there is one.
fn spawn<F>(fut: F) -> Option<JoinHandle<()>>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::runtime::Handle::try_current()
        .ok()
        .map(|h| h.spawn(fut))
}

fn now_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The first `max` characters of `text` (never splits a character).
fn truncate_chars(text: &str, max: usize) -> &str {
    match text.char_indices().nth(max) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

#[async_trait]
impl ChatPlatform for TwitchPlatform {
    fn platform(&self) -> Platform {
        Platform::Twitch
    }

    fn status(&self) -> PlatformStatus {
        let channel = (!self.cfg.channel.is_empty()).then(|| self.cfg.channel.clone());
        if self.cfg.client_id.is_empty() {
            return PlatformStatus {
                platform: Platform::Twitch,
                state: PlatformState::Disconnected,
                bot_login: None,
                channel,
                detail: Some("no [twitch] client_id configured".to_string()),
            };
        }
        let live = self.live();
        PlatformStatus {
            platform: Platform::Twitch,
            state: live.state,
            bot_login: live.bot_login,
            channel,
            detail: live.detail,
        }
    }

    async fn delete(&self, message_id: &str) -> Result<(), PlatformError> {
        self.helix_delete_message(message_id).await
    }

    async fn timeout(
        &self,
        viewer_id: &str,
        seconds: u32,
        reason: &str,
    ) -> Result<(), PlatformError> {
        self.helix_ban(viewer_id, Some(seconds), reason).await
    }

    async fn ban(&self, viewer_id: &str, reason: &str) -> Result<(), PlatformError> {
        self.helix_ban(viewer_id, None, reason).await
    }

    async fn unban(&self, viewer_id: &str) -> Result<(), PlatformError> {
        self.helix_unban(viewer_id).await
    }

    async fn say(&self, text: &str) -> Result<(), PlatformError> {
        self.helix_say(text).await
    }
}
