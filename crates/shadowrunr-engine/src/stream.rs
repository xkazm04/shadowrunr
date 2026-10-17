//! Parser for Claude Code `--output-format stream-json --verbose --include-partial-messages`.
//!
//! Line types seen on 2.1.302 (see `tests/fixtures/`): `system` (`init`, `status`),
//! `stream_event` (raw Messages API stream events: `message_start`, `content_block_delta`
//! with `text_delta`, ...), `assistant` (the complete message; on an API failure a synthetic
//! one carrying `error` and `api_error_status`), `rate_limit_event`, `result` (final text,
//! usage, `total_cost_usd`, `is_error`, `subtype`, `api_error_status`), and in stream-json
//! input mode `control_response`.

use serde_json::Value;
use shadowrunr_types::{EngineError, Usage};

use crate::limits::{self, Failure, Signals};

/// What one fed line meant for the reader loop.
#[derive(Debug, Clone, PartialEq)]
pub enum Feed {
    Continue,
    /// The `result` line arrived; the turn is over.
    Done,
    /// A `control_response` for the given request id.
    ControlResponse {
        request_id: String,
        ok: bool,
    },
}

/// The parsed turn, successful or not.
#[derive(Debug, Clone, Default)]
pub struct TurnOutcome {
    pub text: String,
    pub first_text_ms: Option<u64>,
    pub usage: Usage,
    pub cost_usd: Option<f64>,
    pub session_id: Option<String>,
    pub model: Option<String>,
    /// `duration_ms` as the CLI reported it.
    pub reported_duration_ms: Option<u64>,
    /// `ttft_ms` as the CLI reported it.
    pub reported_ttft_ms: Option<u64>,
    /// `subtype` of the result line (`success`, `error_max_turns`, ...).
    pub result_subtype: Option<String>,
    /// Utilisation of the tightest plan window from the last `rate_limit_event`, 0..1.
    pub plan_utilization: Option<f64>,
}

/// Incremental parser for one turn.
#[derive(Debug, Default)]
pub struct TurnParser {
    out: TurnOutcome,
    delta_text: String,
    assistant_text: String,
    result_text: Option<String>,
    is_error: bool,
    signals: Signals,
    api_error_message: bool,
    done: bool,
    lines_seen: usize,
}

