//! Fleet-loss policy. When the engine fleet is degraded or down, Malakor keeps chatting for
//! a grace period and then signs off. The brain does not depend on WP1 internals: the
//! daemon adapts its fleet state to [`FleetProbe`], or pushes a mode with
//! `Brain::set_fleet_mode`. Fleet-class engine errors on the core thread count as `Down`
//! until the next successful call.

use shadowrunr_types::TsMs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FleetMode {
    Healthy,
    Degraded,
    Down,
}

/// Something that knows the fleet's state (WP1's `FleetState`, adapted by the daemon).
pub trait FleetProbe: Send + Sync {
    fn mode(&self) -> FleetMode;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FleetVerdict {
    Continue,
    /// The grace period ran out this beat.
    SignOff,
    /// Already signed off.
    Gone,
}

#[derive(Debug, Clone)]
pub struct FleetWatch {
    grace_ms: i64,
    since: Option<TsMs>,
    signed_off: bool,
}

impl FleetWatch {
    pub fn new(grace_s: u64) -> Self {
        Self {
            grace_ms: (grace_s as i64).saturating_mul(1000),
            since: None,
            signed_off: false,
        }
    }

    /// Feed the current mode; a healthy reading resets the clock.
    pub fn observe(&mut self, now_ms: TsMs, mode: FleetMode) -> FleetVerdict {
        if self.signed_off {
            return FleetVerdict::Gone;
        }
        if mode == FleetMode::Healthy {
            self.since = None;
            return FleetVerdict::Continue;
        }
        let since = *self.since.get_or_insert(now_ms);
        if now_ms - since >= self.grace_ms {
            self.signed_off = true;
            FleetVerdict::SignOff
        } else {
            FleetVerdict::Continue
        }
    }

    /// When the degraded/down period started, if one is running.
    pub fn since(&self) -> Option<TsMs> {
        self.since
    }

    pub fn signed_off(&self) -> bool {
        self.signed_off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grace_then_sign_off_once() {
        let mut w = FleetWatch::new(300);
        assert_eq!(w.observe(0, FleetMode::Healthy), FleetVerdict::Continue);
        assert_eq!(
            w.observe(1_000, FleetMode::Degraded),
            FleetVerdict::Continue
        );
        assert_eq!(w.observe(200_000, FleetMode::Down), FleetVerdict::Continue);
        assert_eq!(w.observe(300_999, FleetMode::Down), FleetVerdict::Continue);
        assert_eq!(w.observe(301_000, FleetMode::Down), FleetVerdict::SignOff);
        assert_eq!(w.observe(302_000, FleetMode::Healthy), FleetVerdict::Gone);
    }

    #[test]
    fn recovery_resets_the_clock() {
        let mut w = FleetWatch::new(10);
        w.observe(0, FleetMode::Down);
        w.observe(9_000, FleetMode::Healthy);
        assert_eq!(w.observe(12_000, FleetMode::Down), FleetVerdict::Continue);
        assert_eq!(w.observe(22_000, FleetMode::Down), FleetVerdict::SignOff);
    }
}
