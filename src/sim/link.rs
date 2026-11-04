//! The Earth link (semantics §8.1–§8.2): the one-way light-time delay, the
//! seeded schedule of outages and the conjunction blackout, the `comms`
//! reports, and store-and-forward delivery.
//!
//! - **Delay.** Earth and Mars on coplanar circular orbits: by the law of
//!   cosines, `d(θ)² = a − b·cos θ`, where θ is the heliocentric angle
//!   between the planets and advances at the synodic rate. `a` and `b` are
//!   fitted so that `d` runs from `delay_min_s` at opposition (θ = 0) to
//!   `delay_max_s` at solar conjunction (θ = π), so it is smooth and can
//!   never leave that range.
//! - **Schedule.** Unplanned outages, and in the conjunction variant a
//!   blackout around conjunction, are drawn up front from the `comms`
//!   stream. Every coordinator on a seed faces the same link.
//! - **Delivery.** A message sent at `t` leaves at the first time at or after
//!   `t` when the link is up, and arrives `d` (at that time) later.

use super::params::Params;
use super::rng::Rng;
use super::time::{SOL_MS, SimMs, ms, secs};
use arcadia_protocol::{CommsCause, Link};
use std::f64::consts::PI;

/// A `comms` event is due whenever the delay has drifted this far since the
/// previous one (semantics §8.1).
pub const DRIFT_REPORT_S: f64 = 60.0;

/// Step of the search for the next drift report. The delay changes by well
/// under a second an hour, so a crossing can never be stepped over.
const DRIFT_SCAN_MS: SimMs = 3_600_000;

/// Why the link is down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Down {
    Conjunction,
    Outage,
}

/// A stretch of time with the link down, `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    pub start: SimMs,
    pub end: SimMs,
    pub cause: Down,
}

/// What a `comms` event reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Report {
    pub t: SimMs,
    pub link: Link,
    pub cause: CommsCause,
    pub delay_s: f64,
    /// When the link will be back: known for a conjunction, not for an outage.
    pub expected_restore: Option<SimMs>,
}

/// The link schedule of one run.
#[derive(Debug, Clone)]
pub struct EarthLink {
    a: f64,
    b: f64,
    /// θ at t = 0, rad.
    theta0: f64,
    /// dθ/dt, rad per ms.
    omega: f64,
    /// Down stretches, in time order, never overlapping.
    gaps: Vec<Gap>,
    /// Every `comms` report after the one at t = 0, in time order.
    reports: Vec<Report>,
}

impl EarthLink {
    pub fn generate(seed: u64, time_limit: SimMs, p: &Params) -> Self {
        let mut rng = Rng::stream(seed, "comms");
        let sol_s = secs(SOL_MS);
        // Drawn in every variant, so the outages below are the same in all of them.
        let u_conjunction = rng.uniform();
        let blackout = p.conjunction.then(|| {
            let (lo, hi) = p.conjunction_start_sols;
            let start = ms((lo + (hi - lo) * u_conjunction) * sol_s);
            Gap {
                start,
                end: start + ms(p.conjunction_sols * sol_s).max(1),
                cause: Down::Conjunction,
            }
        });

        // Unplanned outages: Poisson arrivals while the link is up.
        let mut gaps = Vec::new();
        if p.outage_per_sol > 0.0 {
            let mut t: SimMs = 0;
            loop {
                let wait_s = -libm::log(1.0 - rng.uniform()) * sol_s / p.outage_per_sol;
                let start = t.saturating_add(ms(wait_s));
                if start >= time_limit {
                    break;
                }
                let hours = rng.range(p.outage_hours.0, p.outage_hours.1);
                let end = start + ms(hours * 3600.0).max(1);
                t = end;
                // The blackout wins: an outage that would start during it never
                // happens, and one running into it gives way to it.
                let end = match blackout {
                    Some(b) if start >= b.start && start < b.end => continue,
                    Some(b) if start < b.start && end > b.start => b.start,
                    _ => end,
                };
                gaps.push(Gap {
                    start,
                    end,
                    cause: Down::Outage,
                });
            }
        }
        gaps.extend(blackout);
        gaps.sort_by_key(|g| g.start);

        let (dmin, dmax) = (p.delay_min_s, p.delay_max_s.max(p.delay_min_s));
        let a = (dmax * dmax + dmin * dmin) / 2.0;
        let b = (dmax * dmax - dmin * dmin) / 2.0;
        let omega = 2.0 * PI / (p.synodic_period_sols * SOL_MS as f64);
        let theta0 = match blackout {
            // Conjunction falls in the middle of the blackout.
            Some(g) => PI - omega * (g.start + (g.end - g.start) / 2) as f64,
            // Mars receding, the delay growing from `start_delay_s`.
            None if b > 0.0 => {
                let d0 = p.start_delay_s.clamp(dmin, dmax);
                libm::acos(((a - d0 * d0) / b).clamp(-1.0, 1.0))
            }
            None => 0.0,
        };
        let mut link = Self {
            a,
            b,
            theta0,
            omega,
            gaps,
            reports: Vec::new(),
        };
        link.reports = link.plan_reports(time_limit);
        link
    }

