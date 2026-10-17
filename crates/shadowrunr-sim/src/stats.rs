//! Rates with honest bounds. A rate is never reported without its n.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundMethod {
    /// Wilson score interval, 95%.
    Wilson,
    /// Rule of three: with zero events in n trials, the 95% upper bound on the event rate is
    /// about 3/n.
    RuleOfThree,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bound {
    pub lo: f64,
    pub hi: f64,
    pub method: BoundMethod,
}

/// 95% two-sided normal quantile.
const Z95: f64 = 1.959_963_984_540_054;

/// Wilson score interval for k successes in n trials (95%).
pub fn wilson(k: u32, n: u32) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let n = f64::from(n);
    let p = f64::from(k) / n;
    let z2 = Z95 * Z95;
    let denom = 1.0 + z2 / n;
    let centre = (p + z2 / (2.0 * n)) / denom;
    let half = Z95 * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denom;
    ((centre - half).max(0.0), (centre + half).min(1.0))
}

/// The 95% bound for a rate of k in n: rule of three when k is 0 or n (zero events on one
/// side), Wilson otherwise. `None` when n is 0: there is no rate to bound.
pub fn rate_bound(k: u32, n: u32) -> Option<Bound> {
    if n == 0 {
        return None;
    }
    let three = (3.0 / f64::from(n)).min(1.0);
    if k == 0 {
        Some(Bound {
            lo: 0.0,
            hi: three,
            method: BoundMethod::RuleOfThree,
        })
    } else if k >= n {
        Some(Bound {
            lo: 1.0 - three,
            hi: 1.0,
            method: BoundMethod::RuleOfThree,
        })
    } else {
        let (lo, hi) = wilson(k, n);
        Some(Bound {
            lo,
            hi,
            method: BoundMethod::Wilson,
        })
    }
}

/// Nearest-rank percentile (p in 0..=100) of an unsorted sample.
pub fn percentile(values: &[u64], p: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    let rank = ((p / 100.0) * v.len() as f64).ceil() as usize;
    Some(v[rank.clamp(1, v.len()) - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn wilson_matches_reference_values() {
        // Reference: 5/10 -> [0.2366, 0.7634]; 1/20 -> [0.0089, 0.2361].
        let (lo, hi) = wilson(5, 10);
        assert!(close(lo, 0.2366) && close(hi, 0.7634), "{lo} {hi}");
        let (lo, hi) = wilson(1, 20);
        assert!(close(lo, 0.0089) && close(hi, 0.2361), "{lo} {hi}");
    }

    #[test]
    fn rule_of_three_at_the_edges() {
        let b = rate_bound(0, 30).unwrap();
        assert_eq!(b.method, BoundMethod::RuleOfThree);
        assert!(close(b.hi, 0.1) && b.lo == 0.0);
        let b = rate_bound(30, 30).unwrap();
        assert!(close(b.lo, 0.9) && b.hi == 1.0);
        // Tiny n: the bound saturates instead of going negative.
        let b = rate_bound(2, 2).unwrap();
        assert_eq!(b.lo, 0.0);
        assert_eq!(rate_bound(3, 10).unwrap().method, BoundMethod::Wilson);
        assert!(rate_bound(0, 0).is_none());
    }

    #[test]
    fn percentiles() {
        let v = [5u64, 1, 4, 2, 3, 10, 9, 8, 7, 6];
        assert_eq!(percentile(&v, 50.0), Some(5));
        assert_eq!(percentile(&v, 95.0), Some(10));
        assert_eq!(percentile(&[42], 95.0), Some(42));
        assert_eq!(percentile(&[], 50.0), None);
    }
}
