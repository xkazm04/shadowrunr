//! Warm sessions.
//!
//! [`ClaudeWarmSession`] keeps one live `claude -p --input-format stream-json` process.
//! `send` writes a user-message line and reads until the `result` line. `interrupt` writes
//! the stream-json control request `{"type":"control_request","request_id":…,
//! "request":{"subtype":"interrupt"}}`. That message is not in `claude --help`; it is the
//! wire form behind the Agent SDK's documented `interrupt()`, and it was verified live on
//! 2.1.302 (`tests/fixtures/claude_session_interrupt.jsonl` ends with the matching
//! `control_response`). If no acknowledgement arrives within 5 s, the process is killed and
//! the next `send` respawns it (resuming the session id).
//!
//! The process is reaped after the idle timeout (default 20 min) and respawned with
//! `--resume <session_id>` on the next `send`. If it dies mid-turn, the session respawns once
//! and retries. A failed turn walks the same ladder as a one-shot call ([`crate::ladder`]):
//! a rate limit or a bad login cools the account fleet-wide, an overload does not, and either
//! way the session moves to the next healthy account (fresh context, `fallback_reason` set);
//! a request-side failure is returned unchanged. When no account is left and an API key is
//! configured, the session degrades in place to a Messages API conversation seeded with its
//! own turn mirror, and returns to a CLI account (fresh process) once one is healthy again.
//! Every answered turn reports the fleet mode.
//!
//! [`ApiSession`] is the fleet-loss stand-in: a Messages API conversation that keeps its own
//! history. Its `interrupt` is a no-op because a non-streaming request cannot be cut short.

use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::json;
use shadowrunr_types::{EngineError, EngineKind, EngineReply, EngineRequest, WarmSession};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, OwnedSemaphorePermit};
use tokio::task::JoinHandle;

use crate::api::Turn;
use crate::claude_cli::{elapsed_ms, CallMode};
use crate::fleet::{api_reply, FleetInner};
use crate::ladder::{self, Attempts, Step};
use crate::ledger::LedgerRow;
use crate::limits::Failure;
use crate::stream::{Feed, TurnOutcome, TurnParser};

/// How long `interrupt` waits for the control response before killing the process.
pub const INTERRUPT_ACK_WAIT: Duration = Duration::from_secs(5);

struct LiveProc {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    _permit: OwnedSemaphorePermit,
}

impl LiveProc {
    async fn write_line(&mut self, v: &serde_json::Value) -> std::io::Result<()> {
        let mut line = v.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await
    }

    async fn kill(mut self) {
        let _ = self.child.kill().await;
    }
}

struct Shared {
    proc: Mutex<Option<LiveProc>>,
    epoch: Instant,
    last_used_ms: AtomicU64,
    idle_ms: u64,
}

impl Shared {
    fn touch(&self) {
        self.last_used_ms
            .store(elapsed_ms(self.epoch), Ordering::SeqCst);
    }
    fn idle_for_ms(&self) -> u64 {
        elapsed_ms(self.epoch).saturating_sub(self.last_used_ms.load(Ordering::SeqCst))
    }
}

/// Kill the process once it has been idle for `idle_ms`. Ends when the session is gone.
fn spawn_reaper(shared: Weak<Shared>, idle_ms: u64) -> JoinHandle<()> {
    let tick = Duration::from_millis((idle_ms / 4).clamp(10, 30_000));
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tick).await;
            let Some(s) = shared.upgrade() else { return };
            if s.idle_for_ms() < s.idle_ms {
                continue;
            }
            // A held lock means a turn is running: not idle.
            let reaped = match s.proc.try_lock() {
                Ok(mut g) if s.idle_for_ms() >= s.idle_ms => g.take(),
                _ => None,
            };
            if let Some(p) = reaped {
                p.kill().await;
            }
        }
    })
}

enum TurnErr {
    Died(String),
    Timeout,
    Failed(Failure),
}

