//! Quota pacing (WP13): a tournament pauses when no account can serve, instead of burning
//! runs on a cooling fleet.
//!
//! - [`QuotaGate`]: asks the engine whether any account can serve, and until when it cannot.
//!   [`FleetQuota`] reads the live fleet's own cooling state.
//! - [`PaceClock`]: the clock the pause waits on, injectable so tests move time instantly.
//!
//! Pauses happen only between runs (never inside one), so a run's wall clock is never held
//! open by a pause.

use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use shadowrunr_engine::Fleet;
use shadowrunr_types::{Engine, EngineRequest, Role};

/// Default total pause cap: 8 hours.
pub const DEFAULT_MAX_PAUSE_MS: u64 = 8 * 3600 * 1000;
/// Margin added to a known reset before resuming.
pub const RESUME_MARGIN_MS: i64 = 60_000;
/// Upper bound of the jitter added to the margin.
pub const RESUME_JITTER_MS: i64 = 30_000;
/// In-session retries of one run (engine_failed re-run or judge_pending re-judge).
pub const MAX_RETRIES: u32 = 2;
/// Fallback cooldown when the gate cannot say (matches the engine's default).
pub const DEFAULT_COOLDOWN_MS: u64 = shadowrunr_engine::config::DEFAULT_COOLDOWN_MS;

/// No account can serve until `until_ms` (`None` = the engine gave no reset time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaBlock {
    pub accounts: Vec<String>,
    pub until_ms: Option<i64>,
}

/// What the tournament asks the engine before starting a run or a judge call.
pub trait QuotaGate: Send + Sync {
    /// `None` when some account can serve the tournament's roles now.
    fn blocked(&self) -> Option<QuotaBlock>;
    /// The fleet's cooldown for a limit without a reset time.
    fn default_cooldown_ms(&self) -> u64 {
        DEFAULT_COOLDOWN_MS
    }
}

/// The clock pauses wait on.
#[async_trait]
pub trait PaceClock: Send + Sync {
    fn now_ms(&self) -> i64;
    async fn sleep_until_ms(&self, until_ms: i64);
}

/// Wall clock plus `tokio::time::sleep`.
pub struct SystemPaceClock;

#[async_trait]
impl PaceClock for SystemPaceClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
    async fn sleep_until_ms(&self, until_ms: i64) {
        let wait = until_ms.saturating_sub(self.now_ms()).max(0) as u64;
        tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
    }
}

/// A shareable gate (Debug for option structs).
#[derive(Clone)]
pub struct GateHandle(pub Arc<dyn QuotaGate>);

impl fmt::Debug for GateHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GateHandle")
    }
}

/// A shareable clock (Debug for option structs).
#[derive(Clone)]
pub struct ClockHandle(pub Arc<dyn PaceClock>);

impl fmt::Debug for ClockHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClockHandle")
    }
}

/// The live fleet as a gate. Account cooling is per account, not per role, so "can serve the
/// needed roles" means "some Claude Code account is not cooling". A fleet with no accounts
/// (API key only) is never blocked.
pub struct FleetQuota {
    fleet: Arc<Fleet>,
}

impl FleetQuota {
    pub fn new(fleet: Arc<Fleet>) -> Self {
        Self { fleet }
    }
}

impl QuotaGate for FleetQuota {
    fn blocked(&self) -> Option<QuotaBlock> {
        if self.fleet.can_serve() {
            return None;
        }
        Some(QuotaBlock {
            accounts: self.fleet.accounts().into_iter().map(|a| a.label).collect(),
            until_ms: self.fleet.earliest_reset_ms(),
        })
    }
    fn default_cooldown_ms(&self) -> u64 {
        self.fleet.config().cooldown_ms()
    }
}

/// The cheapest call that tells whether the engine serves again: the chat picker's route
/// (Haiku, low effort, shortest timeout), a one-word prompt.
pub fn probe_request() -> EngineRequest {
    let (tier, effort, timeout_ms) = shadowrunr_engine::route(Role::ChatPicker);
    EngineRequest {
        role: Role::ChatPicker,
        tier,
        effort,
        system: "Quota probe. Reply with the single word ok.".into(),
        prompt: "ok?".into(),
        allowed_tools: Vec::new(),
        timeout_ms,
        json_schema: None,
        correlation_id: Some("quota-probe".into()),
    }
}

/// Probe the engine with [`probe_request`].
pub async fn probe(engine: &dyn Engine) -> bool {
    engine.complete(probe_request()).await.is_ok()
}

/// Deterministic jitter in `[0, RESUME_JITTER_MS)` for the `n`-th pause.
pub fn jitter_ms(n: u32, until_ms: i64) -> i64 {
    let mut r = crate::rng::Rng::new(u64::from(n) ^ (until_ms as u64));
    (r.next_u64() % RESUME_JITTER_MS as u64) as i64
}

/// `YYYY-MM-DD HH:MM:SS UTC` for an epoch-ms instant.
pub fn utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Civil-from-days (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[test]
    fn utc_formats_known_instants() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        // The evidence's reset: 2026-10-18 14:20 UTC.
        assert_eq!(utc(1_792_333_200_000), "2026-10-18 14:20:00 UTC");
    }

    #[test]
    fn jitter_is_bounded() {
        for n in 0..50 {
            let j = jitter_ms(n, 1_792_333_200_000 + i64::from(n));
            assert!((0..RESUME_JITTER_MS).contains(&j));
        }
    }

    /// A manual clock: sleeping jumps time forward. Records every sleep and whether any run
    /// was in flight while it slept.
    pub(crate) struct ManualClock {
        pub now: AtomicI64,
        pub sleeps: Mutex<Vec<(i64, i64)>>,
        pub in_flight: Arc<AtomicUsize>,
        pub slept_with_runs: AtomicUsize,
    }

    impl ManualClock {
        pub(crate) fn new(t0: i64) -> Arc<Self> {
            Arc::new(Self {
                now: AtomicI64::new(t0),
                sleeps: Mutex::new(Vec::new()),
                in_flight: Arc::new(AtomicUsize::new(0)),
                slept_with_runs: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait]
    impl PaceClock for ManualClock {
        fn now_ms(&self) -> i64 {
            self.now.load(Ordering::SeqCst)
        }
        async fn sleep_until_ms(&self, until_ms: i64) {
            if self.in_flight.load(Ordering::SeqCst) > 0 {
                self.slept_with_runs.fetch_add(1, Ordering::SeqCst);
            }
            let from = self.now_ms();
            self.sleeps.lock().unwrap().push((from, until_ms));
            self.now.fetch_max(until_ms, Ordering::SeqCst);
        }
    }
}
