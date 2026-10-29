//! Property tests for the engine's process admission ([`shadowrunr_engine::admission`]). Each
//! property names the invariant it holds the code to, taken from the code and its doc
//! comments. Every input is generated; no real process is spawned.

use std::time::{Duration, Instant};

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use shadowrunr_engine::admission::{
    deadline_in, lane, remaining_ms, spent, whole_budget, Admission, Admitted, Lane,
};
use shadowrunr_engine::config::ROLES;
use shadowrunr_engine::limits::Failure;
use shadowrunr_types::{EngineError, Role};

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 128,
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource(
            "proptest-regressions",
        ))),
        ..ProptestConfig::default()
    }
}

fn role() -> impl Strategy<Value = Role> {
    (0..ROLES.len()).prop_map(|i| ROLES[i])
}

fn live_role() -> impl Strategy<Value = Role> {
    role().prop_filter("live lane", |r| lane(*r) == Lane::Live)
}

fn batch_role() -> impl Strategy<Value = Role> {
    role().prop_filter("batch lane", |r| lane(*r) == Lane::Batch)
}

/// Admit `role` against a deadline that has already passed: a free slot still admits at
/// once, a full lane expires without waiting for anyone.
fn try_admit(a: &Admission, role: Role) -> Option<Admitted> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(a.admit(role, Instant::now())).ok()
}

fn admit_all(a: &Admission, roles: &[Role]) -> Vec<Admitted> {
    roles.iter().filter_map(|r| try_admit(a, *r)).collect()
}

proptest! {
    #![proptest_config(config())]

    // new_clamps_total_to_one_and_reserve_below_total: total = max(max_live, 1) and the
    // reserve never reaches it, so the batch lane keeps a slot.
    #[test]
    fn new_clamps_total_to_one_and_reserve_below_total(
        max_live in 0usize..40,
        reserved in 0usize..60,
    ) {
        let a = Admission::new(max_live, reserved);
        let total = max_live.max(1);
        prop_assert_eq!(a.total(), total);
        prop_assert_eq!(a.reserved(), reserved.min(total - 1));
        prop_assert!(a.reserved() < a.total());
        prop_assert_eq!(a.available(), (total - a.reserved(), a.reserved()));
        prop_assert_eq!(a.waiting(), 0);
    }

    // admitted_permits_never_exceed_max_live: however many callers arrive, at most `total`
    // hold a permit, and the first `total` arrivals of live roles are all admitted.
    #[test]
    fn admitted_permits_never_exceed_max_live(
        max_live in 0usize..10,
        reserved in 0usize..10,
        roles in proptest::collection::vec(role(), 0..40),
    ) {
        let a = Admission::new(max_live, reserved);
        let held = admit_all(&a, &roles);
        prop_assert!(held.len() <= a.total());
        prop_assert!(held.len() <= roles.len());
        prop_assert_eq!(a.waiting(), 0);
    }

    // available_plus_held_equals_the_totals: free slots plus held permits always sum to
    // the total, and neither pool exceeds its own size.
    #[test]
    fn available_plus_held_equals_the_totals(
        max_live in 0usize..10,
        reserved in 0usize..10,
        roles in proptest::collection::vec(role(), 0..40),
    ) {
        let a = Admission::new(max_live, reserved);
        let held = admit_all(&a, &roles);
        let (shared, reserve) = a.available();
        prop_assert_eq!(shared + reserve + held.len(), a.total());
        prop_assert!(shared <= a.total() - a.reserved());
        prop_assert!(reserve <= a.reserved());
    }

    // batch_roles_never_take_the_reserve: batch callers alone get exactly total - reserved
    // permits and the live reserve stays whole.
    #[test]
    fn batch_roles_never_take_the_reserve(
        max_live in 0usize..10,
        reserved in 0usize..10,
        roles in proptest::collection::vec(batch_role(), 0..40),
    ) {
        let a = Admission::new(max_live, reserved);
        let held = admit_all(&a, &roles);
        let cap = a.total() - a.reserved();
        prop_assert_eq!(held.len(), roles.len().min(cap));
        prop_assert_eq!(a.available().1, a.reserved());
    }

    // live_roles_spend_the_reserve_first_then_share: live callers reach every slot, and the
    // shared slots are untouched until the reserve is spent.
    #[test]
    fn live_roles_spend_the_reserve_first_then_share(
        max_live in 0usize..10,
        reserved in 0usize..10,
        roles in proptest::collection::vec(live_role(), 0..40),
    ) {
        let a = Admission::new(max_live, reserved);
        let held = admit_all(&a, &roles);
        prop_assert_eq!(held.len(), roles.len().min(a.total()));
        let took_reserve = held.len().min(a.reserved());
        prop_assert_eq!(a.available().1, a.reserved() - took_reserve);
        prop_assert_eq!(
            a.available().0,
            (a.total() - a.reserved()) - (held.len() - took_reserve)
        );
    }

    // dropping_a_permit_returns_its_slot: each drop frees exactly one slot, releasing every
    // permit restores the full pools, and a full round of live callers fits again.
    #[test]
    fn dropping_a_permit_returns_its_slot(
        max_live in 0usize..10,
        reserved in 0usize..10,
        roles in proptest::collection::vec(role(), 0..40),
    ) {
        let a = Admission::new(max_live, reserved);
        let before = a.available();
        let mut held = admit_all(&a, &roles);
        while let Some(h) = held.pop() {
            drop(h);
            let (shared, reserve) = a.available();
            prop_assert_eq!(shared + reserve, a.total() - held.len());
        }
        prop_assert_eq!(a.available(), before);
        let again: Vec<Admitted> = (0..a.total())
            .filter_map(|_| try_admit(&a, Role::CoreThread))
            .collect();
        prop_assert_eq!(again.len(), a.total());
    }

    // remaining_ms_never_underflows_and_never_exceeds_the_budget: a deadline in the past
    // reads 0, a future one reads at most the budget it was built from.
    #[test]
    fn remaining_ms_never_underflows_and_never_exceeds_the_budget(
        budget in 0u64..10_000_000,
        past_ms in 0u64..10_000,
    ) {
        prop_assert!(remaining_ms(deadline_in(budget)) <= budget);
        if let Some(past) = Instant::now().checked_sub(Duration::from_millis(past_ms)) {
            prop_assert_eq!(remaining_ms(past), 0);
        }
    }

    // a_timeout_reports_the_whole_budget: whole_budget rewrites any fatal timeout to the
    // budget the caller asked for and leaves every other failure alone.
    #[test]
    fn a_timeout_reports_the_whole_budget(attempt in 0u64..1_000_000, whole in 0u64..1_000_000) {
        let f = whole_budget(spent(attempt), whole);
        let rewritten =
            matches!(f, Failure::Fatal(EngineError::Timeout { after_ms }) if after_ms == whole);
        prop_assert!(rewritten);
        let t = whole_budget(Failure::Transient { detail: "x".into() }, whole);
        let untouched = matches!(t, Failure::Transient { .. });
        prop_assert!(untouched);
    }
}
