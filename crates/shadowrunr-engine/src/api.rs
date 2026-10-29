//! The Anthropic Messages API fallback (raw HTTP over reqwest with rustls; Rust has no
//! official SDK). Served only when every fleet account is cooling or down.
//!
//! Request shape per the current API: effort in `output_config.effort`; structured output in
//! `output_config.format`; `cache_control` on the system block. Thinking is turned off for
//! `core_thread` and `chat_picker` where the model allows it: Haiku 5.5 takes
//! `{"type":"disabled"}` at effort `high` or below, Sonnet 5.5 takes
//! `{"type":"between_tools"}` at `high` or below, and Opus 5.5 cannot disable thinking at
//! all (the parameter is omitted and the low effort does the work).

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{json, Value};
use shadowrunr_types::{Effort, EngineError, EngineRequest, ModelTier, Role, Usage};

use crate::limits::{self, Failure, Signals};
use crate::model::{effort_str, estimate_cost_usd, model_id};
use crate::stream::usage_from;

pub const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    /// Lower-cased header names.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// HTTP seam: reqwest in production, a stub in tests (tests never call the API).
#[async_trait]
pub trait HttpTransport: Send + Sync {
    async fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &Value,
        timeout: Duration,
    ) -> Result<HttpResponse, EngineError>;
}

#[derive(Debug, Clone, Default)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl HttpTransport for ReqwestTransport {
    async fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &Value,
        timeout: Duration,
    ) -> Result<HttpResponse, EngineError> {
        let mut rb = self.client.post(url).timeout(timeout).json(body);
        for (k, v) in headers {
            rb = rb.header(k, v);
        }
        let resp = rb.send().await.map_err(|e| {
            if e.is_timeout() {
                EngineError::Timeout {
                    after_ms: timeout.as_millis() as u64,
                }
            } else {
                // `without_url` keeps the request URL (and anything in it) out of the error.
                EngineError::Io {
                    detail: format!("anthropic api: {}", e.without_url()),
                }
            }
        })?;
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_ascii_lowercase(),
                    v.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        let body = resp.text().await.map_err(|e| EngineError::Io {
            detail: format!("anthropic api body: {}", e.without_url()),
        })?;
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

/// Roles whose replies must be fast and plain: thinking off where the model allows it.
pub fn wants_thinking_off(role: Role) -> bool {
    matches!(role, Role::CoreThread | Role::ChatPicker)
}

/// The `thinking` value to send, or `None` to omit the parameter.
pub fn thinking_param(role: Role, tier: ModelTier, effort: Effort) -> Option<Value> {
    if !wants_thinking_off(role) {
        return None;
    }
    let at_or_below_high = matches!(effort, Effort::Low | Effort::Medium | Effort::High);
    match tier {
        ModelTier::Haiku if at_or_below_high => Some(json!({"type": "disabled"})),
        ModelTier::Sonnet if at_or_below_high => Some(json!({"type": "between_tools"})),
        _ => None,
    }
}

pub fn max_tokens_for(role: Role) -> u32 {
    match role {
        Role::CoreThread | Role::ChatPicker | Role::Chatter => 4096,
        _ => 16_000,
    }
}

/// One conversation message for the API.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub role: &'static str,
    pub text: String,
}

/// Constraints the API's structured output refuses (400 "properties maximum, minimum are not
/// supported"). `minItems` is kept only at 0 or 1.
const UNSUPPORTED_CONSTRAINTS: [&str; 8] = [
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "minLength",
    "maxLength",
    "maxItems",
];

/// The schema as the Messages API accepts it. The CLI's `--json-schema` takes the role schemas
/// as written; the API answers 400 on two things they use, which left the judge and featured
/// chatters unavailable on an API-only fleet:
/// - every object schema must state `additionalProperties: false`;
/// - numeric, length and item-count constraints are refused. They are dropped from the schema
///   and restated in the node's `description`, so the model still reads them; the role parsers
///   stay the validators.
pub fn strict_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len() + 1);
            let mut moved: Vec<String> = Vec::new();
            for (k, v) in map {
                let refused = UNSUPPORTED_CONSTRAINTS.contains(&k.as_str())
                    || (k == "minItems" && v.as_u64().is_some_and(|n| n > 1));
                if refused && !v.is_object() {
                    moved.push(format!("{k} {v}"));
                    continue;
                }
                out.insert(k.clone(), strict_schema(v));
            }
            if !moved.is_empty() {
                let note = format!("Constraint: {}.", moved.join(", "));
                let desc = match out.get("description").and_then(Value::as_str) {
                    Some(d) => format!("{d} {note}"),
                    None => note,
                };
                out.insert("description".into(), Value::String(desc));
            }
            let is_object = map.get("type").and_then(Value::as_str) == Some("object")
                || map.get("properties").is_some_and(Value::is_object);
            if is_object && !out.contains_key("additionalProperties") {
                out.insert("additionalProperties".into(), Value::Bool(false));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(strict_schema).collect()),
        other => other.clone(),
    }
}

