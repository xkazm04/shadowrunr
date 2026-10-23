//! `<data>/moderator/`: `rules.json` (the operator list, hot-applied on write; seeded once
//! from the Twitch hard rules of the policy file), `state.json` (the mode) and
//! `decisions.jsonl` (append-only [`shadowrunr_types::moderation::ModLogEntry`] lines).

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use shadowrunr_types::moderation::{
    ModActionKind, ModLogEntry, ModMode, ModRule, ModRuleKind, ModRules,
};

/// The seeded rules' timeout (the policy file's `block` becomes a ten-minute timeout).
pub const SEED_TIMEOUT_S: u32 = 600;
/// The platform whose policy rules seed the list.
const SEED_PLATFORM: &str = "twitch";
/// Marks an operator-installed placeholder pattern in the policy file: not a usable regex.
const PLACEHOLDER_MARK: &str = shadowrunr_brain::moderation::PLACEHOLDER_MARK;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ModState {
    #[serde(default)]
    mode: ModMode,
}

/// The moderator's files under one directory.
pub struct ModStore {
    dir: PathBuf,
    policy_file: PathBuf,
    /// Serialises appends to `decisions.jsonl` (one whole line per write).
    log_lock: Mutex<()>,
}

/// Write `bytes` to a temp file beside `path`, then rename it over `path`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Debug, Deserialize)]
struct PolicyFile {
    #[serde(default)]
    term_rules: Vec<PolicyTermRule>,
}

#[derive(Debug, Deserialize)]
struct PolicyTermRule {
    id: String,
    #[serde(default)]
    pattern: String,
    #[serde(default)]
    platforms: Vec<String>,
    #[serde(default)]
    action: String,
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The seed list from a `platform-rules.json`: its `twitch` term rules (or rules for every
/// platform) that block and carry a usable pattern, as enabled ten-minute timeouts. A missing
/// or unreadable file seeds nothing.
pub fn seed_rules(policy_file: &Path) -> Vec<ModRule> {
    let Ok(text) = std::fs::read_to_string(policy_file) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<PolicyFile>(&text) else {
        eprintln!(
            "moderator: {} does not parse; seeding no rules",
            policy_file.display()
        );
        return Vec::new();
    };
    file.term_rules
        .into_iter()
        .filter(|r| r.action == "block")
        .filter(|r| r.platforms.is_empty() || r.platforms.iter().any(|p| p == SEED_PLATFORM))
        .filter(|r| !r.pattern.trim().is_empty() && !r.pattern.contains(PLACEHOLDER_MARK))
        .filter(|r| safe_id(&r.id))
        .filter(|r| super::rules::compile(&r.pattern).is_ok())
        .map(|r| ModRule {
            name: r.id.replace('-', " "),
            id: r.id,
            kind: ModRuleKind::Pattern,
            pattern: Some(r.pattern),
            text: None,
            action: ModActionKind::Timeout,
            timeout_s: Some(SEED_TIMEOUT_S),
            strikes: None,
            enabled: true,
        })
        .collect()
}

impl ModStore {
    /// `<data>/moderator`, seeding from `<policy>/platform-rules.json`. Touches nothing on disk
    /// until a read or write.
    pub fn new(data: &Path, policy: &Path) -> Self {
        Self {
            dir: data.join("moderator"),
            policy_file: policy.join("platform-rules.json"),
            log_lock: Mutex::new(()),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn rules_path(&self) -> PathBuf {
        self.dir.join("rules.json")
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    pub fn log_path(&self) -> PathBuf {
        self.dir.join("decisions.jsonl")
    }

    /// The stored rule list. Absent: seeded once from the policy file and written.
    pub fn load_rules(&self) -> Result<Vec<ModRule>, String> {
        let path = self.rules_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<ModRules>(&text)
                .map(|r| r.rules)
                .map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let rules = seed_rules(&self.policy_file);
                self.save_rules(&rules)?;
                Ok(rules)
            }
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn save_rules(&self, rules: &[ModRule]) -> Result<(), String> {
        let body = serde_json::to_vec_pretty(&ModRules {
            rules: rules.to_vec(),
        })
        .map_err(|e| format!("rules: {e}"))?;
        write_atomic(&self.rules_path(), &body)
    }

    /// The stored mode; shadow when absent or unreadable.
    pub fn load_mode(&self) -> ModMode {
        std::fs::read_to_string(self.state_path())
            .ok()
            .and_then(|t| serde_json::from_str::<ModState>(&t).ok())
            .map(|s| s.mode)
            .unwrap_or_default()
    }

    pub fn save_mode(&self, mode: ModMode) -> Result<(), String> {
        let body =
            serde_json::to_vec_pretty(&ModState { mode }).map_err(|e| format!("state: {e}"))?;
        write_atomic(&self.state_path(), &body)
    }

    /// Append one entry as one line.
    pub fn append(&self, entry: &ModLogEntry) -> Result<(), String> {
        let mut line = serde_json::to_string(entry).map_err(|e| format!("log entry: {e}"))?;
        line.push('\n');
        let _g = self.log_lock.lock().unwrap_or_else(|p| p.into_inner());
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        let path = self.log_path();
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        f.write_all(line.as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Up to `limit` entries, newest first. Reads the file backwards in chunks, so a long log
    /// costs only its tail. Lines that do not parse are skipped.
    pub fn read_log(&self, limit: usize) -> Result<Vec<ModLogEntry>, String> {
        let path = self.log_path();
        let mut f = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let io = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut pos = f.seek(SeekFrom::End(0)).map_err(io)?;
        const CHUNK: u64 = 64 * 1024;
        let mut out = Vec::new();
        // Bytes after the last newline seen so far (the head of a line split by a chunk).
        let mut carry: Vec<u8> = Vec::new();
        while pos > 0 && out.len() < limit {
            let start = pos.saturating_sub(CHUNK);
            let mut buf = vec![0u8; (pos - start) as usize];
            f.seek(SeekFrom::Start(start)).map_err(io)?;
            f.read_exact(&mut buf).map_err(io)?;
            buf.extend_from_slice(&carry);
            pos = start;
            let mut lines: Vec<&[u8]> = buf.split(|b| *b == b'\n').collect();
            // The first piece may be cut mid-line unless we reached the start of the file.
            carry = if pos > 0 {
                lines.remove(0).to_vec()
            } else {
                Vec::new()
            };
            for l in lines.into_iter().rev() {
                if out.len() >= limit {
                    break;
                }
                if let Some(e) = parse_line(l) {
                    out.push(e);
                }
            }
        }
        Ok(out)
    }
}

fn parse_line(l: &[u8]) -> Option<ModLogEntry> {
    let s = std::str::from_utf8(l).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    serde_json::from_str(s).ok()
}