    /// One-way light time at `t`, s.
    pub fn delay_s(&self, t: SimMs) -> f64 {
        let theta = self.theta0 + self.omega * t as f64;
        let d2 = self.a - self.b * libm::cos(theta);
        libm::sqrt(d2.clamp(self.a - self.b, self.a + self.b))
    }

    /// The down stretch `t` falls in, if any.
    pub fn gap_at(&self, t: SimMs) -> Option<&Gap> {
        self.gaps.iter().find(|g| g.start <= t && t < g.end)
    }

    /// Every down stretch, in time order.
    pub fn gaps(&self) -> &[Gap] {
        &self.gaps
    }

    /// The link's state at `t`, as a `comms` event would report it.
    pub fn report_at(&self, t: SimMs) -> Report {
        let (link, cause, expected_restore) = match self.gap_at(t) {
            None => (Link::Up, CommsCause::Nominal, None),
            Some(g) => match g.cause {
                Down::Conjunction => (Link::Down, CommsCause::Conjunction, Some(g.end)),
                Down::Outage => (Link::Down, CommsCause::Outage, None),
            },
        };
        Report {
            t,
            link,
            cause,
            delay_s: self.delay_s(t),
            expected_restore,
        }
    }

    /// The `comms` reports after the one at t = 0: one at every change of
    /// link state, and one whenever the delay has drifted by
    /// [`DRIFT_REPORT_S`] since the previous report.
    pub fn reports(&self) -> &[Report] {
        &self.reports
    }

    /// When a message sent at `t` leaves: the first time at or after `t` with the link up.
    pub fn transmit_time(&self, t: SimMs) -> SimMs {
        let mut t = t;
        while let Some(g) = self.gap_at(t) {
            t = g.end;
        }
        t
    }

    /// When a message sent at `t` arrives at the other end.
    pub fn arrival(&self, t: SimMs) -> SimMs {
        let sent = self.transmit_time(t);
        sent.saturating_add(ms(self.delay_s(sent)))
    }

    /// Time within `[t0, t1)` with the link down for `cause`, ms.
    pub fn down_ms(&self, t0: SimMs, t1: SimMs, cause: Down) -> SimMs {
        self.gaps
            .iter()
            .filter(|g| g.cause == cause)
            .map(|g| t1.min(g.end).saturating_sub(t0.max(g.start)))
            .sum()
    }

    fn plan_reports(&self, time_limit: SimMs) -> Vec<Report> {
        let state = |t: SimMs| {
            let r = self.report_at(t);
            (r.link, r.cause)
        };
        let mut changes: Vec<SimMs> = self
            .gaps
            .iter()
            .flat_map(|g| [g.start, g.end])
            .filter(|&t| t > 0 && t < time_limit && state(t) != state(t - 1))
            .collect();
        changes.sort_unstable();
        changes.dedup();

        let mut reports = Vec::new();
        let mut last_delay = self.delay_s(0);
        let mut t = 0;
        let mut next = changes.into_iter().peekable();
        loop {
            let change = next.peek().copied().unwrap_or(time_limit);
            let at = match self.drift_crossing(t, change, last_delay) {
                Some(drift) => drift,
                None if change < time_limit => {
                    next.next();
                    change
                }
                None => break,
            };
            let report = self.report_at(at);
            last_delay = report.delay_s;
            reports.push(report);
            t = at;
        }
        reports
    }

