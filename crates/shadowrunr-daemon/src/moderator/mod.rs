//! The chat moderator (docs/spec/contract.md "Moderation", docs/guide/moderator.md).
//!
//! One lane in the daemon, ahead of everything else that reads chat. Every platform message
//! goes to [`lane`], which runs whether or not a show is on. Pattern rules decide at once,
//! plain-language rules go to a classifier in batches, and a self-disclosure is never acted
//! on. When armed, the lane deletes, times out or bans through [`crate::platform`], and
//! [`voice`] writes the moderator character's line. Every decision is appended to
//! `<data>/moderator/decisions.jsonl` ([`store`]).

pub mod lane;
pub mod rules;
pub mod store;
pub mod voice;

#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use shadowrunr_brain::moderation::Policy;
use shadowrunr_types::moderation::{
    ModCounts, ModDecision, ModLog, ModLogEntry, ModLogParams, ModMode, ModModeSetParams, ModRule,
    ModRules, ModRulesSetParams, ModRulesTestParams, ModRulesTestResult, ModUnban, ModUnbanParams,
    ModeratorStatus, PlatformState, TwitchConnect,
};
use shadowrunr_types::{ChatMsg, Engine, ErrorCode, Platform, Rank, Role, TsMs, Viewer};
use tokio::sync::{broadcast, mpsc};

use crate::config::Config;
use crate::platform::twitch::TwitchPlatform;
use crate::platform::{ChatInbox, ChatPlatform};
use crate::rpc::RpcError;
use crate::server::AppState;

use rules::{ClassifierRoute, RuleSet, Strikes};
use store::ModStore;

/// `[moderator]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeratorConfig {
    /// The moderator character's package id (`characters/<id>/`, `role = "moderator"`).
    pub character: String,
    /// The `shadowrunr mcp` moderator write tools run only when true.
    pub mcp_allow_writes: bool,
    /// How long the classifier collects messages before one call (plain-language rules).
    pub classifier_batch_ms: u64,
}

impl Default for ModeratorConfig {
    fn default() -> Self {
        Self {
            character: "moderator".into(),
            mcp_allow_writes: false,
            classifier_batch_ms: 1500,
        }
    }
}

/// `moderator.log` page size when `limit` is absent, and its cap.
pub const DEFAULT_LOG_LIMIT: u32 = 100;
pub const MAX_LOG_LIMIT: u32 = 1000;
/// Inbound messages buffered between the platforms and the lane.
const INBOX_CAPACITY: usize = 1024;
/// Screened messages buffered for subscribers of [`ModeratorHub::screened`].
const SCREENED_CAPACITY: usize = 256;

/// The moderator's state in the daemon (one per process). Building it touches nothing on disk:
/// the rule list is read (and seeded) on first use.
pub struct ModeratorHub {
    pub twitch: Arc<TwitchPlatform>,
    cfg: Config,
    platforms: Vec<Arc<dyn ChatPlatform>>,
    store: ModStore,
    rules: RwLock<Option<Arc<RuleSet>>>,
    /// Serialises `rules.set` (file write, then swap).
    rules_write: Mutex<()>,
    mode: RwLock<Option<ModMode>>,
    strikes: Mutex<Strikes>,
    policy: OnceLock<Arc<Policy>>,
    decisions: AtomicU32,
    executed: AtomicU32,
    next_id: AtomicU64,
    inbox: ChatInbox,
    inbox_rx: Mutex<Option<mpsc::Receiver<ChatMsg>>>,
    screened: broadcast::Sender<ChatMsg>,
}

impl ModeratorHub {
    pub fn new(cfg: &Config) -> Arc<Self> {
        let twitch = TwitchPlatform::new(cfg.twitch.clone());
        let platforms: Vec<Arc<dyn ChatPlatform>> = vec![twitch.clone()];
        Self::build(cfg, twitch, platforms)
    }

    /// A hub whose lane acts through `platforms` instead of the configured adapters (tests
    /// drive it with [`crate::platform::FakePlatform`]).
    pub fn with_platforms(cfg: &Config, platforms: Vec<Arc<dyn ChatPlatform>>) -> Arc<Self> {
        Self::build(cfg, TwitchPlatform::new(cfg.twitch.clone()), platforms)
    }