pub struct ClaudeWarmSession {
    inner: Arc<FleetInner>,
    account_idx: usize,
    template: EngineRequest,
    shared: Arc<Shared>,
    session_id: Option<String>,
    /// A `send` was cancelled mid-turn; its result line is still pending on stdout.
    in_flight: bool,
    fallback_reason: Option<String>,
    seq: u64,
    reaper: JoinHandle<()>,
    /// Every answered turn since open (user prompt, assistant text), whichever backend
    /// answered it: what an in-place switch to the API carries.
    mirror: Vec<Turn>,
    /// No account was left: turns go to the Messages API until one is healthy again.
    on_api: bool,
}

impl ClaudeWarmSession {
    /// Open on the given account and spawn the process eagerly, so the first `send` is warm.
    pub async fn open(
        inner: Arc<FleetInner>,
        account_idx: usize,
        template: EngineRequest,
    ) -> Result<Self, EngineError> {
        let idle_ms = inner.cfg.idle_timeout_ms();
        let shared = Arc::new(Shared {
            proc: Mutex::new(None),
            epoch: Instant::now(),
            last_used_ms: AtomicU64::new(0),
            idle_ms,
        });
        let reaper = spawn_reaper(Arc::downgrade(&shared), idle_ms);
        let s = Self {
            inner,
            account_idx,
            template,
            shared,
            session_id: None,
            in_flight: false,
            fallback_reason: None,
            seq: 0,
            reaper,
            mirror: Vec::new(),
            on_api: false,
        };
        let p = s
            .spawn(None)
            .await
            .map_err(|f| f.into_engine_error(&s.inner.accounts[s.account_idx].label))?;
        *s.shared.proc.lock().await = Some(p);
        s.shared.touch();
        Ok(s)
    }

    pub fn account_label(&self) -> &str {
        &self.inner.accounts[self.account_idx].label
    }

    async fn spawn(&self, resume: Option<String>) -> Result<LiveProc, Failure> {
        let cli = &self.inner.cli;
        let permit = cli
            .sem
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Failure::Transient {
                detail: "process semaphore closed".into(),
            })?;
        let sp = cli
            .system_prompt_file(&self.template.system)
            .map_err(Failure::Fatal)?;
        let argv = cli.argv_for(&self.template, &sp, CallMode::Warm { resume });
        let account = &self.inner.accounts[self.account_idx];
        let mut cmd = cli.command(account, &argv);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| Failure::Transient {
            detail: format!("spawn {}: {e}", cli.bin),
        })?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(Failure::Transient {
                detail: "child pipes missing".into(),
            });
        };
        Ok(LiveProc {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            _permit: permit,
        })
    }

    async fn turn(p: &mut LiveProc, prompt: &str, timeout_ms: u64) -> Result<TurnOutcome, TurnErr> {
        let msg = json!({"type": "user", "message": {"role": "user", "content": prompt}});
        let started = Instant::now();
        p.write_line(&msg)
            .await
            .map_err(|e| TurnErr::Died(format!("write: {e}")))?;
        let mut parser = TurnParser::new();
        let read = async {
            loop {
                match p.lines.next_line().await {
                    Ok(Some(l)) => {
                        if parser.feed(&l, elapsed_ms(started)) == Feed::Done {
                            return Ok(());
                        }
                    }
                    Ok(None) => return Err(TurnErr::Died("process exited mid-turn".into())),
                    Err(e) => return Err(TurnErr::Died(format!("read: {e}"))),
                }
            }
        };
        match tokio::time::timeout(Duration::from_millis(timeout_ms), read).await {
            Err(_) => Err(TurnErr::Timeout),
            Ok(Err(e)) => Err(e),
            Ok(Ok(())) => parser.finish().map_err(TurnErr::Failed),
        }
    }

    /// Read and discard until the pending result of a cancelled turn arrives.
    async fn drain_result(p: &mut LiveProc, wait: Duration) -> bool {
        let read = async {
            let mut parser = TurnParser::new();
            while let Ok(Some(l)) = p.lines.next_line().await {
                if parser.feed(&l, 0) == Feed::Done {
                    return true;
                }
            }
            false
        };
        tokio::time::timeout(wait, read).await.unwrap_or(false)
    }

    fn row(&self, reply: &EngineReply) -> LedgerRow {
        let mut r = LedgerRow::from_reply(
            self.inner.now(),
            self.template.role,
            reply,
            self.template.correlation_id.clone(),
        );
        r.warm = true;
        r
    }

    /// The API rung of a warm turn: no account is left, so the conversation continues in
    /// place on the Messages API, seeded with the mirror. Without an API key the fleet is
    /// down.
    async fn api_rung(
        &mut self,
        prompt: String,
        mut attempts: Attempts,
    ) -> Result<EngineReply, EngineError> {
        if self.inner.api.is_none() {
            ladder::down(&self.inner);
            return Err(attempts.unavailable());
        }
        self.on_api = true;
        self.fallback_reason = Some(attempts.joined());
        let res = api_turn(
            &self.inner,
            &self.template,
            &mut self.mirror,
            prompt,
            attempts.joined(),
        )
        .await;
        match res {
            Ok(reply) => Ok(reply),
            Err(f) => {
                let msg = f.clone().into_engine_error("api");
                if let Step::Stop(e) = ladder::step(f, "api") {
                    return Err(e);
                }
                attempts.push(format!("api: {msg}"));
                ladder::down(&self.inner);
                Err(attempts.unavailable())
            }
        }
    }

    fn fail_row(&self, started: Instant, kind: &str) {
        let label = self.account_label().to_string();
        self.inner.record(self.inner.failure_row(
            &self.template,
            EngineKind::ClaudeCli,
            Some(label),
            started.elapsed().as_millis() as u64,
            kind,
            true,
        ));
    }
}

