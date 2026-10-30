//! Property tests for the pace module's public API ([`shadowrunr_brain::pace`]). Each property
//! names the invariant it holds the code to, taken from the code and its doc comments. Inputs
//! are generated and simulated; no real chat.
//!
//! Proven red on a seeded fault (each fault seeded locally in `src/pace.rs`, the one test run,
//! the source restored; property -> function: fault -> observed failure):
//! - `speech_ms_is_never_negative` -> `paced_speech_ms`: `line_pause_ms.max(0)` became
//!   `line_pause_ms` -> `speech_ms(&text) >= 0` failed on a negative pause.
//! - `speech_ms_is_non_decreasing_in_text_length` -> `paced_speech_ms`: the character count
//!   became `count % 100` -> `speech_ms(&head) <= speech_ms(&longer)` failed.
//! - `speech_ms_depends_only_on_the_character_count` -> `paced_speech_ms`: `chars().count()`
//!   became `len()` (bytes) -> the ascii and the two-byte line took different times.
//! - `a_slower_cue_never_speaks_quicker` -> `PACE_FACTOR_SLOW` 0.72 became 1.2 -> `slow 833 <
//!   normal 1000`.
//! - `the_pace_factor_is_positive` -> `PACE_FACTOR_FAST` 1.04 became 0.0 -> `pace_factor(p) > 0.0`
//!   failed for `Some(Fast)`.
//! - `the_speech_clock_stays_consistent_after_any_says` -> `say_paced`: `u.max(at)` became
//!   `u.min(at)` -> `until >= start + paced_speech_ms` failed.
//! - `a_fresh_clock_queues_nothing` -> `free_at`: `map_or(now, ..)` became `map_or(0, ..)` ->
//!   `free_at(now) == now` failed, now = 1.
//! - `the_latency_median_is_one_of_the_last_samples` -> `CoreLatency::median`: `v[len / 2]`
//!   became `v[0]` -> the below/above half-count bound failed.
//! - `the_lookahead_lead_stays_within_its_bounds` -> `lookahead_lead`: the clamp floor
//!   `speech_lead_ms` became 0 -> `lead >= speech_lead_ms` failed.
//! - `the_lookahead_lead_is_non_decreasing_in_core_latency` -> `lookahead_lead`: `c + render_lag`
//!   became `render_lag - c` -> `lead(lo) <= lead(hi)` failed.
//! - `the_lookahead_budget_stays_between_a_third_and_the_budget` -> `lookahead_budget`:
//!   `clamp(budget / 3, budget)` became `min(budget)` -> `0 < 1` (below a third).
//! - `the_lookahead_budget_is_non_increasing_in_queued_speech` -> `lookahead_budget`: the start
//!   `queued - core` became `core - queued` -> `budget(hi) <= budget(lo)` failed.
//! - `over_lag_drops_a_tail_of_in_range_indices_never_the_first` -> `over_lag`: `i > 0 &&`
//!   dropped -> the late list was `[0]`.
//! - `over_lag_drops_nothing_that_starts_within_the_ceiling` -> `over_lag`: the test became the
//!   line's end, not its start, past the ceiling -> a non-empty list inside the ceiling.
//! - `over_budget_drops_the_fewest_last_lines_never_the_first` -> `over_budget_holding`:
//!   `i > held` became `i >= held` -> the dropped list was `[0]`.
//! - `the_closing_budget_never_exceeds_the_budget` -> `closing_budget`: `/ 2` became `/ 2 + 1`
//!   -> `c <= b` failed, b = 0.
//! - `closing_now_is_always_closing` -> `is_closing`: the prefix became `CLOSING:` -> a
//!   `CLOSING NOW` line was not closing.
//! - `a_closing_line_anywhere_opens_the_window` -> `is_closing`: `trim_start()` dropped ->
//!   `is_closing` failed on an indented line (indent = tab).
//! - `a_state_without_the_prefix_is_not_closing` -> `is_closing`: matched case-insensitively
//!   (`to_uppercase()`) -> `!is_closing(&text)` failed. NOT red at first: the lower-case state
//!   never held the word `closing`, so the generator now mixes in lower-case `closing` lines.
//! - `speech_ms_survives_a_tiny_positive_rate` -> `paced_speech_ms`: `saturating_add` became
//!   `+` -> `attempt to add with overflow` panic at `pace.rs:130`.

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use shadowrunr_brain::pace::{
    closing_budget, is_closing, is_closing_now, lookahead_budget, lookahead_lead, over_budget,
    over_lag, pace_factor, CoreLatency, PaceConfig, SpeechClock, CLOSING_NOW_PREFIX,
    CLOSING_PREFIX, LATENCY_SAMPLES,
};
use shadowrunr_types::delivery::Pace;

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource(
            "proptest-regressions",
        ))),
        ..ProptestConfig::default()
    }
}

