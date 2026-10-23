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
use crate::engine::EngineHandle;
use crate::server::AppState;
use crate::{catalog, paging, preflight, report_wire, runs};

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

/// `tournament.results`: `results.json`, read through the contract's typed `TournamentResults`
/// so a file the console's door would reject fails here, as `io`, with the file named.
fn tournament_results(
    state: &AppState,
    tournament_id: &str,
) -> Result<shadowrunr_types::results::TournamentResults, RpcError> {
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

/// `engine.status`: the routes the engine applies, account cooldowns (labels only) and the
/// fleet state. The scripted engine reports the default route table, no accounts and mode
/// `scripted`.
pub fn engine_status(engine: &EngineHandle) -> EngineStatus {
    use shadowrunr_engine::{EngineConfig, FleetMode, ROLES};
    let default_cfg = EngineConfig::default();
    let cfg = engine.fleet.as_ref().map_or(&default_cfg, |f| f.config());
    let routes = ROLES
        .iter()
        .map(|&role| {
            let (tier, effort, timeout_ms) = cfg.route(role);
            EngineRoute {
                role,
                tier,
                effort,
                timeout_ms,
                overridden: engine.fleet.is_some() && cfg.route_overridden(role),
            }
        })
        .collect();
    let Some(fleet) = &engine.fleet else {
        return EngineStatus {
            routes,
            accounts: Vec::new(),
            fleet: EngineFleet {
                mode: EngineFleetMode::Scripted,
                since_ms: 0,
            },
            fallbacks: Vec::new(),
            api_fallback: false,
        };
    };
    let state = fleet.fleet_state();
    EngineStatus {
        routes,
        accounts: fleet
            .accounts()
            .into_iter()
            .map(|a| EngineAccount {
                label: a.label,
                cooling_until_ms: a.cooling_until_ms,
            })
            .collect(),
        fleet: EngineFleet {
            mode: match state.mode {
                FleetMode::Primary => EngineFleetMode::Primary,
                FleetMode::Degraded => EngineFleetMode::Degraded,
                FleetMode::Down => EngineFleetMode::Down,
            },
            since_ms: state.since_ms,
        },
        fallbacks: cfg
            .fallback_drivers()
            .into_iter()
            .map(|d| d.name().to_string())
            .collect(),
        api_fallback: cfg.api_key_env.is_some(),
    }
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
            ok(tournament_results(state, &p.tournament_id)?)
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
            ok(report_wire::report_reply(
                state.reconciler.stored(&p.report_id)?,
            ))
        }
        "reconcile.list" => ok(state.reconciler.list()?),
        "reconcile.incidents" => {
            let p: ReconcileIncidentsParams = params(req.params)?;
            let report = state.reconciler.report(&p.report_id)?;
            ok(report_wire::incidents_page(&report, &p)?)
        }
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
        "character.doc.get" => {
            let _: shadowrunr_types::catalog::CharacterDocGetParams = params(req.params)?;
            Err(RpcError::internal(
                "character.doc.get is not implemented yet",
            ))
        }
        "character.doc.set" => {
            let _: shadowrunr_types::catalog::CharacterDocSetParams = params(req.params)?;
            Err(RpcError::internal(
                "character.doc.set is not implemented yet",
            ))
        }
        "character.validate" => {
            let p: CharacterValidateParams = params(req.params)?;
            let dir = catalog::character_dir(cfg, &p.id)?;
            ok(crate::character::validate(&dir))
        }
        "character.tone.set" => {
            let p: CharacterToneSetParams = params(req.params)?;
            ok(catalog::character_tone_set(cfg, &p.id, &p.tone)?)
        }
        "segments.tables" => ok(catalog::segment_tables(cfg)?),
        "ranks.table" => {
            let p: RanksTableParams = params(req.params)?;
            ok(catalog::rank_table(cfg, p.character.as_deref())?)
        }
        "ledger.page" => ok(paging::ledger_page(cfg, params(req.params)?)?),
        "ledger.summary" => ok(paging::ledger_summary(cfg, params(req.params)?)?),
        "runs.page" => ok(paging::runs_page(&sim_root, params(req.params)?)?),
        "tournaments.page" => ok(paging::tournaments_page(&sim_root, params(req.params)?)?),
        "engine.status" => {
            let _: EmptyParams = params(req.params)?;
            ok(engine_status(&state.engine))
        }
        "clips.list" => {
            let p: shadowrunr_types::clips::ClipsListParams = params(req.params)?;
            ok(cfg.clip_store().page(&p)?)
        }
        "clips.get" => {
            let p: shadowrunr_types::clips::ClipIdParams = params(req.params)?;
            ok(cfg.clip_store().load(&p.id)?)
        }
        "clips.scan" => {
            let p: shadowrunr_types::clips::ClipsScanParams = params(req.params)?;
            ok(crate::clips::scan(cfg, state.engine.engine.clone(), &p).await?)
        }
        "clips.render" => {
            let p: shadowrunr_types::clips::ClipIdParams = params(req.params)?;
            let cfg = cfg.clone();
            let c = tokio::task::spawn_blocking(move || crate::clips::render(&cfg, &p.id))
                .await
                .map_err(|e| RpcError::internal(format!("render task: {e}")))??;
            ok(c)
        }
        "clips.approve" => {
            let p: shadowrunr_types::clips::ClipsApproveParams = params(req.params)?;
            ok(cfg.clip_store().approve(
                &p.id,
                &p.signature,
                p.override_reason.as_deref(),
                p.note.as_deref(),
            )?)
        }
        "clips.reject" => {
            let p: shadowrunr_types::clips::ClipsRejectParams = params(req.params)?;
            ok(cfg.clip_store().reject(&p.id, &p.reason, p.by.as_deref())?)
        }
        "clips.publish" => {
            let p: shadowrunr_types::clips::ClipsPublishParams = params(req.params)?;
            ok(shadowrunr_clips::upload::publish_dry_run(
                &cfg.clip_store(),
                &p.id,
                p.platforms.as_deref(),
            )?)
        }
        "clips.feedback" => {
            let p: shadowrunr_types::clips::ClipsFeedbackParams = params(req.params)?;
            ok(cfg
                .clip_store()
                .feedback(&p.id, &p.tags, p.note.as_deref())?)
        }
        "clips.recording" => {
            let p: shadowrunr_types::clips::ClipsRecordingParams = params(req.params)?;
            ok(crate::clips::recording(cfg, &p.session_id)?)
        }
        "sandbox.start" => ok(crate::sandbox::start(state.clone(), params(req.params)?).await?),
        "sandbox.stop" => ok(crate::sandbox::stop(state, params(req.params)?).await?),
        "sandbox.status" => ok(crate::sandbox::status(state, params(req.params)?).await?),
        "review.sessions" => ok(crate::review::sessions(state, params(req.params)?).await?),
        "review.session" => ok(crate::review::session(state, params(req.params)?).await?),
        "review.feedback.set" => ok(crate::review::feedback_set(state, params(req.params)?).await?),
        "review.feedback.list" => {
            ok(crate::review::feedback_list(state, params(req.params)?).await?)
        }
        "review.tag" => ok(crate::review::tag(state, params(req.params)?).await?),
        "prep.generate" => ok(crate::prep::generate(state.clone(), params(req.params)?).await?),
        "prep.list" => ok(crate::prep::list(state).await?),
        "prep.get" => ok(crate::prep::get(state, params(req.params)?).await?),
        "prep.approve" => ok(crate::prep::approve(state, params(req.params)?).await?),
        "prep.retire" => ok(crate::prep::retire(state, params(req.params)?).await?),
        "tone.variants" => ok(crate::tone::rpc_variants(state, params(req.params)?).await?),
        "tone.lines.list" => ok(crate::tone::rpc_lines_list(state, params(req.params)?).await?),
        "tone.lines.add" => ok(crate::tone::rpc_lines_add(state, params(req.params)?).await?),
        "tone.lines.set_status" => {
            ok(crate::tone::rpc_lines_set_status(state, params(req.params)?).await?)
        }
        "tone.directions.get" => {
            ok(crate::tone::rpc_directions_get(state, params(req.params)?).await?)
        }
        "tone.directions.set" => {
            ok(crate::tone::rpc_directions_set(state, params(req.params)?).await?)
        }
        "tone.inbox" => ok(crate::tone::rpc_inbox(state, params(req.params)?).await?),
        "tone.review_line" => ok(crate::tone::rpc_review_line(state, params(req.params)?).await?),
        "segments.directions.get" => {
            ok(crate::segments_dir::rpc_get(state, params(req.params)?).await?)
        }
        "segments.directions.set" => {
            ok(crate::segments_dir::rpc_set(state, params(req.params)?).await?)
        }
        "voice.settings.get" => {
            ok(crate::voice::settings::settings_get(state, params(req.params)?).await?)
        }
        "voice.settings.set" => {
            ok(crate::voice::settings::settings_set(state, params(req.params)?).await?)
        }
        "voice.verify" => ok(crate::voice::settings::verify(state, params(req.params)?).await?),
        "voice.preview" => ok(crate::voice::settings::preview(state, params(req.params)?).await?),
        "voice.bench" => ok(crate::voice::settings::bench(state, params(req.params)?).await?),
        "moderator.status" => ok(crate::moderator::rpc_status(state).await?),
        "moderator.rules.list" => ok(crate::moderator::rpc_rules_list(state).await?),
        "moderator.rules.set" => {
            ok(crate::moderator::rpc_rules_set(state, params(req.params)?).await?)
        }
        "moderator.rules.test" => {
            ok(crate::moderator::rpc_rules_test(state, params(req.params)?).await?)
        }
        "moderator.mode.set" => {
            ok(crate::moderator::rpc_mode_set(state, params(req.params)?).await?)
        }
        "moderator.log" => ok(crate::moderator::rpc_log(state, params(req.params)?).await?),
        "moderator.unban" => ok(crate::moderator::rpc_unban(state, params(req.params)?).await?),
        "twitch.connect" => ok(crate::moderator::rpc_twitch_connect(state).await?),
        "twitch.disconnect" => ok(crate::moderator::rpc_twitch_disconnect(state).await?),
        other => Err(RpcError::new(
            ErrorCode::MethodNotFound,
            format!("unknown method {other:?}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use shadowrunr_engine::{route, AccountConfig, EngineConfig, Fleet, RouteOverride};
    use shadowrunr_types::{Effort, ModelTier, Role};

    use super::*;

    #[test]
    fn engine_status_for_the_scripted_engine() {
        let st = engine_status(&EngineHandle::scripted());
        assert_eq!(st.routes.len(), shadowrunr_engine::ROLES.len());
        assert_eq!(st.routes[0].role, Role::CoreThread);
        for r in &st.routes {
            assert_eq!((r.tier, r.effort, r.timeout_ms), route(r.role));
            assert!(!r.overridden);
        }
        assert!(st.accounts.is_empty());
        assert_eq!(st.fleet.mode, EngineFleetMode::Scripted);
        assert!(st.fallbacks.is_empty());
        assert!(!st.api_fallback);
        let v = serde_json::to_value(&st).unwrap();
        assert_eq!(v["fleet"], json!({ "mode": "scripted", "since_ms": 0 }));
    }

    #[test]
    fn engine_status_reports_overrides_cooldowns_and_labels_only() {
        let tmp = tempfile::tempdir().unwrap();
        let secret_dir = tmp.path().join("secret-claude-config");
        let mut routes = HashMap::new();
        routes.insert(
            Role::CoreThread,
            RouteOverride {
                tier: Some(ModelTier::Sonnet),
                ..RouteOverride::default()
            },
        );
        let cfg = EngineConfig {
            accounts: vec![
                AccountConfig {
                    label: "main".into(),
                    config_dir: Some(secret_dir.to_string_lossy().into_owned()),
                    fallback: false,
                },
                AccountConfig {
                    label: "spare".into(),
                    config_dir: None,
                    fallback: false,
                },
            ],
            api_key_env: Some("SHADOWRUNR_TEST_API_KEY".into()),
            cli_fallbacks: vec!["agy".into(), "nonsense".into(), "codex_cli".into()],
            ledger_path: Some(
                tmp.path()
                    .join("ledger.jsonl")
                    .to_string_lossy()
                    .into_owned(),
            ),
            routes,
            ..EngineConfig::default()
        };
        let fleet = Arc::new(Fleet::new(&cfg));
        let until = fleet.inner().now() + 600_000;
        fleet.inner().cool(1, Some(until));
        let handle = EngineHandle {
            engine: fleet.clone(),
            fleet: Some(fleet),
        };
        let st = engine_status(&handle);

        let core = &st.routes[0];
        assert_eq!(core.role, Role::CoreThread);
        assert_eq!(
            (core.tier, core.effort, core.timeout_ms, core.overridden),
            (ModelTier::Sonnet, Effort::Low, 30_000, true)
        );
        assert!(st.routes[1..].iter().all(|r| !r.overridden));
        let labels: Vec<(&str, Option<i64>)> = st
            .accounts
            .iter()
            .map(|a| (a.label.as_str(), a.cooling_until_ms))
            .collect();
        assert_eq!(labels, vec![("main", None), ("spare", Some(until))]);
        assert_eq!(st.fleet.mode, EngineFleetMode::Primary);
        assert_eq!(st.fallbacks, vec!["agy", "codex"]);
        assert!(st.api_fallback);

        let text = serde_json::to_string(&st).unwrap();
        assert!(!text.contains("secret-claude-config"), "{text}");
        assert!(!text.contains("SHADOWRUNR_TEST_API_KEY"), "{text}");
        assert!(!text.contains("null"), "{text}");
    }
}
