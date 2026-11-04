//! Site coverage (semantics §8.4–§8.5): which robots can hear the base
//! station, directly or through active relays; what robots out of coverage
//! do (hold their events back, show a last-contact snapshot); and the
//! lost-link return of idle robots stranded out of coverage.
//!
//! Coverage depends only on where robots are and which relays are holding,
//! so the simulator re-derives it after every event and after every command.
//! A moving robot's coverage can also change in the middle of a drive: for
//! each moving robot, the moment its cell next crosses the edge of coverage
//! is queued as a check.

use super::robot::{Activity, Step, Travel};
use super::time::{SimMs, ms, secs};
use super::{Ev, Noise, Sim};
use arcadia_protocol::*;
use std::collections::VecDeque;

/// Whether two cells are within `range_m` of each other, measured between
/// cell centres; the boundary counts.
pub fn within(a: GridCoord, b: GridCoord, range_m: f64, cell_m: f64) -> bool {
    let dx = f64::from(a.x) - f64::from(b.x);
    let dy = f64::from(a.y) - f64::from(b.y);
    (dx * dx + dy * dy) * cell_m * cell_m <= range_m * range_m
}

/// Which robots are in coverage: within the base station's range, or within
/// the range of an active relay that is itself in coverage, transitively.
/// `relays[i]` is robot `i`'s relay range if it is an active relay.
pub fn coverage(
    base: &BaseStation,
    cell_m: f64,
    positions: &[GridCoord],
    relays: &[Option<f64>],
) -> Vec<bool> {
    let mut covered: Vec<bool> = positions
        .iter()
        .map(|&p| within(p, base.position, base.range_m, cell_m))
        .collect();
    let mut frontier: Vec<usize> = (0..positions.len())
        .filter(|&i| covered[i] && relays[i].is_some())
        .collect();
    while let Some(j) = frontier.pop() {
        let range = relays[j].unwrap_or(0.0);
        for i in 0..positions.len() {
            if !covered[i] && within(positions[i], positions[j], range, cell_m) {
                covered[i] = true;
                if relays[i].is_some() {
                    frontier.push(i);
                }
            }
        }
    }
    covered
}

