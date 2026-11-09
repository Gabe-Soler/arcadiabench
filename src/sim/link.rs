//! The Earth link (semantics §8.1–§8.2): the one-way light-time delay, the
//! seeded schedule of pass windows, outages and the conjunction blackout,
//! the `comms` reports, and store-and-forward delivery.
//!
//! - **Delay.** Earth and Mars on coplanar circular orbits: by the law of
//!   cosines, `d(θ)² = a − b·cos θ`, where θ is the heliocentric angle
//!   between the planets and advances at the synodic rate. `a` and `b` are
//!   fitted so that `d` runs from `delay_min_s` at opposition (θ = 0) to
//!   `delay_max_s` at solar conjunction (θ = π), so it is smooth and can
//!   never leave that range.
//! - **Schedule.** Unplanned outages, and in the conjunction variant a
//!   blackout around conjunction, are drawn up front from the `comms`
//!   stream. When the link runs in pass windows, they are drawn from the
//!   `passes` stream, so turning them on changes no outage. Every
//!   coordinator on a seed faces the same link.
//! - **Up or down.** The link is up inside a pass window (or always, with no
//!   pass schedule) unless the blackout or an outage holds it down. While it
//!   is down its cause is the first of [`Down`] that holds.
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

/// Why the link is down. When several hold at once, the first one here is
/// the cause (semantics §8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Down {
    Conjunction,
    Outage,
    /// The gap between two pass windows.
    BetweenPasses,
}

impl Down {
    fn cause(self) -> CommsCause {
        match self {
            Down::Conjunction => CommsCause::Conjunction,
            Down::Outage => CommsCause::Outage,
            Down::BetweenPasses => CommsCause::BetweenPasses,
        }
    }
}

/// A stretch of time, `[start, end)`, with the link held down by the
/// blackout or by an unplanned outage.
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
    /// While down: when the link is next scheduled to come up. Known for the
    /// blackout and for a gap between passes, not for an outage.
    pub expected_restore: Option<SimMs>,
    /// While up in a pass window: when the link is next scheduled to go down.
    pub window_end: Option<SimMs>,
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
    /// When the run starts.
    start: SimMs,
    /// The blackout and the outages, in time order, never overlapping.
    gaps: Vec<Gap>,
    /// The pass windows `[start, end)`, in time order with a gap between
    /// each two; `None` for a continuous link.
    windows: Option<Vec<(SimMs, SimMs)>>,
    /// Every `comms` report after the one at the start, in time order.
    reports: Vec<Report>,
}

impl EarthLink {
    /// The link of a run from `start` to `time_limit`.
    pub fn generate(seed: u64, start: SimMs, time_limit: SimMs, p: &Params) -> Self {
        let mut rng = Rng::stream(seed, "comms");
        let sol_s = secs(SOL_MS);
        // Drawn in every variant, so the outages below are the same in all of them.
        let u_conjunction = rng.uniform();
        let blackout = p.conjunction.then(|| {
            let (lo, hi) = p.conjunction_start_sols;
            let begin = start + ms((lo + (hi - lo) * u_conjunction) * sol_s);
            Gap {
                start: begin,
                end: begin + ms(p.conjunction_sols * sol_s).max(1),
                cause: Down::Conjunction,
            }
        });

        // Unplanned outages: Poisson arrivals while the link is up.
        let mut gaps = Vec::new();
        if p.outage_per_sol > 0.0 {
            let mut t: SimMs = start;
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
            // Mars receding, the delay growing from `start_delay_s` at the start.
            None if b > 0.0 => {
                let d0 = p.start_delay_s.clamp(dmin, dmax);
                libm::acos(((a - d0 * d0) / b).clamp(-1.0, 1.0)) - omega * start as f64
            }
            None => 0.0,
        };
        let windows = p
            .pass_windows
            .then(|| Self::pass_windows(seed, time_limit, p));
        let mut link = Self {
            a,
            b,
            theta0,
            omega,
            start,
            gaps,
            windows,
            reports: Vec::new(),
        };
        link.reports = link.plan_reports(time_limit);
        link
    }

