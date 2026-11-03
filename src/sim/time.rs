//! Simulation time. Internally integer milliseconds, so event ordering is exact;
//! converted to f64 seconds only at the protocol boundary.

use arcadia_protocol::SimTime;

pub type SimMs = u64;

/// One sol (88,775 s).
pub const SOL_MS: SimMs = 88_775_000;

/// Milliseconds → protocol seconds.
pub fn secs(t: SimMs) -> SimTime {
    t as f64 / 1000.0
}

/// Seconds → milliseconds, rounded; negative or non-finite values clamp to 0 / MAX.
pub fn ms(s: f64) -> SimMs {
    if s.is_nan() || s <= 0.0 {
        0
    } else if s >= (u64::MAX / 1000) as f64 {
        u64::MAX / 2
    } else {
        (s * 1000.0).round() as SimMs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_clamps() {
        assert_eq!(ms(1.2345), 1235);
        assert_eq!(secs(1500), 1.5);
        assert_eq!(ms(-3.0), 0);
        assert_eq!(ms(f64::NAN), 0);
        assert!(ms(f64::INFINITY) > SOL_MS * 1000);
    }
}