#[async_trait]
impl WarmSession for ClaudeWarmSession {
    async fn send(&mut self, prompt: String) -> Result<EngineReply, EngineError> {
        let shared = self.shared.clone();
        let mut guard = shared.proc.lock().await;
        if self.in_flight {
            let drained = match guard.as_mut() {
                Some(p) => Self::drain_result(p, INTERRUPT_ACK_WAIT).await,
                None => true,
            };
            if !drained {
                if let Some(p) = guard.take() {
                    p.kill().await;
                }
            }
            self.in_flight = false;
        }
        if self.on_api {
            // Degraded in place: come back to a CLI account (fresh process; the API turns
            // stay in the mirror) as soon as one is healthy again.
            match self.inner.pick_account(&[]) {
                Some(j) => {
                    self.on_api = false;
                    self.account_idx = j;
                    self.session_id = None;
                    self.fallback_reason = Some(format!(
                        "api fallback ended; session moved to account {}, context reset",
                        self.account_label()
                    ));
                }
                None => {
                    let mut attempts = Attempts::default();
                    attempts.accounts_exhausted(&self.inner);
                    return self.api_rung(prompt, attempts).await;
                }
            }
        }
        let timeout_ms = self.template.timeout_ms;
        let mut respawns_left = 1u32;
        let mut tried = vec![self.account_idx];
        let mut attempts = Attempts::default();
        loop {
            let started = Instant::now();
            let spawn_failure = if guard.is_none() {
                match self.spawn(self.session_id.clone()).await {
                    Ok(p) => {
                        *guard = Some(p);
                        None
                    }
                    Err(f) => Some(f),
                }
            } else {
                None
            };
            let failure = match (spawn_failure, guard.as_mut()) {
                (Some(f), _) => f,
                (None, None) => continue,
                (None, Some(p)) => {
                    self.in_flight = true;
                    let res = Self::turn(p, &prompt, timeout_ms).await;
                    self.in_flight = false;
                    let duration_ms = started.elapsed().as_millis() as u64;
                    match res {
                        Ok(o) => {
                            if let Some(id) = &o.session_id {
                                self.session_id = Some(id.clone());
                            }
                            shared.touch();
                            let account = self.inner.accounts[self.account_idx].clone();
                            let reply = self.inner.cli_reply(
                                &account,
                                &self.template,
                                o,
                                duration_ms,
                                self.fallback_reason.clone(),
                            );
                            self.inner.record(self.row(&reply));
                            push_exchange(&mut self.mirror, prompt, reply.text.clone());
                            ladder::served(&self.inner, reply.engine);
                            return Ok(reply);
                        }
                        Err(TurnErr::Died(detail)) => {
                            self.fail_row(started, "transient");
                            if let Some(p) = guard.take() {
                                p.kill().await;
                            }
                            if respawns_left == 0 {
                                return Err(EngineError::Unavailable {
                                    reason: format!(
                                        "warm session on {} died twice: {detail}",
                                        self.account_label()
                                    ),
                                });
                            }
                            respawns_left -= 1;
                            continue;
                        }
                        Err(TurnErr::Timeout) => {
                            self.fail_row(started, "timeout");
                            if let Some(p) = guard.take() {
                                p.kill().await;
                            }
                            return Err(EngineError::Timeout {
                                after_ms: timeout_ms,
                            });
                        }
                        Err(TurnErr::Failed(f)) => f,
                    }
                }
            };
            // The same ladder as a one-shot call.
            self.fail_row(started, failure.kind());
            let from = self.account_label().to_string();
            let (cool, reason) = match ladder::step(failure, &from) {
                Step::Stop(e) => return Err(e),
                Step::Rotate { cool, reason } => (cool, reason),
            };
            if let Some(reset_at_ms) = cool {
                self.inner.cool(self.account_idx, reset_at_ms);
            }
            if let Some(p) = guard.take() {
                p.kill().await;
            }
            attempts.push(reason);
            self.session_id = None;
            match self.inner.pick_account(&tried) {
                Some(j) => {
                    tried.push(j);
                    self.account_idx = j;
                    respawns_left = 1;
                    self.fallback_reason = Some(format!(
                        "{}; session moved, context reset",
                        attempts.joined()
                    ));
                }
                None => {
                    attempts.accounts_exhausted(&self.inner);
                    return self.api_rung(prompt, attempts).await;
                }
            }
        }
    }

