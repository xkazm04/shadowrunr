//! Stream prep: the planner writes a stream's cold open, premise deck and connector lines
//! before the stream (`prep.generate`), mechanically checked and staged under
//! `<data>/prep/<prep_id>/prep.json`; the owner approves one (`prep.approve`) and streams use the
//! newest approved prep.
//!
//! - [`planner`]: the `planner` role's input, prompt and reply parsing. The plan's text is
//!   fenced as untrusted data.
//! - [`checks`]: the mechanical checks (lengths, counts, real names, links, callbacks to earlier
//!   streams, segments, duplicates, the brain's output gate). A failed check stays on the
//!   staged prep and holds its approval, with no override.
//! - [`store`]: the durable, locked store and the owner's transitions.
//! - [`cli`]: `shadowrunr prep ...`.
//!
//! Simulated runs (sims, tournaments, sandbox sessions) use the newest approved prep for their
//! character, else the character's fictional `prep.sample.json` ([`for_sim`]); each records the
//! one it used as `prep` in `run.json` or `spec.json`.

pub mod checks;
pub mod cli;
pub mod planner;
pub mod store;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use shadowrunr_brain::moderation::Policy;
use shadowrunr_reconcile::Reconciler;
use shadowrunr_types::prep::{
    PrepApproveParams, PrepCheck, PrepGenerateParams, PrepIdParams, PrepStatus, PrepSummary,
    StreamPrep,
};
use shadowrunr_types::{Engine, EngineRequest, ErrorCode, Role, TsMs};

use crate::config::Config;
use crate::rpc::RpcError;
use crate::server::AppState;
use crate::{catalog, runs};

pub use store::PrepStore;

/// `characters/<id>/prep.sample.json`: a fictional prep for sims when none is approved.
pub const SAMPLE_FILE: &str = "prep.sample.json";

/// The prep store of this config (`<data>/prep`).
pub fn store(cfg: &Config) -> PrepStore {
    PrepStore::new(&cfg.paths.data)
}

/// The checks over `p` with this config's policy and real-names list.
pub fn check(
    cfg: &Config,
    p: &StreamPrep,
    dropped: &checks::Dropped,
) -> Result<Vec<PrepCheck>, RpcError> {
    let policy: Arc<Policy> = crate::clips::policy(cfg)?;
    let real_names = checks::load_real_names(&cfg.paths.policy);
    Ok(checks::run(
        p,
        &checks::CheckContext {
            policy: &policy,
            real_names: &real_names,
        },
        dropped,
    ))
}

/// What the planner is given for `character`: its package (constitution, active tone, the
/// bible's running jokes and segment notes), the active segments and the plan.
pub fn planner_input(
    cfg: &Config,
    character: &str,
    plan: Option<shadowrunr_types::reconcile::NextStreamPlan>,
) -> Result<planner::PlannerInput, RpcError> {
    let summary = catalog::character_get(cfg, character)?;
    let pkg = shadowrunr_brain::load_character(
        &cfg.paths.characters.join(character),
        &summary.active_tone,
    )
    .map_err(|e| RpcError::io(format!("character {character}: {e}")))?;
    let bible = pkg.bible.clone().unwrap_or_default();
    let segments = shadowrunr_segments::active_segments();
    let segment_notes = segments
        .iter()
        .map(|s| {
            let name = serde_json::to_value(s)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            (*s, planner::section(&bible, &format!("{name}:")))
        })
        .collect();
    Ok(planner::PlannerInput {
        character_name: pkg.name.clone(),
        persona: pkg.persona.clone(),
        constitution: pkg.constitution.clone(),
        tone_id: pkg.tone_variant.clone(),
        tone_text: pkg.tone_text.clone(),
        running_jokes: planner::section(&bible, "Running jokes"),
        segment_notes,
        segments,
        plan,
    })
}

/// The plan a prep is written from: the newest plan chosen from `report_id`, or the current
/// plan (`<data>/next_plan.json`); `None` when no plan was chosen yet.
pub fn plan_for(
    reconciler: &Reconciler,
    report_id: Option<&str>,
) -> Result<Option<shadowrunr_types::reconcile::NextStreamPlan>, RpcError> {
    match report_id {
        Some(id) => {
            runs::check_id("report_id", id)?;
            let stored = reconciler.stored(id)?;
            match stored.plans.last() {
                Some(p) => Ok(Some(p.clone())),
                None => Err(RpcError::invalid(format!(
                    "report {id} has no chosen plan: choose its options first (plan.choose)"
                ))),
            }
        }
        None => Ok(reconciler.next_plan()?),
    }
}

