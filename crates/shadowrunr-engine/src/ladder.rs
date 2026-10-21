//! The fleet's failure ladder, written once.
//!
//! One-shot calls ([`crate::fleet::Fleet::complete`]) and warm turns
//! ([`crate::session::ClaudeWarmSession`]) both ask [`step`] what a failed attempt means:
//!
//! | failure      | verdict                                   |
//! |--------------|-------------------------------------------|
//! | RateLimited  | cool the account until its reset, rotate  |
//! | Auth         | cool the account (default cooldown), rotate |
//! | Transient    | rotate, no cooldown                       |
//! | Fatal        | stop: the request is the problem          |
//!
//! A rotation is a candidate failure and moves to the next rung; a stop is a request
//! failure and ends the ladder. [`Attempts`] keeps the elimination reasons that become the
//! reply's `fallback_reason`, and [`served`] / [`down`] report the fleet mode from what
//! actually answered.

use shadowrunr_types::{EngineError, EngineKind};

use crate::fleet::{FleetInner, FleetMode};
use crate::limits::Failure;

/// What the ladder does after a failed attempt.
#[derive(Debug)]
pub enum Step {
    /// Try the next candidate. `cool` is `Some(reset_at_ms)` when the account must cool
    /// first (`Some(None)` = the default cooldown); `reason` names the elimination.
    Rotate {
        cool: Option<Option<i64>>,
        reason: String,
    },
    /// Return this error to the caller; no other candidate would do better.
    Stop(EngineError),
}

/// The verdict for one failed attempt on the candidate named `who` (an account label, or
/// `api`).
pub fn step(f: Failure, who: &str) -> Step {
    match f {
        Failure::RateLimited { reset_at_ms, rule } => Step::Rotate {
            cool: Some(reset_at_ms),
            reason: format!("account {who} rate-limited ({rule})"),
        },
        Failure::Auth { detail } => Step::Rotate {
            cool: Some(None),
            reason: format!("account {who} auth: {detail}"),
        },
        Failure::Transient { detail } => Step::Rotate {
            cool: None,
            reason: format!("account {who}: {detail}"),
        },
        Failure::Fatal(e) => Step::Stop(e),
    }
}

/// The elimination reasons gathered while walking the ladder.
#[derive(Debug, Clone, Default)]
pub struct Attempts {
    reasons: Vec<String>,
}

impl Attempts {
    pub fn push(&mut self, reason: String) {
        self.reasons.push(reason);
    }

    pub fn is_empty(&self) -> bool {
        self.reasons.is_empty()
    }

    /// `fallback_reason` for a reply served after at least one elimination.
    pub fn fallback(&self) -> Option<String> {
        (!self.reasons.is_empty()).then(|| self.joined())
    }

    pub fn joined(&self) -> String {
        self.reasons.join("; ")
    }

    /// Called when no account is left: make sure the reasons say why, even when no account
    /// was tried at all.
    pub fn accounts_exhausted(&mut self, inner: &FleetInner) {
        if self.reasons.is_empty() {
            self.reasons.push(if inner.accounts.is_empty() {
                "no claude cli accounts configured".into()
            } else {
                "all claude cli accounts cooling".into()
            });
        }
    }

    /// The error when every rung failed.
    pub fn unavailable(&self) -> EngineError {
        EngineError::Unavailable {
            reason: self.joined(),
        }
    }
}

/// The fleet mode a reply from `engine` puts the fleet in.
pub fn mode_for(engine: EngineKind) -> FleetMode {
    match engine {
        EngineKind::ClaudeCli => FleetMode::Primary,
        _ => FleetMode::Degraded,
    }
}

/// Record that `engine` served a call (one-shot or warm turn).
pub fn served(inner: &FleetInner, engine: EngineKind) {
    inner.set_mode(mode_for(engine));
}

/// Record that nothing could serve a call.
pub fn down(inner: &FleetInner) {
    inner.set_mode(FleetMode::Down);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One table for every failure kind: both `Fleet::complete` and
    /// `ClaudeWarmSession::send` route their failures through [`step`], so this is the
    /// verdict each of them takes.
    #[test]
    fn step_table_covers_every_failure_kind() {
        let cases: Vec<(Failure, &str, Option<Option<i64>>, bool)> = vec![
            (
                Failure::RateLimited {
                    reset_at_ms: Some(42),
                    rule: "status:429",
                },
                "rate-limited (status:429)",
                Some(Some(42)),
                true,
            ),
            (
                Failure::Auth {
                    detail: "not logged in".into(),
                },
                "auth: not logged in",
                Some(None),
                true,
            ),
            (
                Failure::Transient {
                    detail: "http 529".into(),
                },
                "account a: http 529",
                None,
                true,
            ),
            (
                Failure::Fatal(EngineError::Unavailable {
                    reason: "http 404".into(),
                }),
                "",
                None,
                false,
            ),
        ];
        for (f, want_reason, want_cool, rotates) in cases {
            let kind = f.kind();
            match step(f, "a") {
                Step::Rotate { cool, reason } => {
                    assert!(rotates, "{kind} must not rotate");
                    assert_eq!(cool, want_cool, "{kind}");
                    assert!(reason.contains(want_reason), "{kind}: {reason}");
                    assert!(reason.starts_with("account a"), "{kind}: {reason}");
                }
                Step::Stop(e) => {
                    assert!(!rotates, "{kind} must rotate");
                    assert!(e.to_string().contains("404"), "{e}");
                }
            }
        }
    }

    #[test]
    fn mode_follows_what_served() {
        assert_eq!(mode_for(EngineKind::ClaudeCli), FleetMode::Primary);
        assert_eq!(mode_for(EngineKind::AnthropicApi), FleetMode::Degraded);
        assert_eq!(mode_for(EngineKind::CodexCli), FleetMode::Degraded);
    }

    #[test]
    fn attempts_name_the_eliminations() {
        let mut a = Attempts::default();
        assert_eq!(a.fallback(), None);
        a.push("account a rate-limited (x)".into());
        a.push("account b: http 529".into());
        assert_eq!(
            a.fallback().as_deref(),
            Some("account a rate-limited (x); account b: http 529")
        );
    }
}
