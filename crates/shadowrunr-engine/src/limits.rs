//! Failure classification. Rate- and usage-limit detection is table-driven so a new error
//! shape is one row, not a code change.
//!
//! Shapes the table is built from:
//! - Claude Code 2.1.302 stream-json, synthetic API error (captured live,
//!   `tests/fixtures/claude_error_model_not_found.jsonl`): the `assistant` line carries
//!   `"error": "<code>"`, `"is_api_error_message": true`, `"api_error_status": <http>`, and the
//!   `result` line carries `"is_error": true`, `"api_error_status": <http>`.
//! - A real Claude Code 2.1.299 session that hit the five-hour limit (transcript record):
//!   `"error": "rate_limit"`, `"apiError": "usage_limit_reached"`, `"apiErrorStatus": 429`,
//!   `apiErrorParams.rate_limit_info.resetsAt` (Unix seconds), text
//!   "You've hit your session limit · resets 7:30pm (Europe/Prague)".
//! - Claude Code 2.1.302 `rate_limit_event` lines (captured live): `rate_limit_info.status` is
//!   `allowed` / `allowed_warning` / `rejected`, with `resetsAt` in Unix seconds.
//! - Older Claude Code text form: "Claude AI usage limit reached|<unix seconds>".
//! - Messages API: HTTP 429 with `{"type":"error","error":{"type":"rate_limit_error"}}` and a
//!   `retry-after` header in seconds.

use shadowrunr_types::EngineError;

/// Why one attempt on one backend failed, and what the fleet should do about it.
#[derive(Debug, Clone)]
pub enum Failure {
    /// Cool this account until `reset_at_ms` (or the default cooldown) and try the next one.
    RateLimited {
        reset_at_ms: Option<i64>,
        rule: &'static str,
    },
    /// The account has no usable login. Cool it and try the next one.
    Auth { detail: String },
    /// Overload, server error, crashed process: try the next backend, no cooldown.
    Transient { detail: String },
    /// The request itself is the problem (timeout, refusal, bad model, parse error). Retrying
    /// elsewhere would not help; return it to the caller.
    Fatal(EngineError),
}

impl Failure {
    /// Stable ledger label.
    pub fn kind(&self) -> &'static str {
        match self {
            Failure::RateLimited { .. } => "rate_limited",
            Failure::Auth { .. } => "auth",
            Failure::Transient { .. } => "transient",
            Failure::Fatal(e) => error_kind(e),
        }
    }

    pub fn into_engine_error(self, account: &str) -> EngineError {
        match self {
            Failure::RateLimited { reset_at_ms, .. } => EngineError::RateLimited {
                account: account.to_string(),
                reset_at_ms,
            },
            Failure::Auth { detail } => EngineError::Unavailable {
                reason: format!("auth on {account}: {detail}"),
            },
            Failure::Transient { detail } => EngineError::Unavailable {
                reason: format!("{account}: {detail}"),
            },
            Failure::Fatal(e) => e,
        }
    }
}

/// Stable ledger label for an engine error (matches its serde tag).
pub fn error_kind(e: &EngineError) -> &'static str {
    match e {
        EngineError::RateLimited { .. } => "rate_limited",
        EngineError::Unavailable { .. } => "unavailable",
        EngineError::Timeout { .. } => "timeout",
        EngineError::Refused { .. } => "refused",
        EngineError::Parse { .. } => "parse",
        EngineError::Io { .. } => "io",
    }
}