    async fn interrupt(&mut self) -> Result<(), EngineError> {
        let shared = self.shared.clone();
        let mut guard = shared.proc.lock().await;
        let Some(p) = guard.as_mut() else {
            return Ok(());
        };
        self.seq += 1;
        let id = format!("shadowrunr-int-{}", self.seq);
        let req = json!({"type": "control_request", "request_id": id,
                         "request": {"subtype": "interrupt"}});
        let need_result = self.in_flight;
        let acked = if p.write_line(&req).await.is_ok() {
            let wait = async {
                let mut parser = TurnParser::new();
                let mut got_ack = false;
                let mut got_result = !need_result;
                while let Ok(Some(l)) = p.lines.next_line().await {
                    match parser.feed(&l, 0) {
                        Feed::ControlResponse { request_id, .. } if request_id == id => {
                            got_ack = true
                        }
                        Feed::Done => got_result = true,
                        _ => {}
                    }
                    if got_ack && got_result {
                        return true;
                    }
                    if parser.is_done() {
                        parser = TurnParser::new();
                    }
                }
                false
            };
            tokio::time::timeout(INTERRUPT_ACK_WAIT, wait)
                .await
                .unwrap_or(false)
        } else {
            false
        };
        if !acked {
            // Documented fallback: kill now, respawn (with --resume) on the next send.
            if let Some(p) = guard.take() {
                p.kill().await;
            }
        }
        self.in_flight = false;
        Ok(())
    }

