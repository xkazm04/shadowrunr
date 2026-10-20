//! In-character stalling while the engine fleet fails. A beat whose core call failed with a
//! fleet-class error has no line from the model; once `BrainTuning.stall_every_s` passed
//! since Malakor last spoke, the brain speaks a model-free stall line from the character
//! package (`stalls.toml`) instead of leaving the throne room silent. The same file carries
//! the fleet-loss sign-off.
//!
//! Stall lines are not the model's words: they never enter the turn record, the repetition
//! guard, the working set or memory. Each one is a `say` followed by the output moderation
//! record `engine_stall` / `spoke`.

use std::collections::VecDeque;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use shadowrunr_types::TsMs;

use crate::fleet::FleetMode;
use crate::util::fnv1a64;
use crate::BrainError;

/// The output moderation rule recording a stall line (action `spoke`).
pub const STALL_RULE: &str = "engine_stall";

/// Seconds since Malakor last spoke before a failed beat speaks a stall line.
pub const DEFAULT_STALL_EVERY_S: u64 = 12;

/// A stall line is not repeated within this many stall lines (fewer when the bank is
/// smaller: never within `len - 1`).
pub const STALL_REPEAT_WINDOW: usize = 10;

/// Stall lines when the package has no `stalls.toml`.
const DEFAULT_LINES: &[&str] = &[
    "The scrying glass has clouded over. A moment, minions, while I polish it.",
    "The bone-clerks are slow tonight. I am told this is a union matter.",
    "The Spire's runes are flickering again. Nobody touch anything.",
    "The ledger is jammed. Eleven centuries of accounts, and today it chooses to stick.",
    "Hold your places, minions. The Phylactery Orb refuses to be rushed.",
    "Do not mistake this pause for weakness. It is a dramatic pause. A long one.",
];

/// The fleet-loss sign-off when the package has no `stalls.toml`.
const DEFAULT_SIGN_OFF: &str = "The Lord of the Darkness recalls me to the crypt... Guard the \
    throne room, minions. I shall return.";

/// `stalls.toml`: model-free lines for an engine outage, and the fleet-loss sign-off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StallBank {
    pub lines: Vec<String>,
    pub sign_off: String,
}

