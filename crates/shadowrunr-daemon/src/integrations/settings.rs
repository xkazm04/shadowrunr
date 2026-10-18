//! `[personas]` in `shadowrunr.toml`. Secrets never live here: the activity webhook's HMAC
//! secret is named by its environment variable (`secret_env`), like `[engine] api_key_env`.
//!
//! ```toml
//! [personas]
//! mcp_allow_writes = false        # `start_sim` over MCP
//!
//! [personas.activity]
//! enabled = true
//! webhook_url = "http://127.0.0.1:9420"
//! trigger_id = "<webhook trigger id>"
//! secret_env = "SHADOWRUNR_PERSONAS_WEBHOOK_SECRET"
//! events = ["stream_end", "segment_end", "handover", "tournament_complete", "report_ready"]
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::config::Config;

/// The default Personas webhook server (`PERSONAS_WEBHOOK_PORT` moves it).
pub const DEFAULT_WEBHOOK_URL: &str = "http://127.0.0.1:9420";

/// Event kinds forwarded when `events` is not set.
pub const DEFAULT_EVENTS: [&str; 5] = [
    "stream_end",
    "segment_end",
    "handover",
    "tournament_complete",
    "report_ready",
];

/// Every kind `events` may name. `moderation` forwards every moderation event; the safety
/// ones (`priority:safety`, `character_break`, `persona_leak`) are forwarded regardless.
pub const KNOWN_EVENTS: [&str; 6] = [
    "stream_end",
    "segment_end",
    "handover",
    "tournament_complete",
    "report_ready",
    "moderation",
];

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PersonasSettings {
    /// Lets the MCP server's `start_sim` run. Off by default: MCP is read-only.
    pub mcp_allow_writes: bool,
    /// The handshake file (default: `$PERSONAS_DATA_DIR/local-http.json`, else
    /// `~/.personas/local-http.json`).
    pub handshake: Option<String>,
    pub activity: Option<ActivitySettings>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActivitySettings {
    pub enabled: bool,
    /// Base URL of the Personas webhook server; calls go to `<webhook_url>/webhook/<trigger_id>`.
    pub webhook_url: String,
    pub trigger_id: String,
    /// Name of the environment variable holding the trigger's HMAC secret.
    pub secret_env: String,
    pub events: Vec<String>,
    /// Events held while Personas is slow or down; more are dropped and counted.
    pub queue: usize,
    /// Delivery attempts per event (the first try included).
    pub max_attempts: u32,
    /// First retry delay; doubles per attempt up to `max_backoff_ms`.
    pub backoff_ms: u64,
    pub max_backoff_ms: u64,
    /// Per-request timeout.
    pub timeout_ms: u64,
    /// After an event exhausts its attempts, events are dropped (and counted) for this long
    /// before delivery is tried again.
    pub down_cooldown_ms: u64,
}

impl Default for ActivitySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            webhook_url: DEFAULT_WEBHOOK_URL.into(),
            trigger_id: String::new(),
            secret_env: String::new(),
            events: DEFAULT_EVENTS.iter().map(|s| s.to_string()).collect(),
            queue: 256,
            max_attempts: 4,
            backoff_ms: 250,
            max_backoff_ms: 5_000,
            timeout_ms: 3_000,
            down_cooldown_ms: 30_000,
        }
    }
}

impl ActivitySettings {
    /// `<webhook_url>/webhook/<trigger_id>`.
    pub fn endpoint(&self) -> String {
        format!(
            "{}/webhook/{}",
            self.webhook_url.trim_end_matches('/'),
            self.trigger_id.trim()
        )
    }

    /// Problems that keep the forwarder off. Names the variable, never its value.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let url = self.webhook_url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            out.push(format!("webhook_url {url:?} is not an http(s) URL"));
        }
        let t = self.trigger_id.trim();
        if t.is_empty() {
            out.push("trigger_id is empty".into());
        } else if !t
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            out.push(format!(
                "trigger_id {t:?} has characters outside [A-Za-z0-9_-]"
            ));
        }
        if self.secret_env.trim().is_empty() {
            out.push("secret_env is empty (name the variable that holds the secret)".into());
        }
        for e in &self.events {
            if !KNOWN_EVENTS.contains(&e.as_str()) {
                out.push(format!(
                    "unknown event {e:?} (known: {})",
                    KNOWN_EVENTS.join(", ")
                ));
            }
        }
        if self.queue == 0 || self.max_attempts == 0 {
            out.push("queue and max_attempts must be at least 1".into());
        }
        out
    }

    pub fn backoff(&self) -> Duration {
        Duration::from_millis(self.backoff_ms)
    }
    pub fn max_backoff(&self) -> Duration {
        Duration::from_millis(self.max_backoff_ms.max(self.backoff_ms))
    }
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.max(1))
    }
    pub fn down_cooldown(&self) -> Duration {
        Duration::from_millis(self.down_cooldown_ms)
    }
}

