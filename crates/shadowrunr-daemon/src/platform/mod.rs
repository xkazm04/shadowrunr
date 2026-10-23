//! Chat platforms the moderator guards (docs/spec/contract.md, "Moderation").
//!
//! A platform adapter does two things: it pushes every inbound chat message, as a
//! [`ChatMsg`] with verified platform metadata, into the inbox it was started with; and it
//! carries out moderation actions and posts the moderator's lines through [`ChatPlatform`].
//! The moderation lane sees nothing else of it. Twitch is the one adapter (`twitch`); Kick
//! will be a second implementation of the same trait.

pub mod twitch;

use std::sync::Mutex;

use async_trait::async_trait;
use shadowrunr_types::moderation::{PlatformState, PlatformStatus};
use shadowrunr_types::{ChatMsg, Platform};
use tokio::sync::mpsc;

/// A platform call failed. The text is safe to log: it never carries a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformError(pub String);

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PlatformError {}

/// Where an adapter pushes inbound chat.
pub type ChatInbox = mpsc::Sender<ChatMsg>;

/// The moderation actions and the moderator's voice on one platform. Viewer and message ids
/// are the platform's own (`Viewer::id`, `ChatMsg::id`).
#[async_trait]
pub trait ChatPlatform: Send + Sync {
    fn platform(&self) -> Platform;
    fn status(&self) -> PlatformStatus;
    async fn delete(&self, message_id: &str) -> Result<(), PlatformError>;
    async fn timeout(
        &self,
        viewer_id: &str,
        seconds: u32,
        reason: &str,
    ) -> Result<(), PlatformError>;
    async fn ban(&self, viewer_id: &str, reason: &str) -> Result<(), PlatformError>;
    async fn unban(&self, viewer_id: &str) -> Result<(), PlatformError>;
    /// Post one line in chat as the bot account.
    async fn say(&self, text: &str) -> Result<(), PlatformError>;
}

/// One call a [`FakePlatform`] received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeCall {
    Delete(String),
    Timeout(String, u32),
    Ban(String),
    Unban(String),
    Say(String),
}

/// A platform for tests: records every call, and fails them all while `fail` is set.
#[derive(Debug, Default)]
pub struct FakePlatform {
    pub calls: Mutex<Vec<FakeCall>>,
    pub fail: Mutex<Option<String>>,
}

impl FakePlatform {
    pub fn calls(&self) -> Vec<FakeCall> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }

    fn record(&self, call: FakeCall) -> Result<(), PlatformError> {
        if let Ok(f) = self.fail.lock() {
            if let Some(e) = f.as_ref() {
                return Err(PlatformError(e.clone()));
            }
        }
        if let Ok(mut c) = self.calls.lock() {
            c.push(call);
        }
        Ok(())
    }
}

#[async_trait]
impl ChatPlatform for FakePlatform {
    fn platform(&self) -> Platform {
        Platform::Twitch
    }
    fn status(&self) -> PlatformStatus {
        PlatformStatus {
            platform: Platform::Twitch,
            state: PlatformState::Connected,
            bot_login: Some("fake-bot".into()),
            channel: Some("fake-channel".into()),
            detail: None,
        }
    }
    async fn delete(&self, message_id: &str) -> Result<(), PlatformError> {
        self.record(FakeCall::Delete(message_id.into()))
    }
    async fn timeout(
        &self,
        viewer_id: &str,
        seconds: u32,
        _reason: &str,
    ) -> Result<(), PlatformError> {
        self.record(FakeCall::Timeout(viewer_id.into(), seconds))
    }
    async fn ban(&self, viewer_id: &str, _reason: &str) -> Result<(), PlatformError> {
        self.record(FakeCall::Ban(viewer_id.into()))
    }
    async fn unban(&self, viewer_id: &str) -> Result<(), PlatformError> {
        self.record(FakeCall::Unban(viewer_id.into()))
    }
    async fn say(&self, text: &str) -> Result<(), PlatformError> {
        self.record(FakeCall::Say(text.into()))
    }
}
