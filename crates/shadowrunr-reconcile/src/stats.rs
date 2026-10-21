//! Rates with honest bounds: the simulator's statistics (`results.json` uses the same
//! functions, so a rate Reconcile shows and one the results carry never differ in method),
//! plus [`rate`], a k-of-n [`Rate`] for the report's tables.

use crate::types::Rate;
pub use shadowrunr_sim::stats::{percentile, rate_bound, wilson};

/// k of n as a [`Rate`]; `None` for n = 0.
pub fn rate(k: u32, n: u32) -> Option<Rate> {
    rate_bound(k, n).map(|bound| Rate {
        rate: f64::from(k.min(n)) / f64::from(n),
        n,
        bound,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BoundMethod;

    #[test]
    fn rule_of_three_at_the_edges_and_wilson_inside() {
        let b = rate_bound(0, 30).unwrap();
        assert_eq!(b.method, BoundMethod::RuleOfThree);
        assert!((b.hi - 0.1).abs() < 1e-12);
        let b = rate_bound(10, 10).unwrap();
        assert!((b.lo - 0.7).abs() < 1e-12);
        let (lo, hi) = wilson(5, 10);
        assert!((lo - 0.2366).abs() < 1e-3 && (hi - 0.7634).abs() < 1e-3);
        assert!(rate(1, 0).is_none());
        assert_eq!(rate(1, 2).unwrap().rate, 0.5);
    }

    #[test]
    fn nearest_rank_percentiles() {
        assert_eq!(percentile(&[800, 1200, 1000], 50.0), Some(1000));
        assert_eq!(percentile(&[800, 1200, 1000], 95.0), Some(1200));
        assert_eq!(percentile(&[], 50.0), None);
    }
}
