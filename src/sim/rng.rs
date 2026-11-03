//! In-house deterministic RNG: SplitMix64 seeding + xoshiro256**.
//!
//! Kept in-tree (instead of the `rand` crates) so a seed produces the same run
//! forever, regardless of dependency updates. Transcendentals go through `libm`
//! so results are identical on every platform.

use std::f64::consts::PI;

#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
}

fn splitmix64(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut x = seed;
        Self {
            s: [
                splitmix64(&mut x),
                splitmix64(&mut x),
                splitmix64(&mut x),
                splitmix64(&mut x),
            ],
        }
    }

    /// An independent stream per subsystem (`"terrain"`, `"weather"`, ...), so
    /// one subsystem's draws never shift another's. That keeps terrain and
    /// weather identical for every coordinator on the same seed.
    pub fn stream(seed: u64, name: &str) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a
        for b in name.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut x = seed ^ h;
        Self::new(splitmix64(&mut x))
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.uniform()
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.uniform() < p
    }

    /// Standard normal (Box–Muller).
    pub fn normal(&mut self) -> f64 {
        let u1 = 1.0 - self.uniform(); // (0, 1]
        let u2 = self.uniform();
        libm::sqrt(-2.0 * libm::log(u1)) * libm::cos(2.0 * PI * u2)
    }

    /// Multiplicative noise with mean 1: lognormal with log-sd `sigma`.
    pub fn lognormal_mean1(&mut self, sigma: f64) -> f64 {
        libm::exp(sigma * self.normal() - 0.5 * sigma * sigma)
    }

    /// Poisson count (Knuth); fine for the small rates used here.
    pub fn poisson(&mut self, lambda: f64) -> u32 {
        let limit = libm::exp(-lambda);
        let mut k = 0;
        let mut p = self.uniform();
        while p > limit {
            k += 1;
            p *= self.uniform();
        }
        k
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden values: if this fails, every seeded run in existence changes.
    #[test]
    fn stream_is_stable() {
        let mut r = Rng::new(42);
        let got: Vec<u64> = (0..3).map(|_| r.next_u64()).collect();
        assert_eq!(got, GOLDEN_42);
    }

    const GOLDEN_42: [u64; 3] = [
        1546998764402558742,
        6990951692964543102,
        12544586762248559009,
    ];

    #[test]
    fn named_streams_differ_and_repeat() {
        let a1 = Rng::stream(7, "terrain").next_u64();
        let a2 = Rng::stream(7, "terrain").next_u64();
        let b = Rng::stream(7, "weather").next_u64();
        assert_eq!(a1, a2);
        assert_ne!(a1, b);
    }

    #[test]
    fn distributions_are_sane() {
        let mut r = Rng::new(1);
        let n = 20_000;
        let mean_u: f64 = (0..n).map(|_| r.uniform()).sum::<f64>() / n as f64;
        let mean_n: f64 = (0..n).map(|_| r.normal()).sum::<f64>() / n as f64;
        let mean_l: f64 = (0..n).map(|_| r.lognormal_mean1(0.2)).sum::<f64>() / n as f64;
        let mean_p: f64 = (0..n).map(|_| f64::from(r.poisson(0.5))).sum::<f64>() / n as f64;
        assert!((mean_u - 0.5).abs() < 0.01, "{mean_u}");
        assert!(mean_n.abs() < 0.03, "{mean_n}");
        assert!((mean_l - 1.0).abs() < 0.01, "{mean_l}");
        assert!((mean_p - 0.5).abs() < 0.03, "{mean_p}");
    }
}
