//! A warm-session open failure or death is on the record as a `session` event, and the beat
//! still completes through the cold fallback. No test spends quota.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use shadowrunr_types::{
    Engine, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport, SegmentContext,
    SegmentId, SessionEventKind, StreamEvent, WarmSession,
};

use crate::tests::malakor_dir;
use crate::*;

fn reply() -> EngineReply {
    EngineReply {
        text: "Bow before me, minions.".into(),
        engine: EngineKind::ClaudeCli,
        account: None,
        model: "fake".into(),
        first_text_ms: Some(42),
        duration_ms: 1,
        usage: Default::default(),
        cost_usd: None,
        fallback_reason: None,
        session_id: None,
    }
}

/// Its first warm open fails and its first warm send dies; everything else answers.
#[derive(Default)]
struct Flaky {
    opens: AtomicUsize,
    sends: Arc<AtomicUsize>,
    cold: AtomicUsize,
}

struct FlakySession {
    sends: Arc<AtomicUsize>,
}

#[async_trait]
impl WarmSession for FlakySession {
    async fn send(&mut self, _prompt: String) -> Result<EngineReply, EngineError> {
        if self.sends.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(EngineError::Io {
                detail: "pipe closed: SECRET-PROMPT-TEXT".into(),
            });
        }
        Ok(reply())
    }
    async fn interrupt(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn close(self: Box<Self>) -> Result<(), EngineError> {
        Ok(())
    }
}

#[async_trait]
impl Engine for Flaky {
    async fn complete(&self, _req: EngineRequest) -> Result<EngineReply, EngineError> {
        self.cold.fetch_add(1, Ordering::SeqCst);
        Ok(reply())
    }
    async fn open_session(
        &self,
        _template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError> {
        if self.opens.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(EngineError::Unavailable {
                reason: "all claude cli accounts cooling".into(),
            });
        }
        Ok(Box::new(FlakySession {
            sends: self.sends.clone(),
        }))
    }
    async fn health(&self) -> Vec<HealthReport> {
        Vec::new()
    }
}

#[tokio::test]
async fn a_failed_open_and_a_dead_send_each_leave_one_session_event() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(Flaky::default());
    let cfg = BrainConfig {
        character: load_character(&malakor_dir(), "theatrical").unwrap(),
        memory_dir: dir.path().to_path_buf(),
        stream_id: "s1".into(),
        core_tier: shadowrunr_types::ModelTier::Haiku,
        pool_size: 2,
    };
    let t = BrainTuning {
        policy: PolicySource::Builtin,
        warm_session: true,
        warm_recycle_turns: 0,
        ..BrainTuning::default()
    };
    let mut b = Brain::new_with(cfg, engine.clone(), t).await.unwrap();
    let c = SegmentContext {
        segment: SegmentId::Seg01ImprovisedRoom,
        rules_prompt: "SEG-01 Improvised Room: riff with chat.".into(),
        state_summary: "No poll open. No foil on stage.".into(),
        state: "opening".into(),
        allowed_ops: Vec::new(),
        ccv: 30,
        elapsed_s: 120,
        hexes: Vec::new(),
    };
    let beats = 15;
    let mut all = Vec::new();
    for i in 0..beats {
        let ev = b.tick(i * 5_000, Vec::new(), &c).await.unwrap();
        all.extend(ev);
    }
    let sessions: Vec<(SessionEventKind, Option<String>)> = all
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Session { kind, detail, .. } => Some((*kind, detail.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(sessions.len(), 2, "{sessions:?}");
    assert_eq!(sessions[0].0, SessionEventKind::OpenFailed);
    assert_eq!(sessions[1].0, SessionEventKind::Died);
    for (_, detail) in &sessions {
        let d = detail.as_deref().unwrap();
        assert!(!d.contains("SECRET-PROMPT-TEXT"), "{d}");
    }
    // Every beat was answered: the open-failed beat waits out the retry back-off cold, the
    // dead-send beat falls back cold, and the rest run warm.
    let cold = engine.cold.load(Ordering::SeqCst);
    let warm_ok = engine.sends.load(Ordering::SeqCst) - 1;
    assert!(cold >= 2 && warm_ok >= 1, "cold {cold}, warm {warm_ok}");
    assert_eq!(cold + warm_ok, beats as usize);
}