/// Build the Messages API body. `history` holds earlier turns of a warm API session; the
/// new prompt is appended as the last user message.
pub fn build_body(req: &EngineRequest, history: &[Turn]) -> Value {
    let mut messages: Vec<Value> = history
        .iter()
        .map(|t| json!({"role": t.role, "content": t.text}))
        .collect();
    messages.push(json!({"role": "user", "content": req.prompt}));
    let mut output_config = json!({"effort": effort_str(req.effort)});
    if let Some(schema) = &req.json_schema {
        output_config["format"] = json!({"type": "json_schema", "schema": strict_schema(schema)});
    }
    let mut body = json!({
        "model": model_id(req.tier),
        "max_tokens": max_tokens_for(req.role),
        "system": [{
            "type": "text",
            "text": req.system,
            "cache_control": {"type": "ephemeral"}
        }],
        "messages": messages,
        "output_config": output_config,
    });
    if let Some(t) = thinking_param(req.role, req.tier, req.effort) {
        body["thinking"] = t;
    }
    body
}

pub fn build_headers(api_key: &str) -> Vec<(String, String)> {
    vec![
        ("x-api-key".into(), api_key.into()),
        ("anthropic-version".into(), ANTHROPIC_VERSION.into()),
        ("content-type".into(), "application/json".into()),
    ]
}

/// A successful API reply.
#[derive(Debug, Clone)]
pub struct ApiOutcome {
    pub text: String,
    pub usage: Usage,
    pub cost_usd: f64,
    pub model: String,
    pub duration_ms: u64,
}

/// Classify a response. `now_ms` turns a `retry-after` header into a reset time.
pub fn parse_response(
    tier: ModelTier,
    resp: &HttpResponse,
    now_ms: i64,
) -> Result<(String, Usage, f64), Failure> {
    let v: Value = serde_json::from_str(&resp.body).unwrap_or(Value::Null);
    if !(200..300).contains(&resp.status) {
        let mut s = Signals {
            api_status: Some(resp.status),
            ..Default::default()
        };
        if let Some(t) = v["error"].get("type").and_then(Value::as_str) {
            s.error_codes.push(t.to_string());
        }
        s.error_text = v["error"]
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut f = limits::classify(&s);
        if let Failure::RateLimited { reset_at_ms, .. } = &mut f {
            if let Some(secs) = resp
                .header("retry-after")
                .and_then(|h| h.trim().parse::<f64>().ok())
            {
                *reset_at_ms = Some(now_ms + (secs * 1000.0) as i64);
            }
        }
        return Err(f);
    }
    if v.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
        return Err(Failure::Fatal(EngineError::Refused {
            reason: v["stop_details"]
                .get("explanation")
                .and_then(Value::as_str)
                .unwrap_or("refusal")
                .to_string(),
        }));
    }
    let Some(blocks) = v.get("content").and_then(Value::as_array) else {
        return Err(Failure::Fatal(EngineError::Parse {
            detail: "messages api reply has no content".into(),
        }));
    };
    let text = blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    let usage = usage_from(&v["usage"]);
    let cost = estimate_cost_usd(tier, &usage);
    Ok((text, usage, cost))
}

/// The API fallback backend. The key is read from the named env var at call time and only
/// ever placed in the request header.
pub struct AnthropicApi {
    pub key_env: String,
    pub base_url: String,
    pub transport: Arc<dyn HttpTransport>,
}

