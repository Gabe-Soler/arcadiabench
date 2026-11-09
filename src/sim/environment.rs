//! Sol clock, daylight, temperature and the seeded dust-storm schedule.
//!
//! The whole schedule is drawn up front from the "weather" stream, so every
//! coordinator on the same seed faces exactly the same weather. The
//! `dust_storm` variant adds a storm front from its own `dust_storm` stream,
//! so the seasonal storms it leaves in place are the same as in every other
//! variant.

use super::params::Params;
use super::rng::Rng;
use super::time::{SOL_MS, SimMs, ms, secs};
use std::f64::consts::PI;

#[derive(Debug, Clone, PartialEq)]
pub struct Storm {
    pub start: SimMs,
    pub end: SimMs,
    pub peak_tau: f64,
    /// When a storm front's opacity peaks. `None` for a seasonal storm,
    /// which builds over the first fifth of its length.
    pub front_peak: Option<SimMs>,
}

impl Storm {
    /// When opacity peaks: a fifth of the way through a seasonal storm, or
    /// at a front's peak. Opacity falls linearly from there to the end.
    pub fn peak(&self) -> SimMs {
        self.front_peak
            .unwrap_or(self.start + (self.end - self.start) / 5)
    }
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

/// When robot heaters run: the part of each sol colder than the heater threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cold {
    Never,
    Always,
    /// Cold from `start` to `end` ms after midnight; wraps past midnight when `start > end`.
    Window {
        start: SimMs,
        end: SimMs,
    },
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub storms: Vec<Storm>,
    clear_tau: f64,
    extinction: f64,
    cold: Cold,
}

const SUNRISE: f64 = 0.25;
const NOON: f64 = 0.5;
const SUNSET: f64 = 0.75;
/// Daily temperature cycle: mean, half-swing, and the phase at which it crosses the mean going up.
const TEMP_MEAN_C: f64 = -50.0;
const TEMP_SWING_C: f64 = 30.0;
const TEMP_RISING_PHASE: f64 = 0.375;

