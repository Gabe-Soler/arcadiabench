//! Run metrics (semantics §11): tallies kept during the run, and the final
//! [`RunMetrics`] computed from them and from ground truth.

use super::Sim;
use super::link::Down;
use super::time::{SimMs, secs};
use arcadia_protocol::*;

/// One robot's out-of-action clock for `robot_hours_lost`.
#[derive(Debug, Clone, Default)]
pub(crate) struct OutOfAction {
    since: Option<SimMs>,
    total: SimMs,
}

#[derive(Debug, Clone)]
pub(crate) struct Tally {
    pub decisions: u64,
    pub commands: u64,
    pub commands_rejected: u64,
    /// Of `commands_rejected`, those rejected `out_of_coverage`.
    pub out_of_coverage_rejections: u64,
    pub faults: u64,
    pub grid_energy_wh: f64,
    pub min_storage_wh: f64,
    pub deficit_ms: SimMs,
    pub downlink_messages: u64,
    pub approval_requests: u64,
    pub notifications: u64,
    pub status_reports: u64,
    out_of_action: Vec<OutOfAction>,
}

impl Tally {
    pub fn new(robots: usize, initial_storage_wh: f64) -> Self {
        Self {
            decisions: 0,
            commands: 0,
            commands_rejected: 0,
            out_of_coverage_rejections: 0,
            faults: 0,
            grid_energy_wh: 0.0,
            min_storage_wh: initial_storage_wh,
            deficit_ms: 0,
            downlink_messages: 0,
            approval_requests: 0,
            notifications: 0,
            status_reports: 0,
            out_of_action: vec![OutOfAction::default(); robots],
        }
    }

    /// Count a message the coordinator sent to Earth.
    pub fn downlink(&mut self, m: &SiteToEarth) {
        self.downlink_messages += 1;
        match &m.body {
            Downlink::Notify { .. } => self.notifications += 1,
            Downlink::StatusReport { .. } => self.status_reports += 1,
            Downlink::ApprovalRequest { .. } => self.approval_requests += 1,
        }
    }

    fn mark(&mut self, r: usize, now: SimMs, out: bool) {
        let clock = &mut self.out_of_action[r];
        match (out, clock.since) {
            (true, None) => clock.since = Some(now),
            (false, Some(since)) => {
                clock.total += now - since;
                clock.since = None;
            }
            _ => {}
        }
    }

    fn hours_lost(&self, now: SimMs) -> f64 {
        self.out_of_action
            .iter()
            .map(|c| secs(c.total + c.since.map_or(0, |s| now - s)) / 3600.0)
            .fold(0.0, |a, b| a + b)
    }
}

impl Sim {
    /// Start or stop each robot's out-of-action clock: failed, stuck, being
    /// towed, offline, lost, or waiting for or receiving service.
    pub(crate) fn refresh_out_of_action(&mut self) {
        for r in 0..self.robots.len() {
            let robot = &self.robots[r];
            let out = matches!(
                robot.reported_status(),
                RobotStatus::Failed
                    | RobotStatus::Stuck
                    | RobotStatus::BeingTowed
                    | RobotStatus::Offline
                    | RobotStatus::Lost
            ) || robot.in_service();
            self.tally.mark(r, self.now, out);
        }
    }

    /// The run's metrics at its end. Settles power and drains first, so call
    /// it once, after the run has ended.
    pub fn metrics(&mut self, reason: EndReason) -> RunMetrics {
        self.settle_power();
        for r in 0..self.robots.len() {
            self.settle_drain(r);
        }
        self.refresh_out_of_action();
        let now = self.now;
        let t = &self.tally;
        // Sums start from +0.0: an empty f64 `sum()` is -0.0, which would reach the wire.
        let robot_energy_wh = self
            .robots
            .iter()
            .map(|r| r.spec.battery_capacity_wh + r.received_wh - r.battery_wh)
            .fold(0.0, |a, b| a + b)
            .max(0.0);
        let capacity = self.grid.bank_capacity_wh;
        RunMetrics {
            completed: reason == EndReason::GoalsComplete,
            sols: secs(now - self.start) / secs(super::time::SOL_MS),
            decisions: t.decisions,
            commands: t.commands,
            commands_rejected: t.commands_rejected,
            quality: self
                .mission
                .as_ref()
                .map(|m| m.quality(&self.terrain))
                .unwrap_or_default(),
            energy: EnergyMetrics {
                robot_energy_wh,
                grid_energy_wh: t.grid_energy_wh,
                min_storage_wh: t.min_storage_wh.max(0.0),
                min_storage_fraction: if capacity > 0.0 {
                    (t.min_storage_wh / capacity).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                deficit_s: secs(t.deficit_ms),
            },
            fleet: FleetMetrics {
                faults: t.faults,
                robot_hours_lost: t.hours_lost(now),
                robots_lost: self.robots.iter().filter(|r| r.lost).count() as u64,
            },
            autonomy: AutonomyMetrics {
                downlink_messages: t.downlink_messages,
                approval_requests: t.approval_requests,
                notifications: t.notifications,
                status_reports: t.status_reports,
                approval_wait_s: secs(self.authority.approval_wait_ms(now)),
                policy_violations: self.authority.violations(),
            },
            conditions: ConditionMetrics {
                storm_s: secs(self.env.storm_ms(self.start, now)),
                blackout_s: secs(self.link.down_ms(self.start, now, Down::Conjunction)),
                outage_s: secs(self.link.down_ms(self.start, now, Down::Outage)),
                // No scenario has a pass schedule yet.
                between_passes_s: Some(0.0),
            },
            coverage: Some(CoverageMetrics {
                out_of_coverage_rejections: t.out_of_coverage_rejections,
                lost_link_returns: self.lost_link_returns,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(body: Downlink) -> SiteToEarth {
        SiteToEarth {
            message_id: "m".into(),
            body,
        }
    }

    #[test]
    fn downlinks_are_counted_by_type() {
        let mut t = Tally::new(1, 0.0);
        t.downlink(&message(Downlink::Notify {
            action: "replan".into(),
            summary: String::new(),
            commands: None,
        }));
        for _ in 0..2 {
            t.downlink(&message(Downlink::ApprovalRequest {
                action: "cannibalize".into(),
                summary: String::new(),
                decide_by_t: Some(600.0),
                fallback: None,
            }));
        }
        t.downlink(&message(Downlink::StatusReport {
            summary: String::new(),
            progress: None,
        }));
        assert_eq!(
            (
                t.downlink_messages,
                t.notifications,
                t.approval_requests,
                t.status_reports
            ),
            (4, 1, 2, 1)
        );
    }

    #[test]
    fn out_of_action_time_adds_up_including_open_intervals() {
        let mut t = Tally::new(2, 0.0);
        t.mark(0, 1_000, true);
        t.mark(0, 4_601_000, false);
        t.mark(1, 0, true);
        assert!((t.hours_lost(3_600_000) - (4_600.0 + 3_600.0) / 3600.0).abs() < 1e-12);
        t.mark(0, 5_000_000, false);
        assert!((t.hours_lost(3_600_000) - (4_600.0 + 3_600.0) / 3600.0).abs() < 1e-12);
    }
}
