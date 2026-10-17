//! Rates with honest bounds.
//!
//! Minimal local duplicate of the statistics in `shadowrunr-sim` (`stats::{wilson,
//! rate_bound, percentile}`): that crate was still being built when WP6 landed, so this crate
//! does not depend on it. Same formulas: Wilson 95% for 0 < k < n, rule of three (3/n) when
//! k is 0 or n, nearest-rank percentiles. Fold into the sim's versions once they are public
//! and stable.

use crate::types::{Bound, BoundMethod, Rate};

/// 95% two-sided normal quantile.
const Z95: f64 = 1.959_963_984_540_054;

/// Wilson score interval for k successes in n trials (95%).
pub fn wilson(k: u32, n: u32) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let nf = f64::from(n);
    let p = f64::from(k.min(n)) / nf;
    let z2 = Z95 * Z95;
    let denom = 1.0 + z2 / nf;
    let centre = (p + z2 / (2.0 * nf)) / denom;
    let half = Z95 * (p * (1.0 - p) / nf + z2 / (4.0 * nf * nf)).sqrt() / denom;
    ((centre - half).max(0.0), (centre + half).min(1.0))
}

/// The 95% bound for k in n: rule of three when k is 0 or n, Wilson otherwise. `None` for
/// n = 0: there is no rate to bound.
pub fn rate_bound(k: u32, n: u32) -> Option<Bound> {
    if n == 0 {
        return None;
    }
    let three = (3.0 / f64::from(n)).min(1.0);
    Some(if k == 0 {
        Bound {
            lo: 0.0,
            hi: three,
            method: BoundMethod::RuleOfThree,
        }
    } else if k >= n {
        Bound {
            lo: 1.0 - three,
            hi: 1.0,
            method: BoundMethod::RuleOfThree,
        }
    } else {
        let (lo, hi) = wilson(k, n);
        Bound {
            lo,
            hi,
            method: BoundMethod::Wilson,
        }
    })
}

/// k of n as a [`Rate`]; `None` for n = 0.
pub fn rate(k: u32, n: u32) -> Option<Rate> {
    rate_bound(k, n).map(|bound| Rate {
        rate: f64::from(k.min(n)) / f64::from(n),
        n,
        bound,
    })
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
