//! The MCP tool table: each tool calls the same `rpc::dispatch` handler as `POST /rpc` on an
//! in-process [`AppState`] (never over HTTP). Input schemas are cut from the generated wire
//! contract (`shadowrunr_types::schema::contract_schema`), with `$ref`s inlined and a few
//! fields relaxed so an agent may omit them (`limit`, the sim spec's fields).
//!
//! Read-only by default. `start_sim` runs only with `[personas] mcp_allow_writes = true`, and
//! uses the scripted engine unless `live: true`. The moderator's writes (`moderator_rules_set`,
//! `moderator_mode_set`, `moderator_unban`, `twitch_*`, `moderator_character_set`) run only with
//! `[moderator] mcp_allow_writes = true`; every moderator tool goes to the running daemon when
//! one answers, so the live lane sees each write (docs/guide/moderator.md).
//!
//! `start_sim` and `preflight` go through a running daemon when one answers on
//! `[daemon] port` ([`super::mcp_backend`]): its guards and its engine, never a second set
//! beside it. The result's `host` says where a sim runs (`daemon` | `mcp`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Map, Value};

pub use super::mcp_schema::{input_schema, params_schema, tool_list};
use shadowrunr_types::rpc::RpcRequest;
use shadowrunr_types::ErrorCode;

use crate::config::Config;
use crate::engine::EngineHandle;
use crate::paging;
use crate::rpc::{self, RpcError};
use crate::server::AppState;

/// Builds the engine for `live: true` sims and the live pre-flight.
pub type EngineFactory = Arc<dyn Fn(&Config) -> EngineHandle + Send + Sync>;

/// Default page sizes when a caller leaves `limit` out.
pub const DEFAULT_RUNS_LIMIT: u32 = 50;
pub const DEFAULT_TRANSCRIPT_LIMIT: u32 = 200;

pub struct ToolDef {
    pub name: &'static str,
    /// The `/rpc` method the tool calls.
    pub method: &'static str,
    pub description: &'static str,
    pub write: bool,
    /// A moderator tool: its writes are gated by `[moderator] mcp_allow_writes`, and every call
    /// goes to the running daemon when one answers.
    pub moderator: bool,
}