    /// The first ms in `(t0, t1)` at which the delay is [`DRIFT_REPORT_S`] or
    /// more away from `reported`.
    fn drift_crossing(&self, t0: SimMs, t1: SimMs, reported: f64) -> Option<SimMs> {
        let drifted = |t: SimMs| (self.delay_s(t) - reported).abs() >= DRIFT_REPORT_S;
        let mut lo = t0;
        while lo + 1 < t1 {
            let hi = (lo + DRIFT_SCAN_MS).min(t1 - 1);
            if drifted(hi) {
                // `lo` has not drifted (or is t0 itself); find the first ms that has.
                let (mut a, mut b) = (lo, hi);
                while b - a > 1 {
                    let mid = a + (b - a) / 2;
                    if drifted(mid) {
                        b = mid;
                    } else {
                        a = mid;
                    }
                }
                return Some(b);
            }
            lo = hi;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: SimMs = 60 * SOL_MS;

    fn link(seed: u64, p: &Params) -> EarthLink {
        EarthLink::generate(seed, LIMIT, p)
    }

    #[test]
    fn the_delay_is_smooth_and_stays_between_4_and_24_minutes() {
        for p in [Params::nominal(), Params::conjunction()] {
            for seed in 1..=20 {
                let l = link(seed, &p);
                let mut prev = l.delay_s(0);
                for i in 0..=(LIMIT / 600_000) {
                    let d = l.delay_s(i * 600_000);
                    assert!((240.0..=1440.0).contains(&d), "seed {seed}: {d}");
                    assert!((d - prev).abs() < 0.2, "a gentle drift: {prev} -> {d}");
                    prev = d;
                }
            }
        }
        // The whole orbit stays in range too, and reaches both ends.
        let l = link(1, &Params::nominal());
        let period = (Params::nominal().synodic_period_sols * SOL_MS as f64) as SimMs;
        let samples: Vec<f64> = (0..1000).map(|i| l.delay_s(i * period / 1000)).collect();
        let lo = samples.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = samples.iter().copied().fold(0.0, f64::max);
        assert!((240.0..241.0).contains(&lo), "{lo}");
        assert!((1439.0..=1440.0).contains(&hi), "{hi}");
    }

    #[test]
    fn nominal_starts_near_12_minutes_and_conjunction_near_the_long_end() {
        let n = link(3, &Params::nominal());
        assert!((n.delay_s(0) - 720.0).abs() < 1e-6, "{}", n.delay_s(0));
        assert!(n.delay_s(LIMIT) > n.delay_s(0), "receding");
        assert!(n.delay_s(LIMIT) < 1000.0, "{}", n.delay_s(LIMIT));
        for seed in 1..=10 {
            let c = link(seed, &Params::conjunction());
            let b = *c
                .gaps()
                .iter()
                .find(|g| g.cause == Down::Conjunction)
                .expect("a blackout");
            let mid = b.start + (b.end - b.start) / 2;
            assert!(c.delay_s(mid) > 1439.99, "{}", c.delay_s(mid));
            assert!(c.delay_s(0) > 1400.0 && c.delay_s(LIMIT) > 1390.0);
            // About 14 sols, starting 0.01 to 0.03 sols into the run.
            assert_eq!(b.end - b.start, 14 * SOL_MS);
            assert!(
                b.start >= SOL_MS / 100 && b.start <= 3 * SOL_MS / 100,
                "{}",
                b.start
            );
        }
    }

    #[test]
    fn outages_are_seeded_and_the_same_in_every_variant_outside_the_blackout() {
        let p = Params::nominal();
        let (a, b) = (link(9, &p), link(9, &p));
        assert_eq!(a.gaps(), b.gaps());
        assert_ne!(a.gaps(), link(10, &p).gaps());
        let mut total = 0;
        for seed in 1..=40 {
            let l = link(seed, &p);
            assert!(l.gaps().windows(2).all(|w| w[0].end <= w[1].start));
            for g in l.gaps() {
                assert_eq!(g.cause, Down::Outage);
                let h = secs(g.end - g.start) / 3600.0;
                assert!((2.0..=8.0).contains(&h), "{h}");
            }
            total += l.gaps().len();
            let c = link(seed, &Params::conjunction());
            let blackout = *c
                .gaps()
                .iter()
                .find(|g| g.cause == Down::Conjunction)
                .unwrap();
            for g in l.gaps().iter().filter(|g| g.start < blackout.start) {
                let same = c.gaps().iter().any(|h| {
                    h.cause == Down::Outage
                        && h.start == g.start
                        && h.end == g.end.min(blackout.start)
                });
                assert!(same, "seed {seed}: {g:?}");
            }
            assert!(c.gaps().windows(2).all(|w| w[0].end <= w[1].start));
        }
        // 0.15 a sol over 60 sols: about 9 a run.
        let mean = total as f64 / 40.0;
        assert!((6.0..12.0).contains(&mean), "{mean}");
    }

    #[test]
    fn messages_wait_for_the_link_and_arrive_one_delay_later() {
        let l = link(4, &Params::conjunction());
        let g = l.gaps()[0];
        // While the link is up a message leaves at once.
        let up = g.start - 1;
        assert_eq!(l.transmit_time(up), up);
        assert_eq!(l.arrival(up), up + ms(l.delay_s(up)));
        // During a gap it waits for the link to come back.
        let mid = g.start + (g.end - g.start) / 2;
        let back = l.transmit_time(mid);
        assert!(l.gap_at(back).is_none() && back >= g.end);
        assert_eq!(l.arrival(mid), back + ms(l.delay_s(back)));
        // An outage that gives way to the blackout chains into it.
        let chained = EarthLink {
            gaps: vec![
                Gap {
                    start: 100,
                    end: 200,
                    cause: Down::Outage,
                },
                Gap {
                    start: 200,
                    end: 900,
                    cause: Down::Conjunction,
                },
            ],
            ..l.clone()
        };
        assert_eq!(chained.transmit_time(150), 900);
        assert_eq!(chained.transmit_time(900), 900);
    }

    #[test]
    fn reports_cover_every_change_and_every_60_s_of_drift() {
        for p in [Params::nominal(), Params::conjunction()] {
            for seed in 1..=10 {
                let l = link(seed, &p);
                let reports = l.reports();
                assert!(reports.windows(2).all(|w| w[0].t < w[1].t));
                assert!(reports.iter().all(|r| r.t > 0 && r.t < LIMIT));
                // Every change of state is reported, at the moment it happens.
                for g in l.gaps().iter().filter(|g| g.start < LIMIT) {
                    let r = reports.iter().find(|r| r.t == g.start).expect("down");
                    assert_eq!(r.link, Link::Down);
                    assert_eq!(r.expected_restore.is_some(), g.cause == Down::Conjunction);
                    if g.end < LIMIT && l.gap_at(g.end).is_none() {
                        let up = reports.iter().find(|r| r.t == g.end).expect("up");
                        assert_eq!((up.link, up.cause), (Link::Up, CommsCause::Nominal));
                    }
                }
                // Between reports the delay never drifts 60 s from the last one.
                let mut last = l.delay_s(0);
                let mut i = 0;
                for t in (0..LIMIT).step_by(600_000) {
                    while i < reports.len() && reports[i].t <= t {
                        last = reports[i].delay_s;
                        i += 1;
                    }
                    assert!(
                        (l.delay_s(t) - last).abs() < DRIFT_REPORT_S,
                        "seed {seed} t {t}"
                    );
                }
            }
        }
        // Nominal drifts about 274 s in 60 sols: with no outages resetting the
        // baseline, that is four drift reports, about 13 sols apart.
        let calm = Params {
            outage_per_sol: 0.0,
            ..Params::nominal()
        };
        let l = link(1, &calm);
        assert!(l.gaps().is_empty());
        let reports = l.reports();
        assert_eq!(reports.len(), 4, "{reports:?}");
        for (i, r) in reports.iter().enumerate() {
            let expected = 720.0 + 60.0 * (i + 1) as f64;
            assert!((r.delay_s - expected).abs() < 1e-3, "{r:?}");
            assert_eq!((r.link, r.cause), (Link::Up, CommsCause::Nominal));
        }
        assert!(reports.windows(2).all(|w| w[1].t - w[0].t > 10 * SOL_MS));
    }

    #[test]
    fn down_time_is_exact() {
        let l = link(2, &Params::conjunction());
        let blackout: SimMs = l
            .gaps()
            .iter()
            .filter(|g| g.cause == Down::Conjunction)
            .map(|g| g.end.min(LIMIT) - g.start)
            .sum();
        assert_eq!(l.down_ms(0, LIMIT, Down::Conjunction), blackout);
        let g = l.gaps()[0];
        assert_eq!(l.down_ms(g.start, g.start + 1000, g.cause), 1000);
        assert_eq!(l.down_ms(0, g.start, g.cause), 0);
    }
}
