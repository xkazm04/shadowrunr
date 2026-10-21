//! Pacing the beats to his own speech. The stage plays one voice in real time, so a beat that
//! writes more than can be spoken before the next is lost: sbx-20261020-221153-5m wrote about
//! 630 s of speech into a 248 s show (91 lines at a measured 18.3 characters a second).
//!
//! - **The speech clock.** Every line the brain puts on air (his lines, foil lines, stall and
//!   cover lines, the sign-off) advances `speaking_until`:
//!   `max(now, speaking_until) + chars / speech_cps * 1000 + line_pause_ms`. Silence is
//!   measured from where it ends: "nobody is speaking", not "no event".
//! - **The hold.** While more than `speech_lead_ms` of speech is still queued, a beat is a
//!   `hold` (reason `speaking: N.N s queued`) and makes no core call. A safety beat always runs
//!   at once, and so does a goodbye beat (the state carries a [`CLOSING_NOW_PREFIX`] line): the
//!   sign-off is never held past the session end. A goodbye beat's budget is cut to
//!   [`closing_budget`] instead, so what it adds behind the queue is short. The wrap-up window
//!   before it (a plain [`CLOSING_PREFIX`] line: tie things off, no goodbye yet) is paced like
//!   any other beat, so a minute of wrap-up never piles up speech the stream end would cut.
//! - **The budget.** The compose prompt asks for at most two sentences and `budget_chars`
//!   characters a beat; the gate keeps the beat's spoken lines within it by dropping whole
//!   lines, last first, never the first one and never part of one ([`over_budget`]; output
//!   record [`PACE_RULE`] / [`TRIMMED`] per line dropped).
//!
//! The defaults are `speech_cps` 18 (measured: Kokoro `af_heart` speaks about 18.3) and three
//! assumptions to tune against voiced sandbox runs: `line_pause_ms` 250, `speech_lead_ms` 1500
//! and `budget_chars` 260 (about 14 s of speech, inside the 15 s say-gap ceiling).

use shadowrunr_types::TsMs;

/// The output moderation rule that records a spoken line dropped to keep a beat in budget.
pub const PACE_RULE: &str = "pace_budget";
/// Its action.
pub const TRIMMED: &str = "trimmed";
/// A state-summary line starting with this is the closing window: wrap up (a plain `CLOSING:`
/// line, no goodbye yet) or say goodbye and sign off ([`CLOSING_NOW_PREFIX`]).
pub const CLOSING_PREFIX: &str = "CLOSING";
/// A state-summary line starting with this asks for the goodbye and the sign-off now.
pub const CLOSING_NOW_PREFIX: &str = "CLOSING NOW";

/// The speech clock's and the per-beat budget's knobs ([`crate::BrainTuning::pace`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PaceConfig {
    /// Characters spoken per second (measured: 18.3 for Kokoro `af_heart`).
    pub speech_cps: f64,
    /// The pause after each line (assumption).
    pub line_pause_ms: i64,
    /// A beat holds while more than this much speech is still queued (assumption).
    pub speech_lead_ms: i64,
    /// Characters of spoken lines a beat may add (assumption: about 14 s of speech).
    pub budget_chars: usize,
}

impl Default for PaceConfig {
    fn default() -> Self {
        Self {
            speech_cps: 18.0,
            line_pause_ms: 250,
            speech_lead_ms: 1_500,
            budget_chars: 260,
        }
    }
}

impl PaceConfig {
    /// How long a line takes on air: its characters at `speech_cps`, plus the line pause.
    pub fn speech_ms(&self, text: &str) -> i64 {
        let chars = text.chars().count() as f64;
        let cps = if self.speech_cps > 0.0 {
            self.speech_cps
        } else {
            PaceConfig::default().speech_cps
        };
        (chars / cps * 1000.0).round() as i64 + self.line_pause_ms.max(0)
    }
}

/// When the speech already put on air ends.
#[derive(Debug, Clone, Default)]
pub struct SpeechClock {
    until: Option<TsMs>,
}