fn pace() -> impl Strategy<Value = Option<Pace>> {
    prop_oneof![
        Just(None),
        Just(Some(Pace::Slow)),
        Just(Some(Pace::Normal)),
        Just(Some(Pace::Fast)),
    ]
}

/// A speaking rate: a realistic one, or one the code must fall back from (zero, negative, NaN).
fn speech_cps() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => 1.0f64..60.0,
        1 => Just(0.0),
        1 => -50.0f64..0.0,
        1 => Just(f64::NAN),
    ]
}

/// A pace configuration with every knob in a plausible range, the pause possibly negative.
fn pace_config() -> impl Strategy<Value = PaceConfig> {
    (
        speech_cps(),
        -1_000i64..2_000,
        0i64..10_000,
        0usize..1_000,
        0i64..5_000,
        0i64..20_000,
        0i64..30_000,
    )
        .prop_map(
            |(
                speech_cps,
                line_pause_ms,
                speech_lead_ms,
                budget_chars,
                render_lag_ms,
                lookahead_max_ms,
                lag_ceiling_ms,
            )| PaceConfig {
                speech_cps,
                line_pause_ms,
                speech_lead_ms,
                budget_chars,
                render_lag_ms,
                lookahead_max_ms,
                lag_ceiling_ms,
            },
        )
}

/// A spoken line: any characters, multi-byte ones included, up to a few hundred; often empty,
/// so the bounds at zero characters are reached.
fn line() -> impl Strategy<Value = String> {
    prop_oneof![
        1 => Just(String::new()),
        4 => "\\PC{0,300}",
    ]
}

/// A state-summary line: plain, or one that starts (maybe indented) with a closing prefix.
fn state_line() -> impl Strategy<Value = String> {
    prop_oneof![
        "\\PC{0,20}",
        (
            "[ \t]{0,2}",
            prop::sample::select(&["CLOSING", "CLOSING NOW", "CLOSING:"][..]),
            "\\PC{0,12}"
        )
            .prop_map(|(indent, prefix, rest)| format!("{indent}{prefix}{rest}")),
    ]
}

