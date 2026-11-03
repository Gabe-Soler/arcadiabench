//! Sol clock, daylight, temperature and the seeded dust-storm schedule.
//!
//! The whole schedule is drawn up front from the "weather" stream, so every
//! coordinator on the same seed faces exactly the same weather.

use super::params::Params;
use super::rng::Rng;
use super::time::{SOL_MS, SimMs, ms, secs};
use std::f64::consts::PI;

#[derive(Debug, Clone, PartialEq)]
pub struct Storm {
    pub start: SimMs,
    pub end: SimMs,
    pub peak_tau: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvEvent {
    Sunrise,
    Noon,
    Sunset,
    StormStart(usize),
    /// Dust opacity peaks (end of the storm's build-up).
    StormPeak(usize),
    StormEnd(usize),
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub storms: Vec<Storm>,
    clear_tau: f64,
    extinction: f64,
}

const SUNRISE: f64 = 0.25;
const NOON: f64 = 0.5;
const SUNSET: f64 = 0.75;

impl Environment {
    pub fn generate(seed: u64, time_limit: SimMs, p: &Params) -> Self {
        let mut rng = Rng::stream(seed, "weather");
        let mut storms = Vec::new();
        let mut sol = 0;
        while sol * SOL_MS < time_limit {
            let sol_start = sol * SOL_MS;
            let free = storms.last().is_none_or(|s: &Storm| s.end <= sol_start);
            if free && rng.chance(p.storm_chance_per_sol) {
                let start = sol_start + ms(rng.uniform() * secs(SOL_MS));
                let length = ms(rng.range(p.storm_sols.0, p.storm_sols.1) * secs(SOL_MS));
                storms.push(Storm {
                    start,
                    end: start + length,
                    peak_tau: rng.range(p.storm_peak_tau.0, p.storm_peak_tau.1),
                });
            }
            sol += 1;
        }
        Self {
            storms,
            clear_tau: p.clear_tau,
            extinction: p.dust_extinction,
        }
    }

    /// Fraction of the current sol elapsed; t = 0 is local midnight.
    pub fn sol_phase(t: SimMs) -> f64 {
        (t % SOL_MS) as f64 / SOL_MS as f64
    }

    pub fn daylight(t: SimMs) -> bool {
        (SUNRISE..SUNSET).contains(&Self::sol_phase(t))
    }

    /// Clear-sky solar output as a fraction of peak (0 at night).
    pub fn solar_factor(t: SimMs) -> f64 {
        let phase = Self::sol_phase(t);
        if (SUNRISE..SUNSET).contains(&phase) {
            libm::sin(PI * (phase - SUNRISE) / (SUNSET - SUNRISE))
        } else {
            0.0
        }
    }

    pub fn storm_at(&self, t: SimMs) -> Option<&Storm> {
        self.storms.iter().find(|s| s.start <= t && t < s.end)
    }

    /// Dust optical depth: ramps up over the first 20% of a storm, then decays.
    pub fn tau(&self, t: SimMs) -> f64 {
        match self.storm_at(t) {
            None => self.clear_tau,
            Some(s) => {
                let f = (t - s.start) as f64 / (s.end - s.start) as f64;
                let shape = if f < 0.2 { f / 0.2 } else { (1.0 - f) / 0.8 };
                self.clear_tau + (s.peak_tau - self.clear_tau) * shape
            }
        }
    }

    /// Solar transmittance relative to a clear sky.
    pub fn transmittance(&self, t: SimMs) -> f64 {
        libm::exp(-self.extinction * (self.tau(t) - self.clear_tau)).min(1.0)
    }

    /// Surface air temperature, °C: about -80 before dawn, -20 mid-afternoon.
    pub fn temperature_c(t: SimMs) -> f64 {
        -50.0 + 30.0 * libm::sin(2.0 * PI * (Self::sol_phase(t) - 0.375))
    }

    /// Every scheduled environment event up to `until`, in time order.
    pub fn schedule(&self, until: SimMs) -> Vec<(SimMs, EnvEvent)> {
        let mut out = Vec::new();
        let mut sol = 0;
        while sol * SOL_MS < until {
            for (phase, ev) in [
                (SUNRISE, EnvEvent::Sunrise),
                (NOON, EnvEvent::Noon),
                (SUNSET, EnvEvent::Sunset),
            ] {
                let t = sol * SOL_MS + (phase * SOL_MS as f64) as SimMs;
                if t < until {
                    out.push((t, ev));
                }
            }
            sol += 1;
        }
        for (i, s) in self.storms.iter().enumerate() {
            if s.start < until {
                out.push((s.start, EnvEvent::StormStart(i)));
            }
            let peak = s.start + (s.end - s.start) / 5;
            if peak < until {
                out.push((peak, EnvEvent::StormPeak(i)));
            }
            if s.end < until {
                out.push((s.end, EnvEvent::StormEnd(i)));
            }
        }
        out.sort_by_key(|&(t, _)| t);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_night_cycle() {
        assert!(!Environment::daylight(0));
        assert!(Environment::daylight(SOL_MS / 2));
        assert!((Environment::solar_factor(SOL_MS / 2) - 1.0).abs() < 1e-9);
        assert_eq!(Environment::solar_factor(SOL_MS / 10), 0.0);
        assert!(Environment::temperature_c(0) < Environment::temperature_c(SOL_MS * 6 / 10));
    }

    #[test]
    fn storms_are_seeded_and_dim_the_sun() {
        let p = Params {
            storm_chance_per_sol: 0.5,
            ..Params::default()
        };
        let a = Environment::generate(9, 60 * SOL_MS, &p);
        assert_eq!(a.storms, Environment::generate(9, 60 * SOL_MS, &p).storms);
        let s = a
            .storms
            .first()
            .expect("with p=0.5 a storm is near-certain");
        let peak = s.start + (s.end - s.start) / 5;
        // Integer-ms peak time lands within a millisecond of the true peak.
        assert!(
            (a.tau(peak) - s.peak_tau).abs() < 1e-4,
            "{} vs {}",
            a.tau(peak),
            s.peak_tau
        );
        assert!(a.tau(peak) > 1.5);
        assert!(
            a.schedule(60 * SOL_MS)
                .contains(&(peak, EnvEvent::StormPeak(0)))
        );
        assert!(a.transmittance(peak) < 0.5);
        assert!(a.storms.windows(2).all(|w| w[0].end <= w[1].start));
    }

    #[test]
    fn schedule_is_sorted_and_has_three_solar_events_per_sol() {
        let env = Environment::generate(1, 3 * SOL_MS, &Params::default());
        let sched = env.schedule(3 * SOL_MS);
        assert!(sched.windows(2).all(|w| w[0].0 <= w[1].0));
        let solar = sched
            .iter()
            .filter(|(_, e)| matches!(e, EnvEvent::Sunrise | EnvEvent::Noon | EnvEvent::Sunset))
            .count();
        assert_eq!(solar, 9);
    }
}
