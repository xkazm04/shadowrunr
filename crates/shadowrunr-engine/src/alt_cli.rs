//! EXPERIMENTAL codex / agy drivers: argv builders and output parsers, modelled on
//! `ai-registry/skills/contest/scripts/lib/participants.mjs`. Enabled only when named in
//! `EngineConfig::cli_fallbacks`. Unlike the contest runner, these never pass an
//! approval-bypass flag: codex runs with `--sandbox read-only`, agy with `--mode plan
//! --sandbox`, so a fallback reply can only talk, never act.
//!
//! Flags checked against `codex exec --help` (codex-cli 0.160.0) and `agy --help` on this
//! machine, 2026-10-18.

use std::path::Path;

use serde_json::Value;
use shadowrunr_types::{Effort, EngineRequest, Usage};

use crate::limits::Failure;

/// A ready-to-spawn command: full argv (head included), stdin text, extra env.
#[derive(Debug, Clone, PartialEq)]
pub struct AltCommand {
    pub argv: Vec<String>,
    pub stdin: String,
    pub env: Vec<(String, String)>,
}

/// Neither CLI has a system-prompt flag in headless mode; the stable prefix rides first.
pub fn fold_system(req: &EngineRequest) -> String {
    if req.system.trim().is_empty() {
        req.prompt.clone()
    } else {
        format!(
            "<system>\n{}\n</system>\n\n{}",
            req.system.trim_end(),
            req.prompt
        )
    }
}

/// codex reasoning effort: it has no `max`, so `max` maps to `xhigh`.
pub fn codex_effort(e: Effort) -> &'static str {
    match e {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh | Effort::Max => "xhigh",
    }
}

/// `codex exec --json ... -` with the prompt on stdin.
pub fn codex_command(
    head: &[String],
    model: &str,
    req: &EngineRequest,
    workspace: &Path,
) -> AltCommand {
    let mut argv: Vec<String> = head.to_vec();
    argv.extend(
        [
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--ephemeral",
            "--ignore-user-config",
            "--ignore-rules",
            "--sandbox",
            "read-only",
            "-C",
        ]
        .map(String::from),
    );
    argv.push(workspace.to_string_lossy().into_owned());
    argv.extend(["-m".into(), model.to_string()]);
    argv.extend([
        "-c".into(),
        format!("model_reasoning_effort=\"{}\"", codex_effort(req.effort)),
        "-".into(),
    ]);
    AltCommand {
        argv,
        stdin: fold_system(req),
        env: vec![],
    }
}

/// agy model slugs carry the effort (`gemini-3.8-flash-medium`); `xhigh`/`max` map to `high`.
pub fn agy_model(model: &str, effort: Effort) -> String {
    let tier = match effort {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High | Effort::Xhigh | Effort::Max => "high",
    };
    if ["-low", "-medium", "-high"]
        .iter()
        .any(|s| model.ends_with(s))
    {
        model.to_string()
    } else {
        format!("{model}-{tier}")
    }
}

/// `agy -p <prompt> ...`. agy blocks on an open stdin pipe on Windows, so the prompt rides as
/// the argument and stdin is closed empty.
pub fn agy_command(head: &[String], model: &str, req: &EngineRequest) -> AltCommand {
    let mut argv: Vec<String> = head.to_vec();
    argv.extend(["-p".into(), fold_system(req)]);
    argv.extend(["--model".into(), agy_model(model, req.effort)]);
    argv.extend(
        [
            "--output-format",
            "json",
            "--mode",
            "plan",
            "--sandbox",
            "--disable-slash-commands",
        ]
        .map(String::from),
    );
    argv.extend([
        "--print-timeout".into(),
        format!("{}s", req.timeout_ms.div_ceil(1000).max(1)),
    ]);
    AltCommand {
        argv,
        stdin: String::new(),
        env: vec![],
    }
}

#[derive(Debug, Clone, Default)]
pub struct AltParsed {
    pub text: String,
    pub errors: Vec<String>,
    pub usage: Usage,
}

fn alt_usage(v: &Value) -> Usage {
    let g = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        input_tokens: g("input_tokens"),
        output_tokens: g("output_tokens"),
        cache_read_tokens: g("cached_input_tokens"),
        cache_creation_tokens: 0,
    }
}

