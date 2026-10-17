//! Background worker pool. Fixes three Athena faults:
//! 1. jobs ran one at a time → a bounded pool (`pool_size` concurrent jobs);
//! 2. jobs could not be cancelled → a cancellation token per job;
//! 3. results landed in the default session → every update carries the origin (stream or
//!    thread id) that dispatched it, and `collect(origin)` returns only that origin's.
//!
//! The core thread never awaits a job: `collect` is a non-blocking drain.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use shadowrunr_types::{Engine, EngineRequest, Role};
use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Progress(String),
    Done(String),
    Failed(String),
    Cancelled,
}

impl JobStatus {
    /// The `StreamEvent::Job.status` wire string.
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Progress(_) => "progress",
            JobStatus::Done(_) => "done",
            JobStatus::Failed(_) => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }

    pub fn summary(&self) -> Option<&str> {
        match self {
            JobStatus::Progress(s) | JobStatus::Done(s) | JobStatus::Failed(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Done(_) | JobStatus::Failed(_) | JobStatus::Cancelled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobUpdate {
    pub job_id: String,
    pub origin: String,
    pub brief: String,
    pub status: JobStatus,
}

/// What a running job can see: its id, brief, cancellation token and a progress channel.
#[derive(Clone)]
pub struct JobCtx {
    pub job_id: String,
    pub brief: String,
    pub cancel: CancellationToken,
    origin: String,
    tx: mpsc::UnboundedSender<JobUpdate>,
}

impl JobCtx {
    pub fn progress(&self, msg: impl Into<String>) {
        let _ = self.tx.send(JobUpdate {
            job_id: self.job_id.clone(),
            origin: self.origin.clone(),
            brief: self.brief.clone(),
            status: JobStatus::Progress(msg.into()),
        });
    }
}

/// A unit of background work. `Ok` is the result summary, `Err` the failure reason.
#[async_trait]
pub trait JobRunner: Send + Sync {
    async fn run(&self, ctx: JobCtx) -> Result<String, String>;
}

/// Research through the engine with `Role::Researcher`.
pub struct ResearchRunner {
    pub engine: Arc<dyn Engine>,
    pub character_name: String,
}

#[async_trait]
impl JobRunner for ResearchRunner {
    async fn run(&self, ctx: JobCtx) -> Result<String, String> {
        let (tier, effort, timeout_ms) = shadowrunr_engine::route(Role::Researcher);
        let req = EngineRequest {
            role: Role::Researcher,
            tier,
            effort,
            system: format!(
                "You are a research minion for {}, an AI livestream character. Research the \
                 brief using public sources only. Return a concise factual summary (at most \
                 600 characters) with the key source named. Mark anything unconfirmed as \
                 alleged. Never research private individuals.",
                self.character_name
            ),
            prompt: ctx.brief.clone(),
            allowed_tools: vec!["WebSearch".into(), "WebFetch".into()],
            timeout_ms,
            json_schema: None,
            correlation_id: Some(ctx.job_id.clone()),
        };
        ctx.progress("researching");
        self.engine
            .complete(req)
            .await
            .map(|r| r.text.trim().to_string())
            .map_err(|e| e.to_string())
    }
}

/// A job to dispatch.
pub struct Job {
    /// Preferred id (e.g. the model's `dispatch_research.job_id`); made unique if taken.
    pub job_id: Option<String>,
    pub brief: String,
    pub runner: Arc<dyn JobRunner>,
}

pub struct WorkerPool {
    size: usize,
    sem: Arc<Semaphore>,
    tx: mpsc::UnboundedSender<JobUpdate>,
    rx: Mutex<mpsc::UnboundedReceiver<JobUpdate>>,
    routed: Mutex<HashMap<String, Vec<JobUpdate>>>,
    tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
    counter: AtomicU64,
}

impl WorkerPool {
    pub fn new(pool_size: usize) -> Self {
        let size = pool_size.max(1);
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            size,
            sem: Arc::new(Semaphore::new(size)),
            tx,
            rx: Mutex::new(rx),
            routed: Mutex::new(HashMap::new()),
            tokens: Arc::new(Mutex::new(HashMap::new())),
            counter: AtomicU64::new(0),
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// Jobs that are queued or running.
    pub fn active(&self) -> usize {
        self.tokens.lock().map(|t| t.len()).unwrap_or(0)
    }

    /// Queue a job for `origin`. Never blocks; must be called inside a tokio runtime.
    pub fn dispatch(&self, origin: &str, job: Job) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let job_id = {
            let tokens = self.tokens.lock().unwrap_or_else(|p| p.into_inner());
            match job.job_id.filter(|s| !s.trim().is_empty()) {
                Some(id) if !tokens.contains_key(&id) => id,
                Some(id) => format!("{id}-{n}"),
                None => format!("job-{n}"),
            }
        };
        let token = CancellationToken::new();
        self.tokens
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(job_id.clone(), token.clone());

        let update = {
            let job_id = job_id.clone();
            let origin = origin.to_string();
            let brief = job.brief.clone();
            move |status| JobUpdate {
                job_id: job_id.clone(),
                origin: origin.clone(),
                brief: brief.clone(),
                status,
            }
        };
        let tx = self.tx.clone();
        let _ = tx.send(update(JobStatus::Queued));
        let ctx = JobCtx {
            job_id: job_id.clone(),
            brief: job.brief,
            cancel: token.clone(),
            origin: origin.to_string(),
            tx: tx.clone(),
        };
        let sem = self.sem.clone();
        let tokens = self.tokens.clone();
        let runner = job.runner;
        let id_for_task = job_id.clone();
        tokio::spawn(async move {
            let status = tokio::select! {
                biased;
                _ = token.cancelled() => JobStatus::Cancelled,
                permit = sem.acquire_owned() => match permit {
                    Err(_) => JobStatus::Failed("pool closed".into()),
                    Ok(_permit) => {
                        let _ = tx.send(update(JobStatus::Running));
                        tokio::select! {
                            biased;
                            _ = token.cancelled() => JobStatus::Cancelled,
                            r = runner.run(ctx) => match r {
                                Ok(s) => JobStatus::Done(s),
                                Err(e) => JobStatus::Failed(e),
                            },
                        }
                    }
                },
            };
            tokens
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&id_for_task);
            let _ = tx.send(update(status));
        });
        job_id
    }

    /// Cancel a queued or running job. Returns false when the job is unknown or finished.
    pub fn cancel(&self, job_id: &str) -> bool {
        let tokens = self.tokens.lock().unwrap_or_else(|p| p.into_inner());
        match tokens.get(job_id) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    /// Non-blocking: every update for `origin` since the last call, in arrival order.
    /// Updates for other origins stay queued for them.
    pub fn collect(&self, origin: &str) -> Vec<JobUpdate> {
        let mut routed = self.routed.lock().unwrap_or_else(|p| p.into_inner());
        {
            let mut rx = self.rx.lock().unwrap_or_else(|p| p.into_inner());
            while let Ok(u) = rx.try_recv() {
                routed.entry(u.origin.clone()).or_default().push(u);
            }
        }
        routed.remove(origin).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    struct SlowJob {
        current: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
        ms: u64,
    }

    #[async_trait]
    impl JobRunner for SlowJob {
        async fn run(&self, ctx: JobCtx) -> Result<String, String> {
            let now = self.current.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            ctx.progress("halfway");
            tokio::time::sleep(Duration::from_millis(self.ms)).await;
            self.current.fetch_sub(1, Ordering::SeqCst);
            Ok(format!("{} done", ctx.brief))
        }
    }

    async fn wait_terminal(pool: &WorkerPool, origin: &str, n: usize, acc: &mut Vec<JobUpdate>) {
        for _ in 0..400 {
            acc.extend(pool.collect(origin));
            if acc.iter().filter(|u| u.status.is_terminal()).count() >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("jobs did not finish: {acc:?}");
    }

    #[tokio::test]
    async fn bounded_concurrency_cancel_and_routing() {
        let pool = WorkerPool::new(2);
        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mk = |brief: &str, ms| Job {
            job_id: None,
            brief: brief.into(),
            runner: Arc::new(SlowJob {
                current: current.clone(),
                peak: peak.clone(),
                ms,
            }),
        };
        let a1 = pool.dispatch("stream-a", mk("a1", 150));
        let a2 = pool.dispatch("stream-a", mk("a2", 150));
        let b1 = pool.dispatch("thread-b", mk("b1", 150));
        let a3 = pool.dispatch("stream-a", mk("a3", 5_000));
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(pool.cancel(&a3), "a3 is queued or running");

        let mut a = Vec::new();
        wait_terminal(&pool, "stream-a", 3, &mut a).await;
        let mut b = Vec::new();
        wait_terminal(&pool, "thread-b", 1, &mut b).await;

        assert_eq!(
            peak.load(Ordering::SeqCst),
            2,
            "never more than pool_size at once"
        );
        assert!(a.iter().all(|u| u.origin == "stream-a"));
        assert!(b.iter().all(|u| u.origin == "thread-b"));
        let terminal = |v: &[JobUpdate], id: &str| {
            v.iter()
                .find(|u| u.job_id == id && u.status.is_terminal())
                .map(|u| u.status.clone())
        };
        assert_eq!(terminal(&a, &a1), Some(JobStatus::Done("a1 done".into())));
        assert_eq!(terminal(&a, &a2), Some(JobStatus::Done("a2 done".into())));
        assert_eq!(terminal(&a, &a3), Some(JobStatus::Cancelled));
        assert_eq!(terminal(&b, &b1), Some(JobStatus::Done("b1 done".into())));
        assert!(a.iter().any(|u| u.status == JobStatus::Queued));
        assert!(b
            .iter()
            .any(|u| u.status == JobStatus::Progress("halfway".into())));
        assert!(!pool.cancel(&a1), "finished jobs cannot be cancelled");
        assert_eq!(pool.active(), 0);
    }

    #[tokio::test]
    async fn duplicate_job_ids_are_made_unique() {
        let pool = WorkerPool::new(1);
        let peak = Arc::new(AtomicUsize::new(0));
        let mk = || Job {
            job_id: Some("r1".into()),
            brief: "x".into(),
            runner: Arc::new(SlowJob {
                current: Arc::new(AtomicUsize::new(0)),
                peak: peak.clone(),
                ms: 200,
            }),
        };
        let x = pool.dispatch("s", mk());
        let y = pool.dispatch("s", mk());
        assert_eq!(x, "r1");
        assert_ne!(x, y);
    }
}