pub const TOOLS: [ToolDef; 19] = [
    ToolDef {
        name: "list_runs",
        method: "runs.page",
        description: "Simulated runs ordered by (tournament_id, run_id), one page at a time. Pass next_cursor back as cursor for the next page. limit defaults to 50 (max 500).",
        write: false,
        moderator: false,
    },
    ToolDef {
        name: "get_transcript",
        method: "run.transcript",
        description: "One run's transcript (StreamEvents with their 1-based seq), paged: limit defaults to 200 (max 500); pass next_cursor back as cursor.",
        write: false,
        moderator: false,
    },
    ToolDef {
        name: "get_tournament_results",
        method: "tournament.results",
        description: "A tournament's results.json: per-cell scores, judged and void counts.",
        write: false,
        moderator: false,
    },
    ToolDef {
        name: "start_sim",
        method: "sim.start",
        description: "Start one text simulation in the background and return its run_id (read it later with get_transcript). Spec fields default to the [sim] config. Scripted engine (no quota) unless live is true. When a daemon is running the sim runs in it, and live must match the daemon's engine. Disabled unless [personas] mcp_allow_writes = true.",
        write: true,
        moderator: false,
    },
    ToolDef {
        name: "get_reconcile_report",
        method: "reconcile.report",
        description: "A stored Reconcile & Plan report: findings, metrics and summary.",
        write: false,
        moderator: false,
    },
    ToolDef {
        name: "list_plan_options",
        method: "plan.proposals",
        description: "The plan options proposed by a Reconcile & Plan report.",
        write: false,
        moderator: false,
    },
    ToolDef {
        name: "list_directives",
        method: "directive.list",
        description: "The operator's directives (prevent / try) in three states: active (active: true, fed to streams), proposed (active: false, proposed: true; suggested by the Reconciler in lord.chat and not yet kept or dropped by the operator, so no stream reads it) and inactive (active: false, retired or dropped).",
        write: false,
        moderator: false,
    },
    ToolDef {
        name: "preflight",
        method: "preflight.run",
        description: "Run the pre-flight checks (PF-01..PF-08, plus PF-09 when Personas activity is configured). Spends no quota.",
        write: false,
        moderator: false,
    },
    ToolDef {
        name: "moderator_status",
        method: "moderator.status",
        description: "The chat moderator at a glance: its mode (shadow = decide and log only, armed = act on chat), the moderator character, each chat platform's connection (Twitch: disconnected, connecting, connected or error, with the bot account and channel) and how many decisions it made and carried out since the daemon started.",
        write: false,
        moderator: true,
    },
    ToolDef {
        name: "moderator_rules_list",
        method: "moderator.rules.list",
        description: "The moderator's whole rule list, in order. A pattern rule matches a term or regex at once (case-insensitive, on the text as written and normalised); a plain rule is a sentence a classifier model judges (\"no spoilers for the finale\"). Each rule has an action (delete, timeout with timeout_s, ban), optional strikes (after count hits in window_s the action becomes then) and enabled.",
        write: false,
        moderator: true,
    },
    ToolDef {
        name: "moderator_rules_set",
        method: "moderator.rules.set",
        description: "Replace the whole moderator rule list (read it first with moderator_rules_list, edit, send every rule back). The first enabled pattern rule that matches wins; plain rules judge what no pattern took. An invalid rule refuses the whole write and names the rule; the old list stays. Applies to the next chat message. Ids are [a-z0-9-]; name is what the moderator's chat line mentions. Disabled unless [moderator] mcp_allow_writes = true.",
        write: true,
        moderator: true,
    },
    ToolDef {
        name: "moderator_rules_test",
        method: "moderator.rules.test",
        description: "Try a sample chat message against the current rules: returns the decision the moderator would make (rule, action after strikes) or none, and disclosure: true when the message reads as a self-harm disclosure (never acted on). No side effects: nothing is logged, posted or done on the platform. Plain rules call the classifier model of the running daemon (with no daemon, only pattern rules are judged).",
        write: false,
        moderator: true,
    },
    ToolDef {
        name: "moderator_mode_set",
        method: "moderator.mode.set",
        description: "Switch the moderator between shadow (decide and log what it would have done, touch nothing) and armed (delete, time out and ban on the platform, and post its line in chat). Start in shadow, review moderator_log, then arm. Disabled unless [moderator] mcp_allow_writes = true.",
        write: true,
        moderator: true,
    },
    ToolDef {
        name: "moderator_log",
        method: "moderator.log",
        description: "The moderator's decisions and unbans, newest first: who, which message, which rule, the action, whether it was carried out (executed), the moderator's line and any platform error. limit defaults to 100 (max 1000).",
        write: false,
        moderator: true,
    },
    ToolDef {
        name: "moderator_unban",
        method: "moderator.unban",
        description: "Lift a ban or timeout the moderator (or anyone) placed: give the platform and the viewer_id from moderator_log. The unban is appended to the log; executed is false with the error when the platform is not connected. Disabled unless [moderator] mcp_allow_writes = true.",
        write: true,
        moderator: true,
    },
    ToolDef {
        name: "twitch_connect",
        method: "twitch.connect",
        description: "Log the moderator's Twitch bot account in: returns a verification_uri and a user_code. Open the link in a browser signed in as the bot account and enter the code before expires_s; the daemon then connects on its own (check moderator_status). Needs [twitch] client_id and channel in shadowrunr.toml and a running daemon. Disabled unless [moderator] mcp_allow_writes = true.",
        write: true,
        moderator: true,
    },
    ToolDef {
        name: "twitch_disconnect",
        method: "twitch.disconnect",
        description: "Disconnect the moderator's Twitch bot and delete its stored login from the OS credential store; returns the moderator status. Needs a running daemon. Disabled unless [moderator] mcp_allow_writes = true.",
        write: true,
        moderator: true,
    },
    ToolDef {
        name: "moderator_character_get",
        method: "character.get",
        description: "The moderator character ([moderator] character): its id, name, tagline and [identity] (the words its chat lines are written from).",
        write: false,
        moderator: true,
    },
    ToolDef {
        name: "moderator_character_set",
        method: "",
        description: "Change the moderator character's name, tagline or [identity]. Not available yet: the daemon has no RPC that writes character.toml, so edit characters/<id>/character.toml by hand and restart the daemon. Disabled unless [moderator] mcp_allow_writes = true.",
        write: true,
        moderator: true,
    },
];