/// The first time after `after` at which a drive's cell changes. Each
/// coordinate of the interpolated cell moves monotonically, so the cell,
/// once changed, never comes back, and a bisection finds the change.
pub fn next_cell_change(tr: &Travel, after: SimMs) -> Option<SimMs> {
    let here = tr.position_at(after);
    if after >= tr.end || tr.position_at(tr.end) == here {
        return None;
    }
    let (mut lo, mut hi) = (after, tr.end);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if tr.position_at(mid) == here {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(hi)
}

/// What the site knows of one robot's link to it.
#[derive(Debug, Clone)]
pub(crate) struct Contact {
    pub covered: bool,
    /// When the robot last lost contact.
    pub lost_at: SimMs,
    /// The robot as the coordinator last heard it, while out of coverage.
    pub snapshot: Option<RobotState>,
    /// Events held back until the robot is back in coverage.
    pub held: Vec<Event>,
    /// The pending lost-link timer, if any.
    pub timer: Option<SimMs>,
    /// What the queued coverage check was planned for.
    planned: Option<Planned>,
}

impl Default for Contact {
    fn default() -> Self {
        Self {
            covered: true,
            lost_at: 0,
            snapshot: None,
            held: Vec::new(),
            timer: None,
            planned: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Planned {
    travel: Option<(GridCoord, GridCoord, SimMs, SimMs)>,
    region: u64,
    covered: bool,
}

impl Sim {
    /// Robot `r`'s relay range, if it is holding as an active relay.
    fn relay_range(&self, r: usize) -> Option<f64> {
        let robot = &self.robots[r];
        if !matches!(robot.running_step(), Some(Step::Hold)) {
            return None;
        }
        robot
            .capability(CapabilityName::Relay)
            .and_then(|c| c.range_m)
    }

    /// Which robots are in coverage right now.
    pub(crate) fn coverage_now(&self) -> Vec<bool> {
        let positions: Vec<GridCoord> = self
            .robots
            .iter()
            .map(|r| r.position_at(self.now))
            .collect();
        let relays: Vec<Option<f64>> = (0..self.robots.len())
            .map(|r| self.relay_range(r))
            .collect();
        coverage(&self.base, self.terrain.cell_size_m, &positions, &relays)
    }

    /// Whether a cell is covered by the base station or a covered active relay.
    fn in_region(&self, c: GridCoord) -> bool {
        let cell_m = self.terrain.cell_size_m;
        within(c, self.base.position, self.base.range_m, cell_m)
            || self
                .region
                .iter()
                .any(|&(p, range)| within(c, p, range, cell_m))
    }

    /// Re-derive coverage now: robots that lose contact freeze their
    /// snapshot; robots that regain it deliver what they held back (a
    /// decision point) and end any lost-link return; stranded idle robots
    /// start their lost-link return when due; moving robots get their next
    /// coverage check queued.
    pub(crate) fn refresh_coverage(&mut self) {
        let covered = self.coverage_now();
        let region: Vec<(GridCoord, f64)> = (0..self.robots.len())
            .filter(|&r| covered[r])
            .filter_map(|r| self.relay_range(r).map(|range| (self.robots[r].pos, range)))
            .collect();
        if region != self.region {
            self.region = region;
            self.region_version += 1;
        }
        for (r, &now_covered) in covered.iter().enumerate() {
            match (self.contact[r].covered, now_covered) {
                (true, false) => self.lose_contact(r),
                (false, true) => self.regain_contact(r),
                _ => {}
            }
        }
        for r in 0..self.robots.len() {
            self.lost_link_check(r);
        }
        for r in 0..self.robots.len() {
            self.plan_coverage_check(r);
        }
    }

    fn lose_contact(&mut self, r: usize) {
        // The last telemetry the site received, with its own noise draw.
        let noise = Noise {
            battery_wh: self.params.telemetry_battery_noise_frac
                * self.robots[r].spec.battery_capacity_wh
                * self.rng_coverage.normal(),
            health: std::array::from_fn(|_| self.rng_coverage.normal()),
        };
        let mut snapshot = self.live_state(r, Some(&noise));
        snapshot.in_relay_coverage = false;
        snapshot.telemetry_t = secs(self.now);
        let contact = &mut self.contact[r];
        contact.covered = false;
        contact.lost_at = self.now;
        contact.snapshot = Some(snapshot);
        contact.timer = None;
    }

    fn regain_contact(&mut self, r: usize) {
        if self.robots[r].activity.as_ref().is_some_and(|a| a.internal) {
            self.stop_lost_link(r);
        }
        let contact = &mut self.contact[r];
        contact.covered = true;
        contact.snapshot = None;
        contact.timer = None;
        let held = std::mem::take(&mut contact.held);
        self.pending.extend(held);
        self.decision_due = true;
    }

    /// An idle robot with no command, out of coverage for at least
    /// `lost_link_timeout_s`, starts back toward the base station.
    fn lost_link_check(&mut self, r: usize) {
        let robot = &self.robots[r];
        let contact = &self.contact[r];
        let stranded = !contact.covered
            && !robot.lost
            && robot.reported_status() == RobotStatus::Idle
            && robot.activity.is_none()
            && robot.engaged_by.is_none();
        if !stranded {
            return;
        }
        let due = contact
            .lost_at
            .saturating_add(ms(self.params.lost_link_timeout_s));
        if self.now >= due {
            self.start_lost_link(r);
        } else if contact.timer != Some(due) {
            self.contact[r].timer = Some(due);
            self.queue.push(due, Ev::LostLink { robot: r });
        }
    }

    /// A lost-link timer fired; the refresh that follows acts on it.
    pub(crate) fn on_lost_link_timer(&mut self, r: usize) {
        if self.contact[r].timer == Some(self.now) {
            self.contact[r].timer = None;
        }
    }

    /// Drive straight toward the base station, as far as the first covered
    /// cell on the way. The drive is like any other (energy, wear, faults,
    /// getting stuck) but has no command, so it reports nothing.
    fn start_lost_link(&mut self, r: usize) {
        let from = self.robots[r].pos;
        let to = self.base.position;
        let path = Travel {
            from,
            to,
            start: 0,
            end: 1_000_000,
        };
        let mut target = to;
        let mut t = 0;
        while let Some(change) = next_cell_change(&path, t) {
            let cell = path.position_at(change);
            if self.in_region(cell) {
                target = cell;
                break;
            }
            t = change;
        }
        let command = Command {
            command_id: "lost-link".into(),
            robot_id: self.robots[r].spec.robot_id.clone(),
            approval_id: None,
            action: Action::MoveTo { target },
        };
        self.settle_drain(r);
        let robot = &mut self.robots[r];
        robot.drain_since = None;
        robot.activity = Some(Activity {
            command,
            steps: VecDeque::from([Step::Travel(target)]),
            running: None,
            energy_used_wh: 0.0,
            internal: true,
        });
        self.lost_link_returns += 1;
        self.start_next_step(r);
    }

    /// Back in coverage: the lost-link return stops where the robot is.
    fn stop_lost_link(&mut self, r: usize) {
        self.interrupt_step(r);
        let now = self.now;
        let robot = &mut self.robots[r];
        robot.activity = None;
        robot.status = RobotStatus::Idle;
        robot.drain_since = if robot.offline { None } else { Some(now) };
        self.schedule_depletion(r);
    }

    /// Queue a check for when a moving robot's cell next crosses the edge of
    /// coverage, unless one is already queued for this drive and this coverage.
    fn plan_coverage_check(&mut self, r: usize) {
        let travel = self.robots[r].travel.clone();
        let planned = Planned {
            travel: travel.as_ref().map(|t| (t.from, t.to, t.start, t.end)),
            region: self.region_version,
            covered: self.contact[r].covered,
        };
        if self.contact[r].planned == Some(planned) {
            return;
        }
        self.contact[r].planned = Some(planned);
        let Some(tr) = travel else { return };
        let inside = self.in_region(tr.position_at(self.now));
        let mut t = self.now;
        while let Some(change) = next_cell_change(&tr, t) {
            if change > self.time_limit {
                return;
            }
            if self.in_region(tr.position_at(change)) != inside {
                self.queue.push(change, Ev::CoverageCheck);
                return;
            }
            t = change;
        }
    }

    /// Events robots held back and never delivered, in time order.
    pub(crate) fn take_held(&mut self) -> Vec<Event> {
        let mut held: Vec<Event> = self
            .contact
            .iter_mut()
            .flat_map(|c| std::mem::take(&mut c.held))
            .collect();
        held.sort_by(|a, b| a.t.total_cmp(&b.t));
        held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: u32, y: u32) -> GridCoord {
        GridCoord { x, y }
    }

    fn base() -> BaseStation {
        BaseStation {
            position: at(0, 0),
            range_m: 10.0,
        }
    }

    #[test]
    fn range_is_euclidean_between_cell_centres_and_includes_the_edge() {
        assert!(within(at(6, 8), at(0, 0), 10.0, 1.0));
        assert!(!within(at(6, 9), at(0, 0), 10.0, 1.0));
        assert!(within(at(3, 4), at(0, 0), 10.0, 2.0));
        assert!(!within(at(3, 5), at(0, 0), 10.0, 2.0));
    }

    #[test]
    fn relays_extend_coverage_transitively_but_only_from_coverage() {
        // A chain: base -> relay 1 -> relay 2 -> a robot far out.
        let positions = [at(9, 0), at(18, 0), at(27, 0), at(50, 50)];
        let relays = [Some(10.0), Some(10.0), None, Some(100.0)];
        assert_eq!(
            coverage(&base(), 1.0, &positions, &relays),
            [true, true, true, false],
            "a relay out of coverage extends nothing"
        );
        // Break the chain in the middle and everything past it is lost.
        let relays = [Some(10.0), None, None, Some(100.0)];
        assert_eq!(
            coverage(&base(), 1.0, &positions, &relays),
            [true, true, false, false]
        );
        // A relay without a range extends nothing either.
        let relays = [None, Some(10.0), None, None];
        assert_eq!(
            coverage(&base(), 1.0, &positions, &relays),
            [true, false, false, false]
        );
    }

    #[test]
    fn cell_changes_along_a_drive_are_found_exactly() {
        let tr = Travel {
            from: at(0, 0),
            to: at(7, 3),
            start: 1_000,
            end: 15_000,
        };
        let mut t = tr.start;
        let mut cells = vec![tr.position_at(t)];
        while let Some(change) = next_cell_change(&tr, t) {
            assert_ne!(tr.position_at(change), tr.position_at(change - 1));
            assert!(change > t && change <= tr.end);
            cells.push(tr.position_at(change));
            t = change;
        }
        assert_eq!(cells.first(), Some(&at(0, 0)));
        assert_eq!(cells.last(), Some(&at(7, 3)));
        // Each step moves to a neighbouring cell, and every cell is visited once.
        assert!(
            cells
                .windows(2)
                .all(|w| w[0].x.abs_diff(w[1].x) <= 1 && w[0].y.abs_diff(w[1].y) <= 1)
        );
        let mut sorted = cells.clone();
        sorted.sort_by_key(|c| (c.x, c.y));
        sorted.dedup();
        assert_eq!(sorted.len(), cells.len());
        // Brute force agrees.
        let distinct = (tr.start..=tr.end).fold(vec![], |mut v: Vec<GridCoord>, t| {
            let c = tr.position_at(t);
            if v.last() != Some(&c) {
                v.push(c);
            }
            v
        });
        assert_eq!(distinct, cells);
    }
}
