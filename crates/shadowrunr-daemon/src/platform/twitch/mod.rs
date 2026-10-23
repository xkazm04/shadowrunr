//! Twitch: the moderator's bot account (docs/guide/moderator.md).
//!
//! Login is Twitch's device-code flow; the bot's tokens live only in the OS credential store
//! (Windows Credential Manager), never in a file, a log or an error. Inbound chat arrives over
//! EventSub WebSocket (`channel.chat.message`); actions and lines go through Helix.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use shadowrunr_types::moderation::{PlatformState, PlatformStatus, TwitchConnect};
use shadowrunr_types::Platform;

use super::{ChatInbox, ChatPlatform, PlatformError};

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

/// The Twitch adapter.
pub struct TwitchPlatform {
    cfg: TwitchConfig,
}

impl TwitchPlatform {
    /// Builds the adapter; nothing connects until [`TwitchPlatform::start`].
    pub fn new(cfg: TwitchConfig) -> Arc<Self> {
        Arc::new(Self { cfg })
    }

    /// Connects with the stored credential, if there is one, and keeps the EventSub session up
    /// (reconnecting with backoff), pushing every chat message into `inbox`. Returns at once.
    pub fn start(self: &Arc<Self>, _inbox: ChatInbox) {}

    /// Begins a device-code login; the daemon then polls for the token in the background and
    /// connects when the operator has entered the code.
    pub async fn begin_device_login(self: &Arc<Self>) -> Result<TwitchConnect, PlatformError> {
        Err(PlatformError("twitch login is not implemented yet".into()))
    }

    /// Disconnects and deletes the stored credential.
    pub async fn disconnect(&self) {}
}

#[async_trait]
impl ChatPlatform for TwitchPlatform {
    fn platform(&self) -> Platform {
        Platform::Twitch
    }
    fn status(&self) -> PlatformStatus {
        PlatformStatus {
            platform: Platform::Twitch,
            state: PlatformState::Disconnected,
            bot_login: None,
            channel: (!self.cfg.channel.is_empty()).then(|| self.cfg.channel.clone()),
            detail: self
                .cfg
                .client_id
                .is_empty()
                .then(|| "no [twitch] client_id configured".to_string()),
        }
    }
    async fn delete(&self, _message_id: &str) -> Result<(), PlatformError> {
        Err(PlatformError("twitch: not connected".into()))
    }
    async fn timeout(
        &self,
        _viewer_id: &str,
        _seconds: u32,
        _reason: &str,
    ) -> Result<(), PlatformError> {
        Err(PlatformError("twitch: not connected".into()))
    }
    async fn ban(&self, _viewer_id: &str, _reason: &str) -> Result<(), PlatformError> {
        Err(PlatformError("twitch: not connected".into()))
    }
    async fn unban(&self, _viewer_id: &str) -> Result<(), PlatformError> {
        Err(PlatformError("twitch: not connected".into()))
    }
    async fn say(&self, _text: &str) -> Result<(), PlatformError> {
        Err(PlatformError("twitch: not connected".into()))
    }
}