    /// The pass windows, from the `passes` stream: each sol has one
    /// direct-to-Earth window and a few relay passes, and windows that
    /// overlap or touch are merged. They are laid out by the calendar, from
    /// sol 0, so they depend on the seed alone. The schedule runs one whole
    /// sol past `time_limit`, so that to the end of the run there is always
    /// a next window to expect.
    fn pass_windows(seed: u64, time_limit: SimMs, p: &Params) -> Vec<(SimMs, SimMs)> {
        let mut rng = Rng::stream(seed, "passes");
        let sol_s = secs(SOL_MS);
        let mut windows = Vec::new();
        let mut sol = 0;
        while sol * SOL_MS < time_limit.saturating_add(SOL_MS) {
            let base = sol * SOL_MS;
            let open = base + ms(rng.range(p.dte_start_phase.0, p.dte_start_phase.1) * sol_s);
            let length = ms(rng.range(p.dte_hours.0, p.dte_hours.1) * 3600.0);
            windows.push((open, open + length));
            let (lo, hi) = p.relay_passes_per_sol;
            let span = f64::from(hi.saturating_sub(lo)) + 1.0;
            let passes = (lo + (rng.uniform() * span) as u32).min(hi.max(lo));
            for _ in 0..passes {
                let open = base + ms(rng.uniform() * sol_s);
                let (m0, m1) = p.relay_pass_minutes;
                windows.push((open, open + ms(rng.range(m0, m1) * 60.0)));
            }
            sol += 1;
        }
        windows.retain(|&(open, close)| close > open);
        windows.sort_unstable();
        let mut merged: Vec<(SimMs, SimMs)> = Vec::new();
        for (open, close) in windows {
            match merged.last_mut() {
                Some(last) if open <= last.1 => last.1 = last.1.max(close),
                _ => merged.push((open, close)),
            }
        }
        merged
    }

    /// One-way light time at `t`, s.
    pub fn delay_s(&self, t: SimMs) -> f64 {
        let theta = self.theta0 + self.omega * t as f64;
        let d2 = self.a - self.b * libm::cos(theta);
        libm::sqrt(d2.clamp(self.a - self.b, self.a + self.b))
    }

    /// The blackout or outage `t` falls in, if any.
    pub fn gap_at(&self, t: SimMs) -> Option<&Gap> {
        self.gaps.iter().find(|g| g.start <= t && t < g.end)
    }

    /// The blackout and every outage, in time order.
    pub fn gaps(&self) -> &[Gap] {
        &self.gaps
    }

    /// The pass windows, in time order; `None` for a continuous link.
    pub fn windows(&self) -> Option<&[(SimMs, SimMs)]> {
        self.windows.as_deref()
    }

    /// The pass window `t` falls in, if the link has a pass schedule and `t`
    /// is inside one of its windows.
    fn window_at(&self, t: SimMs) -> Option<(SimMs, SimMs)> {
        let windows = self.windows.as_ref()?;
        let opened = windows.partition_point(|&(open, _)| open <= t);
        (opened > 0 && t < windows[opened - 1].1).then(|| windows[opened - 1])
    }

    /// The first pass window to open after `t`.
    fn next_window(&self, t: SimMs) -> Option<(SimMs, SimMs)> {
        let windows = self.windows.as_ref()?;
        windows
            .get(windows.partition_point(|&(open, _)| open <= t))
            .copied()
    }

    fn blackout(&self) -> Option<&Gap> {
        self.gaps.iter().find(|g| g.cause == Down::Conjunction)
    }

    /// Why the link is down at `t`, if it is.
    pub fn down_at(&self, t: SimMs) -> Option<Down> {
        match self.gap_at(t) {
            Some(g) => Some(g.cause),
            None if self.windows.is_some() && self.window_at(t).is_none() => {
                Some(Down::BetweenPasses)
            }
            None => None,
        }
    }

    /// The first time at or after `t` at which the link is scheduled to be
    /// up: inside a pass window, if it has a pass schedule, and outside the
    /// blackout. Outages are unplanned, so they do not count. `None` when no
    /// window is left.
    fn scheduled_up(&self, mut t: SimMs) -> Option<SimMs> {
        loop {
            match self.blackout() {
                Some(b) if b.start <= t && t < b.end => t = b.end,
                _ if self.windows.is_none() || self.window_at(t).is_some() => return Some(t),
                _ => t = self.next_window(t)?.0,
            }
        }
    }

    /// When the link, up at `t`, is next scheduled to go down: the end of
    /// its pass window, or the start of the blackout if that comes first.
    /// `None` for a continuous link.
    fn window_end(&self, t: SimMs) -> Option<SimMs> {
        let (_, close) = self.window_at(t)?;
        Some(match self.blackout() {
            Some(b) if b.start > t && b.start < close => b.start,
            _ => close,
        })
    }

    /// The link's state at `t`, as a `comms` event would report it.
    pub fn report_at(&self, t: SimMs) -> Report {
        let down = self.down_at(t);
        Report {
            t,
            link: if down.is_some() { Link::Down } else { Link::Up },
            cause: down.map_or(CommsCause::Nominal, Down::cause),
            delay_s: self.delay_s(t),
            expected_restore: match down {
                Some(Down::Conjunction | Down::BetweenPasses) => self.scheduled_up(t),
                Some(Down::Outage) | None => None,
            },
            window_end: match down {
                None => self.window_end(t),
                Some(_) => None,
            },
        }
    }