impl AnthropicApi {
    pub fn new(key_env: String, base_url: String, transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            key_env,
            base_url,
            transport,
        }
    }

    pub fn key_present(&self) -> bool {
        std::env::var(&self.key_env)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    }

    pub async fn complete(
        &self,
        req: &EngineRequest,
        history: &[Turn],
        now_ms: i64,
    ) -> Result<ApiOutcome, Failure> {
        let key = std::env::var(&self.key_env).unwrap_or_default();
        if key.trim().is_empty() {
            return Err(Failure::Auth {
                detail: format!("env var {} is not set", self.key_env),
            });
        }
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let body = build_body(req, history);
        let started = Instant::now();
        let resp = self
            .transport
            .post_json(
                &url,
                &build_headers(key.trim()),
                &body,
                Duration::from_millis(req.timeout_ms),
            )
            .await
            .map_err(|e| match e {
                EngineError::Timeout { .. } => Failure::Fatal(e),
                other => Failure::Transient {
                    detail: other.to_string(),
                },
            })?;
        let (text, usage, cost_usd) = parse_response(req.tier, &resp, now_ms)?;
        Ok(ApiOutcome {
            text,
            usage,
            cost_usd,
            model: model_id(req.tier).to_string(),
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    pub fn req(role: Role, tier: ModelTier, effort: Effort) -> EngineRequest {
        EngineRequest {
            role,
            tier,
            effort,
            system: "You are Malakor.".into(),
            prompt: "chat: hello".into(),
            allowed_tools: vec![],
            timeout_ms: 1000,
            json_schema: None,
            correlation_id: None,
        }
    }

    #[test]
    fn core_thread_body_disables_thinking_and_caches_system() {
        let b = build_body(&req(Role::CoreThread, ModelTier::Haiku, Effort::Low), &[]);
        assert_eq!(b["model"], "claude-haiku-5-5");
        assert_eq!(b["thinking"], json!({"type": "disabled"}));
        assert_eq!(b["output_config"]["effort"], "low");
        assert_eq!(
            b["system"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );
        assert_eq!(b["system"][0]["text"], "You are Malakor.");
        assert_eq!(
            b["messages"],
            json!([{"role": "user", "content": "chat: hello"}])
        );
        assert_eq!(b["max_tokens"], 4096);
    }

    #[test]
    fn thinking_rules_per_tier_and_role() {
        let p = |r, t, e| thinking_param(r, t, e);
        assert_eq!(
            p(Role::ChatPicker, ModelTier::Sonnet, Effort::Low),
            Some(json!({"type": "between_tools"}))
        );
        assert_eq!(p(Role::CoreThread, ModelTier::Opus, Effort::Low), None);
        assert_eq!(p(Role::CoreThread, ModelTier::Haiku, Effort::Max), None);
        assert_eq!(p(Role::Planner, ModelTier::Haiku, Effort::Low), None);
        let b = build_body(&req(Role::Planner, ModelTier::Sonnet, Effort::Medium), &[]);
        assert!(b.get("thinking").is_none());
        assert_eq!(b["model"], "claude-sonnet-5-5");
        let b = build_body(&req(Role::Reconciler, ModelTier::Opus, Effort::High), &[]);
        assert_eq!(b["model"], "claude-opus-5-5");
    }

    #[test]
    fn schema_and_history_are_carried() {
        let mut r = req(Role::Judge, ModelTier::Sonnet, Effort::Medium);
        r.json_schema = Some(json!({"type": "object"}));
        let hist = [
            Turn {
                role: "user",
                text: "a".into(),
            },
            Turn {
                role: "assistant",
                text: "b".into(),
            },
        ];
        let b = build_body(&r, &hist);
        assert_eq!(
            b["output_config"]["format"],
            json!({"type": "json_schema", "schema": {"type": "object", "additionalProperties": false}})
        );
        assert_eq!(b["messages"].as_array().unwrap().len(), 3);
        assert_eq!(b["messages"][1]["role"], "assistant");
    }

    #[test]
    fn strict_schema_closes_every_object_and_keeps_an_explicit_choice() {
        let s = json!({
            "type": "object",
            "properties": {
                "items": {"type": "array", "items": {"type": "object", "properties": {"a": {"type": "string"}}}},
                "pick": {"anyOf": [{"type": "object", "properties": {}}, {"type": "null"}]},
                "open": {"type": "object", "additionalProperties": false},
                "type": {"type": "string"}
            },
            "$defs": {"d": {"properties": {"x": {"type": "integer"}}}}
        });
        let out = strict_schema(&s);
        assert_eq!(out["additionalProperties"], false);
        assert_eq!(
            out["properties"]["items"]["items"]["additionalProperties"],
            false
        );
        assert_eq!(
            out["properties"]["pick"]["anyOf"][0]["additionalProperties"],
            false
        );
        assert!(out["properties"]["pick"]["anyOf"][1]
            .get("additionalProperties")
            .is_none());
        assert_eq!(out["properties"]["open"]["additionalProperties"], false);
        assert!(out["properties"]["type"]
            .get("additionalProperties")
            .is_none());
        assert_eq!(out["$defs"]["d"]["additionalProperties"], false);
        // the properties map itself is not a schema and gains nothing
        assert!(out["properties"].get("additionalProperties").is_none());
    }

    #[test]
    fn strict_schema_moves_refused_constraints_into_the_description() {
        let s = json!({
            "type": "object",
            "properties": {
                "score": {"type": "integer", "minimum": 0, "maximum": 10, "description": "Laugh score."},
                "tags": {"type": "array", "items": {"type": "string", "maxLength": 40}, "minItems": 2, "maxItems": 5},
                "one": {"type": "array", "items": {"type": "string"}, "minItems": 1},
                "maximum": {"type": "string"}
            }
        });
        let out = strict_schema(&s);
        let score = &out["properties"]["score"];
        assert!(score.get("minimum").is_none() && score.get("maximum").is_none());
        assert_eq!(
            score["description"],
            "Laugh score. Constraint: maximum 10, minimum 0."
        );
        let tags = &out["properties"]["tags"];
        assert!(tags.get("minItems").is_none() && tags.get("maxItems").is_none());
        assert_eq!(tags["description"], "Constraint: maxItems 5, minItems 2.");
        assert!(tags["items"].get("maxLength").is_none());
        assert_eq!(out["properties"]["one"]["minItems"], 1);
        // a property that happens to be named like a constraint is a schema, not a constraint
        assert_eq!(out["properties"]["maximum"]["type"], "string");
    }

    #[test]
    fn headers_carry_version() {
        let h = build_headers("test-key");
        assert!(h.contains(&("anthropic-version".into(), "2023-06-01".into())));
        assert!(h.contains(&("x-api-key".into(), "test-key".into())));
    }

    #[test]
    fn parses_success_and_rate_limit_responses() {
        let ok = HttpResponse {
            status: 200,
            headers: vec![],
            body: r#"{"content":[{"type":"text","text":"Kneel."}],"stop_reason":"end_turn",
                "usage":{"input_tokens":10,"output_tokens":2,"cache_read_input_tokens":100}}"#
                .into(),
        };
        let (text, usage, cost) = parse_response(ModelTier::Haiku, &ok, 0).unwrap();
        assert_eq!(text, "Kneel.");
        assert_eq!(usage.cache_read_tokens, 100);
        assert!(cost > 0.0);
        let limited = HttpResponse {
            status: 429,
            headers: vec![("retry-after".into(), "30".into())],
            body: r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#
                .into(),
        };
        match parse_response(ModelTier::Haiku, &limited, 1_000) {
            Err(Failure::RateLimited { reset_at_ms, .. }) => {
                assert_eq!(reset_at_ms, Some(31_000))
            }
            other => panic!("{other:?}"),
        }
    }

    /// Records requests; answers with a canned response. Never touches the network.
    pub struct StubTransport {
        pub response: Mutex<Option<HttpResponse>>,
        pub seen: Mutex<Vec<(String, Value)>>,
    }

    impl StubTransport {
        pub fn new(resp: HttpResponse) -> Self {
            Self {
                response: Mutex::new(Some(resp)),
                seen: Mutex::new(vec![]),
            }
        }
        pub fn ok(text: &str) -> Self {
            Self::new(HttpResponse {
                status: 200,
                headers: vec![],
                body: json!({"content":[{"type":"text","text":text}],"stop_reason":"end_turn",
                    "usage":{"input_tokens":5,"output_tokens":1}})
                .to_string(),
            })
        }
    }

    #[async_trait]
    impl HttpTransport for StubTransport {
        async fn post_json(
            &self,
            url: &str,
            headers: &[(String, String)],
            body: &Value,
            _timeout: Duration,
        ) -> Result<HttpResponse, EngineError> {
            assert!(headers.iter().any(|(k, _)| k == "x-api-key"));
            self.seen.lock().unwrap().push((url.into(), body.clone()));
            Ok(self
                .response
                .lock()
                .unwrap()
                .clone()
                .expect("stub response"))
        }
    }
}
