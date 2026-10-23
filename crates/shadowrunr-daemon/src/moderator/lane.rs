//! The moderation lane: receives every platform message, screens it ([`super::rules`]),
//! carries decisions out when armed, logs them ([`super::store`]), and asks
//! [`super::voice`] for the line. Runs whenever a platform is connected, show or no show.
//!
//! Pattern decisions are made as each message arrives. Messages no pattern took collect for
//! `[moderator] classifier_batch_ms` and go to the classifier as one call (none while no
//! enabled plain rule exists). A decision's platform action and its line start together, and
//! the action never waits on the line; the decision is logged once, after the line is ready or
//! the `ModVoice` route's timeout passed (then without a line).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use shadowrunr_types::moderation::{ModActionKind, ModDecision, ModLogEntry, ModMode, ModRule};
use shadowrunr_types::{ChatMsg, Engine};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::voice::ModVoice;
use super::{platform_name, rules, ModeratorHub};
use crate::platform::{ChatPlatform, PlatformError};

/// Messages per classifier call; a fuller batch goes out before its window ends.
pub const MAX_BATCH: usize = 50;

/// Writes the moderator's line for a decision. [`VoiceLiner`] is the character's voice; tests
/// put a slow or counting one in its place.
#[async_trait]
pub trait Liner: Send + Sync {
    async fn line(&self, decision: &ModDecision, rule_name: &str) -> String;
}

/// The moderator character's voice ([`ModVoice::line`]).
pub struct VoiceLiner {
    pub voice: ModVoice,
    pub engine: Arc<dyn Engine>,
}

#[async_trait]
impl Liner for VoiceLiner {
    async fn line(&self, decision: &ModDecision, rule_name: &str) -> String {
        self.voice.line(&self.engine, decision, rule_name).await
    }
}

/// One running lane.
#[derive(Clone)]
pub struct Lane {
    hub: Arc<ModeratorHub>,
    engine: Arc<dyn Engine>,
    liner: Arc<dyn Liner>,
}

/// Start the lane on the hub's inbox. `None` when a lane already runs on it.
pub fn spawn(
    hub: Arc<ModeratorHub>,
    engine: Arc<dyn Engine>,
    liner: Arc<dyn Liner>,
) -> Option<JoinHandle<()>> {
    let rx = hub.take_inbox_rx()?;
    // Load (and seed) the rules now, so a broken file is reported at start.
    let _ = hub.rules();
    let lane = Lane { hub, engine, liner };
    Some(tokio::spawn(lane.run(rx)))
}

async fn act_on(p: &dyn ChatPlatform, d: &ModDecision, reason: &str) -> Result<(), PlatformError> {
    match d.action {
        ModActionKind::Delete => p.delete(&d.message_id).await,
        ModActionKind::Timeout => {
            p.timeout(
                &d.viewer_id,
                d.timeout_s.unwrap_or(super::store::SEED_TIMEOUT_S),
                reason,
            )
            .await
        }
        ModActionKind::Ban => p.ban(&d.viewer_id, reason).await,
    }
}

