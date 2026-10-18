//! `shadowrunr mcp`: a Model Context Protocol server on stdin/stdout (JSON-RPC 2.0:
//! `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`), so a
//! Personas agent (Athena) or Claude Code can read runs, transcripts and reports. The tools
//! are [`super::mcp_tools`].
//!
//! Framing is detected per message and answered in kind: newline-delimited JSON (the MCP
//! stdio transport) or `Content-Length:` headers (what the Personas MCP client writes and
//! expects, `engine/mcp_tools.rs`). stdout carries protocol messages only; diagnostics go to
//! stderr.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};

use super::mcp_tools::{EngineFactory, ToolError, Tools};
use super::settings::PersonasSettings;
use crate::config::Config;
use crate::engine::EngineHandle;

/// Protocol revisions this server speaks, newest first. A client asking for one of these
/// gets it back; any other request gets the newest.
pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// The largest message accepted.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// One JSON message per line.
    Lines,
    /// `Content-Length: N\r\n\r\n` then N bytes.
    ContentLength,
}

fn too_big() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, "message too large")
}

/// The next message and how it was framed; `None` at end of input.
pub async fn read_message<R: AsyncBufRead + Unpin>(
    r: &mut R,
) -> std::io::Result<Option<(Framing, Vec<u8>)>> {
    loop {
        let mut line = Vec::new();
        if r.read_until(b'\n', &mut line).await? == 0 {
            return Ok(None);
        }
        if line.len() > MAX_MESSAGE_BYTES {
            return Err(too_big());
        }
        let text = String::from_utf8_lossy(&line);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        let Some(len) = lower.strip_prefix("content-length:") else {
            return Ok(Some((Framing::Lines, trimmed.as_bytes().to_vec())));
        };
        let len: usize = len.trim().parse().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "bad Content-Length")
        })?;
        if len > MAX_MESSAGE_BYTES {
            return Err(too_big());
        }
        // Skip any further headers up to the blank line.
        loop {
            let mut h = Vec::new();
            if r.read_until(b'\n', &mut h).await? == 0 {
                return Ok(None);
            }
            if h.iter().all(u8::is_ascii_whitespace) {
                break;
            }
        }
        let mut body = vec![0u8; len];
        r.read_exact(&mut body).await?;
        return Ok(Some((Framing::ContentLength, body)));
    }
}

pub async fn write_message<W: AsyncWrite + Unpin>(
    w: &mut W,
    framing: Framing,
    msg: &Value,
) -> std::io::Result<()> {
    let body = serde_json::to_string(msg)?;
    match framing {
        Framing::Lines => {
            w.write_all(body.as_bytes()).await?;
            w.write_all(b"\n").await?;
        }
        Framing::ContentLength => {
            w.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
                .await?;
            w.write_all(body.as_bytes()).await?;
        }
    }
    w.flush().await
}

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

fn text_result(v: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

pub struct McpServer {
    tools: Tools,
}

impl McpServer {
    pub fn new(cfg: Config, allow_writes: bool, factory: EngineFactory) -> Self {
        Self {
            tools: Tools::new(cfg, allow_writes, factory),
        }
    }

    pub fn tools(&self) -> &Tools {
        &self.tools
    }

    fn initialize(params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str);
        let version = asked
            .filter(|v| PROTOCOL_VERSIONS.contains(v))
            .unwrap_or(PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "shadowrunr", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Shadowrunr OS: read simulated runs, transcripts, tournament results, Reconcile & Plan reports, directives and pre-flight. Paged tools return next_cursor while more follows.",
        })
    }

    /// Handle one decoded message; `None` for notifications and stray responses.
    pub async fn handle(&self, msg: &Value) -> Option<Value> {
        let Some(obj) = msg.as_object() else {
            return Some(error(
                &Value::Null,
                INVALID_REQUEST,
                "not a JSON-RPC object",
            ));
        };
        let id = obj.get("id").cloned();
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            // A response to something we never sent, or garbage with an id.
            return id.map(|id| error(&id, INVALID_REQUEST, "no method"));
        };
        let Some(id) = id else {
            // Notifications (`notifications/initialized`, `notifications/cancelled`, ...).
            return None;
        };
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        Some(match method {
            "initialize" => reply(&id, Self::initialize(&params)),
            "ping" => reply(&id, json!({})),
            "tools/list" => reply(&id, json!({ "tools": super::mcp_tools::tool_list() })),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else {
                    return Some(error(&id, INVALID_PARAMS, "tools/call needs a name"));
                };
                let args = params.get("arguments").cloned().unwrap_or(Value::Null);
                match self.tools.call(name, args).await {
                    Ok(v) => reply(&id, text_result(&v, false)),
                    Err(ToolError::Failed(e)) => reply(&id, text_result(&e.body(), true)),
                    Err(ToolError::Unknown(m)) => error(&id, INVALID_PARAMS, m),
                }
            }
            other => error(&id, METHOD_NOT_FOUND, format!("method {other:?} not found")),
        })
    }

    async fn handle_bytes(&self, body: &[u8]) -> Option<Value> {
        let msg: Value = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(e) => {
                return Some(error(
                    &Value::Null,
                    PARSE_ERROR,
                    format!("parse error: {e}"),
                ))
            }
        };
        match msg {
            Value::Array(batch) => {
                let mut out = Vec::new();
                for m in &batch {
                    if let Some(r) = self.handle(m).await {
                        out.push(r);
                    }
                }
                (!out.is_empty()).then_some(Value::Array(out))
            }
            m => self.handle(&m).await,
        }
    }

    /// Serve until the input ends.
    pub async fn serve<R, W>(&self, input: R, mut output: W) -> std::io::Result<()>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut reader = BufReader::new(input);
        while let Some((framing, body)) = read_message(&mut reader).await? {
            if let Some(resp) = self.handle_bytes(&body).await {
                write_message(&mut output, framing, &resp).await?;
            }
        }
        Ok(())
    }
}

/// `shadowrunr mcp`: serve on stdio, then let runs started here finish.
pub async fn run_stdio(cfg: Config) -> Result<(), String> {
    let settings = PersonasSettings::load(&cfg)?;
    let factory: EngineFactory = Arc::new(|c: &Config| EngineHandle::build(c, false));
    let server = McpServer::new(cfg, settings.mcp_allow_writes, factory);
    eprintln!(
        "shadowrunr mcp: serving on stdio ({})",
        if settings.mcp_allow_writes {
            "start_sim enabled"
        } else {
            "read-only"
        }
    );
    let res = server
        .serve(tokio::io::stdin(), tokio::io::stdout())
        .await
        .map_err(|e| format!("stdio: {e}"));
    server.tools().wait_idle().await;
    res
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