    async fn close(self: Box<Self>) -> Result<(), EngineError> {
        self.reaper.abort();
        let taken = self.shared.proc.lock().await.take();
        if let Some(mut p) = taken {
            let _ = p.stdin.shutdown().await;
            drop(p.stdin);
            if tokio::time::timeout(Duration::from_secs(5), p.child.wait())
                .await
                .is_err()
            {
                let _ = p.child.kill().await;
            }
        }
        Ok(())
    }
}

impl Drop for ClaudeWarmSession {
    fn drop(&mut self) {
        // The child is `kill_on_drop`, so dropping the shared state ends the process.
        self.reaper.abort();
    }
}

/// Most turns a conversation history keeps. Whole exchanges are dropped from the front, so
/// it always starts with a user turn.
pub const MIRROR_MAX_TURNS: usize = 200;

/// Append one answered exchange to a conversation history, oldest exchanges first out.
fn push_exchange(history: &mut Vec<Turn>, prompt: String, reply: String) {
    history.push(Turn {
        role: "user",
        text: prompt,
    });
    history.push(Turn {
        role: "assistant",
        text: reply,
    });
    let excess = history.len().saturating_sub(MIRROR_MAX_TURNS);
    if excess > 0 {
        history.drain(..excess + excess % 2);
    }
}

/// One warm turn on the Messages API with `history` as the conversation so far: ledgers the
/// outcome (warm), extends the history and reports the fleet mode on success.
async fn api_turn(
    inner: &FleetInner,
    template: &EngineRequest,
    history: &mut Vec<Turn>,
    prompt: String,
    fallback_reason: String,
) -> Result<EngineReply, Failure> {
    let Some(api) = &inner.api else {
        return Err(Failure::Fatal(EngineError::Unavailable {
            reason: "no api key configured".into(),
        }));
    };
    let mut req = template.clone();
    req.prompt = prompt.clone();
    let started = Instant::now();
    match api.complete(&req, history, inner.now()).await {
        Ok(o) => {
            let reply = api_reply(o, fallback_reason);
            push_exchange(history, prompt, reply.text.clone());
            let mut row =
                LedgerRow::from_reply(inner.now(), req.role, &reply, req.correlation_id.clone());
            row.warm = true;
            inner.record(row);
            ladder::served(inner, reply.engine);
            Ok(reply)
        }
        Err(f) => {
            inner.record(inner.failure_row(
                &req,
                EngineKind::AnthropicApi,
                None,
                started.elapsed().as_millis() as u64,
                f.kind(),
                true,
            ));
            Err(f)
        }
    }
}

/// Messages API conversation used when no fleet account can hold a warm session.
pub struct ApiSession {
    inner: Arc<FleetInner>,
    template: EngineRequest,
    history: Vec<Turn>,
    fallback_reason: String,
}

impl ApiSession {
    pub fn new(inner: Arc<FleetInner>, template: EngineRequest, fallback_reason: String) -> Self {
        Self {
            inner,
            template,
            history: Vec::new(),
            fallback_reason,
        }
    }
}

#[async_trait]
impl WarmSession for ApiSession {
    async fn send(&mut self, prompt: String) -> Result<EngineReply, EngineError> {
        api_turn(
            &self.inner,
            &self.template,
            &mut self.history,
            prompt,
            self.fallback_reason.clone(),
        )
        .await
        .map_err(|f| f.into_engine_error("api"))
    }

    async fn interrupt(&mut self) -> Result<(), EngineError> {
        Ok(())
    }

    async fn close(self: Box<Self>) -> Result<(), EngineError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mirror_keeps_whole_exchanges_and_starts_with_a_user_turn() {
        let mut h = Vec::new();
        for i in 0..(MIRROR_MAX_TURNS / 2 + 3) {
            push_exchange(&mut h, format!("q{i}"), format!("a{i}"));
        }
        assert_eq!(h.len(), MIRROR_MAX_TURNS);
        assert_eq!(h[0].role, "user");
        assert_eq!(h[0].text, "q3");
        assert_eq!(h.last().unwrap().role, "assistant");
    }
}