/// The moderator character as `moderator_character_get` shows it: id, name, tagline and
/// identity of a `character.get` detail (absent fields omitted).
fn character_view(detail: &Value) -> Value {
    let mut out = Map::new();
    for k in ["id", "name", "tagline", "identity"] {
        if let Some(v) = detail.get(k).filter(|v| !v.is_null()) {
            out.insert(k.into(), v.clone());
        }
    }
    Value::Object(out)
}

pub fn find(name: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.name == name)
}

/// Why a call produced no result.
#[derive(Debug)]
pub enum ToolError {
    /// No such tool: a protocol error (-32602).
    Unknown(String),
    /// The handler failed: an `isError` result carrying the `/rpc` error envelope.
    Failed(RpcError),
}

impl From<RpcError> for ToolError {
    fn from(e: RpcError) -> Self {
        Self::Failed(e)
    }
}

pub struct Tools {
    cfg: Arc<Config>,
    allow_writes: bool,
    /// Scripted engine: every read tool and scripted sims.
    scripted: Arc<AppState>,
    live: tokio::sync::OnceCell<Arc<AppState>>,
    factory: EngineFactory,
    /// Runs this server started: `(state, run_id)`.
    started: Mutex<Vec<(Arc<AppState>, String)>>,
}

impl Tools {
    pub fn new(cfg: Config, allow_writes: bool, factory: EngineFactory) -> Self {
        let scripted = AppState::new(cfg.clone(), EngineHandle::scripted());
        Self {
            cfg: Arc::new(cfg),
            allow_writes,
            scripted,
            live: tokio::sync::OnceCell::new(),
            factory,
            started: Mutex::new(Vec::new()),
        }
    }

    async fn live_state(&self) -> Arc<AppState> {
        self.live
            .get_or_init(|| async { AppState::new((*self.cfg).clone(), (self.factory)(&self.cfg)) })
            .await
            .clone()
    }

    async fn rpc(
        &self,
        state: &Arc<AppState>,
        method: &str,
        params: Value,
    ) -> Result<Value, RpcError> {
        rpc::dispatch(
            state,
            RpcRequest {
                method: method.into(),
                params,
            },
        )
        .await
    }