    /// The `comms` reports after the one at the start: one at every change
    /// of the link's state or cause, and one whenever the delay has drifted
    /// by [`DRIFT_REPORT_S`] since the previous report.
    pub fn reports(&self) -> &[Report] {
        &self.reports
    }

    /// When a message sent at `t` leaves: the first time at or after `t`
    /// with the link up. Never, if the pass schedule has run out.
    pub fn transmit_time(&self, t: SimMs) -> SimMs {
        let mut t = t;
        loop {
            if let Some(g) = self.gap_at(t) {
                t = g.end;
            } else if self.windows.is_none() || self.window_at(t).is_some() {
                return t;
            } else if let Some((open, _)) = self.next_window(t) {
                t = open;
            } else {
                return SimMs::MAX;
            }
        }
    }

    /// When a message sent at `t` arrives at the other end.
    pub fn arrival(&self, t: SimMs) -> SimMs {
        let sent = self.transmit_time(t);
        sent.saturating_add(ms(self.delay_s(sent)))
    }

    /// Time within `[t0, t1)` with the link down for `cause`, ms. The three
    /// causes together account for all the time the link is down.
    pub fn down_ms(&self, t0: SimMs, t1: SimMs, cause: Down) -> SimMs {
        if cause != Down::BetweenPasses {
            // The blackout and the outages never overlap, and nothing outranks them.
            return self
                .gaps
                .iter()
                .filter(|g| g.cause == cause)
                .map(|g| t1.min(g.end).saturating_sub(t0.max(g.start)))
                .sum();
        }
        let Some(windows) = &self.windows else {
            return 0;
        };
        // The state only changes at the edge of a window or of a gap, so it
        // is constant between consecutive edges.
        let mut edges: Vec<SimMs> = windows
            .iter()
            .flat_map(|&(open, close)| [open, close])
            .chain(self.gaps.iter().flat_map(|g| [g.start, g.end]))
            .filter(|&t| t > t0 && t < t1)
            .chain([t0, t1])
            .collect();
        edges.sort_unstable();
        edges.dedup();
        edges
            .windows(2)
            .filter(|w| self.down_at(w[0]) == Some(Down::BetweenPasses))
            .map(|w| w[1] - w[0])
            .sum()
    }

    fn plan_reports(&self, time_limit: SimMs) -> Vec<Report> {
        let state = |t: SimMs| {
            let r = self.report_at(t);
            (r.link, r.cause)
        };
        let start = self.start;
        let mut changes: Vec<SimMs> = self
            .gaps
            .iter()
            .flat_map(|g| [g.start, g.end])
            .chain(
                self.windows
                    .iter()
                    .flatten()
                    .flat_map(|&(open, close)| [open, close]),
            )
            .filter(|&t| t > start && t < time_limit && state(t) != state(t - 1))
            .collect();
        changes.sort_unstable();
        changes.dedup();

        let mut reports = Vec::new();
        let mut last_delay = self.delay_s(start);
        let mut t = start;
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
        EarthLink::generate(seed, 0, LIMIT, p)
    }

    /// A link that runs in pass windows.
    fn passes() -> Params {
        Params {
            pass_windows: true,
            ..Params::nominal()
        }
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
        // A continuous link is never between passes.
        assert!(l.windows().is_none());
        assert_eq!(l.down_ms(0, LIMIT, Down::BetweenPasses), 0);
    }

    /// A hand-made link: two windows, [1000, 2000) and [5000, 6000), an
    /// outage and a blackout.
    fn handmade(gaps: Vec<Gap>) -> EarthLink {
        EarthLink {
            gaps,
            windows: Some(vec![(1000, 2000), (5000, 6000)]),
            ..link(1, &Params::nominal())
        }
    }

