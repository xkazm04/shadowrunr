//! A tiny deterministic PRNG (SplitMix64). Owned here so a seed reproduces the same run on
//! every machine and every dependency upgrade.

#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// An independent stream for one purpose (population, chat, schedule...), so adding a
    /// draw in one place does not reshuffle another.
    pub fn derive(seed: u64, label: &str) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in label.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        let mut r = Rng::new(seed ^ h);
        r.next_u64();
        r
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn range_f64(&mut self, lo: f64, hi: f64) -> f64 {
        if hi <= lo {
            return lo;
        }
        lo + (hi - lo) * self.f64()
    }

    /// Uniform in [0, n). Returns 0 when n is 0.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// Uniform integer in [lo, hi] (inclusive).
    pub fn range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % u64::from(hi - lo + 1)) as u32
    }

    pub fn chance(&mut self, p: f64) -> bool {
        p > 0.0 && self.f64() < p
    }

    /// Poisson draw: Knuth for small lambda, a rounded normal approximation above 30.
    pub fn poisson(&mut self, lambda: f64) -> u32 {
        if lambda <= 0.0 || !lambda.is_finite() {
            return 0;
        }
        if lambda < 30.0 {
            let l = (-lambda).exp();
            let mut k = 0u32;
            let mut p = 1.0;
            loop {
                p *= self.f64();
                if p <= l {
                    return k;
                }
                k += 1;
            }
        }
        let u1 = self.f64().max(f64::MIN_POSITIVE);
        let u2 = self.f64();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        (lambda + z * lambda.sqrt()).round().max(0.0) as u32
    }

    /// Index drawn proportionally to non-negative weights; `None` when all are zero.
    pub fn weighted(&mut self, weights: &[f64]) -> Option<usize> {
        let total: f64 = weights.iter().filter(|w| **w > 0.0).sum();
        if total <= 0.0 {
            return None;
        }
        let mut x = self.f64() * total;
        for (i, w) in weights.iter().enumerate() {
            if *w <= 0.0 {
                continue;
            }
            if x < *w {
                return Some(i);
            }
            x -= *w;
        }
        weights.iter().rposition(|w| *w > 0.0)
    }

    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> Option<&'a T> {
        if v.is_empty() {
            None
        } else {
            v.get(self.below(v.len()))
        }
    }

    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::derive(7, "x");
        let mut b = Rng::derive(7, "x");
        let mut c = Rng::derive(7, "y");
        let va: Vec<u64> = (0..5).map(|_| a.next_u64()).collect();
        let vb: Vec<u64> = (0..5).map(|_| b.next_u64()).collect();
        let vc: Vec<u64> = (0..5).map(|_| c.next_u64()).collect();
        assert_eq!(va, vb);
        assert_ne!(va, vc);
    }

    #[test]
    fn poisson_mean_is_close() {
        let mut r = Rng::new(1);
        for lambda in [0.5, 4.0, 60.0] {
            let n = 4000;
            let sum: u64 = (0..n).map(|_| u64::from(r.poisson(lambda))).sum();
            let mean = sum as f64 / n as f64;
            assert!(
                (mean - lambda).abs() < lambda * 0.1 + 0.1,
                "{lambda}: {mean}"
            );
        }
    }
}