#[derive(Deserialize)]
struct Wrapper {
    #[serde(default)]
    personas: Option<PersonasSettings>,
}

impl PersonasSettings {
    /// Parse the `[personas]` section of a whole `shadowrunr.toml` (absent = defaults).
    /// The error carries the message only, never the offending source line, so a value
    /// pasted where it does not belong is not echoed.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        toml::from_str::<Wrapper>(text)
            .map(|w| w.personas.unwrap_or_default())
            .map_err(|e| format!("[personas]: {}", e.message()))
    }

    /// The section of the config file `cfg` was loaded from; defaults without a file.
    pub fn load(cfg: &Config) -> Result<Self, String> {
        let Some(path) = &cfg.source else {
            return Ok(Self::default());
        };
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_toml(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Activity settings that are present and enabled.
    pub fn activity_on(&self) -> Option<&ActivitySettings> {
        self.activity.as_ref().filter(|a| a.enabled)
    }

    /// The handshake file to read, resolved against the config root when relative.
    pub fn handshake_path(&self, root: &Path) -> Option<PathBuf> {
        match &self.handshake {
            Some(p) => {
                if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
                    return home_dir().map(|h| h.join(rest));
                }
                let p = PathBuf::from(p);
                Some(if p.is_absolute() { p } else { root.join(p) })
            }
            None => crate::integrations::door::default_handshake_path(
                std::env::var("PERSONAS_DATA_DIR").ok().as_deref(),
                home_dir(),
            ),
        }
    }
}

/// `%USERPROFILE%` on Windows, `$HOME` elsewhere (either is tried).
pub fn home_dir() -> Option<PathBuf> {
    ["USERPROFILE", "HOME"]
        .iter()
        .filter_map(std::env::var_os)
        .find(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_section_is_off() {
        let s = PersonasSettings::from_toml("[daemon]\nport = 1\n").unwrap();
        assert!(!s.mcp_allow_writes);
        assert!(s.activity_on().is_none());
    }

    #[test]
    fn activity_defaults_and_problems() {
        let s = PersonasSettings::from_toml(
            "[personas]\nmcp_allow_writes = true\n[personas.activity]\ntrigger_id = \"t-1\"\nsecret_env = \"X_SECRET\"\n",
        )
        .unwrap();
        assert!(s.mcp_allow_writes);
        let a = s.activity_on().unwrap();
        assert_eq!(a.endpoint(), "http://127.0.0.1:9420/webhook/t-1");
        assert_eq!(a.events, DEFAULT_EVENTS);
        assert!(a.problems().is_empty(), "{:?}", a.problems());

        let bad = ActivitySettings {
            trigger_id: "../x".into(),
            events: vec!["chat".into()],
            ..ActivitySettings::default()
        };
        let p = bad.problems().join("; ");
        assert!(p.contains("trigger_id") && p.contains("secret_env") && p.contains("chat"));
    }

    #[test]
    fn a_secret_value_key_is_refused() {
        // Only the variable's name may appear; an inline `secret` is an unknown field.
        let e = PersonasSettings::from_toml(
            "[personas.activity]\ntrigger_id = \"t\"\nsecret = \"hunter2\"\n",
        )
        .unwrap_err();
        assert!(e.contains("unknown field"), "{e}");
        assert!(!e.contains("hunter2"), "the value must not be echoed: {e}");
    }

    #[test]
    fn the_example_config_section_parses() {
        let text = include_str!("../../../../shadowrunr.example.toml");
        let s = PersonasSettings::from_toml(text).unwrap();
        assert!(!s.mcp_allow_writes);
        assert!(s.activity_on().is_none(), "the example ships activity off");
    }
}