/// Everything a failed turn told us, gathered from whichever lines carried it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Signals {
    /// The `error` / `apiError` code on an API-error message (`rate_limit`, `model_not_found`).
    pub error_codes: Vec<String>,
    /// HTTP status of the failed API call.
    pub api_status: Option<u16>,
    /// `rate_limit_info.status` of the last `rate_limit_event`.
    pub rate_limit_status: Option<String>,
    /// `rate_limit_info.resetsAt` (Unix seconds) from the event or the error params.
    pub resets_at_s: Option<i64>,
    /// Error text (result text of an `is_error` result, or the API-error message text).
    pub error_text: Option<String>,
    /// `stop_reason` of the final message or result.
    pub stop_reason: Option<String>,
    /// `subtype` of the result line when it is not `success`.
    pub result_subtype: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum Signal {
    ErrorCode(&'static str),
    Status(u16),
    RateLimitStatus(&'static str),
    /// Lower-case substring of the error text.
    TextContains(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub id: &'static str,
    pub signal: Signal,
}

/// Rate-limit and usage-limit signals. Text rules only ever see error text, never a normal
/// reply, so Malakor saying "rate limit" on stream cannot cool an account.
pub const RATE_LIMIT_RULES: &[Rule] = &[
    Rule {
        id: "error:rate_limit",
        signal: Signal::ErrorCode("rate_limit"),
    },
    Rule {
        id: "error:usage_limit_reached",
        signal: Signal::ErrorCode("usage_limit_reached"),
    },
    Rule {
        id: "error:rate_limit_error",
        signal: Signal::ErrorCode("rate_limit_error"),
    },
    Rule {
        id: "status:429",
        signal: Signal::Status(429),
    },
    Rule {
        id: "rate_limit_event:rejected",
        signal: Signal::RateLimitStatus("rejected"),
    },
    Rule {
        id: "text:hit-your-limit",
        signal: Signal::TextContains("hit your"),
    },
    Rule {
        id: "text:usage-limit-reached",
        signal: Signal::TextContains("usage limit reached"),
    },
    Rule {
        id: "text:rate-limit",
        signal: Signal::TextContains("rate limit"),
    },
    Rule {
        id: "text:out-of-usage",
        signal: Signal::TextContains("out of extra usage"),
    },
];

/// No usable login on the account.
pub const AUTH_RULES: &[Rule] = &[
    Rule {
        id: "status:401",
        signal: Signal::Status(401),
    },
    Rule {
        id: "status:403",
        signal: Signal::Status(403),
    },
    Rule {
        id: "error:authentication_failed",
        signal: Signal::ErrorCode("authentication_failed"),
    },
    Rule {
        id: "error:authentication_error",
        signal: Signal::ErrorCode("authentication_error"),
    },
    Rule {
        id: "text:not-logged-in",
        signal: Signal::TextContains("not logged in"),
    },
    Rule {
        id: "text:login",
        signal: Signal::TextContains("please run /login"),
    },
    Rule {
        id: "text:invalid-api-key",
        signal: Signal::TextContains("invalid api key"),
    },
];

/// Overload and server-side failures worth one try on the next backend.
pub const TRANSIENT_RULES: &[Rule] = &[
    Rule {
        id: "status:500",
        signal: Signal::Status(500),
    },
    Rule {
        id: "status:502",
        signal: Signal::Status(502),
    },
    Rule {
        id: "status:503",
        signal: Signal::Status(503),
    },
    Rule {
        id: "status:529",
        signal: Signal::Status(529),
    },
    Rule {
        id: "error:overloaded",
        signal: Signal::ErrorCode("overloaded"),
    },
    Rule {
        id: "error:overloaded_error",
        signal: Signal::ErrorCode("overloaded_error"),
    },
    Rule {
        id: "error:api_error",
        signal: Signal::ErrorCode("api_error"),
    },
    Rule {
        id: "text:overloaded",
        signal: Signal::TextContains("overloaded"),
    },
];

fn rule_hits(rule: &Rule, s: &Signals, text_lc: &str) -> bool {
    match rule.signal {
        Signal::ErrorCode(code) => s.error_codes.iter().any(|c| c == code),
        Signal::Status(st) => s.api_status == Some(st),
        Signal::RateLimitStatus(st) => s.rate_limit_status.as_deref() == Some(st),
        Signal::TextContains(needle) => !text_lc.is_empty() && text_lc.contains(needle),
    }
}

/// First rule in `table` that matches, by id.
pub fn first_match(table: &[Rule], s: &Signals) -> Option<&'static str> {
    let text_lc = s.error_text.as_deref().unwrap_or("").to_lowercase();
    table
        .iter()
        .find(|r| rule_hits(r, s, &text_lc))
        .map(|r| r.id)
}

/// Reset time in epoch ms: `resetsAt` when known, else the legacy "…|<unix seconds>" text.
pub fn reset_at_ms(s: &Signals) -> Option<i64> {
    if let Some(sec) = s.resets_at_s {
        return Some(sec.saturating_mul(1000));
    }
    let text = s.error_text.as_deref()?;
    let (_, tail) = text.rsplit_once('|')?;
    let sec: i64 = tail.trim().parse().ok()?;
    (sec > 1_000_000_000).then_some(sec.saturating_mul(1000))
}

/// Classify a failed turn. Rate limits win over everything because they decide rotation.
pub fn classify(s: &Signals) -> Failure {
    if let Some(rule) = first_match(RATE_LIMIT_RULES, s) {
        return Failure::RateLimited {
            reset_at_ms: reset_at_ms(s),
            rule,
        };
    }
    let detail = describe(s);
    if first_match(AUTH_RULES, s).is_some() {
        return Failure::Auth { detail };
    }
    if first_match(TRANSIENT_RULES, s).is_some() {
        return Failure::Transient { detail };
    }
    if s.stop_reason.as_deref() == Some("refusal") {
        return Failure::Fatal(EngineError::Refused { reason: detail });
    }
    Failure::Fatal(EngineError::Unavailable { reason: detail })
}

fn describe(s: &Signals) -> String {
    let mut parts = Vec::new();
    if let Some(st) = s.api_status {
        parts.push(format!("http {st}"));
    }
    if !s.error_codes.is_empty() {
        parts.push(s.error_codes.join(","));
    }
    if let Some(sub) = &s.result_subtype {
        parts.push(sub.clone());
    }
    if let Some(t) = &s.error_text {
        parts.push(t.chars().take(200).collect());
    }
    if parts.is_empty() {
        "unknown error".into()
    } else {
        parts.join(": ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_from_error_code_with_reset() {
        let s = Signals {
            error_codes: vec!["rate_limit".into()],
            api_status: Some(429),
            resets_at_s: Some(1_791_999_000),
            ..Default::default()
        };
        assert!(matches!(
            classify(&s),
            Failure::RateLimited {
                reset_at_ms: Some(1_791_999_000_000),
                rule: "error:rate_limit"
            }
        ));
    }

    #[test]
    fn legacy_text_form_carries_reset() {
        let s = Signals {
            error_text: Some("Claude AI usage limit reached|1760000000".into()),
            ..Default::default()
        };
        match classify(&s) {
            Failure::RateLimited { reset_at_ms, rule } => {
                assert_eq!(reset_at_ms, Some(1_760_000_000_000));
                assert_eq!(rule, "text:usage-limit-reached");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn model_not_found_is_fatal_and_overload_is_transient() {
        let s = Signals {
            error_codes: vec!["model_not_found".into()],
            api_status: Some(404),
            ..Default::default()
        };
        assert!(matches!(classify(&s), Failure::Fatal(_)));
        let s = Signals {
            api_status: Some(529),
            ..Default::default()
        };
        assert!(matches!(classify(&s), Failure::Transient { .. }));
        let s = Signals {
            error_text: Some("Not logged in · Please run /login".into()),
            ..Default::default()
        };
        assert!(matches!(classify(&s), Failure::Auth { .. }));
    }
}