/// Write, check and stage a prep for `character` from `plan`: one `planner` call.
pub async fn generate_with(
    cfg: &Config,
    engine: &dyn Engine,
    character: &str,
    plan: Option<shadowrunr_types::reconcile::NextStreamPlan>,
    now: TsMs,
) -> Result<StreamPrep, RpcError> {
    runs::check_id("character", character)?;
    let plan_report = plan.as_ref().and_then(|p| p.report_id.clone());
    let input = planner_input(cfg, character, plan)?;
    let (prompt, fence) = planner::prompt(&input).map_err(RpcError::internal)?;
    let (tier, effort, timeout_ms) = cfg.engine.route(Role::Planner);
    let reply = engine
        .complete(EngineRequest {
            role: Role::Planner,
            tier,
            effort,
            system: planner::system(&input),
            prompt,
            allowed_tools: Vec::new(),
            timeout_ms,
            json_schema: Some(planner::schema(&input.segments)),
            correlation_id: Some("prep.generate".into()),
        })
        .await
        .map_err(|e| RpcError::new(ErrorCode::Engine, format!("planner: {e}")))?;
    if fence.as_ref().is_some_and(|f| f.appears_in(&reply.text)) {
        return Err(RpcError::new(
            ErrorCode::Engine,
            "planner: the reply echoed the untrusted-data fence token (boundary event); nothing staged",
        )
        .with_detail(serde_json::json!({ "reason": "fence_echo" })));
    }
    let material = planner::parse(&reply.text).ok_or_else(|| {
        RpcError::new(
            ErrorCode::Engine,
            "planner: the reply was not a JSON object; nothing staged",
        )
        .with_detail(serde_json::json!({ "reason": "planner_output" }))
    })?;
    let mut prep = StreamPrep {
        prep_id: String::new(),
        character: character.to_string(),
        created_ms: now,
        status: PrepStatus::Staged,
        plan_report,
        cold_open: material.cold_open,
        bits: material.bits,
        connectors: material.connectors,
        checks: Vec::new(),
        approved_by: None,
        approved_ms: None,
        fictional: false,
    };
    prep.checks = check(cfg, &prep, &material.dropped)?;
    store(cfg).stage(prep)
}

/// `prep.generate`.
pub async fn generate(state: Arc<AppState>, p: PrepGenerateParams) -> Result<StreamPrep, RpcError> {
    let plan = plan_for(&state.reconciler, p.report_id.as_deref())?;
    generate_with(
        &state.cfg,
        state.engine.engine.as_ref(),
        &state.cfg.sim.character,
        plan,
        runs::now_ms(),
    )
    .await
}

/// `prep.list`.
pub async fn list(state: &Arc<AppState>) -> Result<Vec<PrepSummary>, RpcError> {
    store(&state.cfg).list()
}

/// `prep.get`.
pub async fn get(state: &Arc<AppState>, p: PrepIdParams) -> Result<StreamPrep, RpcError> {
    store(&state.cfg).load(&p.prep_id)
}

/// Approve with this config's checks re-run over the stored material.
pub fn approve_in(
    cfg: &Config,
    id: &str,
    signature: &str,
    now: TsMs,
) -> Result<StreamPrep, RpcError> {
    let policy = crate::clips::policy(cfg)?;
    let real_names = checks::load_real_names(&cfg.paths.policy);
    store(cfg).approve(id, signature, now, |p| {
        checks::run(
            p,
            &checks::CheckContext {
                policy: &policy,
                real_names: &real_names,
            },
            &checks::Dropped::default(),
        )
    })
}

/// `prep.approve`: refused with `conflict` while any check fails.
pub async fn approve(state: &Arc<AppState>, p: PrepApproveParams) -> Result<StreamPrep, RpcError> {
    approve_in(&state.cfg, &p.prep_id, &p.signature, runs::now_ms())
}

/// `prep.retire`.
pub async fn retire(state: &Arc<AppState>, p: PrepIdParams) -> Result<StreamPrep, RpcError> {
    store(&state.cfg).retire(&p.prep_id)
}

/// `characters/<character>/prep.sample.json`.
pub fn sample_path(cfg: &Config, character: &str) -> PathBuf {
    cfg.paths.characters.join(character).join(SAMPLE_FILE)
}

/// The character's fictional sample prep. Refused unless it is marked `fictional`.
pub fn sample(cfg: &Config, character: &str) -> Result<Option<StreamPrep>, RpcError> {
    runs::check_id("character", character)?;
    let path = sample_path(cfg, character);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let p: StreamPrep = serde_json::from_str(&text)
        .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
    if !p.fictional {
        return Err(RpcError::invalid(format!(
            "{}: a sample prep must be marked fictional",
            path.display()
        )));
    }
    Ok(Some(p))
}

/// The prep a simulated run of `character` uses: the newest approved one, else the fictional
/// sample, else none. A store or sample that cannot be read is reported and skipped: a
/// simulation never fails for want of prep.
pub fn for_sim(cfg: &Config, character: &str) -> Option<StreamPrep> {
    match store(cfg).newest_approved(character) {
        Ok(Some(p)) => return Some(p),
        Ok(None) => {}
        Err(e) => eprintln!("warning: prep store: {e}; trying the sample"),
    }
    match sample(cfg, character) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("warning: sample prep: {e}; running without prep");
            None
        }
    }
}
