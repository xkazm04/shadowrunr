//! Process admission: who may start a CLI process, and how long they may wait for one.
//!
//! Every `claude`, codex and agy process the engine spawns holds one permit for its whole
//! life. The arrivals are not alike - a sub-second chat pick next to a quarter-hour judge -
//! so they do not share one line:
//!
//! | lane  | roles                                  | may take                    |
//! |-------|----------------------------------------|-----------------------------|
//! | live  | CoreThread, ChatPicker, Chatter        | a reserved permit or a shared one, whichever frees first |
//! | batch | every other role                       | a shared permit only        |
//!
//! `max_live_processes` permits in total, `live_reserved_processes` of them reserved for the
//! live lane (clamped below the total, so with one permit the single slot is shared, as
//! before). A wait is bounded by the request's deadline, computed when the call began: a
//! wait that outlives it is [`Expired`], and the caller reports a timeout with the time it
//! spent queued. The time waited is returned either way, so a slow turn can be told from a
//! slow queue.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use shadowrunr_types::{EngineError, Role};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::limits::Failure;

/// Which line a role waits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// The live voice: answers someone who is waiting now.
    Live,
    /// Background work: research, planning, consolidation, judging, reconciling, tagging.
    Batch,
}

/// The lane of a role. Exhaustive on purpose: a new role must choose its line.
pub fn lane(role: Role) -> Lane {
    match role {
        Role::CoreThread
        | Role::ChatPicker
        | Role::Chatter
        | Role::ModClassifier
        | Role::ModVoice => Lane::Live,
        Role::Planner
        | Role::Researcher
        | Role::Consolidator
        | Role::Judge
        | Role::Reconciler
        | Role::Clipper
        | Role::Tagger => Lane::Batch,
    }
}

/// One admitted process slot. Dropping it frees the slot.
#[derive(Debug)]
pub struct Permit {
    _inner: OwnedSemaphorePermit,
}

/// A granted admission and how long it waited (0 when a slot was free at once).
#[derive(Debug)]
pub struct Admitted {
    pub permit: Permit,
    pub waited_ms: u64,
}

/// The deadline passed before a slot freed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expired {
    pub waited_ms: u64,
}

/// The engine's process cap, split into a shared lane and a live reserve.
#[derive(Debug, Clone)]
pub struct Admission {
    shared: Arc<Semaphore>,
    reserve: Arc<Semaphore>,
    waiting: Arc<AtomicUsize>,
    total: usize,
    reserved: usize,
}