proptest! {
    #![proptest_config(config())]

    /// `speech_ms` is never negative: characters over a positive rate plus a pause floored at 0.
    #[test]
    fn speech_ms_is_never_negative(cfg in pace_config(), text in line(), p in pace()) {
        prop_assert!(cfg.speech_ms(&text) >= 0);
        prop_assert!(cfg.paced_speech_ms(&text, p) >= 0);
    }

    /// `speech_ms` never shrinks as a line grows: appending text cannot make it quicker to say.
    #[test]
    fn speech_ms_is_non_decreasing_in_text_length(
        cfg in pace_config(),
        head in line(),
        tail in line(),
        p in pace(),
    ) {
        let longer = format!("{head}{tail}");
        prop_assert!(cfg.speech_ms(&head) <= cfg.speech_ms(&longer));
        prop_assert!(cfg.paced_speech_ms(&head, p) <= cfg.paced_speech_ms(&longer, p));
    }

    /// `speech_ms` counts characters, not bytes: two lines of equal character count take equally
    /// long, whatever their encoding.
    #[test]
    fn speech_ms_depends_only_on_the_character_count(cfg in pace_config(), n in 0usize..300) {
        let ascii = "x".repeat(n);
        let wide = "\u{00e9}".repeat(n);
        prop_assert_eq!(cfg.speech_ms(&ascii), cfg.speech_ms(&wide));
    }

    /// A slow cue takes at least as long as an uncued line and a fast cue at most as long
    /// ([`pace_factor`]: slow 0.72 < normal 1.0 < fast 1.04); an uncued line is a normal one.
    #[test]
    fn a_slower_cue_never_speaks_quicker(cfg in pace_config(), text in line()) {
        let slow = cfg.paced_speech_ms(&text, Some(Pace::Slow));
        let normal = cfg.paced_speech_ms(&text, Some(Pace::Normal));
        let fast = cfg.paced_speech_ms(&text, Some(Pace::Fast));
        prop_assert!(slow >= normal, "slow {slow} < normal {normal}");
        prop_assert!(normal >= fast, "normal {normal} < fast {fast}");
        prop_assert_eq!(normal, cfg.paced_speech_ms(&text, None));
        prop_assert_eq!(normal, cfg.speech_ms(&text));
    }

    /// The pace factor is positive for every cue, so a rate scaled by it stays positive.
    #[test]
    fn the_pace_factor_is_positive(p in pace()) {
        prop_assert!(pace_factor(p) > 0.0);
    }

    /// After any sequence of `say` calls the clock is consistent: `free_at(now) >= now`,
    /// `queued_ms(now) >= 0` and equals `free_at(now) - now`, the end of speech never moves
    /// back, and each line ends no earlier than its own start plus its speech time.
    #[test]
    fn the_speech_clock_stays_consistent_after_any_says(
        cfg in pace_config(),
        says in prop::collection::vec((0i64..1_000_000, line(), pace()), 0..20),
        probes in prop::collection::vec(-1_000_000i64..2_000_000, 1..8),
    ) {
        let mut clock = SpeechClock::default();
        prop_assert_eq!(clock.until(), None);
        let mut last_until = None;
        for (at, text, p) in &says {
            let before = clock.until();
            clock.say_paced(*at, text, *p, &cfg);
            let until = clock.until().expect("a said line sets the clock");
            let start = before.map_or(*at, |u| u.max(*at));
            prop_assert!(until >= start.saturating_add(cfg.paced_speech_ms(text, *p)));
            if let Some(prev) = last_until {
                prop_assert!(until >= prev, "the clock moved back: {prev} -> {until}");
            }
            last_until = Some(until);
            for &now in &probes {
                prop_assert!(clock.free_at(now) >= now);
                prop_assert!(clock.queued_ms(now) >= 0);
                prop_assert_eq!(clock.queued_ms(now), clock.free_at(now) - now);
            }
        }
    }

    /// A fresh clock has nothing queued: `free_at(now) == now` and `queued_ms(now) == 0`.
    #[test]
    fn a_fresh_clock_queues_nothing(now in any::<i64>()) {
        let clock = SpeechClock::default();
        prop_assert_eq!(clock.free_at(now), now);
        prop_assert_eq!(clock.queued_ms(now), 0);
    }

    /// The latency median is `None` before the first sample, and then one of the last
    /// [`LATENCY_SAMPLES`] recorded values (each floored at 0).
    #[test]
    fn the_latency_median_is_one_of_the_last_samples(
        samples in prop::collection::vec(-100_000i64..100_000, 0..30),
    ) {
        let mut lat = CoreLatency::default();
        prop_assert_eq!(lat.median(), None);
        for &s in &samples {
            lat.record(s);
        }
        let kept: Vec<i64> = samples
            .iter()
            .skip(samples.len().saturating_sub(LATENCY_SAMPLES))
            .map(|s| (*s).max(0))
            .collect();
        match lat.median() {
            None => prop_assert!(samples.is_empty()),
            Some(m) => {
                prop_assert!(m >= 0);
                prop_assert!(kept.contains(&m), "median {m} not among the kept {kept:?}");
                let below = kept.iter().filter(|&&k| k < m).count();
                let above = kept.iter().filter(|&&k| k > m).count();
                prop_assert!(below <= kept.len() / 2 && above <= kept.len() / 2);
            }
        }
    }

    /// The hold's lead is `speech_lead_ms` without a measured latency and otherwise lies between
    /// `speech_lead_ms` and `max(lookahead_max_ms, speech_lead_ms)`.
    #[test]
    fn the_lookahead_lead_stays_within_its_bounds(
        cfg in pace_config(),
        core in prop::option::of(-10_000i64..100_000),
    ) {
        let lead = lookahead_lead(&cfg, core);
        match core {
            None => prop_assert_eq!(lead, cfg.speech_lead_ms),
            Some(_) => {
                prop_assert!(lead >= cfg.speech_lead_ms);
                prop_assert!(lead <= cfg.lookahead_max_ms.max(cfg.speech_lead_ms));
            }
        }
    }

    /// A slower core call never shortens the lead: `lookahead_lead` is non-decreasing in it.
    #[test]
    fn the_lookahead_lead_is_non_decreasing_in_core_latency(
        cfg in pace_config(),
        a in 0i64..100_000,
        b in 0i64..100_000,
    ) {
        let (lo, hi) = (a.min(b), a.max(b));
        prop_assert!(lookahead_lead(&cfg, Some(lo)) <= lookahead_lead(&cfg, Some(hi)));
    }

    /// A beat composed ahead never asks for more than `budget` and never under a third of it.
    #[test]
    fn the_lookahead_budget_stays_between_a_third_and_the_budget(
        cfg in pace_config(),
        budget in 0usize..2_000,
        queued in -10_000i64..100_000,
        core in -10_000i64..100_000,
    ) {
        let b = lookahead_budget(&cfg, budget, queued, core);
        prop_assert!(b <= budget, "{b} > {budget}");
        prop_assert!(b >= budget / 3, "{b} < {}", budget / 3);
    }

    /// More speech queued never buys a bigger budget: `lookahead_budget` is non-increasing in
    /// `queued_ms` (its lines start later, so less fits under the lag ceiling).
    #[test]
    fn the_lookahead_budget_is_non_increasing_in_queued_speech(
        cfg in pace_config(),
        budget in 0usize..2_000,
        a in 0i64..100_000,
        b in 0i64..100_000,
        core in 0i64..20_000,
    ) {
        let (lo, hi) = (a.min(b), a.max(b));
        prop_assert!(
            lookahead_budget(&cfg, budget, hi, core) <= lookahead_budget(&cfg, budget, lo, core)
        );
    }

    /// `over_lag` returns in-range, strictly increasing indices, never the first line, and once
    /// a line is late every later one is too (start times only grow), so the late lines are a
    /// tail of the beat.
    #[test]
    fn over_lag_drops_a_tail_of_in_range_indices_never_the_first(
        cfg in pace_config(),
        lines in prop::collection::vec(line(), 0..10),
        queued in -10_000i64..40_000,
    ) {
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let late = over_lag(&cfg, &refs, queued);
        prop_assert!(late.iter().all(|&i| i > 0 && i < lines.len()), "{late:?}");
        prop_assert!(late.windows(2).all(|w| w[0] < w[1]), "{late:?}");
        if let Some(&first) = late.first() {
            let tail: Vec<usize> = (first..lines.len()).collect();
            prop_assert_eq!(late, tail);
        }
    }

    /// Nothing is late while the queue ahead plus every line before the last still starts within
    /// `lag_ceiling_ms`.
    #[test]
    fn over_lag_drops_nothing_that_starts_within_the_ceiling(
        cfg in pace_config(),
        lines in prop::collection::vec(line(), 0..10),
        queued in 0i64..40_000,
    ) {
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let before_last: i64 = refs
            .iter()
            .take(refs.len().saturating_sub(1))
            .map(|l| cfg.speech_ms(l))
            .sum();
        if queued + before_last <= cfg.lag_ceiling_ms {
            prop_assert!(over_lag(&cfg, &refs, queued).is_empty());
        }
    }

    /// `over_budget` drops whole lines, the last first: its indices are in range, strictly
    /// decreasing and a contiguous run ending at the last line, never the first line; what is
    /// kept fits `budget` (or is the first line alone), and it drops no more than it must.
    #[test]
    fn over_budget_drops_the_fewest_last_lines_never_the_first(
        lens in prop::collection::vec(0usize..600, 0..10),
        budget in 0usize..1_500,
    ) {
        let dropped = over_budget(&lens, budget);
        prop_assert!(dropped.iter().all(|&i| i > 0 && i < lens.len()), "{dropped:?}");
        prop_assert!(dropped.windows(2).all(|w| w[0] > w[1]), "{dropped:?}");
        let kept = lens.len() - dropped.len();
        let expected: Vec<usize> = (kept..lens.len()).rev().collect();
        prop_assert_eq!(&dropped, &expected);
        let kept_sum: usize = lens[..kept].iter().sum();
        prop_assert!(kept_sum <= budget || kept <= 1, "kept {kept_sum} > {budget}");
        if !dropped.is_empty() {
            // Keeping the last line dropped as well would have gone past the budget.
            prop_assert!(kept_sum + lens[kept] > budget);
        }
    }

    /// A goodbye beat's budget is half the beat budget, so never more than it.
    #[test]
    fn the_closing_budget_never_exceeds_the_budget(b in any::<usize>()) {
        let c = closing_budget(b);
        prop_assert!(c <= b);
        prop_assert_eq!(c, b / 2);
    }

    /// The goodbye is part of the closing window: `is_closing_now` implies `is_closing`.
    #[test]
    fn closing_now_is_always_closing(lines in prop::collection::vec(state_line(), 0..6)) {
        let text = lines.join("\n");
        if is_closing_now(&text) {
            prop_assert!(is_closing(&text));
        }
    }

    /// A state line that starts (after indentation) with the prefix opens the window wherever it
    /// sits among other lines; [`CLOSING_NOW_PREFIX`] asks for the goodbye too.
    #[test]
    fn a_closing_line_anywhere_opens_the_window(
        before in prop::collection::vec("[a-z ]{0,20}", 0..4),
        after in prop::collection::vec("[a-z ]{0,20}", 0..4),
        indent in "[ \t]{0,3}",
        rest in "[a-z :.]{0,20}",
        now in any::<bool>(),
    ) {
        let prefix = if now { CLOSING_NOW_PREFIX } else { CLOSING_PREFIX };
        let mut lines = before.clone();
        lines.push(format!("{indent}{prefix}{rest}"));
        lines.extend(after.iter().cloned());
        let state = lines.join("\n");
        prop_assert!(is_closing(&state));
        if now {
            prop_assert!(is_closing_now(&state));
        }
    }

    /// A state with no uppercase letters carries no closing line (the prefix is uppercase), even
    /// when a line says `closing` in lower case.
    #[test]
    fn a_state_without_the_prefix_is_not_closing(
        lines in prop::collection::vec(
            prop_oneof![
                3 => "[a-z0-9 :.]{0,30}",
                1 => Just("closing: the stream ends soon".to_string()),
                1 => Just("  closing now: say goodbye".to_string()),
            ],
            0..6,
        ),
    ) {
        let text = lines.join("\n");
        prop_assert!(!is_closing(&text));
        prop_assert!(!is_closing_now(&text));
    }
}

proptest! {
    #![proptest_config(config())]

    /// `speech_ms` is never negative and never panics for any positive `speech_cps`, however
    /// small (a character's `[tuning] speech_cps` is configuration).
    ///
    /// A real violation once, fixed in `paced_speech_ms`: a positive rate below about 1e-16
    /// makes the speech time infinite or past `i64::MAX`, the `as i64` cast saturates to `i64::MAX`, and adding a
    /// positive `line_pause_ms` overflows: a panic in a debug build, a wrapped negative time in
    /// a release one (`PaceConfig { speech_cps: 1e-300, line_pause_ms: 250, .. }`, text "x").
    #[test]
    fn speech_ms_survives_a_tiny_positive_rate(
        cps in f64::MIN_POSITIVE..1e-16,
        pause in 1i64..2_000,
        text in "\\PC{1,40}",
    ) {
        let cfg = PaceConfig { speech_cps: cps, line_pause_ms: pause, ..PaceConfig::default() };
        let ms = std::panic::catch_unwind(|| cfg.speech_ms(&text));
        prop_assert!(matches!(ms, Ok(ms) if ms >= 0), "speech_ms panicked or went negative");
    }
}