impl Environment {
    /// The weather of a run from `start` to `time_limit`. Seasonal storms
    /// follow the calendar from sol 0, so they depend on the seed alone; the
    /// `dust_storm` variant's front is timed from the start of the run.
    pub fn generate(seed: u64, start: SimMs, time_limit: SimMs, p: &Params) -> Self {
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
                    front_peak: None,
                });
            }
            sol += 1;
        }
        if p.dust_storm {
            let front = Self::storm_front(seed, start, p);
            // Seasonal storms that would overlap the front give way to it.
            storms.retain(|s| s.end <= front.start || s.start >= front.end);
            let at = storms.partition_point(|s| s.start < front.start);
            storms.insert(at, front);
        }
        Self {
            storms,
            clear_tau: p.clear_tau,
            extinction: p.dust_extinction,
            cold: Self::cold_window(p.heater_threshold_c),
        }
    }

    /// The `dust_storm` variant's storm: a front arriving early in a run that
    /// starts at `run_start`, drawn from its own stream.
    fn storm_front(seed: u64, run_start: SimMs, p: &Params) -> Storm {
        let mut rng = Rng::stream(seed, "dust_storm");
        let sol_s = secs(SOL_MS);
        let (s0, s1) = p.dust_storm_start_sols;
        let start = run_start + ms(rng.range(s0, s1) * sol_s);
        let length = ms(rng.range(p.dust_storm_sols.0, p.dust_storm_sols.1) * sol_s).max(2);
        let peak_tau = rng.range(p.dust_storm_peak_tau.0, p.dust_storm_peak_tau.1);
        let rise = ms(p.dust_storm_rise_sols * sol_s).clamp(1, length - 1);
        Storm {
            start,
            end: start + length,
            peak_tau,
            front_peak: Some(start + rise),
        }
    }

    /// The part of each sol whose temperature is below `threshold_c`.
    fn cold_window(threshold_c: f64) -> Cold {
        let s = (threshold_c - TEMP_MEAN_C) / TEMP_SWING_C;
        if s <= -1.0 {
            return Cold::Never;
        }
        if s >= 1.0 {
            return Cold::Always;
        }
        // Warm while sin(2π(φ - rising)) >= s, i.e. for φ in [rising + a/2π, rising + 1/2 - a/2π].
        let a = libm::asin(s) / (2.0 * PI);
        let frac = |x: f64| x - libm::floor(x);
        let at = |phase: f64| ((frac(phase) * SOL_MS as f64).round() as SimMs).min(SOL_MS - 1);
        Cold::Window {
            start: at(TEMP_RISING_PHASE + 0.5 - a),
            end: at(TEMP_RISING_PHASE + 1.0 + a),
        }
    }

    /// Milliseconds of `[t0, t1)` colder than the heater threshold.
    pub fn cold_ms(&self, t0: SimMs, t1: SimMs) -> SimMs {
        if t1 <= t0 {
            return 0;
        }
        let overlap = |a0: SimMs, a1: SimMs| t1.min(a1).saturating_sub(t0.max(a0));
        match self.cold {
            Cold::Never => 0,
            Cold::Always => t1 - t0,
            Cold::Window { start, end } => (t0 / SOL_MS..=(t1 - 1) / SOL_MS)
                .map(|k| {
                    let base = k * SOL_MS;
                    if start <= end {
                        overlap(base + start, base + end)
                    } else {
                        overlap(base, base + end) + overlap(base + start, base + SOL_MS)
                    }
                })
                .sum(),
        }
    }

    /// Whether robot heaters run at `t`.
    pub fn is_cold(&self, t: SimMs) -> bool {
        self.cold_ms(t, t + 1) == 1
    }

    /// The next time after `t` at which heaters switch on or off.
    pub fn next_cold_change(&self, t: SimMs) -> Option<SimMs> {
        match self.cold {
            Cold::Never | Cold::Always => None,
            Cold::Window { start, end } => {
                let base = t / SOL_MS * SOL_MS;
                [start, end, SOL_MS + start, SOL_MS + end]
                    .into_iter()
                    .map(|o| base + o)
                    .filter(|&c| c > t)
                    .min()
            }
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

    /// Dust optical depth: ramps up linearly to the storm's peak (the first
    /// 20% of a seasonal storm), then decays linearly to clear at its end.
    pub fn tau(&self, t: SimMs) -> f64 {
        match self.storm_at(t) {
            None => self.clear_tau,
            Some(s) => {
                let shape = match s.front_peak {
                    None => {
                        let f = (t - s.start) as f64 / (s.end - s.start) as f64;
                        if f < 0.2 { f / 0.2 } else { (1.0 - f) / 0.8 }
                    }
                    Some(peak) if t < peak => (t - s.start) as f64 / (peak - s.start) as f64,
                    Some(peak) => (s.end - t) as f64 / (s.end - peak) as f64,
                };
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
        TEMP_MEAN_C + TEMP_SWING_C * libm::sin(2.0 * PI * (Self::sol_phase(t) - TEMP_RISING_PHASE))
    }

    /// Whether a dust storm is active at `t`.
    pub fn storm_active(&self, t: SimMs) -> bool {
        self.storm_at(t).is_some()
    }

    /// Total time within `[t0, t1)` with a dust storm active, ms.
    pub fn storm_ms(&self, t0: SimMs, t1: SimMs) -> SimMs {
        self.storms
            .iter()
            .map(|s| t1.min(s.end).saturating_sub(t0.max(s.start)))
            .sum()
    }

    /// Clear-sky optical depth.
    pub fn clear_tau(&self) -> f64 {
        self.clear_tau
    }

    /// Every scheduled environment event in `[from, until)`, in time order.
    /// A storm under way at `from` has no start event, only what is left of it.
    pub fn schedule(&self, from: SimMs, until: SimMs) -> Vec<(SimMs, EnvEvent)> {
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
            let peak = s.peak();
            if peak < until {
                out.push((peak, EnvEvent::StormPeak(i)));
            }
            if s.end < until {
                out.push((s.end, EnvEvent::StormEnd(i)));
            }
        }
        out.retain(|&(t, _)| t >= from);
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
        let a = Environment::generate(9, 0, 60 * SOL_MS, &p);
        assert_eq!(
            a.storms,
            Environment::generate(9, 0, 60 * SOL_MS, &p).storms
        );
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
            a.schedule(0, 60 * SOL_MS)
                .contains(&(peak, EnvEvent::StormPeak(0)))
        );
        assert!(a.transmittance(peak) < 0.5);
        assert!(a.storms.windows(2).all(|w| w[0].end <= w[1].start));
    }

    #[test]
    fn cold_window_matches_the_temperature_curve() {
        let p = Params::default();
        let env = Environment::generate(1, 0, SOL_MS, &p);
        // Sampled every minute over two sols, away from the boundaries.
        let mut cold_minutes = 0;
        for i in 0..(2 * SOL_MS / 60_000) {
            let t = i * 60_000;
            let temp = Environment::temperature_c(t);
            if (temp - p.heater_threshold_c).abs() > 0.05 {
                assert_eq!(
                    env.is_cold(t),
                    temp < p.heater_threshold_c,
                    "t = {t}, {temp} C"
                );
            }
            cold_minutes += u64::from(env.is_cold(t));
        }
        // About 39% of a sol is below -60 C.
        let frac = cold_minutes as f64 / (2 * SOL_MS / 60_000) as f64;
        assert!((0.35..0.43).contains(&frac), "{frac}");
        // Coldest before dawn, warm in the afternoon.
        assert!(env.is_cold(SOL_MS / 8));
        assert!(!env.is_cold(SOL_MS * 6 / 10));
    }

    #[test]
    fn cold_time_adds_up_and_changes_are_found() {
        let env = Environment::generate(1, 0, 3 * SOL_MS, &Params::default());
        let (a, b, c) = (12_345, SOL_MS + 777, 2 * SOL_MS + 99_999);
        assert_eq!(env.cold_ms(a, b) + env.cold_ms(b, c), env.cold_ms(a, c));
        assert_eq!(env.cold_ms(a, a), 0);
        let total = env.cold_ms(0, SOL_MS);
        let mut t = 0;
        let mut summed = 0;
        while let Some(next) = env.next_cold_change(t).filter(|&n| n <= SOL_MS) {
            assert!(next > t);
            if env.is_cold(t) {
                summed += next - t;
            }
            t = next;
        }
        if env.is_cold(t) {
            summed += SOL_MS - t;
        }
        assert_eq!(summed, total);
        let never = Environment::generate(
            1,
            0,
            SOL_MS,
            &Params {
                heater_threshold_c: -200.0,
                ..Params::default()
            },
        );
        assert_eq!(never.cold_ms(0, 10 * SOL_MS), 0);
        assert_eq!(never.next_cold_change(5), None);
    }

    #[test]
    fn storm_time_is_exact() {
        let p = Params {
            storm_chance_per_sol: 0.5,
            ..Params::default()
        };
        let limit = 60 * SOL_MS;
        let env = Environment::generate(9, 0, limit, &p);
        let expected: SimMs = env.storms.iter().map(|s| s.end.min(limit) - s.start).sum();
        assert_eq!(env.storm_ms(0, limit), expected);
        let s = env.storms[0].clone();
        assert_eq!(
            env.storm_ms(0, s.start),
            0,
            "nothing before the first storm"
        );
        assert_eq!(env.storm_ms(s.start, s.start + 1000), 1000);
        assert!(env.storm_active(s.start) && !env.storm_active(s.end));
    }

    #[test]
    fn schedule_is_sorted_and_has_three_solar_events_per_sol() {
        let env = Environment::generate(1, 0, 3 * SOL_MS, &Params::default());
        let sched = env.schedule(0, 3 * SOL_MS);
        assert!(sched.windows(2).all(|w| w[0].0 <= w[1].0));
        let solar = sched
            .iter()
            .filter(|(_, e)| matches!(e, EnvEvent::Sunrise | EnvEvent::Noon | EnvEvent::Sunset))
            .count();
        assert_eq!(solar, 9);
    }

    #[test]
    fn the_dust_storm_variant_brings_a_severe_storm_early_and_keeps_the_rest() {
        let limit = 60 * SOL_MS;
        let p = Params::dust_storm();
        for seed in 1..=40 {
            let nominal = Environment::generate(seed, 0, limit, &Params::nominal());
            let env = Environment::generate(seed, 0, limit, &p);
            let front: Vec<&Storm> = env
                .storms
                .iter()
                .filter(|s| s.front_peak.is_some())
                .collect();
            assert_eq!(front.len(), 1, "seed {seed}");
            let s = front[0];
            let sol = |x: f64| ms(x * secs(SOL_MS));
            assert!(
                s.start >= sol(p.dust_storm_start_sols.0)
                    && s.start <= sol(p.dust_storm_start_sols.1)
            );
            assert!(
                s.end - s.start >= sol(p.dust_storm_sols.0)
                    && s.end - s.start <= sol(p.dust_storm_sols.1)
            );
            assert_eq!(s.peak() - s.start, sol(p.dust_storm_rise_sols));
            assert!((p.dust_storm_peak_tau.0..=p.dust_storm_peak_tau.1).contains(&s.peak_tau));
            // Opacity climbs to the peak, then falls back to clear by the end.
            assert!((env.tau(s.peak()) - s.peak_tau).abs() < 1e-9);
            let mid_rise = s.start + (s.peak() - s.start) / 2;
            let half = env.clear_tau() + (s.peak_tau - env.clear_tau()) / 2.0;
            assert!((env.tau(mid_rise) - half).abs() < 1e-3);
            assert!(env.tau(s.end - 1) - env.clear_tau() < 1e-3);
            assert!(env.transmittance(s.peak()) < 0.2);
            // Seasonal storms clear of the front are untouched; none overlap it.
            assert!(env.storms.windows(2).all(|w| w[0].end <= w[1].start));
            for n in &nominal.storms {
                let clear = n.end <= s.start || n.start >= s.end;
                assert_eq!(env.storms.contains(n), clear, "seed {seed}");
            }
            assert!(
                env.schedule(0, limit)
                    .iter()
                    .any(|&(t, e)| t == s.peak() && matches!(e, EnvEvent::StormPeak(_)))
            );
        }
        // Nominal never draws a front.
        let env = Environment::generate(3, 0, limit, &Params::nominal());
        assert!(env.storms.iter().all(|s| s.front_peak.is_none()));
    }

    #[test]
    fn a_run_that_starts_mid_sol_keeps_the_calendar_and_times_its_front_from_the_start() {
        let (start, limit) = (SOL_MS * 2 / 3, 60 * SOL_MS);
        // The sky follows the calendar: the seasonal storms are the same.
        let p = Params {
            storm_chance_per_sol: 0.5,
            ..Params::default()
        };
        let midnight = Environment::generate(9, 0, limit, &p);
        let later = Environment::generate(9, start, limit, &p);
        assert_eq!(later.storms, midnight.storms);
        // Nothing is scheduled before the run starts; the rest is unchanged.
        let all = midnight.schedule(0, limit);
        let from_start = later.schedule(start, limit);
        assert!(from_start.iter().all(|&(t, _)| t >= start));
        let expected: Vec<_> = all.iter().copied().filter(|&(t, _)| t >= start).collect();
        assert_eq!(from_start, expected);
        assert!(from_start.len() < all.len(), "sol 0's sunrise is past");
        // The dust storm variant's front arrives that long into the run.
        let p = Params::dust_storm();
        let sol = |x: f64| ms(x * secs(SOL_MS));
        for seed in 1..=10 {
            let at_midnight = Environment::generate(seed, 0, limit, &p);
            let front = |e: &Environment| e.storms.iter().find(|s| s.front_peak.is_some()).cloned();
            let (a, b) = (
                front(&at_midnight).unwrap(),
                front(&Environment::generate(seed, start, limit, &p)).unwrap(),
            );
            assert_eq!(b.start - start, a.start, "seed {seed}");
            assert_eq!(b.end - b.start, a.end - a.start);
            assert!(b.start - start <= sol(p.dust_storm_start_sols.1));
        }
    }
}
