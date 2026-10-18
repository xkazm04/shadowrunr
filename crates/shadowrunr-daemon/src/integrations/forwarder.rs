//! The activity forwarder: a bounded queue and one delivery task with retry, backoff,
//! drop-and-count and a down cooldown. See `super::activity` for the policy.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::mpsc;

use super::activity::{payload, ActivityEvent, Filter};
use super::settings::ActivitySettings;
use super::signing::{sign, Secret, SIGNATURE_HEADER};

/// Everything the delivery task needs.
#[derive(Debug, Clone)]
pub struct ForwarderConfig {
    pub endpoint: String,
    pub secret: Secret,
    pub filter: Filter,
    pub project_id: Option<String>,
    pub queue: usize,
    pub max_attempts: u32,
    pub backoff: Duration,
    pub max_backoff: Duration,
    pub timeout: Duration,
    pub down_cooldown: Duration,
}

impl ForwarderConfig {
    /// From `[personas.activity]`, reading the secret from its environment variable.
    pub fn from_settings(a: &ActivitySettings, project_id: Option<String>) -> Result<Self, String> {
        let problems = a.problems();
        if !problems.is_empty() {
            return Err(problems.join("; "));
        }
        Ok(Self {
            endpoint: a.endpoint(),
            secret: Secret::from_env(a.secret_env.trim())?,
            filter: Filter::from_names(&a.events),
            project_id,
            queue: a.queue,
            max_attempts: a.max_attempts,
            backoff: a.backoff(),
            max_backoff: a.max_backoff(),
            timeout: a.timeout(),
            down_cooldown: a.down_cooldown(),
        })
    }
}

#[derive(Debug, Default)]
struct Stats {
    accepted: AtomicU64,
    processed: AtomicU64,
    delivered: AtomicU64,
    rejected: AtomicU64,
    failed: AtomicU64,
    dropped_full: AtomicU64,
    dropped_down: AtomicU64,
    lagged: AtomicU64,
    down: AtomicBool,
}

/// Counters since the forwarder started.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct StatsSnapshot {
    /// Queued for delivery.
    pub accepted: u64,
    pub delivered: u64,
    /// Refused by Personas (4xx other than 408/422/429); not retried.
    pub rejected: u64,
    /// Exhausted every attempt.
    pub failed: u64,
    /// Dropped because the queue was full.
    pub dropped_full: u64,
    /// Dropped without an attempt while Personas was marked down.
    pub dropped_down: u64,
    /// Live-run events the watcher fell behind on.
    pub lagged: u64,
    pub down: bool,
}

impl StatsSnapshot {
    /// Every event that did not reach Personas.
    pub fn dropped(&self) -> u64 {
        self.failed + self.dropped_full + self.dropped_down + self.lagged
    }
}

impl Stats {
    fn snapshot(&self) -> StatsSnapshot {
        let l = |a: &AtomicU64| a.load(Ordering::Relaxed);
        StatsSnapshot {
            accepted: l(&self.accepted),
            delivered: l(&self.delivered),
            rejected: l(&self.rejected),
            failed: l(&self.failed),
            dropped_full: l(&self.dropped_full),
            dropped_down: l(&self.dropped_down),
            lagged: l(&self.lagged),
            down: self.down.load(Ordering::Relaxed),
        }
    }
}

/// What [`Forwarder::forward`] did with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forwarded {
    Queued,
    /// Not in the filter.
    Filtered,
    /// The queue was full (or the task is gone); counted.
    Dropped,
}

/// The handle: cheap to call from any task, never blocks.
pub struct Forwarder {
    tx: mpsc::Sender<ActivityEvent>,
    filter: Filter,
    stats: Arc<Stats>,
}