    #[test]
    fn pass_windows_are_seeded_and_leave_the_outages_alone() {
        let (plain, windowed) = (Params::nominal(), passes());
        for seed in 1..=20 {
            let l = link(seed, &windowed);
            // The outages come from the `comms` stream, the passes from their own.
            assert_eq!(l.gaps(), link(seed, &plain).gaps(), "seed {seed}");
            let w = l.windows().expect("a pass schedule");
            assert_eq!(l.windows(), link(seed, &windowed).windows());
            assert!(w.iter().all(|&(open, close)| open < close));
            assert!(
                w.windows(2).all(|p| p[0].1 < p[1].0),
                "a gap between passes"
            );
            // Every sol has its direct-to-Earth window of 2 to 3 hours, and
            // 3 to 5 relay passes of 10 to 15 minutes: 4 to 6 windows unless
            // some merged, up for about a tenth of the sol.
            for sol in 0..60 {
                let (a, b) = (sol * SOL_MS, (sol + 1) * SOL_MS);
                let opened = w.iter().filter(|&&(open, _)| a <= open && open < b).count();
                assert!((1..=6).contains(&opened), "seed {seed} sol {sol}: {opened}");
            }
            let up: SimMs = w
                .iter()
                .map(|&(open, close)| close.min(LIMIT).saturating_sub(open))
                .sum();
            let share = up as f64 / LIMIT as f64;
            assert!((0.08..0.18).contains(&share), "seed {seed}: up {share}");
            // The schedule runs a sol past the limit, so there is always a
            // next window to expect.
            assert!(w.last().unwrap().0 >= LIMIT);
        }
        assert_ne!(link(1, &windowed).windows(), link(2, &windowed).windows());
    }

    #[test]
    fn the_first_cause_that_holds_is_reported() {
        let l = handmade(vec![
            Gap {
                start: 1500,
                end: 5500,
                cause: Down::Outage,
            },
            Gap {
                start: 5800,
                end: 9000,
                cause: Down::Conjunction,
            },
        ]);
        let at = |t| {
            let r = l.report_at(t);
            (r.link, r.cause, r.expected_restore, r.window_end)
        };
        use CommsCause::*;
        // Before the first window: between passes, back when it opens.
        assert_eq!(at(0), (Link::Down, BetweenPasses, Some(1000), None));
        // In the window: up until it closes. The outage is not scheduled.
        assert_eq!(at(1000), (Link::Up, Nominal, None, Some(2000)));
        // The outage outranks the gap between passes, and nobody knows its end.
        assert_eq!(at(1500), (Link::Down, Outage, None, None));
        assert_eq!(at(3000), (Link::Down, Outage, None, None));
        // It ends inside the second window, which the blackout cuts short.
        assert_eq!(at(5500), (Link::Up, Nominal, None, Some(5800)));
        // The blackout outranks everything. No window is left after it.
        assert_eq!(at(5800), (Link::Down, Conjunction, None, None));
        assert_eq!(at(9000), (Link::Down, BetweenPasses, None, None));
        for (t, down) in [
            (0, Some(Down::BetweenPasses)),
            (1499, None),
            (2500, Some(Down::Outage)),
            (5799, None),
            (6500, Some(Down::Conjunction)),
            (9000, Some(Down::BetweenPasses)),
        ] {
            assert_eq!(l.down_at(t), down, "t {t}");
        }
    }

    #[test]
    fn an_outage_that_ends_between_passes_leaves_the_link_down() {
        let l = handmade(vec![Gap {
            start: 1500,
            end: 3000,
            cause: Down::Outage,
        }]);
        let before = l.report_at(2999);
        assert_eq!(
            (before.cause, before.expected_restore),
            (CommsCause::Outage, None)
        );
        // The cause changes while the link stays down, and now its return is known.
        let after = l.report_at(3000);
        assert_eq!(
            (after.link, after.cause, after.expected_restore),
            (Link::Down, CommsCause::BetweenPasses, Some(5000))
        );
        // A blackout that covers the next window pushes the return past it.
        let dark = handmade(vec![Gap {
            start: 2500,
            end: 5400,
            cause: Down::Conjunction,
        }]);
        assert_eq!(dark.report_at(2000).expected_restore, Some(5400));
        assert_eq!(dark.report_at(3000).expected_restore, Some(5400));
        assert_eq!(dark.report_at(5400).window_end, Some(6000));
    }

    #[test]
    fn messages_wait_for_a_pass() {
        let l = handmade(vec![Gap {
            start: 1500,
            end: 5500,
            cause: Down::Outage,
        }]);
        assert_eq!(l.transmit_time(0), 1000, "the first window");
        assert_eq!(l.transmit_time(1200), 1200, "in a window, at once");
        // The outage takes the rest of this window and the start of the next.
        assert_eq!(l.transmit_time(1500), 5500);
        assert_eq!(l.transmit_time(4000), 5500);
        assert_eq!(l.arrival(4000), 5500 + ms(l.delay_s(5500)));
        // After the last window nothing leaves again.
        assert_eq!(l.transmit_time(6000), SimMs::MAX);
        assert_eq!(l.arrival(6000), SimMs::MAX);
    }

