//! In-character stalling while the engine fleet fails. A beat whose core call failed with a
//! fleet-class error has no line from the model; once `BrainTuning.stall_every_s` passed
//! since Malakor last spoke, the brain speaks a model-free stall line from the character
//! package (`stalls.toml`) instead of leaving the throne room silent. The same file carries
//! the fleet-loss sign-off.
//!
//! A live host also covers a SLOW call: when a core call is still running once Malakor has
//! been quiet `stall_every_s`, a stall line goes out at once on the brain's cover channel and
//! the model's reply follows when it lands ([`cover_call`]; record `engine_stall` /
//! `covered`). The 5-minute sandbox smoke run saw an 18.4 s successful call leave a 19.1 s
//! silence, past the 15 s ceiling: failure-only stalling cannot see that case.
//!
//! Stall lines are not the model's words: they never enter the turn record, the repetition
//! guard, the working set or memory. Each one is a `say` followed by the output moderation
//! record `engine_stall` / `spoke`.

use std::collections::VecDeque;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use shadowrunr_types::{StreamEvent, TsMs};
use tokio::time::{sleep_until, Instant};

use crate::fleet::FleetMode;
use crate::util::fnv1a64;
use crate::BrainError;

/// The output moderation rule recording a stall line (action `spoke`, or `covered` for a
/// line that covered a slow call).
pub const STALL_RULE: &str = "engine_stall";

/// The `engine_stall` action of a line spoken while a slow core call was still running.
pub const COVERED: &str = "covered";

/// A failed beat's stall is due this many seconds before `stall_every_s`: one beat, since a
/// failed beat can only speak at a beat boundary.
pub const FAILED_BEAT_SLACK_S: u64 = 5;

/// At most this many cover lines over one core call (its timeout is 30 s).
pub const COVER_LINES: usize = 2;

/// A cover line never goes out sooner than this into a core call.
pub const MIN_COVER_MS: u64 = 4_000;

/// The cover for one core call: prepared lines (each the events of one stall line), when the
/// first goes out after the call starts, and the spacing of the next.
pub struct CoverPlan {
    pub tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    pub lines: Vec<Vec<StreamEvent>>,
    pub first_after: Duration,
    pub every: Duration,
}

/// Await `call`, sending a prepared cover line each time its deadline passes first. Returns
/// the call's result and, when any line went out, how many and when the last did (time since
/// the call started).
pub async fn cover_call<F, R>(call: F, plan: Option<CoverPlan>) -> (R, Option<(usize, Duration)>)
where
    F: Future<Output = R>,
{
    let Some(plan) = plan else {
        return (call.await, None);
    };
    tokio::pin!(call);
    let start = Instant::now();
    let mut next = start + plan.first_after;
    let mut spoken = 0usize;
    let mut last = Duration::ZERO;
    loop {
        tokio::select! {
            biased;
            r = &mut call => return (r, (spoken > 0).then_some((spoken, last))),
            _ = sleep_until(next), if spoken < plan.lines.len() => {
                for ev in &plan.lines[spoken] {
                    let _ = plan.tx.send(ev.clone());
                }
                spoken += 1;
                last = start.elapsed();
                next += plan.every;
            }
        }
    }
}

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

    fn plan(
        n: usize,
        first_s: u64,
    ) -> (CoverPlan, tokio::sync::mpsc::UnboundedReceiver<StreamEvent>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let line = |i: usize| {
            vec![StreamEvent::Moderation {
                ts_ms: 0,
                stage: shadowrunr_types::ModStage::Output,
                rule: STALL_RULE.into(),
                msg_id: None,
                action: format!("line{i}"),
            }]
        };
        let p = CoverPlan {
            tx,
            lines: (0..n).map(line).collect(),
            first_after: Duration::from_secs(first_s),
            every: Duration::from_secs(12),
        };
        (p, rx)
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_call_is_covered_at_its_deadline_and_again_every_interval() {
        let (p, mut rx) = plan(2, 7);
        let call = async {
            tokio::time::sleep(Duration::from_secs(25)).await;
            "reply"
        };
        let (r, covered) = cover_call(call, Some(p)).await;
        assert_eq!(r, "reply");
        // Lines at 7 s and 19 s into the call; the reply at 25 s.
        assert_eq!(covered, Some((2, Duration::from_secs(19))));
        assert!(rx.try_recv().is_ok() && rx.try_recv().is_ok() && rx.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn a_call_that_lands_before_the_deadline_is_never_covered() {
        let (p, mut rx) = plan(2, 7);
        let call = async {
            tokio::time::sleep(Duration::from_secs(6)).await;
            1
        };
        let (_, covered) = cover_call(call, Some(p)).await;
        assert_eq!(covered, None);
        assert!(rx.try_recv().is_err());
        // Without a plan (simulated time) the call is simply awaited.
        assert_eq!(cover_call(async { 2 }, None).await, (2, None));
    }
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
