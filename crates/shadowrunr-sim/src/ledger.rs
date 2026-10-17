//! Per-run engine ledger: a wrapper `Engine` that appends one JSONL row per call (success or
//! failure) to `<run>/ledger.jsonl`. Never records prompts, replies or credentials.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use shadowrunr_types::{
    Effort, Engine, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport, ModelTier,
    Role, Usage, WarmSession,
};

use crate::SimError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRow {
    pub ts_unix_ms: i64,
    pub role: Role,
    pub tier: ModelTier,
    pub effort: Effort,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<EngineKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_text_ms: Option<u64>,
    pub duration_ms: u64,
    #[serde(default)]
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// `EngineError` kind when the call failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    /// True when the call went through a warm session.
    #[serde(default)]
    pub session: bool,
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

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

#[derive(Clone)]
struct Sink {
    file: Arc<Mutex<File>>,
    correlation_id: String,
}

impl Sink {
    fn write(
        &self,
        meta: (Role, ModelTier, Effort, Option<String>),
        started: Instant,
        res: &Result<EngineReply, EngineError>,
        session: bool,
    ) {
        let (role, tier, effort, corr) = meta;
        let mut row = LedgerRow {
            ts_unix_ms: now_unix_ms(),
            role,
            tier,
            effort,
            engine: None,
            account: None,
            model: None,
            first_text_ms: None,
            duration_ms: started.elapsed().as_millis() as u64,
            usage: Usage::default(),
            cost_usd: None,
            fallback_reason: None,
            correlation_id: Some(corr.unwrap_or_else(|| self.correlation_id.clone())),
            error_kind: None,
            session,
        };
        match res {
            Ok(r) => {
                row.engine = Some(r.engine);
                row.account = r.account.clone();
                row.model = Some(r.model.clone());
                row.first_text_ms = r.first_text_ms;
                // The engine's own figure wins; wall-clock covers engines that report 0.
                if r.duration_ms > 0 {
                    row.duration_ms = r.duration_ms;
                }
                row.usage = r.usage.clone();
                row.cost_usd = r.cost_usd;
                row.fallback_reason = r.fallback_reason.clone();
            }
            Err(e) => row.error_kind = Some(error_kind(e).to_string()),
        }
        if let Ok(line) = serde_json::to_string(&row) {
            if let Ok(mut f) = self.file.lock() {
                let _ = writeln!(f, "{line}");
            }
        }
    }
}

/// Wraps an engine so every call lands in this run's ledger.
pub struct LedgerEngine {
    inner: Arc<dyn Engine>,
    sink: Sink,
    path: PathBuf,
}

impl LedgerEngine {
    pub fn create(
        inner: Arc<dyn Engine>,
        path: &Path,
        correlation_id: &str,
    ) -> Result<Self, SimError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| SimError::Io(format!("{}: {e}", path.display())))?;
        Ok(Self {
            inner,
            sink: Sink {
                file: Arc::new(Mutex::new(file)),
                correlation_id: correlation_id.to_string(),
            },
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn meta(req: &EngineRequest) -> (Role, ModelTier, Effort, Option<String>) {
    (req.role, req.tier, req.effort, req.correlation_id.clone())
}

#[async_trait]
impl Engine for LedgerEngine {
    async fn complete(&self, mut req: EngineRequest) -> Result<EngineReply, EngineError> {
        if req.correlation_id.is_none() {
            req.correlation_id = Some(self.sink.correlation_id.clone());
        }
        let m = meta(&req);
        let started = Instant::now();
        let res = self.inner.complete(req).await;
        self.sink.write(m, started, &res, false);
        res
    }

    async fn open_session(
        &self,
        mut template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError> {
        if template.correlation_id.is_none() {
            template.correlation_id = Some(self.sink.correlation_id.clone());
        }
        let m = meta(&template);
        let inner = self.inner.open_session(template).await?;
        Ok(Box::new(LedgeredSession {
            inner,
            meta: m,
            sink: self.sink.clone(),
        }))
    }

    async fn health(&self) -> Vec<HealthReport> {
        self.inner.health().await
    }
}

struct LedgeredSession {
    inner: Box<dyn WarmSession>,
    meta: (Role, ModelTier, Effort, Option<String>),
    sink: Sink,
}

#[async_trait]
impl WarmSession for LedgeredSession {
    async fn send(&mut self, prompt: String) -> Result<EngineReply, EngineError> {
        let started = Instant::now();
        let res = self.inner.send(prompt).await;
        self.sink.write(self.meta.clone(), started, &res, true);
        res
    }
    async fn interrupt(&mut self) -> Result<(), EngineError> {
        self.inner.interrupt().await
    }
    async fn close(self: Box<Self>) -> Result<(), EngineError> {
        self.inner.close().await
    }
}

pub fn read(path: &Path) -> Result<Vec<LedgerRow>, SimError> {
    let f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(SimError::Io(format!("{}: {e}", path.display()))),
    };
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line.map_err(|e| SimError::Io(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        // Rows from a newer writer with extra fields still parse; broken lines are skipped.
        if let Ok(row) = serde_json::from_str::<LedgerRow>(&line) {
            out.push(row);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_engine::ScriptedEngine;

    fn req(role: Role) -> EngineRequest {
        EngineRequest {
            role,
            tier: ModelTier::Haiku,
            effort: Effort::Low,
            system: "s".into(),
            prompt: "p".into(),
            allowed_tools: vec![],
            timeout_ms: 1000,
            json_schema: None,
            correlation_id: None,
        }
    }

    #[tokio::test]
    async fn rows_for_complete_and_session() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ledger.jsonl");
        let eng = LedgerEngine::create(Arc::new(ScriptedEngine), &p, "run-1").unwrap();
        eng.complete(req(Role::Chatter)).await.unwrap();
        let mut s = eng.open_session(req(Role::CoreThread)).await.unwrap();
        s.send("hi".into()).await.unwrap();
        let rows = read(&p).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].role, Role::Chatter);
        assert_eq!(rows[0].correlation_id.as_deref(), Some("run-1"));
        assert!(rows[1].session);
        assert_eq!(rows[1].first_text_ms, Some(0));
        let raw = std::fs::read_to_string(&p).unwrap();
        assert!(
            !raw.contains("null"),
            "optional fields are omitted, never null"
        );
    }
}