impl SpeechClock {
    /// A line goes on air at `at`: it starts when the speech before it ends, and the clock moves
    /// to where it ends.
    pub fn say(&mut self, at: TsMs, text: &str, cfg: &PaceConfig) {
        let start = self.until.map_or(at, |u| u.max(at));
        self.until = Some(start.saturating_add(cfg.speech_ms(text)));
    }

    /// When the last speech ends (or ended); `None` before the first line.
    pub fn until(&self) -> Option<TsMs> {
        self.until
    }

    /// When nobody is speaking any more: `now`, or later while speech is queued.
    pub fn free_at(&self, now: TsMs) -> TsMs {
        self.until.map_or(now, |u| u.max(now))
    }

    /// Speech still queued at `now`.
    pub fn queued_ms(&self, now: TsMs) -> i64 {
        self.free_at(now).saturating_sub(now)
    }
}

/// Whether the state summary carries the closing window (wrap-up or goodbye).
pub fn is_closing(state_summary: &str) -> bool {
    state_summary
        .lines()
        .any(|l| l.trim_start().starts_with(CLOSING_PREFIX))
}

/// Whether the state summary asks for the goodbye now: never held, at [`closing_budget`].
pub fn is_closing_now(state_summary: &str) -> bool {
    state_summary
        .lines()
        .any(|l| l.trim_start().starts_with(CLOSING_NOW_PREFIX))
}

/// A goodbye beat's budget: half the beat budget, so the goodbye queues little.
pub fn closing_budget(budget_chars: usize) -> usize {
    budget_chars / 2
}

/// Which of a beat's spoken lines (their lengths in characters, in order) go past `budget`:
/// whole lines, the last first, until the rest fits. The first line is always kept, so a beat
/// that answered something always says something. Returns the indices dropped.
pub fn over_budget(lens: &[usize], budget: usize) -> Vec<usize> {
    let mut total: usize = lens.iter().sum();
    let mut dropped = Vec::new();
    let mut i = lens.len();
    while total > budget && i > 1 {
        i -= 1;
        total -= lens[i];
        dropped.push(i);
    }
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_advances_by_text_length_and_queues_lines_back_to_back() {
        let cfg = PaceConfig::default();
        // 180 characters at 18 a second: 10 s, plus the 250 ms pause.
        let line = "x".repeat(180);
        assert_eq!(cfg.speech_ms(&line), 10_250);
        let mut c = SpeechClock::default();
        assert_eq!(c.queued_ms(0), 0);
        c.say(1_000, &line, &cfg);
        assert_eq!(c.until(), Some(11_250));
        // A second line in the same beat starts after the first.
        c.say(1_000, &"y".repeat(36), &cfg);
        assert_eq!(c.until(), Some(11_250 + 2_250));
        assert_eq!(c.queued_ms(5_000), 8_500);
        // After a silence the next line starts when it is said.
        c.say(30_000, "z", &cfg);
        assert_eq!(c.until(), Some(30_000 + 56 + 250));
        assert_eq!(c.free_at(40_000), 40_000);
    }

    #[test]
    fn lines_past_the_budget_go_whole_last_first_never_the_first() {
        assert!(over_budget(&[100, 100], 260).is_empty());
        assert_eq!(over_budget(&[100, 100, 100], 260), vec![2]);
        assert_eq!(over_budget(&[200, 100, 100], 260), vec![2, 1]);
        assert_eq!(
            over_budget(&[400, 10], 260),
            vec![1],
            "the first line stays"
        );
        assert!(over_budget(&[400], 260).is_empty());
        assert!(over_budget(&[], 260).is_empty());
    }

    #[test]
    fn a_closing_line_in_the_state_is_the_closing_window() {
        assert!(is_closing(
            "open poll\nCLOSING: the stream ends in 40 s. Wrap up."
        ));
        assert!(is_closing(
            "CLOSING NOW: the operator is ending the stream."
        ));
        assert!(!is_closing("closing remarks are not the window"));
        // Only the goodbye line skips the hold.
        assert!(!is_closing_now(
            "open poll\nCLOSING: the stream ends in 40 s. Wrap up."
        ));
        assert!(is_closing_now(
            "open poll\nCLOSING NOW: the stream ends in 12 s. Say goodbye."
        ));
        assert_eq!(closing_budget(260), 130);
    }
}
