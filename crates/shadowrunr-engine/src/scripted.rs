//! Deterministic, keyless engine. Lets every crate test its logic and the sim harness run a
//! smoke session without spending quota. WP1 may extend it (scripted queues, latency).

use async_trait::async_trait;
use shadowrunr_types::{
    Engine, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport, HealthState, Usage,
    WarmSession,
};

#[derive(Debug, Default, Clone)]
pub struct ScriptedEngine;

fn reply(req: &EngineRequest) -> EngineReply {
    // A stable, role-tagged echo: enough for harness plumbing, useless as content by design.
    let head: String = req
        .prompt
        .chars()
        .rev()
        .take(60)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    EngineReply {
        text: format!("[scripted {:?}] {}", req.role, head),
        engine: EngineKind::Scripted,
        account: None,
        model: "scripted".into(),
        first_text_ms: Some(0),
        duration_ms: 0,
        usage: Usage::default(),
        cost_usd: None,
        fallback_reason: None,
        session_id: None,
    }
}

pub struct ScriptedSession {
    template: EngineRequest,
}

#[async_trait]
impl WarmSession for ScriptedSession {
    async fn send(&mut self, prompt: String) -> Result<EngineReply, EngineError> {
        let mut req = self.template.clone();
        req.prompt = prompt;
        Ok(reply(&req))
    }
    async fn interrupt(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn close(self: Box<Self>) -> Result<(), EngineError> {
        Ok(())
    }
}

#[async_trait]
impl Engine for ScriptedEngine {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
        Ok(reply(&req))
    }
    async fn open_session(
        &self,
        template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError> {
        Ok(Box::new(ScriptedSession { template }))
    }
    async fn health(&self) -> Vec<HealthReport> {
        vec![HealthReport {
            check: "PF-01".into(),
            state: HealthState::Green,
            detail: "scripted engine".into(),
        }]
    }
}