    fn build(
        cfg: &Config,
        twitch: Arc<TwitchPlatform>,
        platforms: Vec<Arc<dyn ChatPlatform>>,
    ) -> Arc<Self> {
        let (tx, rx) = mpsc::channel(INBOX_CAPACITY);
        let (screened, _) = broadcast::channel(SCREENED_CAPACITY);
        Arc::new(Self {
            twitch,
            cfg: cfg.clone(),
            platforms,
            store: ModStore::new(&cfg.paths.data, &cfg.paths.policy),
            rules: RwLock::new(None),
            rules_write: Mutex::new(()),
            mode: RwLock::new(None),
            strikes: Mutex::new(Strikes::default()),
            policy: OnceLock::new(),
            decisions: AtomicU32::new(0),
            executed: AtomicU32::new(0),
            next_id: AtomicU64::new(0),
            inbox: tx,
            inbox_rx: Mutex::new(Some(rx)),
            screened,
        })
    }

    pub fn config(&self) -> &ModeratorConfig {
        &self.cfg.moderator
    }

    pub fn store(&self) -> &ModStore {
        &self.store
    }

    /// Where platform adapters push inbound chat.
    pub fn inbox(&self) -> ChatInbox {
        self.inbox.clone()
    }

    /// The lane's end of the inbox; `None` once a lane took it.
    pub(crate) fn take_inbox_rx(&self) -> Option<mpsc::Receiver<ChatMsg>> {
        self.inbox_rx.lock().ok().and_then(|mut r| r.take())
    }

    /// Screened chat: every message no rule acted on (disclosures and moderators included),
    /// in the order the lane cleared it. This is the seam for the host's chat feed; nothing
    /// subscribes in v1, and a send with no subscriber is dropped.
    pub fn screened(&self) -> &broadcast::Sender<ChatMsg> {
        &self.screened
    }

    pub fn platforms(&self) -> &[Arc<dyn ChatPlatform>] {
        &self.platforms
    }

    pub fn platform(&self, p: Platform) -> Option<Arc<dyn ChatPlatform>> {
        self.platforms.iter().find(|x| x.platform() == p).cloned()
    }

    /// The term policy the disclosure check reads (`<policy>/platform-rules.json`, else the
    /// built-in rules).
    pub fn policy(&self) -> Arc<Policy> {
        self.policy
            .get_or_init(|| {
                crate::clips::policy(&self.cfg).unwrap_or_else(|e| {
                    eprintln!("moderator: {}; using the built-in policy", e.message);
                    Arc::new(Policy::builtin())
                })
            })
            .clone()
    }

    /// The live rule set, loaded (and seeded) on first use. A rules file that does not read
    /// leaves the lane with no rules (and a note on stderr) rather than guessing.
    pub fn rules(&self) -> Arc<RuleSet> {
        if let Some(r) = self.rules.read().ok().and_then(|r| r.clone()) {
            return r;
        }
        let set = match self.load_rules() {
            Ok(set) => set,
            Err(e) => {
                eprintln!("moderator: {e}; no rules apply until rules.json is fixed or replaced");
                Arc::new(RuleSet::default())
            }
        };
        if let Ok(mut w) = self.rules.write() {
            if let Some(existing) = w.as_ref() {
                return existing.clone();
            }
            *w = Some(set.clone());
        }
        set
    }

    fn load_rules(&self) -> Result<Arc<RuleSet>, String> {
        let rules = self.store.load_rules()?;
        rules::validate(&rules)?;
        Ok(Arc::new(RuleSet::new(rules)))
    }

    /// Replace the whole list: validated, written atomically, then applied to the next message.
    /// An invalid list refuses the write and the old list stays.
    pub fn set_rules(&self, rules: Vec<ModRule>) -> Result<Vec<ModRule>, RpcError> {
        rules::validate(&rules).map_err(RpcError::invalid)?;
        let _g = self.rules_write.lock().unwrap_or_else(|p| p.into_inner());
        self.store.save_rules(&rules).map_err(RpcError::io)?;
        let set = Arc::new(RuleSet::new(rules.clone()));
        if let Ok(mut w) = self.rules.write() {
            *w = Some(set);
        }
        Ok(rules)
    }

    pub fn mode(&self) -> ModMode {
        if let Some(m) = self.mode.read().ok().and_then(|m| *m) {
            return m;
        }
        let m = self.store.load_mode();
        if let Ok(mut w) = self.mode.write() {
            *w.get_or_insert(m)
        } else {
            m
        }
    }

