//! Where the bot's tokens live: the OS credential store (Windows Credential Manager), behind
//! [`CredentialStore`] so tests swap in [`MemoryStore`] and never touch the real one.
//!
//! One entry: service [`SERVICE`], account [`ACCOUNT`]. The value is the JSON
//! `{access_token, refresh_token, login, user_id}`. Nothing in this module prints a token:
//! [`Secret`] and [`Credential`] redact themselves in `Debug`, and store errors name the entry,
//! never its value.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The credential store service name.
pub const SERVICE: &str = "shadowrunr-twitch";
/// The credential store account. Fixed (not the bot's login) so the daemon can find the entry
/// on start before it knows who the bot is; the login is inside the value.
pub const ACCOUNT: &str = "bot";

/// A token. Its `Debug` never shows the value, so it cannot reach a log by accident.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw value, for the one place that needs it: a request header or form field.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// The bot account's login: what the credential store holds.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    pub access_token: Secret,
    pub refresh_token: Secret,
    /// The bot's login (lowercase), from `/oauth2/validate`; empty until validated.
    #[serde(default)]
    pub login: String,
    /// The bot's user id, from `/oauth2/validate`; empty until validated.
    #[serde(default)]
    pub user_id: String,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("access_token", &self.access_token)
            .field("refresh_token", &self.refresh_token)
            .field("login", &self.login)
            .field("user_id", &self.user_id)
            .finish()
    }
}

/// The bot's credential, persisted somewhere safe. Errors are safe to log.
pub trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<Credential>, String>;
    fn save(&self, cred: &Credential) -> Result<(), String>;
    /// Deletes the entry; deleting a missing entry is not an error.
    fn delete(&self) -> Result<(), String>;
}

/// Windows Credential Manager: service `shadowrunr-twitch`, account `bot`, shown as the generic
/// credential target `bot.shadowrunr-twitch`.
pub struct KeyringStore;

impl KeyringStore {
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, ACCOUNT)
            .map_err(|e| format!("credential store ({SERVICE}/{ACCOUNT}): {e}"))
    }
}

impl CredentialStore for KeyringStore {
    fn load(&self) -> Result<Option<Credential>, String> {
        match self.entry()?.get_password() {
            Ok(json) => serde_json::from_str(&json).map(Some).map_err(|_| {
                format!("credential store ({SERVICE}/{ACCOUNT}): the entry is not readable")
            }),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("credential store ({SERVICE}/{ACCOUNT}): {e}")),
        }
    }

    fn save(&self, cred: &Credential) -> Result<(), String> {
        let json = serde_json::to_string(cred)
            .map_err(|_| "credential store: could not encode the credential".to_string())?;
        self.entry()?
            .set_password(&json)
            .map_err(|e| format!("credential store ({SERVICE}/{ACCOUNT}): {e}"))
    }

    fn delete(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("credential store ({SERVICE}/{ACCOUNT}): {e}")),
        }
    }
}

/// An in-memory store, for tests.
#[derive(Default)]
pub struct MemoryStore {
    entries: Mutex<HashMap<(String, String), String>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn key() -> (String, String) {
        (SERVICE.to_string(), ACCOUNT.to_string())
    }

    /// The raw stored JSON, if any (tests read it to check what was persisted).
    pub fn raw(&self) -> Option<String> {
        self.entries.lock().ok()?.get(&Self::key()).cloned()
    }
}

impl CredentialStore for MemoryStore {
    fn load(&self) -> Result<Option<Credential>, String> {
        match self.raw() {
            Some(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|_| "memory store: unreadable entry".to_string()),
            None => Ok(None),
        }
    }

    fn save(&self, cred: &Credential) -> Result<(), String> {
        let json = serde_json::to_string(cred).map_err(|_| "memory store: encode".to_string())?;
        let mut e = self
            .entries
            .lock()
            .map_err(|_| "memory store: poisoned".to_string())?;
        e.insert(Self::key(), json);
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        let mut e = self
            .entries
            .lock()
            .map_err(|_| "memory store: poisoned".to_string())?;
        e.remove(&Self::key());
        Ok(())
    }
}