impl Lane {
    async fn run(self, mut rx: mpsc::Receiver<ChatMsg>) {
        let batch_ms = self.hub.config().classifier_batch_ms;
        let mut pending: Vec<ChatMsg> = Vec::new();
        let mut deadline: Option<Instant> = None;
        loop {
            let wait = async move {
                match deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::select! {
                msg = rx.recv() => {
                    let Some(msg) = msg else { break };
                    if let Some(candidate) = self.screen(msg) {
                        if pending.is_empty() {
                            deadline = Some(Instant::now() + Duration::from_millis(batch_ms));
                        }
                        pending.push(candidate);
                        if pending.len() >= MAX_BATCH {
                            deadline = None;
                            self.flush(std::mem::take(&mut pending));
                        }
                    }
                }
                () = wait => {
                    deadline = None;
                    self.flush(std::mem::take(&mut pending));
                }
            }
        }
        if !pending.is_empty() {
            self.flush(pending);
        }
    }

    /// Screen one message at once. Returns it when it waits for the classifier.
    fn screen(&self, msg: ChatMsg) -> Option<ChatMsg> {
        if rules::exempt(&self.hub.policy(), &msg).is_some() {
            self.pass(msg);
            return None;
        }
        let set = self.hub.rules();
        if let Some(rule) = set.pattern_hit(&msg.text) {
            let rule = rule.clone();
            self.decide(&msg, &rule);
            return None;
        }
        if set.has_plain() {
            return Some(msg);
        }
        self.pass(msg);
        None
    }

    /// A message no rule acted on.
    fn pass(&self, msg: ChatMsg) {
        // No subscriber is the v1 normal: the send fails and the message is dropped.
        let _ = self.hub.screened().send(msg);
    }

    /// One classifier call for the batch, off the lane's path.
    fn flush(&self, batch: Vec<ChatMsg>) {
        if batch.is_empty() {
            return;
        }
        let lane = self.clone();
        tokio::spawn(async move {
            let set = lane.hub.rules();
            let hits = rules::classify(
                lane.engine.as_ref(),
                lane.hub.classifier_route(),
                &set,
                &batch,
            )
            .await;
            let mut taken = vec![None; batch.len()];
            for (i, id) in hits {
                if let Some(r) = set.get(&id) {
                    taken[i] = Some(r.clone());
                }
            }
            for (msg, rule) in batch.into_iter().zip(taken) {
                match rule {
                    Some(r) => lane.decide(&msg, &r),
                    None => lane.pass(msg),
                }
            }
        });
    }

    /// Build the decision (strikes counted now, in arrival order), then act, voice and log it
    /// on its own task.
    fn decide(&self, msg: &ChatMsg, rule: &ModRule) {
        let now = crate::runs::now_ms();
        let (action, timeout_s) = self.hub.strike(msg, rule, now);
        let mode = self.hub.mode();
        let decision = ModDecision {
            id: self.hub.decision_id(now),
            ts_ms: now,
            platform: msg.viewer.platform,
            viewer_id: msg.viewer.id.clone(),
            viewer_name: msg.viewer.name.clone(),
            message_id: msg.id.clone(),
            rule_id: rule.id.clone(),
            action,
            timeout_s,
            mode,
            executed: false,
            line: None,
            error: None,
        };
        self.hub.count_decision();
        let lane = self.clone();
        let rule_name = rule.name.clone();
        tokio::spawn(async move { lane.carry_out(decision, rule_name).await });
    }

    async fn carry_out(self, mut d: ModDecision, rule_name: String) {
        // The line starts with the action and never holds it up.
        let line = {
            let liner = self.liner.clone();
            let d = d.clone();
            let name = rule_name.clone();
            let timeout = self.hub.voice_timeout();
            tokio::spawn(async move {
                tokio::time::timeout(timeout, liner.line(&d, &name))
                    .await
                    .ok()
            })
        };
        let platform = self.hub.platform(d.platform);
        if d.mode == ModMode::Armed {
            match &platform {
                None => d.error = Some(format!("no {} adapter", platform_name(d.platform))),
                Some(p) => match act_on(p.as_ref(), &d, &rule_name).await {
                    Ok(()) => {
                        d.executed = true;
                        self.hub.count_executed();
                    }
                    Err(e) => d.error = Some(e.to_string()),
                },
            }
        }
        let line = line.await.ok().flatten();
        if line.is_none() {
            eprintln!(
                "moderator: decision {}: no line within the mod_voice timeout",
                d.id
            );
        }
        if let (true, Some(p), Some(l)) = (d.executed, &platform, &line) {
            if let Err(e) = p.say(l).await {
                eprintln!("moderator: decision {}: posting the line: {e}", d.id);
            }
        }
        d.line = line;
        if let Err(e) = self
            .hub
            .store()
            .append(&ModLogEntry::Decision { decision: d })
        {
            eprintln!("moderator: log decision: {e}");
        }
    }
}