    #[test]
    fn the_three_causes_account_for_all_the_down_time() {
        let l = handmade(vec![
            Gap {
                start: 1500,
                end: 5500,
                cause: Down::Outage,
            },
            Gap {
                start: 5800,
                end: 9000,
                cause: Down::Conjunction,
            },
        ]);
        // Down [0,1000) between passes, [1500,5500) outage, [5800,9000) blackout,
        // [9000,10000) between passes.
        assert_eq!(l.down_ms(0, 10_000, Down::BetweenPasses), 2000);
        assert_eq!(l.down_ms(0, 10_000, Down::Outage), 4000);
        assert_eq!(l.down_ms(0, 10_000, Down::Conjunction), 3200);
        assert_eq!(l.down_ms(500, 9500, Down::BetweenPasses), 1000);
        // On a generated schedule: every millisecond is up or has one cause.
        let p = Params {
            conjunction: true,
            conjunction_start_sols: (3.0, 4.0),
            ..passes()
        };
        for seed in 1..=5 {
            let l = link(seed, &p);
            let down: SimMs = [Down::Conjunction, Down::Outage, Down::BetweenPasses]
                .into_iter()
                .map(|c| l.down_ms(0, LIMIT, c))
                .sum();
            let step = 60_000;
            let sampled = (0..LIMIT / step)
                .filter(|i| l.down_at(i * step).is_some())
                .count() as u64
                * step;
            let err = down.abs_diff(sampled) as f64 / LIMIT as f64;
            assert!(err < 0.005, "seed {seed}: {down} vs sampled {sampled}");
            assert!(l.down_ms(0, LIMIT, Down::Conjunction) > 13 * SOL_MS);
            assert!(l.down_ms(0, LIMIT, Down::BetweenPasses) > 30 * SOL_MS);
        }
    }

    #[test]
    fn reports_follow_the_windows() {
        let p = passes();
        for seed in 1..=5 {
            let l = link(seed, &p);
            let reports = l.reports();
            assert!(reports.windows(2).all(|w| w[0].t < w[1].t));
            // Every report is a change of state or cause, or a drift report.
            let mut last = l.report_at(0);
            for r in reports {
                assert_eq!(*r, l.report_at(r.t));
                let changed = (r.link, r.cause) != (last.link, last.cause);
                let drifted = (r.delay_s - last.delay_s).abs() >= DRIFT_REPORT_S - 1e-6;
                assert!(changed || drifted, "seed {seed}: {r:?} after {last:?}");
                last = *r;
            }
            // Every window that opens with the link free is reported, with its end.
            for &(open, close) in l.windows().unwrap() {
                if open == 0 || open >= LIMIT || l.gap_at(open).is_some() {
                    continue;
                }
                let r = reports.iter().find(|r| r.t == open).expect("the opening");
                assert_eq!((r.link, r.cause), (Link::Up, CommsCause::Nominal));
                assert_eq!(r.window_end, Some(close));
            }
            // While down between passes the next opening is always known.
            for r in reports
                .iter()
                .filter(|r| r.cause == CommsCause::BetweenPasses)
            {
                let back = r.expected_restore.expect("the next window");
                assert!(back > r.t && l.windows().unwrap().iter().any(|w| w.0 == back));
            }
        }
    }

    #[test]
    fn a_run_can_start_at_any_time() {
        let start = SOL_MS * 2 / 3;
        let limit = start + LIMIT;
        // Nominal: the delay is `start_delay_s` at the start, wherever that is.
        let n = EarthLink::generate(3, start, limit, &Params::nominal());
        assert!(
            (n.delay_s(start) - 720.0).abs() < 1e-6,
            "{}",
            n.delay_s(start)
        );
        // No report before the start, and the drift is counted from it.
        assert!(n.reports().iter().all(|r| r.t > start && r.t < limit));
        let calm = Params {
            outage_per_sol: 0.0,
            ..Params::nominal()
        };
        let l = EarthLink::generate(1, start, limit, &calm);
        let first = l.reports()[0];
        assert!((first.delay_s - 780.0).abs() < 1e-3, "{first:?}");
        // No outage begins before the start; the blackout is timed from it.
        assert!(n.gaps().iter().all(|g| g.start >= start));
        let c = EarthLink::generate(3, start, limit, &Params::conjunction());
        let b = c.blackout().expect("a blackout");
        let into = b.start - start;
        assert!(
            (SOL_MS / 100..=3 * SOL_MS / 100).contains(&into),
            "{into} ms into the run"
        );
        // The pass schedule follows the calendar, not the start.
        let late = EarthLink::generate(4, start, limit, &passes());
        let early = EarthLink::generate(4, 0, limit, &passes());
        assert_eq!(late.windows(), early.windows());
    }
}