    pub async fn call(&self, name: &str, args: Value) -> Result<Value, ToolError> {
        let t = find(name).ok_or_else(|| ToolError::Unknown(format!("unknown tool {name:?}")))?;
        let mut args = match args {
            Value::Null => json!({}),
            Value::Object(o) => Value::Object(o),
            _ => return Err(RpcError::invalid("arguments must be an object").into()),
        };
        if t.moderator {
            if t.write && !self.cfg.moderator.mcp_allow_writes {
                return Err(RpcError::new(
                    ErrorCode::InvalidParams,
                    format!("{name} is disabled: the moderator tools are read-only unless [moderator] mcp_allow_writes = true"),
                )
                .with_detail(json!({ "reason": "writes_disabled" }))
                .into());
            }
            return Ok(self.moderator(t, args).await?);
        }
        if t.write && !self.allow_writes {
            return Err(RpcError::new(
                ErrorCode::InvalidParams,
                format!("{name} is disabled: MCP is read-only unless [personas] mcp_allow_writes = true"),
            )
            .with_detail(json!({ "reason": "writes_disabled" }))
            .into());
        }
        let s = &self.scripted;
        Ok(match name {
            "list_runs" => {
                if let Some(o) = args.as_object_mut() {
                    o.entry("limit").or_insert(json!(DEFAULT_RUNS_LIMIT));
                }
                self.rpc(s, t.method, args).await?
            }
            "get_transcript" => self.transcript(args).await?,
            "start_sim" => self.start_sim(args).await?,
            "preflight" => {
                if let Some(d) = super::mcp_backend::probe(self.cfg.daemon.port).await? {
                    return Ok(d.call(t.method, json!({})).await?);
                }
                let state = self.live_state().await;
                let mut v = self.rpc(&state, t.method, json!({})).await?;
                let has_pf09 = v.as_array().is_some_and(|a| {
                    a.iter()
                        .any(|r| r["check"] == super::activity::PREFLIGHT_CHECK)
                });
                if let (false, Some(line), Some(arr)) = (
                    has_pf09,
                    super::activity::preflight_line(&self.cfg).await,
                    v.as_array_mut(),
                ) {
                    arr.push(serde_json::to_value(line).unwrap_or(Value::Null));
                }
                v
            }
            _ => self.rpc(s, t.method, args).await?,
        })
    }

    /// A moderator tool. It goes to the running daemon when one answers, so the live lane sees
    /// every write and the status carries its counts and connections; with no daemon the
    /// rules, mode and log are read and written in-process (the same files), and the Twitch
    /// tools refuse (the login and the lane live in the daemon).
    async fn moderator(&self, t: &ToolDef, args: Value) -> Result<Value, RpcError> {
        let (method, params) = match t.name {
            "moderator_character_get" => (
                "character.get",
                json!({ "id": self.cfg.moderator.character }),
            ),
            "moderator_character_set" => {
                return Err(RpcError::new(
                    ErrorCode::Unavailable,
                    format!(
                        "moderator_character_set is not available yet: no daemon RPC writes character.toml; edit {} and restart the daemon",
                        self.cfg
                            .paths
                            .characters
                            .join(&self.cfg.moderator.character)
                            .join("character.toml")
                            .display()
                    ),
                )
                .with_detail(json!({ "reason": "not_available" })))
            }
            _ => (t.method, args),
        };
        let v = if let Some(d) = super::mcp_backend::probe(self.cfg.daemon.port).await? {
            d.call(method, params).await?
        } else if t.name.starts_with("twitch_") {
            return Err(RpcError::new(
                ErrorCode::Unavailable,
                format!(
                    "{} needs the running daemon (shadowrunr daemon): the Twitch login and the moderation lane live there",
                    t.name
                ),
            )
            .with_detail(json!({ "reason": "no_daemon" })));
        } else {
            self.rpc(&self.scripted, method, params).await?
        };
        Ok(if t.name == "moderator_character_get" {
            character_view(&v)
        } else {
            v
        })
    }