impl Default for StallBank {
    fn default() -> Self {
        Self {
            lines: DEFAULT_LINES.iter().map(|s| s.to_string()).collect(),
            sign_off: DEFAULT_SIGN_OFF.to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct StallsToml {
    #[serde(default)]
    lines: Vec<String>,
    #[serde(default)]
    sign_off: Option<String>,
}

impl StallBank {
    /// `<dir>/stalls.toml`. A missing file, an empty `lines` or a blank `sign_off` falls back
    /// to the built-in defaults; a malformed file is an error.
    pub fn load(dir: &Path) -> Result<Self, BrainError> {
        let path = dir.join("stalls.toml");
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)
            .map_err(|e| BrainError::Package(format!("{}: {e}", path.display())))?;
        let raw: StallsToml =
            toml::from_str(&text).map_err(|e| BrainError::Package(format!("stalls.toml: {e}")))?;
        let defaults = Self::default();
        let lines: Vec<String> = raw
            .lines
            .iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        let sign_off = raw
            .sign_off
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Ok(Self {
            lines: if lines.is_empty() {
                defaults.lines
            } else {
                lines
            },
            sign_off: sign_off.unwrap_or(defaults.sign_off),
        })
    }
}

/// Whether a beat whose core call failed speaks a stall line: the fleet is degraded or down,
/// and Malakor has not spoken for `every_s` seconds (or at all).
pub fn stall_due(mode: FleetMode, last_say_ms: Option<TsMs>, now_ms: TsMs, every_s: u64) -> bool {
    if mode == FleetMode::Healthy {
        return false;
    }
    let every_ms = i64::try_from(every_s.saturating_mul(1_000)).unwrap_or(i64::MAX);
    last_say_ms.is_none_or(|t| now_ms.saturating_sub(t) >= every_ms)
}

/// Picks stall lines: varied (seeded by the brain's salt) and never one of the last
/// [`STALL_REPEAT_WINDOW`] picks.
#[derive(Debug, Clone, Default)]
pub struct Staller {
    recent: VecDeque<usize>,
    picks: u64,
}

impl Staller {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next stall line from `bank`, or `None` when it has no lines.
    pub fn next<'a>(&mut self, bank: &'a StallBank, salt: u64) -> Option<&'a str> {
        let n = bank.lines.len();
        if n == 0 {
            return None;
        }
        let window = STALL_REPEAT_WINDOW.min(n - 1);
        while self.recent.len() > window {
            self.recent.pop_front();
        }
        let free: Vec<usize> = (0..n).filter(|i| !self.recent.contains(i)).collect();
        let h = fnv1a64(format!("{salt}:stall:{}", self.picks).as_bytes());
        let i = free[(h % free.len() as u64) as usize];
        self.picks += 1;
        if window > 0 {
            self.recent.push_back(i);
            while self.recent.len() > window {
                self.recent.pop_front();
            }
        }
        Some(&bank.lines[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moderation::{gate_text, Policy};

    fn malakor_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("characters")
            .join("malakor")
    }

    #[test]
    fn malakor_ships_at_least_twenty_short_stall_lines_and_a_sign_off() {
        let bank = StallBank::load(&malakor_dir()).unwrap();
        assert_ne!(bank, StallBank::default(), "stalls.toml is read");
        assert!(bank.lines.len() >= 20, "{}", bank.lines.len());
        for l in &bank.lines {
            assert!(l.split_whitespace().count() < 25, "{l}");
        }
        let distinct: std::collections::HashSet<&String> = bank.lines.iter().collect();
        assert_eq!(distinct.len(), bank.lines.len(), "no duplicate lines");
        assert!(bank.sign_off.contains("return"), "{}", bank.sign_off);
    }

    #[test]
    fn every_stall_line_and_sign_off_passes_the_output_gate_unchanged() {
        let policy = Policy::builtin();
        let bank = StallBank::load(&malakor_dir()).unwrap();
        let defaults = StallBank::default();
        for l in bank
            .lines
            .iter()
            .chain(defaults.lines.iter())
            .chain([&bank.sign_off, &defaults.sign_off])
        {
            let (out, fired) = gate_text(l, &policy, &[]);
            assert!(fired.is_empty(), "{l}: {fired:?}");
            assert_eq!(&out, l);
            let lower = l.to_lowercase();
            for word in [
                "outage", "server", "api", "model", "engine", "human", "offline",
            ] {
                assert!(
                    !lower
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|w| w == word),
                    "{l} says {word:?}"
                );
            }
        }
    }

    #[test]
    fn a_missing_stalls_file_uses_the_defaults() {
        let d = tempfile::tempdir().unwrap();
        let bank = StallBank::load(d.path()).unwrap();
        assert_eq!(bank, StallBank::default());
        assert!(bank.lines.len() >= 6);
        assert!(!bank.sign_off.is_empty());
    }

    #[test]
    fn an_empty_or_partial_file_falls_back_per_key_and_a_malformed_one_fails() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("stalls.toml"), "sign_off = \"Back soon.\"\n").unwrap();
        let bank = StallBank::load(d.path()).unwrap();
        assert_eq!(bank.lines, StallBank::default().lines);
        assert_eq!(bank.sign_off, "Back soon.");
        fs::write(d.path().join("stalls.toml"), "lines = [\"A.\", \" \"]\n").unwrap();
        let bank = StallBank::load(d.path()).unwrap();
        assert_eq!(bank.lines, vec!["A.".to_string()]);
        assert_eq!(bank.sign_off, StallBank::default().sign_off);
        fs::write(d.path().join("stalls.toml"), "lines = 3\n").unwrap();
        assert!(StallBank::load(d.path()).is_err());
    }

    #[test]
    fn no_stall_line_repeats_within_ten() {
        let bank = StallBank::load(&malakor_dir()).unwrap();
        for salt in [0u64, 1, 42, u64::MAX] {
            let mut s = Staller::new();
            let picks: Vec<&str> = (0..200).map(|_| s.next(&bank, salt).unwrap()).collect();
            for (i, p) in picks.iter().enumerate() {
                let from = i.saturating_sub(STALL_REPEAT_WINDOW);
                assert!(
                    !picks[from..i].contains(p),
                    "salt {salt}: pick {i} repeats within {STALL_REPEAT_WINDOW}: {p}"
                );
            }
            let used: std::collections::HashSet<&&str> = picks.iter().collect();
            assert_eq!(used.len(), bank.lines.len(), "every line gets its turn");
        }
        // The six defaults: never within five.
        let bank = StallBank::default();
        let mut s = Staller::new();
        let picks: Vec<&str> = (0..60).map(|_| s.next(&bank, 7).unwrap()).collect();
        for (i, p) in picks.iter().enumerate() {
            assert!(!picks[i.saturating_sub(5)..i].contains(p), "{i}");
        }
    }

    #[test]
    fn a_stall_is_due_only_in_a_failing_fleet_and_after_the_quiet_interval() {
        assert!(!stall_due(FleetMode::Healthy, None, 0, 12));
        assert!(stall_due(FleetMode::Down, None, 0, 12));
        assert!(!stall_due(FleetMode::Down, Some(10_000), 21_999, 12));
        assert!(stall_due(FleetMode::Degraded, Some(10_000), 22_000, 12));
    }
}