fn u64_at(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Map a Messages API / CLI usage object onto the contract's `Usage`.
pub fn usage_from(v: &Value) -> Usage {
    Usage {
        input_tokens: u64_at(v, "input_tokens"),
        output_tokens: u64_at(v, "output_tokens"),
        cache_read_tokens: u64_at(v, "cache_read_input_tokens"),
        cache_creation_tokens: u64_at(v, "cache_creation_input_tokens"),
    }
}

fn message_text(msg: &Value) -> String {
    msg.get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn resets_at(info: &Value) -> Option<i64> {
    info.get("resetsAt").and_then(Value::as_i64)
}

impl TurnParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn lines_seen(&self) -> usize {
        self.lines_seen
    }

    pub fn session_id(&self) -> Option<&str> {
        self.out.session_id.as_deref()
    }

    /// Feed one stdout line. `elapsed_ms` is the time since the request was written, used
    /// for `first_text_ms`. Lines that are not JSON are ignored.
    pub fn feed(&mut self, line: &str, elapsed_ms: u64) -> Feed {
        let line = line.trim();
        if line.is_empty() {
            return Feed::Continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Feed::Continue;
        };
        self.lines_seen += 1;
        if self.out.session_id.is_none() {
            if let Some(s) = v.get("session_id").and_then(Value::as_str) {
                self.out.session_id = Some(s.to_string());
            }
        }
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "system" => {
                if v.get("subtype").and_then(Value::as_str) == Some("init") {
                    if let Some(m) = v.get("model").and_then(Value::as_str) {
                        self.out.model = Some(m.to_string());
                    }
                }
            }
            "stream_event" => {
                let ev = &v["event"];
                if ev.get("type").and_then(Value::as_str) == Some("content_block_delta")
                    && ev["delta"].get("type").and_then(Value::as_str) == Some("text_delta")
                {
                    let t = ev["delta"]
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if !t.is_empty() && self.out.first_text_ms.is_none() {
                        self.out.first_text_ms = Some(elapsed_ms);
                    }
                    self.delta_text.push_str(t);
                }
            }
            "assistant" => {
                let text = message_text(&v["message"]);
                let api_error = v.get("error").and_then(Value::as_str);
                if let Some(code) = api_error {
                    self.api_error_message = true;
                    self.signals.error_codes.push(code.to_string());
                    if let Some(st) = v.get("api_error_status").and_then(Value::as_u64) {
                        self.signals.api_status = Some(st as u16);
                    }
                    // The transcript form names the params `apiErrorParams`; accept both.
                    for key in ["api_error_params", "apiErrorParams"] {
                        if let Some(r) = v.get(key).and_then(|p| resets_at(&p["rate_limit_info"])) {
                            self.signals.resets_at_s = Some(r);
                        }
                    }
                    for key in ["api_error", "apiError"] {
                        if let Some(c) = v.get(key).and_then(Value::as_str) {
                            self.signals.error_codes.push(c.to_string());
                        }
                    }
                    self.signals.error_text = Some(text);
                } else {
                    // Without partial messages, the first assistant text marks first text.
                    if !text.is_empty() && self.out.first_text_ms.is_none() {
                        self.out.first_text_ms = Some(elapsed_ms);
                    }
                    self.assistant_text.push_str(&text);
                }
                if let Some(sr) = v["message"].get("stop_reason").and_then(Value::as_str) {
                    self.signals.stop_reason = Some(sr.to_string());
                }
            }
            "rate_limit_event" => {
                let info = &v["rate_limit_info"];
                if let Some(st) = info.get("status").and_then(Value::as_str) {
                    self.signals.rate_limit_status = Some(st.to_string());
                    if st == "rejected" {
                        if let Some(r) = resets_at(info) {
                            self.signals.resets_at_s = Some(r);
                        }
                    }
                }
                if let Some(u) = info.get("utilization").and_then(Value::as_f64) {
                    self.out.plan_utilization = Some(u);
                }
            }
            "result" => {
                self.done = true;
                self.is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("");
                self.out.result_subtype = Some(subtype.to_string());
                if !subtype.is_empty() && subtype != "success" {
                    self.is_error = true;
                    self.signals.result_subtype = Some(subtype.to_string());
                }
                if let Some(st) = v.get("api_error_status").and_then(Value::as_u64) {
                    self.signals.api_status = Some(st as u16);
                }
                if let Some(sr) = v.get("stop_reason").and_then(Value::as_str) {
                    self.signals.stop_reason = Some(sr.to_string());
                }
                self.result_text = v.get("result").and_then(Value::as_str).map(str::to_string);
                if self.is_error && self.result_text.is_some() {
                    self.signals.error_text = self.result_text.clone();
                }
                self.out.usage = usage_from(&v["usage"]);
                self.out.cost_usd = v.get("total_cost_usd").and_then(Value::as_f64);
                self.out.reported_duration_ms = v.get("duration_ms").and_then(Value::as_u64);
                self.out.reported_ttft_ms = v.get("ttft_ms").and_then(Value::as_u64);
                return Feed::Done;
            }
            "control_response" => {
                let r = &v["response"];
                return Feed::ControlResponse {
                    request_id: r
                        .get("request_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    ok: r.get("subtype").and_then(Value::as_str) == Some("success"),
                };
            }
            _ => {}
        }
        Feed::Continue
    }

    /// Close the turn. `Err` carries the classified failure.
    pub fn finish(self) -> Result<TurnOutcome, Failure> {
        if !self.done {
            return Err(Failure::Transient {
                detail: "stream ended without a result line".into(),
            });
        }
        if self.is_error || self.api_error_message {
            return Err(limits::classify(&self.signals));
        }
        if self.signals.stop_reason.as_deref() == Some("refusal") {
            return Err(Failure::Fatal(EngineError::Refused {
                reason: self.result_text.unwrap_or_default(),
            }));
        }
        let mut out = self.out;
        out.text = match self.result_text {
            Some(t) => t,
            None if !self.assistant_text.is_empty() => self.assistant_text,
            None => self.delta_text,
        };
        Ok(out)
    }

    /// Signals gathered so far (for tests and diagnostics).
    pub fn signals(&self) -> &Signals {
        &self.signals
    }
}

/// Parse a whole recorded stream, one line per 10 ms of synthetic time.
pub fn parse_stream(stdout: &str) -> Result<TurnOutcome, Failure> {
    let mut p = TurnParser::new();
    for (i, line) in stdout.lines().enumerate() {
        if p.feed(line, i as u64 * 10) == Feed::Done {
            break;
        }
    }
    p.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUCCESS: &str = include_str!("../tests/fixtures/claude_success.jsonl");
    const MODEL_404: &str = include_str!("../tests/fixtures/claude_error_model_not_found.jsonl");
    const RATE_LIMIT: &str =
        include_str!("../tests/fixtures/claude_rate_limit_reconstructed.jsonl");
    const SESSION: &str = include_str!("../tests/fixtures/claude_session_interrupt.jsonl");

    #[test]
    fn parses_real_success_fixture() {
        let out = parse_stream(SUCCESS).expect("success");
        assert_eq!(out.text, "ok");
        // The text_delta is line index 4 of the capture, so 40 ms of synthetic time.
        assert_eq!(out.first_text_ms, Some(40));
        assert_eq!(out.usage.input_tokens, 2);
        assert_eq!(out.usage.output_tokens, 4);
        assert_eq!(out.usage.cache_creation_tokens, 726);
        assert_eq!(out.cost_usd, Some(0.0001474));
        assert_eq!(
            out.session_id.as_deref(),
            Some("548e4fea-c3e5-4bce-9075-b2de500c6ded")
        );
        assert_eq!(out.model.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(out.result_subtype.as_deref(), Some("success"));
        assert_eq!(out.plan_utilization, Some(0.95));
    }

    #[test]
    fn parses_real_error_fixture_as_fatal() {
        // Real 2.1.302 shape: subtype "success" but is_error true and api_error_status 404.
        match parse_stream(MODEL_404) {
            Err(Failure::Fatal(EngineError::Unavailable { reason })) => {
                assert!(reason.contains("http 404"), "{reason}");
                assert!(reason.contains("model_not_found"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_rate_limit_fixture_with_reset() {
        match parse_stream(RATE_LIMIT) {
            Err(Failure::RateLimited { reset_at_ms, rule }) => {
                assert_eq!(reset_at_ms, Some(1_791_999_000_000));
                assert_eq!(rule, "error:rate_limit");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn session_capture_has_result_then_control_response() {
        let mut p = TurnParser::new();
        let mut feeds = Vec::new();
        for (i, l) in SESSION.lines().enumerate() {
            feeds.push(p.feed(l, i as u64));
        }
        assert!(feeds.contains(&Feed::Done));
        assert_eq!(
            feeds.last(),
            Some(&Feed::ControlResponse {
                request_id: "req_int_1".into(),
                ok: true
            })
        );
    }

    #[test]
    fn non_json_lines_are_ignored_and_missing_result_is_transient() {
        let mut p = TurnParser::new();
        assert_eq!(p.feed("warning: banner", 0), Feed::Continue);
        assert!(matches!(p.finish(), Err(Failure::Transient { .. })));
    }
}