    async fn transcript(&self, args: Value) -> Result<Value, RpcError> {
        let take = |k: &str| args.get(k).cloned();
        let limit = match take("limit") {
            None => DEFAULT_TRANSCRIPT_LIMIT,
            Some(v) => v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| RpcError::invalid("limit must be a positive integer"))?,
        };
        let limit = paging::page_limit(limit)?;
        let offset = match take("cursor").as_ref().and_then(Value::as_str) {
            None => 0,
            Some(c) => paging::decode_cursor(c, 1)?[0]
                .parse::<usize>()
                .map_err(|_| RpcError::invalid(format!("invalid cursor {c:?}")))?,
        };
        let mut params = json!({ "run_id": take("run_id").unwrap_or(Value::Null) });
        if let Some(t) = take("tournament_id") {
            params["tournament_id"] = t;
        }
        let events = self.rpc(&self.scripted, "run.transcript", params).await?;
        let events = events.as_array().cloned().unwrap_or_default();
        let total = events.len();
        let end = offset.saturating_add(limit).min(total);
        let items: Vec<Value> = events
            .into_iter()
            .enumerate()
            .skip(offset)
            .take(end.saturating_sub(offset))
            .map(|(i, event)| json!({ "seq": i + 1, "event": event }))
            .collect();
        let mut page = json!({ "items": items, "total": total });
        if end < total {
            page["next_cursor"] = json!(paging::encode_cursor(&[&end.to_string()]));
        }
        Ok(page)
    }

    /// The spec with every absent field taken from `[sim]`, as `shadowrunr sim run` does.
    fn spec_with_defaults(&self, given: Option<&Value>) -> Result<Value, RpcError> {
        let d = &self.cfg.sim;
        let given = match given {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(o)) => o.clone(),
            Some(_) => return Err(RpcError::invalid("spec must be an object")),
        };
        let audience = given
            .get("audience_size")
            .and_then(Value::as_u64)
            .and_then(|a| u32::try_from(a).ok())
            .unwrap_or(d.audience);
        let ser = |v: Result<Value, serde_json::Error>| {
            v.map_err(|e| RpcError::internal(format!("sim defaults: {e}")))
        };
        let mut spec = json!({
            "run_id": "",
            "character": d.character,
            "tone_variant": d.tone,
            "core_tier": ser(serde_json::to_value(d.tier))?,
            "core_effort": ser(serde_json::to_value(d.effort))?,
            "segment": ser(serde_json::to_value(d.segment))?,
            "audience_size": audience,
            "seed": d.seed,
            "duration_s": d.duration_s,
            "population": d.population_for(audience),
        });
        if let Value::Object(o) = &mut spec {
            o.extend(given);
        }
        Ok(spec)
    }

    async fn start_sim(&self, args: Value) -> Result<Value, RpcError> {
        let live = args.get("live").and_then(Value::as_bool).unwrap_or(false);
        let judge = args.get("judge").and_then(Value::as_bool).unwrap_or(false);
        let spec = self.spec_with_defaults(args.get("spec"))?;
        if let Some(d) = super::mcp_backend::probe(self.cfg.daemon.port).await? {
            d.check_engine(live)?;
            let res = d
                .call("sim.start", json!({ "spec": spec, "judge": judge }))
                .await?;
            let run_id = res["run_id"].as_str().unwrap_or_default().to_string();
            return Ok(json!({
                "run_id": run_id,
                "engine": d.engine(),
                "run_dir": self.run_dir(&run_id).to_string_lossy(),
                "host": "daemon",
            }));
        }
        let state = if live {
            self.live_state().await
        } else {
            self.scripted.clone()
        };
        let res = self
            .rpc(&state, "sim.start", json!({ "spec": spec, "judge": judge }))
            .await?;
        let run_id = res["run_id"].as_str().unwrap_or_default().to_string();
        if let Ok(mut s) = self.started.lock() {
            s.push((state.clone(), run_id.clone()));
        }
        Ok(json!({
            "run_id": run_id,
            "engine": if state.engine.is_scripted() { "scripted" } else { "live" },
            "run_dir": self.run_dir(&run_id).to_string_lossy(),
            "host": "mcp",
        }))
    }

    fn run_dir(&self, run_id: &str) -> std::path::PathBuf {
        self.cfg
            .paths
            .sim_runs()
            .join(&self.cfg.sim.adhoc_dir)
            .join(run_id)
    }

    /// Runs started in this process that are still in flight (a daemon-hosted run is the
    /// daemon's, never listed here).
    pub fn running(&self) -> Vec<String> {
        let Ok(s) = self.started.lock() else {
            return Vec::new();
        };
        s.iter()
            .filter(|(state, id)| state.live.get(id).is_some())
            .map(|(_, id)| id.clone())
            .collect()
    }

    /// Wait for every run started here to finish (a closed stdin must not cut a run short).
    pub async fn wait_idle(&self) {
        let mut told = false;
        loop {
            let running = self.running();
            if running.is_empty() {
                return;
            }
            if !told {
                eprintln!(
                    "shadowrunr mcp: waiting for {} to finish",
                    running.join(", ")
                );
                told = true;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}
