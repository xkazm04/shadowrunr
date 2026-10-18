//! `POST /rpc`: `{"method", "params"}` -> `{"result"}` | `{"error": {"code", "message"}}`.
//! Method table: docs/spec/contract.md § Daemon protocol.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shadowrunr_brain::rules::{self, RuleState, RulesOverride};
use shadowrunr_brain::BrainError;
use shadowrunr_reconcile::{DirectiveKind, ReconcileError, Source};
use shadowrunr_types::{JudgeVerdict, SegmentId, SimRunSpec, StreamEvent};

use crate::config::Config;
use crate::preflight;
use crate::runs;
use crate::server::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

impl RpcError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn invalid(m: impl Into<String>) -> Self {
        Self::new("invalid_params", m)
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self::new("not_found", m)
    }
    pub fn io(m: impl Into<String>) -> Self {
        Self::new("io", m)
    }
    pub fn body(&self) -> Value {
        json!({ "error": self })
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<ReconcileError> for RpcError {
    fn from(e: ReconcileError) -> Self {
        Self::new(e.code(), e.to_string())
    }
}

#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

fn params<T: DeserializeOwned>(v: Value) -> Result<T, RpcError> {
    let v = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(v).map_err(|e| RpcError::invalid(e.to_string()))
}

fn ok<T: Serialize>(v: T) -> Result<Value, RpcError> {
    serde_json::to_value(v).map_err(|e| RpcError::io(e.to_string()))
}

#[derive(Deserialize)]
struct RunsListParams {
    #[serde(default)]
    tournament_id: Option<String>,
}

#[derive(Deserialize)]
struct RunParams {
    run_id: String,
    #[serde(default)]
    tournament_id: Option<String>,
}

#[derive(Deserialize)]
struct TournamentParams {
    tournament_id: String,
}

#[derive(Deserialize)]
struct SimStartParams {
    spec: SimRunSpec,
    #[serde(default)]
    judge: bool,
}

#[derive(Deserialize)]
struct ReconcileRunParams {
    source: Source,
}

#[derive(Deserialize)]
struct ReportParams {
    report_id: String,
}

#[derive(Deserialize)]
struct ChooseParams {
    report_id: String,
    option_ids: Vec<String>,
}

#[derive(Deserialize)]
struct DirectiveAddParams {
    kind: DirectiveKind,
    text: String,
    #[serde(default)]
    scope: Option<SegmentId>,
}

#[derive(Deserialize)]
struct DirectiveIdParams {
    id: String,
}

#[derive(Deserialize)]
struct CharacterParams {
    character: String,
}

#[derive(Deserialize)]
struct CharacterRuleSetParams {
    character: String,
    id: String,
    enabled: bool,
}

fn character_dir(cfg: &Config, character: &str) -> Result<std::path::PathBuf, RpcError> {
    runs::check_id("character", character)?;
    let dir = cfg.paths.characters.join(character);
    if !dir.join("character.toml").is_file() {
        return Err(RpcError::not_found(format!("character {character}")));
    }
    Ok(dir)
}

/// `character.rules`: the package's optional rules with the operator's override applied.
pub fn character_rules(cfg: &Config, character: &str) -> Result<Vec<RuleState>, RpcError> {
    let dir = character_dir(cfg, character)?;
    let over = rules::override_path(&cfg.paths.data, character);
    let list = rules::load_rules(&dir, &RulesOverride::File(over))
        .map_err(|e| RpcError::io(e.to_string()))?;
    Ok(list.iter().map(RuleState::from).collect())
}

/// `character.rules.set`: write one rule into `<data>/characters/<id>/rules.json`.
pub fn character_rules_set(
    cfg: &Config,
    character: &str,
    id: &str,
    enabled: bool,
) -> Result<Vec<RuleState>, RpcError> {
    let dir = character_dir(cfg, character)?;
    let over = rules::override_path(&cfg.paths.data, character);
    rules::set_rule(&dir, character, &over, id, enabled).map_err(|e| match e {
        BrainError::Rejected(m) => RpcError::not_found(m),
        other => RpcError::io(other.to_string()),
    })
}

#[derive(Deserialize)]
struct LordChatParams {
    #[serde(default)]
    report_id: Option<String>,
    message: String,
}

/// Dispatch one call.
pub async fn dispatch(state: &Arc<AppState>, req: RpcRequest) -> Result<Value, RpcError> {
    let sim_root = state.cfg.paths.sim_runs();
    match req.method.as_str() {
        "preflight.run" => ok(preflight::run_all(&state.cfg, &state.engine).await),
        "runs.list" => {
            let p: RunsListParams = params(req.params)?;
            let rows: Vec<runs::RunRow> = runs::list_runs(&sim_root, p.tournament_id.as_deref())?
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
        "run.verdict" => {
            let p: RunParams = params(req.params)?;
            let entry = runs::find_run(&sim_root, &p.run_id, p.tournament_id.as_deref())?;
            let path = entry.dir.join("verdict.json");
            let text = std::fs::read_to_string(&path).map_err(|_| {
                RpcError::not_found(format!("run {} has no verdict", entry.row.run_id))
            })?;
            let v: JudgeVerdict = serde_json::from_str(&text)
                .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
            ok(v)
        }
        "tournament.list" => ok(runs::list_tournaments(&sim_root)),
        "tournament.results" => {
            let p: TournamentParams = params(req.params)?;
            runs::check_id("tournament_id", &p.tournament_id)?;
            let dir = sim_root.join(&p.tournament_id);
            if !dir.is_dir() {
                return Err(RpcError::not_found(format!(
                    "tournament {}",
                    p.tournament_id
                )));
            }
            let path = dir.join("results.json");
            let text = std::fs::read_to_string(&path).map_err(|_| {
                RpcError::not_found(format!(
                    "tournament {} has no results.json (run `shadowrunr sim summarize`)",
                    p.tournament_id
                ))
            })?;
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| RpcError::io(format!("{}: {e}", path.display())))?;
            Ok(v)
        }
        "sim.start" => {
            let p: SimStartParams = params(req.params)?;
            let run_id = crate::live::start_sim(state.clone(), p.spec, p.judge).await?;
            Ok(json!({ "run_id": run_id }))
        }
        "reconcile.run" => {
            let p: ReconcileRunParams = params(req.params)?;
            let report_id = state.reconciler.run(p.source).await?;
            Ok(json!({ "report_id": report_id }))
        }
        "reconcile.report" => {
            let p: ReportParams = params(req.params)?;
            ok(state.reconciler.report(&p.report_id)?)
        }
        "plan.proposals" => {
            let p: ReportParams = params(req.params)?;
            ok(state.reconciler.proposals(&p.report_id)?)
        }
        "plan.choose" => {
            let p: ChooseParams = params(req.params)?;
            ok(state.reconciler.choose(&p.report_id, &p.option_ids).await?)
        }
        "directive.add" => {
            let p: DirectiveAddParams = params(req.params)?;
            ok(state.reconciler.directive_add(p.kind, &p.text, p.scope)?)
        }
        "directive.list" => ok(state.reconciler.directive_list()?),
        "directive.deactivate" => {
            let p: DirectiveIdParams = params(req.params)?;
            ok(state.reconciler.directive_deactivate(&p.id)?)
        }
        "character.rules" => {
            let p: CharacterParams = params(req.params)?;
            ok(character_rules(&state.cfg, &p.character)?)
        }
        "character.rules.set" => {
            let p: CharacterRuleSetParams = params(req.params)?;
            ok(character_rules_set(
                &state.cfg,
                &p.character,
                &p.id,
                p.enabled,
            )?)
        }
        "lord.chat" => {
            let p: LordChatParams = params(req.params)?;
            ok(state
                .reconciler
                .lord_chat(p.report_id.as_deref(), &p.message)
                .await?)
        }
        other => Err(RpcError::new(
            "method_not_found",
            format!("unknown method {other:?}"),
        )),
    }
}
