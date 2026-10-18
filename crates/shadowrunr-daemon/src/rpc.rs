//! `POST /rpc`: `{"method", "params"}` -> `{"result"}` | `{"error": {code, message, detail?}}`.
//! The method table, params and results are `shadowrunr_types::rpc`; error codes are the
//! ten of `shadowrunr_types::ErrorCode` (docs/spec/contract.md § Daemon protocol and
//! § Error taxonomy).

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use shadowrunr_reconcile::{self as reconcile, ReconcileError};
use shadowrunr_types::reconcile::{DirectiveKind, Source};
use shadowrunr_types::rpc::*;
use shadowrunr_types::{ErrorBody, ErrorCode, ErrorEnvelope, JudgeVerdict, StreamEvent};

pub use crate::catalog::{character_rules, character_rules_set};
use crate::server::AppState;
use crate::{catalog, paging, preflight, runs};

/// A failed call: one of the ten codes, a message for people, optional structured detail.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: ErrorCode,
    pub message: String,
    pub detail: Option<Value>,
}

impl RpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }
    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }
    pub fn invalid(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParams, m)
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, m)
    }
    pub fn io(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Io, m)
    }
    pub fn internal(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, m)
    }
    pub fn envelope(&self) -> ErrorEnvelope {
        ErrorEnvelope {
            error: ErrorBody {
                code: self.code,
                message: self.message.clone(),
                detail: self.detail.clone(),
            },
        }
    }
    /// `{"error": {...}}` as JSON.
    pub fn body(&self) -> Value {
        serde_json::to_value(self.envelope())
            .unwrap_or_else(|_| json!({ "error": { "code": "internal", "message": "error" } }))
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Reconcile's own codes map onto the taxonomy; a renamed code keeps its old name in
/// `detail.reason` (`synthesis` -> `engine` + `{"reason": "synthesis"}`).
impl From<ReconcileError> for RpcError {
    fn from(e: ReconcileError) -> Self {
        let legacy = e.code();
        let code = ErrorCode::from_legacy(legacy);
        let err = Self::new(code, e.to_string());
        if code.as_str() == legacy {
            err
        } else {
            err.with_detail(json!({ "reason": legacy }))
        }
    }
}

fn params<T: DeserializeOwned>(v: Value) -> Result<T, RpcError> {
    let v = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(v).map_err(|e| RpcError::invalid(e.to_string()))
}

fn ok<T: Serialize>(v: T) -> Result<Value, RpcError> {
    serde_json::to_value(v).map_err(|e| RpcError::internal(format!("serialize result: {e}")))
}

fn source(s: Source) -> reconcile::Source {
    match s {
        Source::Tournament { tournament_id } => reconcile::Source::Tournament { tournament_id },
        Source::Runs { run_ids } => reconcile::Source::Runs { run_ids },
    }
}

fn directive_kind(k: DirectiveKind) -> reconcile::DirectiveKind {
    match k {
        DirectiveKind::Prevent => reconcile::DirectiveKind::Prevent,
        DirectiveKind::Try => reconcile::DirectiveKind::Try,
    }
}

fn read_verdict(state: &AppState, p: RunParams) -> Result<JudgeVerdict, RpcError> {
    let sim_root = state.cfg.paths.sim_runs();
    let entry = runs::find_run(&sim_root, &p.run_id, p.tournament_id.as_deref())?;
    let path = entry.dir.join("verdict.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|_| RpcError::not_found(format!("run {} has no verdict", entry.row.run_id)))?;
    serde_json::from_str(&text).map_err(|e| RpcError::io(format!("{}: {e}", path.display())))
}

fn tournament_results(state: &AppState, tournament_id: &str) -> Result<Value, RpcError> {
    runs::check_id("tournament_id", tournament_id)?;
    let dir = state.cfg.paths.sim_runs().join(tournament_id);
    if !dir.is_dir() {
        return Err(RpcError::not_found(format!("tournament {tournament_id}")));
    }
    let path = dir.join("results.json");
    let text = std::fs::read_to_string(&path).map_err(|_| {
        RpcError::not_found(format!(
            "tournament {tournament_id} has no results.json (run `shadowrunr sim summarize`)"
        ))
    })?;
    serde_json::from_str(&text).map_err(|e| RpcError::io(format!("{}: {e}", path.display())))
}

/// Dispatch one call. Every method of `shadowrunr_types::rpc::visit_methods` is handled here.
pub async fn dispatch(state: &Arc<AppState>, req: RpcRequest) -> Result<Value, RpcError> {
    let sim_root = state.cfg.paths.sim_runs();
    let cfg = &state.cfg;
    match req.method.as_str() {
        "preflight.run" => ok(preflight::run_all(cfg, &state.engine).await),
        "runs.list" => {
            let p: RunsListParams = params(req.params)?;
            let rows: Vec<RunRow> = runs::list_runs(&sim_root, p.tournament_id.as_deref())?
                .into_iter()
                .map(|e| e.row)
                .collect();
            ok(rows)
        }
        "run.transcript" => {
            let p: RunParams = params(req.params)?;
            let entry = runs::find_run(&sim_root, &p.run_id, p.tournament_id.as_deref())?;
            let events: Vec<StreamEvent> =
                runs::read_transcript_lines(&entry.dir.join("transcript.jsonl"))?
                    .into_iter()
                    .map(|(_, ev)| ev)
                    .collect();
            ok(events)
        }
        "run.verdict" => ok(read_verdict(state, params(req.params)?)?),
        "tournament.list" => ok(runs::list_tournaments(&sim_root)),
        "tournament.results" => {
            let p: TournamentParams = params(req.params)?;
            tournament_results(state, &p.tournament_id)
        }
        "sim.start" => {
            let p: SimStartParams = params(req.params)?;
            let run_id = crate::live::start_sim(state.clone(), p.spec, p.judge).await?;
            ok(SimStartResult { run_id })
        }
        "reconcile.run" => {
            let p: ReconcileRunParams = params(req.params)?;
            let report_id = state.reconciler.run(source(p.source)).await?;
            crate::integrations::activity::emit(
                crate::integrations::activity::ActivityEvent::report_ready(&report_id, json!({})),
            );
            ok(ReconcileRunResult { report_id })
        }
        "reconcile.report" => {
            let p: ReportParams = params(req.params)?;
            ok(state.reconciler.report(&p.report_id)?)
        }
        "reconcile.list" => ok(state.reconciler.list()?),
        "plan.current" => match state.reconciler.next_plan()? {
            Some(plan) => ok(plan),
            None => Err(RpcError::new(ErrorCode::NotFound, "no plan chosen yet")),
        },
        "plan.proposals" => {
            let p: ReportParams = params(req.params)?;
            ok(state.reconciler.proposals(&p.report_id)?)
        }
        "plan.choose" => {
            let p: PlanChooseParams = params(req.params)?;
            ok(state.reconciler.choose(&p.report_id, &p.option_ids).await?)
        }
        "directive.add" => {
            let p: DirectiveAddParams = params(req.params)?;
            ok(state
                .reconciler
                .directive_add(directive_kind(p.kind), &p.text, p.scope)?)
        }
        "directive.list" => ok(state.reconciler.directive_list()?),
        "directive.deactivate" => {
            let p: DirectiveIdParams = params(req.params)?;
            ok(state.reconciler.directive_deactivate(&p.id)?)
        }
        "lord.chat" => {
            let p: LordChatParams = params(req.params)?;
            ok(state
                .reconciler
                .lord_chat(p.report_id.as_deref(), &p.message)
                .await?)
        }
        "character.rules" => {
            let p: CharacterRulesParams = params(req.params)?;
            ok(character_rules(cfg, &p.character)?)
        }
        "character.rules.set" => {
            let p: CharacterRulesSetParams = params(req.params)?;
            ok(character_rules_set(cfg, &p.character, &p.id, p.enabled)?)
        }
        "character.list" => ok(catalog::character_list(cfg)),
        "character.get" => {
            let p: CharacterGetParams = params(req.params)?;
            ok(catalog::character_get(cfg, &p.id)?)
        }
        "segments.tables" => ok(catalog::segment_tables()?),
        "ranks.table" => {
            let p: RanksTableParams = params(req.params)?;
            ok(catalog::rank_table(cfg, p.character.as_deref())?)
        }
        "ledger.page" => ok(paging::ledger_page(cfg, params(req.params)?)?),
        "runs.page" => ok(paging::runs_page(&sim_root, params(req.params)?)?),
        "tournaments.page" => ok(paging::tournaments_page(&sim_root, params(req.params)?)?),
        other => Err(RpcError::new(
            ErrorCode::MethodNotFound,
            format!("unknown method {other:?}"),
        )),
    }
}
