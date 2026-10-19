//! Case-file research: the bone-clerks of a fictional case search its planted records, not the
//! web.
//!
//! A fictional case has nothing to find online (and a web search on invented names can only
//! surface someone real), so research on it used to come back empty and Malakor looped on the
//! fog. [`CaseFileResearch`] wraps the brain's engine and answers every `Role::Researcher`
//! request from the case's planted records ([`shadowrunr_segments::research::report`]); every
//! other role goes to the inner engine untouched. It is keyless, costs nothing and is never
//! written to the ledger, because it is not an engine call.
//!
//! The segment runtime stays the referee of the re-search cap: it refuses the attempt past
//! `max_searches_per_hex` and opens the Plebiscite. The layer keeps its own per-hex count only
//! to word that refused attempt's report (the brain dispatches a job before the runtime sees
//! the op). Both count briefs in dispatch order, so they agree, except when one reply sends two
//! briefs for the same hex across the cap: those jobs may reach the layer in either order.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use shadowrunr_segments::research::report;
use shadowrunr_segments::{segment_table, target_hex, CaseFile, SegmentRuntime};
use shadowrunr_types::{
    Case, Engine, EngineError, EngineKind, EngineReply, EngineRequest, HealthReport, Role,
    SegmentId, Usage, WarmSession,
};

/// Model label on case-file replies.
const MODEL: &str = "case-file";

pub struct CaseFileResearch {
    inner: Arc<dyn Engine>,
    case: Case,
    file: CaseFile,
    cap: u32,
    /// hex id -> research briefs answered with records (or "nothing found") so far.
    searches: Mutex<BTreeMap<String, u32>>,
}

impl CaseFileResearch {
    pub fn new(inner: Arc<dyn Engine>, case: Case, file: CaseFile, cap: u32) -> Self {
        Self {
            inner,
            case,
            file,
            cap,
            searches: Mutex::new(BTreeMap::new()),
        }
    }

    /// `engine` behind a case-file layer when the stage's case has planted records, with the
    /// SEG-02A table's cap; `engine` unchanged otherwise.
    pub fn wrap(engine: Arc<dyn Engine>, stage: &SegmentRuntime) -> Arc<dyn Engine> {
        let Some(file) = stage.case_file() else {
            return engine;
        };
        let cap = segment_table(SegmentId::Seg02aHexmap)
            .map(|t| t.clock.max_searches_per_hex)
            .unwrap_or(shadowrunr_segments::DEFAULT_MAX_SEARCHES_PER_HEX);
        Arc::new(Self::new(engine, stage.case().clone(), file.clone(), cap))
    }

    /// The bone-clerks' report on `brief`, counting the search against its hex.
    pub fn answer(&self, brief: &str) -> String {
        let mut searches = self.searches.lock().unwrap_or_else(|p| p.into_inner());
        let hex = target_hex(&self.case, brief).map(|i| self.case.hexes[i].id.clone());
        let prior = hex
            .as_ref()
            .and_then(|h| searches.get(h).copied())
            .unwrap_or(0);
        let text = report(&self.case, &self.file, brief, prior, self.cap);
        if let Some(h) = hex.filter(|_| prior < self.cap) {
            *searches.entry(h).or_default() += 1;
        }
        text
    }
}

#[async_trait]
impl Engine for CaseFileResearch {
    async fn complete(&self, req: EngineRequest) -> Result<EngineReply, EngineError> {
        if req.role != Role::Researcher {
            return self.inner.complete(req).await;
        }
        let started = Instant::now();
        let text = self.answer(&req.prompt);
        Ok(EngineReply {
            text,
            engine: EngineKind::Scripted,
            account: None,
            model: MODEL.into(),
            first_text_ms: None,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            usage: Usage::default(),
            cost_usd: None,
            fallback_reason: None,
            session_id: None,
        })
    }

    async fn open_session(
        &self,
        template: EngineRequest,
    ) -> Result<Box<dyn WarmSession>, EngineError> {
        self.inner.open_session(template).await
    }

    async fn health(&self) -> Vec<HealthReport> {
        self.inner.health().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_engine::ScriptedEngine;
    use shadowrunr_segments::SegmentConfig;

    fn request(role: Role, prompt: &str) -> EngineRequest {
        let (tier, effort, timeout_ms) = shadowrunr_engine::route(role);
        EngineRequest {
            role,
            tier,
            effort,
            system: String::new(),
            prompt: prompt.into(),
            allowed_tools: Vec::new(),
            timeout_ms,
            json_schema: None,
            correlation_id: None,
        }
    }

    fn demo_stage() -> SegmentRuntime {
        SegmentRuntime::with_config(SegmentId::Seg02aHexmap, SegmentConfig::default()).unwrap()
    }

    #[tokio::test]
    async fn research_on_the_demo_case_reads_the_case_file() {
        let engine = CaseFileResearch::wrap(Arc::new(ScriptedEngine), &demo_stage());
        let r = engine
            .complete(request(Role::Researcher, "HEX-01: who runs the syndicate"))
            .await
            .unwrap();
        assert_eq!(r.model, MODEL);
        assert!(r.text.contains("DOC-01 [DOC]"), "{}", r.text);
        assert!(r.text.contains("REP-01 [REP]"), "{}", r.text);
        assert!(r.text.contains("FICTIONAL"), "{}", r.text);
        let r = engine
            .complete(request(Role::Researcher, "HEX-16: the Starfen Ledger crew"))
            .await
            .unwrap();
        assert!(r.text.contains("found nothing on record"), "{}", r.text);
        // Every other role reaches the inner engine.
        let r = engine
            .complete(request(Role::CoreThread, "hello"))
            .await
            .unwrap();
        assert_eq!(r.model, "scripted");
    }

    #[tokio::test]
    async fn the_third_search_on_a_hex_reports_the_plebiscite() {
        let engine = CaseFileResearch::wrap(Arc::new(ScriptedEngine), &demo_stage());
        for _ in 0..2 {
            let r = engine
                .complete(request(Role::Researcher, "HEX-07: moon maintenance"))
                .await
                .unwrap();
            assert!(r.text.contains("CLM-03"), "{}", r.text);
        }
        let r = engine
            .complete(request(Role::Researcher, "hex 7 one more time"))
            .await
            .unwrap();
        assert!(r.text.contains("Plebiscite"), "{}", r.text);
        assert!(!r.text.contains("CLM-03"), "{}", r.text);
    }

    #[test]
    fn a_case_without_planted_records_is_not_wrapped() {
        let mut case = shadowrunr_segments::demo_case();
        case.id = "CUSTOM-01".into();
        let stage = SegmentRuntime::with_config(
            SegmentId::Seg02aHexmap,
            SegmentConfig {
                case: Some(case),
                ..SegmentConfig::default()
            },
        )
        .unwrap();
        let inner: Arc<dyn Engine> = Arc::new(ScriptedEngine);
        let wrapped = CaseFileResearch::wrap(inner.clone(), &stage);
        assert!(Arc::ptr_eq(&inner, &wrapped));
    }

    #[test]
    fn answers_are_deterministic_for_the_same_briefs() {
        let run = || {
            let layer = CaseFileResearch::new(
                Arc::new(ScriptedEngine),
                shadowrunr_segments::demo_case(),
                CaseFile::demo(),
                2,
            );
            ["HEX-04", "hex4", "HEX-04 again", "demo-0001", "nothing"]
                .iter()
                .map(|b| layer.answer(b))
                .collect::<Vec<_>>()
        };
        let a = run();
        assert_eq!(a, run());
        assert!(a[2].contains("Plebiscite"), "{}", a[2]);
    }
}