    pub fn set_mode(&self, mode: ModMode) -> Result<(), RpcError> {
        self.store.save_mode(mode).map_err(RpcError::io)?;
        if let Ok(mut w) = self.mode.write() {
            *w = Some(mode);
        }
        Ok(())
    }

    pub fn counts(&self) -> ModCounts {
        ModCounts {
            decisions: self.decisions.load(Ordering::Relaxed),
            executed: self.executed.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn count_decision(&self) {
        self.decisions.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_executed(&self) {
        self.executed.fetch_add(1, Ordering::Relaxed);
    }

    /// A fresh decision id: `mod-<ms>-<n>`, unique across restarts.
    pub fn decision_id(&self, now_ms: TsMs) -> String {
        format!(
            "mod-{now_ms}-{}",
            self.next_id.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// Record a strike hit and return the (escalated) action.
    pub(crate) fn strike(
        &self,
        msg: &ChatMsg,
        rule: &ModRule,
        now_ms: TsMs,
    ) -> (shadowrunr_types::moderation::ModActionKind, Option<u32>) {
        self.strikes.lock().unwrap_or_else(|p| p.into_inner()).hit(
            msg.viewer.platform,
            &msg.viewer.id,
            rule,
            now_ms,
        )
    }

    pub fn classifier_route(&self) -> ClassifierRoute {
        let (tier, effort, timeout_ms) = self.cfg.engine.route(Role::ModClassifier);
        ClassifierRoute {
            tier,
            effort,
            timeout_ms,
        }
    }

    /// How long a decision's line may take before it is logged without one.
    pub fn voice_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.cfg.engine.route(Role::ModVoice).2)
    }

    pub fn status(&self) -> ModeratorStatus {
        ModeratorStatus {
            mode: self.mode(),
            character: self.cfg.moderator.character.clone(),
            platforms: self.platforms.iter().map(|p| p.status()).collect(),
            counts: self.counts(),
        }
    }

    /// Screen one message with no side effects (`moderator.rules.test`): no strike recorded,
    /// nothing logged, no platform call, no line.
    pub async fn test(&self, engine: &dyn Engine, msg: &ChatMsg) -> ModRulesTestResult {
        if let Some(why) = rules::exempt(&self.policy(), msg) {
            return ModRulesTestResult {
                decisions: Vec::new(),
                disclosure: why == rules::Exempt::Disclosure,
            };
        }
        let set = self.rules();
        let now = crate::runs::now_ms();
        let rule = match set.pattern_hit(&msg.text) {
            Some(r) => Some(r.clone()),
            None => rules::classify(
                engine,
                self.classifier_route(),
                &set,
                std::slice::from_ref(msg),
            )
            .await
            .into_iter()
            .find_map(|(_, id)| set.get(&id).cloned()),
        };
        let decisions = rule
            .map(|r| {
                let (action, timeout_s) = rules::escalate(&r, 0);
                vec![ModDecision {
                    id: self.decision_id(now),
                    ts_ms: now,
                    platform: msg.viewer.platform,
                    viewer_id: msg.viewer.id.clone(),
                    viewer_name: msg.viewer.name.clone(),
                    message_id: msg.id.clone(),
                    rule_id: r.id,
                    action,
                    timeout_s,
                    mode: self.mode(),
                    executed: false,
                    line: None,
                    error: None,
                }]
            })
            .unwrap_or_default();
        ModRulesTestResult {
            decisions,
            disclosure: false,
        }
    }

    /// Unban on `platform` when it is connected; the result is appended to the log either way.
    pub async fn unban(&self, platform: Platform, viewer_id: &str) -> ModUnban {
        let mut u = ModUnban {
            ts_ms: crate::runs::now_ms(),
            platform,
            viewer_id: viewer_id.to_string(),
            executed: false,
            error: None,
        };
        match self.platform(platform) {
            None => u.error = Some(format!("no {} adapter", platform_name(platform))),
            Some(p) => {
                let st = p.status();
                if st.state != PlatformState::Connected {
                    u.error =
                        Some(st.detail.unwrap_or_else(|| {
                            format!("{}: not connected", platform_name(platform))
                        }));
                } else {
                    match p.unban(viewer_id).await {
                        Ok(()) => u.executed = true,
                        Err(e) => u.error = Some(e.to_string()),
                    }
                }
            }
        }
        if let Err(e) = self.store.append(&ModLogEntry::Unban { unban: u.clone() }) {
            eprintln!("moderator: log unban: {e}");
        }
        u
    }
}

pub fn platform_name(p: Platform) -> &'static str {
    match p {
        Platform::Twitch => "twitch",
        Platform::Youtube => "youtube",
        Platform::Kick => "kick",
        Platform::Sim => "sim",
    }
}

/// Starts the lane and the configured platforms. Called once by the daemon after bind.
pub fn start(state: &Arc<AppState>) {
    let hub = &state.moderator;
    let (voice, err) = voice::ModVoice::load(&state.cfg);
    if let Some(e) = err {
        eprintln!("moderator: character {:?}: {e}", hub.config().character);
    }
    let engine = state.engine.engine.clone();
    let liner = Arc::new(lane::VoiceLiner {
        voice,
        engine: engine.clone(),
    });
    lane::spawn(hub.clone(), engine, liner);
    if !state.cfg.twitch.client_id.trim().is_empty() {
        hub.twitch.start(hub.inbox());
    }
}

pub async fn rpc_status(state: &Arc<AppState>) -> Result<ModeratorStatus, RpcError> {
    Ok(state.moderator.status())
}

pub async fn rpc_rules_list(state: &Arc<AppState>) -> Result<ModRules, RpcError> {
    let hub = &state.moderator;
    // Surface a broken rules.json here, where the operator looks.
    if hub.rules.read().ok().and_then(|r| r.clone()).is_none() {
        let rules = hub.store.load_rules().map_err(RpcError::io)?;
        rules::validate(&rules).map_err(|e| RpcError::io(format!("rules.json: {e}")))?;
    }
    Ok(ModRules {
        rules: hub.rules().rules.clone(),
    })
}

pub async fn rpc_rules_set(
    state: &Arc<AppState>,
    p: ModRulesSetParams,
) -> Result<ModRules, RpcError> {
    let rules = state.moderator.set_rules(p.rules)?;
    Ok(ModRules { rules })
}

pub async fn rpc_rules_test(
    state: &Arc<AppState>,
    p: ModRulesTestParams,
) -> Result<ModRulesTestResult, RpcError> {
    let name = p
        .viewer_name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "tester".into());
    let msg = ChatMsg {
        id: "test".into(),
        ts_ms: crate::runs::now_ms(),
        viewer: Viewer {
            id: "test".into(),
            name,
            platform: Platform::Twitch,
            rank: Rank::Thrall,
            sub_tier: None,
            months: 0,
            gifted: 0,
            is_mod: false,
        },
        text: p.text,
        command: None,
    };
    Ok(state
        .moderator
        .test(state.engine.engine.as_ref(), &msg)
        .await)
}

pub async fn rpc_mode_set(
    state: &Arc<AppState>,
    p: ModModeSetParams,
) -> Result<ModeratorStatus, RpcError> {
    state.moderator.set_mode(p.mode)?;
    Ok(state.moderator.status())
}

pub async fn rpc_log(state: &Arc<AppState>, p: ModLogParams) -> Result<ModLog, RpcError> {
    let limit = p.limit.unwrap_or(DEFAULT_LOG_LIMIT).clamp(1, MAX_LOG_LIMIT);
    let entries = state
        .moderator
        .store
        .read_log(limit as usize)
        .map_err(RpcError::io)?;
    Ok(ModLog { entries })
}

pub async fn rpc_unban(state: &Arc<AppState>, p: ModUnbanParams) -> Result<ModUnban, RpcError> {
    if p.viewer_id.trim().is_empty() {
        return Err(RpcError::invalid("viewer_id is empty"));
    }
    Ok(state.moderator.unban(p.platform, &p.viewer_id).await)
}

pub async fn rpc_twitch_connect(state: &Arc<AppState>) -> Result<TwitchConnect, RpcError> {
    state
        .moderator
        .twitch
        .begin_device_login()
        .await
        .map_err(|e| RpcError::new(ErrorCode::Unavailable, e.to_string()))
}

pub async fn rpc_twitch_disconnect(state: &Arc<AppState>) -> Result<ModeratorStatus, RpcError> {
    state.moderator.twitch.disconnect().await;
    Ok(state.moderator.status())
}