impl Forwarder {
    /// Start the delivery task. Must be called inside a tokio runtime.
    pub fn spawn(cfg: ForwarderConfig) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .no_proxy()
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        let (tx, rx) = mpsc::channel(cfg.queue.max(1));
        let stats = Arc::new(Stats::default());
        let filter = cfg.filter.clone();
        tokio::spawn(deliver_all(rx, cfg, http, stats.clone()));
        Ok(Self { tx, filter, stats })
    }

    pub fn forward(&self, e: ActivityEvent) -> Forwarded {
        if !self.filter.wants(&e) {
            return Forwarded::Filtered;
        }
        match self.tx.try_send(e) {
            Ok(()) => {
                self.stats.accepted.fetch_add(1, Ordering::Relaxed);
                Forwarded::Queued
            }
            Err(_) => {
                self.stats.dropped_full.fetch_add(1, Ordering::Relaxed);
                Forwarded::Dropped
            }
        }
    }

    pub fn stats(&self) -> StatsSnapshot {
        self.stats.snapshot()
    }

    pub(crate) fn note_lagged(&self, n: u64) {
        self.stats.lagged.fetch_add(n, Ordering::Relaxed);
    }

    /// Wait until every queued event was delivered, refused or dropped; false on timeout.
    pub async fn drain(&self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        loop {
            let done = self.stats.processed.load(Ordering::Relaxed)
                >= self.stats.accepted.load(Ordering::Relaxed);
            if done {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

enum Outcome {
    Delivered,
    Rejected(u16),
    Failed(String),
}

fn retryable(status: u16) -> bool {
    status >= 500 || matches!(status, 408 | 422 | 429)
}

async fn deliver_one(http: &reqwest::Client, cfg: &ForwarderConfig, body: Vec<u8>) -> Outcome {
    let signature = sign(&cfg.secret, &body);
    let mut last = String::new();
    for attempt in 1..=cfg.max_attempts.max(1) {
        let res = http
            .post(&cfg.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(SIGNATURE_HEADER, &signature)
            .header(
                reqwest::header::USER_AGENT,
                concat!("shadowrunr/", env!("CARGO_PKG_VERSION")),
            )
            .body(body.clone())
            .send()
            .await;
        let mut retry_after = None;
        match res {
            Ok(r) if r.status().is_success() => return Outcome::Delivered,
            Ok(r) => {
                let status = r.status().as_u16();
                if !retryable(status) {
                    return Outcome::Rejected(status);
                }
                retry_after = r
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map(Duration::from_secs);
                last = format!("HTTP {status}");
            }
            Err(e) => {
                last = if e.is_timeout() {
                    "timed out".into()
                } else if e.is_connect() {
                    "connection refused".into()
                } else {
                    "request failed".into()
                };
            }
        }
        if attempt < cfg.max_attempts {
            let exp = cfg.backoff.saturating_mul(1u32 << (attempt - 1).min(16));
            let delay = retry_after.unwrap_or(exp).min(cfg.max_backoff);
            tokio::time::sleep(delay).await;
        }
    }
    Outcome::Failed(last)
}

async fn deliver_all(
    mut rx: mpsc::Receiver<ActivityEvent>,
    cfg: ForwarderConfig,
    http: reqwest::Client,
    stats: Arc<Stats>,
) {
    let mut down_until: Option<Instant> = None;
    while let Some(e) = rx.recv().await {
        if down_until.is_some_and(|t| Instant::now() < t) {
            stats.dropped_down.fetch_add(1, Ordering::Relaxed);
            stats.processed.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let dropped = stats.snapshot().dropped();
        let body = payload(
            &e,
            cfg.project_id.as_deref(),
            dropped,
            crate::runs::now_ms(),
        );
        match deliver_one(&http, &cfg, body).await {
            Outcome::Delivered => {
                stats.delivered.fetch_add(1, Ordering::Relaxed);
                down_until = None;
                if stats.down.swap(false, Ordering::Relaxed) {
                    eprintln!("personas activity: webhook reachable again");
                }
            }
            Outcome::Rejected(status) => {
                stats.rejected.fetch_add(1, Ordering::Relaxed);
                eprintln!(
                    "personas activity: {} refused by the webhook (HTTP {status}); check trigger_id and the secret",
                    e.kind.as_str()
                );
            }
            Outcome::Failed(why) => {
                stats.failed.fetch_add(1, Ordering::Relaxed);
                down_until = Some(Instant::now() + cfg.down_cooldown);
                if !stats.down.swap(true, Ordering::Relaxed) {
                    eprintln!(
                        "personas activity: webhook unreachable ({why}); dropping events for {:?}",
                        cfg.down_cooldown
                    );
                }
            }
        }
        stats.processed.fetch_add(1, Ordering::Relaxed);
    }
}