/// Decrements the waiting count when a queued `admit` ends, however it ends.
struct Waiting<'a>(&'a AtomicUsize);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Admission {
    /// `max_live` permits in total (at least one), `reserved` of them for the live lane,
    /// clamped so the batch lane keeps at least one.
    pub fn new(max_live: usize, reserved: usize) -> Self {
        let total = max_live.max(1);
        let reserved = reserved.min(total - 1);
        Self {
            shared: Arc::new(Semaphore::new(total - reserved)),
            reserve: Arc::new(Semaphore::new(reserved)),
            waiting: Arc::new(AtomicUsize::new(0)),
            total,
            reserved,
        }
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn reserved(&self) -> usize {
        self.reserved
    }

    /// Callers queued for a slot right now.
    pub fn waiting(&self) -> usize {
        self.waiting.load(Ordering::SeqCst)
    }

    /// Free slots right now: (shared, live reserve).
    pub fn available(&self) -> (usize, usize) {
        (
            self.shared.available_permits(),
            self.reserve.available_permits(),
        )
    }

    /// Wait for a slot in `role`'s lane until `deadline`.
    pub async fn admit(&self, role: Role, deadline: Instant) -> Result<Admitted, Expired> {
        let live = lane(role) == Lane::Live;
        // A free slot is taken without queueing; the live lane spends its reserve first so
        // the shared slots stay with the batch lane.
        if live {
            if let Ok(p) = self.reserve.clone().try_acquire_owned() {
                return Ok(admitted(p, 0));
            }
        }
        if let Ok(p) = self.shared.clone().try_acquire_owned() {
            return Ok(admitted(p, 0));
        }
        let started = Instant::now();
        self.waiting.fetch_add(1, Ordering::SeqCst);
        let _queued = Waiting(&self.waiting);
        let expiry = tokio::time::sleep_until(deadline.into());
        let got = if live {
            tokio::select! {
                biased;
                p = self.reserve.clone().acquire_owned() => p.ok(),
                p = self.shared.clone().acquire_owned() => p.ok(),
                _ = expiry => None,
            }
        } else {
            tokio::select! {
                biased;
                p = self.shared.clone().acquire_owned() => p.ok(),
                _ = expiry => None,
            }
        };
        // A wait is never reported as zero: zero means "did not wait".
        let waited_ms = (started.elapsed().as_millis() as u64).max(1);
        match got {
            Some(p) => Ok(admitted(p, waited_ms)),
            None => Err(Expired { waited_ms }),
        }
    }
}

fn admitted(p: OwnedSemaphorePermit, waited_ms: u64) -> Admitted {
    Admitted {
        permit: Permit { _inner: p },
        waited_ms,
    }
}

/// The budget left before `deadline`, in ms (0 when it has passed).
pub fn remaining_ms(deadline: Instant) -> u64 {
    deadline
        .saturating_duration_since(Instant::now())
        .as_millis() as u64
}

/// The deadline of a call that starts now with `timeout_ms` to spend.
pub fn deadline_in(timeout_ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(timeout_ms)
}

/// The error for a call whose `timeout_ms` budget ran out, in the queue or after it.
pub fn spent(timeout_ms: u64) -> Failure {
    Failure::Fatal(EngineError::Timeout {
        after_ms: timeout_ms,
    })
}

/// An attempt runs with the budget left after its wait, so its own timeout names that
/// remainder; report the whole budget the caller asked for instead.
pub fn whole_budget(f: Failure, timeout_ms: u64) -> Failure {
    match f {
        Failure::Fatal(EngineError::Timeout { .. }) => spent(timeout_ms),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soon() -> Instant {
        Instant::now() + Duration::from_millis(50)
    }

    #[test]
    fn every_role_has_a_lane() {
        for r in crate::config::ROLES {
            let want = matches!(
                r,
                Role::CoreThread
                    | Role::ChatPicker
                    | Role::Chatter
                    | Role::ModClassifier
                    | Role::ModVoice
            );
            assert_eq!(lane(r) == Lane::Live, want, "{r:?}");
        }
    }

    #[test]
    fn the_reserve_is_clamped_below_the_total() {
        let a = Admission::new(6, 2);
        assert_eq!((a.total(), a.reserved(), a.available()), (6, 2, (4, 2)));
        let a = Admission::new(2, 5);
        assert_eq!((a.reserved(), a.available()), (1, (1, 1)));
        // One permit: one shared slot, today's behaviour.
        let a = Admission::new(1, 2);
        assert_eq!((a.reserved(), a.available()), (0, (1, 0)));
        let a = Admission::new(0, 0);
        assert_eq!(a.total(), 1);
    }

    #[tokio::test]
    async fn a_free_slot_admits_without_a_wait() {
        let a = Admission::new(2, 1);
        let live = a.admit(Role::CoreThread, soon()).await.unwrap();
        assert_eq!(live.waited_ms, 0);
        // The live lane spent its reserve, not the shared slot.
        assert_eq!(a.available(), (1, 0));
        let batch = a.admit(Role::Judge, soon()).await.unwrap();
        assert_eq!(batch.waited_ms, 0);
        assert_eq!(a.available(), (0, 0));
        drop((live, batch));
        assert_eq!(a.available(), (1, 1));
    }

    #[tokio::test]
    async fn batch_roles_never_take_a_reserved_slot() {
        let a = Admission::new(2, 1);
        let _judge = a.admit(Role::Judge, soon()).await.unwrap();
        // The reserve is free, but a batch role waits for the shared slot until its deadline.
        let e = a.admit(Role::Researcher, soon()).await.unwrap_err();
        assert!(e.waited_ms >= 1);
        assert_eq!(a.available(), (0, 1));
        assert_eq!(a.waiting(), 0, "an expired wait leaves the queue");
        // A live role still gets in.
        let core = a.admit(Role::ChatPicker, soon()).await.unwrap();
        assert_eq!(core.waited_ms, 0);
    }

    #[tokio::test]
    async fn a_live_role_takes_a_shared_slot_when_the_reserve_is_spent() {
        let a = Admission::new(3, 1);
        let _core = a.admit(Role::CoreThread, soon()).await.unwrap();
        let picker = a.admit(Role::ChatPicker, soon()).await.unwrap();
        assert_eq!(picker.waited_ms, 0);
        assert_eq!(a.available(), (1, 0));
    }

    #[tokio::test]
    async fn a_wait_past_the_deadline_expires_and_a_freed_slot_admits() {
        let a = Admission::new(1, 0);
        let held = a.admit(Role::Judge, soon()).await.unwrap();
        // A deadline already passed expires at once.
        let e = a.admit(Role::CoreThread, Instant::now()).await.unwrap_err();
        assert!(e.waited_ms >= 1);
        // A queued live caller is admitted when the holder lets go.
        let a2 = a.clone();
        let far = Instant::now() + Duration::from_secs(30);
        let queued = tokio::spawn(async move { a2.admit(Role::CoreThread, far).await });
        while a.waiting() == 0 {
            tokio::task::yield_now().await;
        }
        drop(held);
        let got = queued.await.unwrap().unwrap();
        assert!(got.waited_ms >= 1);
        assert_eq!(a.waiting(), 0);
    }

    #[test]
    fn a_timeout_reports_the_whole_budget() {
        let f = whole_budget(spent(120), 300);
        assert!(matches!(
            f,
            Failure::Fatal(EngineError::Timeout { after_ms: 300 })
        ));
        let t = whole_budget(Failure::Transient { detail: "x".into() }, 300);
        assert!(matches!(t, Failure::Transient { .. }));
    }

    #[test]
    fn remaining_budget_never_underflows() {
        assert_eq!(remaining_ms(Instant::now()), 0);
        assert!(remaining_ms(deadline_in(10_000)) > 9_000);
    }
}