/// codex `--json` prints JSONL events; the reply is the last `agent_message` item.
pub fn parse_codex(stdout: &str) -> AltParsed {
    let mut out = AltParsed::default();
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "item.completed" => {
                let it = &v["item"];
                if it.get("type").and_then(Value::as_str) == Some("agent_message") {
                    if let Some(t) = it.get("text").and_then(Value::as_str) {
                        out.text = t.to_string();
                    }
                }
            }
            "turn.completed" => out.usage = alt_usage(&v["usage"]),
            "error" | "turn.failed" => {
                let msg = v
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| v["error"].get("message").and_then(Value::as_str));
                if let Some(m) = msg {
                    if !m.starts_with("Reconnecting") {
                        out.errors.push(m.chars().take(300).collect());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The last parseable JSON object in the text (a banner line before it is tolerated).
fn last_json_object(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    if let Ok(v) = serde_json::from_str(&text[start..]) {
        return Some(v);
    }
    let mut idx: Vec<usize> = text.match_indices('{').map(|(i, _)| i).collect();
    idx.reverse();
    idx.into_iter()
        .find_map(|i| serde_json::from_str(&text[i..]).ok())
}

/// agy prints one object: `{status: SUCCESS|ERROR|..., response, error, usage}`.
pub fn parse_agy(stdout: &str) -> AltParsed {
    let Some(v) = last_json_object(stdout) else {
        return AltParsed {
            errors: vec!["no JSON envelope on stdout".into()],
            ..Default::default()
        };
    };
    let mut out = AltParsed {
        text: v
            .get("response")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        usage: alt_usage(&v["usage"]),
        errors: vec![],
    };
    if let Some(st) = v.get("status").and_then(Value::as_str) {
        if st != "SUCCESS" {
            let detail = v
                .get("error")
                .and_then(Value::as_str)
                .or_else(|| v.get("response").and_then(Value::as_str))
                .unwrap_or("");
            out.errors.push(format!(
                "{st}: {}",
                detail.chars().take(300).collect::<String>()
            ));
        }
    }
    out
}

/// Seat or quota limits look like this in either CLI's error text.
const SEAT_LIMIT_NEEDLES: &[&str] = &[
    "usage limit",
    "rate limit",
    "quota",
    "out of usage",
    "out of extra usage",
    "seat",
];

/// `None` when the run succeeded.
pub fn classify(parsed: &AltParsed, exit_ok: bool) -> Option<Failure> {
    if !parsed.errors.is_empty() {
        let text = parsed.errors.join(" ").to_lowercase();
        if SEAT_LIMIT_NEEDLES.iter().any(|n| text.contains(n)) {
            return Some(Failure::RateLimited {
                reset_at_ms: None,
                rule: "alt:seat-limit",
            });
        }
        return Some(Failure::Transient {
            detail: parsed.errors.join("; "),
        });
    }
    if !exit_ok || parsed.text.is_empty() {
        return Some(Failure::Transient {
            detail: "alt cli produced no reply".into(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::{ModelTier, Role};

    fn req(effort: Effort) -> EngineRequest {
        EngineRequest {
            role: Role::CoreThread,
            tier: ModelTier::Haiku,
            effort,
            system: "SYS".into(),
            prompt: "hi".into(),
            allowed_tools: vec![],
            timeout_ms: 30_500,
            json_schema: None,
            correlation_id: None,
        }
    }

    #[test]
    fn codex_argv_is_headless_and_never_bypasses() {
        let c = codex_command(
            &["node".into(), "codex.js".into()],
            "gpt-5.6-sol",
            &req(Effort::Max),
            Path::new("W"),
        );
        assert_eq!(
            c.argv,
            [
                "node",
                "codex.js",
                "exec",
                "--json",
                "--skip-git-repo-check",
                "--ephemeral",
                "--ignore-user-config",
                "--ignore-rules",
                "--sandbox",
                "read-only",
                "-C",
                "W",
                "-m",
                "gpt-5.6-sol",
                "-c",
                "model_reasoning_effort=\"xhigh\"",
                "-"
            ]
        );
        assert!(c.stdin.starts_with("<system>\nSYS\n</system>"));
        assert!(!c.argv.iter().any(|a| a.contains("dangerously")));
    }

    #[test]
    fn agy_argv_folds_effort_into_model() {
        let c = agy_command(&["agy".into()], "gemini-3.8-flash", &req(Effort::Xhigh));
        assert_eq!(c.argv[0], "agy");
        assert_eq!(c.argv[1], "-p");
        let joined = c.argv.join(" ");
        assert!(joined.contains("--model gemini-3.8-flash-high"));
        assert!(joined.contains("--output-format json --mode plan --sandbox"));
        assert!(joined.contains("--print-timeout 31s"));
        assert!(!joined.contains("dangerously"));
        assert_eq!(c.stdin, "");
        assert_eq!(
            agy_model("gemini-3.8-flash-low", Effort::High),
            "gemini-3.8-flash-low"
        );
    }

    #[test]
    fn parses_codex_events() {
        let out = parse_codex(concat!(
            "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n",
            "{\"type\":\"error\",\"message\":\"Reconnecting... 1/5\"}\n",
            "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"Bow.\"}}\n",
            "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":9,\"cached_input_tokens\":3,\"output_tokens\":2}}\n"
        ));
        assert_eq!(out.text, "Bow.");
        assert!(out.errors.is_empty());
        assert_eq!(out.usage.cache_read_tokens, 3);
        assert!(classify(&out, true).is_none());
        let failed = parse_codex(
            "{\"type\":\"turn.failed\",\"error\":{\"message\":\"You've hit your usage limit\"}}",
        );
        assert!(matches!(
            classify(&failed, false),
            Some(Failure::RateLimited { .. })
        ));
    }

    #[test]
    fn parses_agy_envelopes() {
        let ok =
            parse_agy("banner\n{\"status\":\"SUCCESS\",\"response\":\"Kneel.\",\"num_turns\":1}");
        assert_eq!(ok.text, "Kneel.");
        assert!(classify(&ok, true).is_none());
        let err = parse_agy("{\"status\":\"ERROR\",\"error\":\"model unavailable\"}");
        assert_eq!(err.errors, ["ERROR: model unavailable"]);
        assert!(matches!(
            classify(&err, true),
            Some(Failure::Transient { .. })
        ));
        assert_eq!(parse_agy("").errors, ["no JSON envelope on stdout"]);
    }
}
