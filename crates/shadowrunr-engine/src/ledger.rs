//! The call ledger: one JSONL row per engine call, success or failure. Never credentials:
//! accounts appear by label only, and error rows carry a kind, not raw CLI output.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use shadowrunr_types::{EngineKind, EngineReply, Role, Usage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRow {
    pub ts_ms: i64,
    pub role: Role,
    pub engine: EngineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_text_ms: Option<u64>,
    pub duration_ms: u64,
    #[serde(default)]
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Absent on success; otherwise `rate_limited`, `auth`, `transient`, `timeout`,
    /// `refused`, `parse`, `io` or `unavailable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    /// True when the call was a turn of a warm session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub warm: bool,
}

impl LedgerRow {
    pub fn from_reply(
        ts_ms: i64,
        role: Role,
        reply: &EngineReply,
        correlation_id: Option<String>,
    ) -> Self {
        Self {
            ts_ms,
            role,
            engine: reply.engine,
            account: reply.account.clone(),
            model: reply.model.clone(),
            first_text_ms: reply.first_text_ms,
            duration_ms: reply.duration_ms,
            usage: reply.usage.clone(),
            cost_usd: reply.cost_usd,
            fallback_reason: reply.fallback_reason.clone(),
            correlation_id,
            error_kind: None,
            warm: false,
        }
    }
}

/// Append-only JSONL writer. Writes are serialised by a mutex; a write failure is reported
/// on stderr once per call and never fails the engine call itself.
#[derive(Debug)]
pub struct Ledger {
    path: Option<PathBuf>,
    lock: Mutex<()>,
}

impl Ledger {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn disabled() -> Self {
        Self::new(None)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn append(&self, row: &LedgerRow) {
        let Some(path) = &self.path else { return };
        let Ok(line) = serde_json::to_string(row) else {
            return;
        };
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let res = (|| -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                if !dir.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir)?;
                }
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            writeln!(f, "{line}")
        })();
        if let Err(e) = res {
            eprintln!(
                "shadowrunr-engine: ledger write to {} failed: {e}",
                path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_append_and_omit_absent_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("ledger.jsonl");
        let l = Ledger::new(Some(path.clone()));
        let row = LedgerRow {
            ts_ms: 1,
            role: Role::CoreThread,
            engine: EngineKind::ClaudeCli,
            account: Some("main".into()),
            model: "claude-haiku-5-5".into(),
            first_text_ms: None,
            duration_ms: 5,
            usage: Usage::default(),
            cost_usd: None,
            fallback_reason: None,
            correlation_id: None,
            error_kind: Some("timeout".into()),
            warm: false,
        };
        l.append(&row);
        l.append(&row);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(!text.contains("null"));
        assert!(!text.contains("first_text_ms"));
        assert!(!text.contains("warm"));
        let back: LedgerRow = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(&row).unwrap()
        );
    }
}
