//! Runtime state of one robot.

use super::faults::{Exposure, Health};
use super::time::SimMs;
use arcadia_protocol::{
    CapabilityName, CapabilitySpec, Command, GridCoord, GridRect, RobotSpec, RobotState,
    RobotStatus,
};
use std::collections::VecDeque;

/// The wire name of a status, for rejection details.
pub fn status_name(s: RobotStatus) -> &'static str {
    match s {
        RobotStatus::Idle => "idle",
        RobotStatus::Moving => "moving",
        RobotStatus::Working => "working",
        RobotStatus::Charging => "charging",
        RobotStatus::Stuck => "stuck",
        RobotStatus::Failed => "failed",
        RobotStatus::BeingTowed => "being_towed",
        RobotStatus::Offline => "offline",
        RobotStatus::Lost => "lost",
        // For a later minor version's status. The simulator never sets it.
        RobotStatus::Unknown => "unknown",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Travel(GridCoord),
    /// Drive to wherever robot `usize` is when the step starts.
    TravelToRobot(usize),
    Dig {
        cell: GridCoord,
        target: f64,
        spoil_to: Option<GridCoord>,
    },
    Grade {
        cell: GridCoord,
        target: f64,
    },
    Compact(GridCoord),
    Sinter(GridCoord),
    Scan(GridRect),
    Load {
        at: GridCoord,
        kg: f64,
    },
    Dump(GridCoord),
    Wait(SimMs),
    /// Hold position indefinitely (relay) until cancelled.
    Hold,
    ChargeStation {
        station: usize,
        until_wh: Option<f64>,
    },
    ChargeMobile {
        charger: usize,
        until_wh: Option<f64>,
    },
    /// Field repair of robot `target`.
    Repair {
        target: usize,
    },
    /// Tow robot `target` to `to`.
    Tow {
        target: usize,
        to: GridCoord,
    },
    /// Strip robot `target` for parts.
    Strip {
        target: usize,
    },
    /// Install cannibalized parts in robot `beneficiary`.
    Install {
        beneficiary: usize,
    },
    /// Wait for, then receive, service at depot `depot`.
    Service {
        depot: usize,
    },
}

#[derive(Debug, Clone)]
pub struct Running {
    pub step: Step,
    pub start: SimMs,
    /// `None` for open-ended steps (hold, charging, queueing).
    pub end: Option<SimMs>,
    pub energy_wh: f64,
    /// Step-specific quantity fixed at start (e.g. kg to load).
    pub amount: f64,
    /// Wear exposure of the whole step, applied when it ends (pro rata if cut short).
    pub exposure: Exposure,
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub command: Command,
    pub steps: VecDeque<Step>,
    pub running: Option<Running>,
    pub energy_used_wh: f64,
    /// The simulator's own activity (a lost-link return), not a coordinator
    /// command: it has no `current_command` and reports no events.
    pub internal: bool,
}

#[derive(Debug, Clone)]
pub struct Travel {
    pub from: GridCoord,
    pub to: GridCoord,
    pub start: SimMs,
    pub end: SimMs,
}

impl Travel {
    /// The cell at time `t`: the straight line interpolated by time, rounded.
    pub fn position_at(&self, t: SimMs) -> GridCoord {
        let span = self.end.saturating_sub(self.start).max(1) as f64;
        let f = (t.saturating_sub(self.start) as f64 / span).clamp(0.0, 1.0);
        let lerp =
            |a: u32, b: u32| (f64::from(a) + (f64::from(b) - f64::from(a)) * f).round() as u32;
        GridCoord {
            x: lerp(self.from.x, self.to.x),
            y: lerp(self.from.y, self.to.y),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RobotRt {
    pub spec: RobotSpec,
    pub pos: GridCoord,
    pub travel: Option<Travel>,
    pub battery_wh: f64,
    pub payload_kg: f64,
    /// Energy stored for charging other robots (charge capability only).
    pub tank_wh: f64,
    /// What the robot is doing: idle, moving, working or charging. The
    /// reported status also reflects its condition; see [`RobotRt::reported_status`].
    pub status: RobotStatus,
    pub activity: Option<Activity>,
    /// Robot whose command engages this one: it is drawing from this robot as
    /// a mobile charger, or repairing, towing, stripping or fitting it.
    pub engaged_by: Option<usize>,
    /// Bumped whenever the robot's scheduled event changes; stale events are ignored.
    pub generation: u64,
    /// Bumped whenever the robot's exposure to failures changes; stale hazard events are ignored.
    pub hazard_generation: u64,
    /// Idle power is drained lazily from this time while not in a timed step.
    pub drain_since: Option<SimMs>,
    /// Hidden wear, active faults, and whether it is stuck.
    pub health: Health,
    /// The battery is empty.
    pub offline: bool,
    /// Cannibalized: permanently gone.
    pub lost: bool,
    /// Being moved by another robot's tow.
    pub towed: bool,
    /// Energy received by the battery from charging, Wh.
    pub received_wh: f64,
    /// Snapshot taken when the robot was stripped; lost robots report it forever.
    pub frozen: Option<RobotState>,
    /// Radio power drawn on top of idle drain while holding as an active relay, W.
    pub radio_w: f64,
}

impl RobotRt {
    /// A robot at `pos` with a full battery when the run starts, at `start`.
    pub fn new(spec: RobotSpec, pos: GridCoord, start: SimMs) -> Self {
        let tank_wh = spec
            .capability(CapabilityName::Charge)
            .and_then(|c| c.capacity)
            .unwrap_or(0.0);
        Self {
            battery_wh: spec.battery_capacity_wh,
            spec,
            pos,
            travel: None,
            payload_kg: 0.0,
            tank_wh,
            status: RobotStatus::Idle,
            activity: None,
            engaged_by: None,
            generation: 0,
            hazard_generation: 0,
            drain_since: Some(start),
            health: Health::default(),
            offline: false,
            lost: false,
            towed: false,
            received_wh: 0.0,
            frozen: None,
            radio_w: 0.0,
        }
    }

    pub fn capability(&self, name: CapabilityName) -> Option<&CapabilitySpec> {
        self.spec.capability(name)
    }

    pub fn tank_capacity_wh(&self) -> f64 {
        self.capability(CapabilityName::Charge)
            .and_then(|c| c.capacity)
            .unwrap_or(0.0)
    }

    pub fn haul_capacity_kg(&self) -> f64 {
        self.capability(CapabilityName::Haul)
            .and_then(|c| c.capacity)
            .unwrap_or(0.0)
    }

    pub fn busy(&self) -> bool {
        self.activity.is_some() || self.engaged_by.is_some()
    }

    /// The coordinator command the robot is running, if any.
    pub fn running_command(&self) -> Option<&str> {
        self.activity
            .as_ref()
            .filter(|a| !a.internal)
            .map(|a| a.command.command_id.as_str())
    }

    pub fn running_step(&self) -> Option<&Step> {
        self.activity
            .as_ref()
            .and_then(|a| a.running.as_ref())
            .map(|r| &r.step)
    }

    /// Waiting for or receiving depot service.
    pub fn in_service(&self) -> bool {
        matches!(self.running_step(), Some(Step::Service { .. }))
    }

    /// Status by precedence (semantics §3.2): lost, being towed, stuck,
    /// offline, failed, then what it is doing.
    pub fn reported_status(&self) -> RobotStatus {
        if self.lost {
            RobotStatus::Lost
        } else if self.towed {
            RobotStatus::BeingTowed
        } else if self.health.stuck {
            RobotStatus::Stuck
        } else if self.offline {
            RobotStatus::Offline
        } else if self.health.has_disabled() {
            RobotStatus::Failed
        } else {
            self.status
        }
    }

    /// Idle, moving, working or charging, by its reported status.
    pub fn commandable(&self) -> bool {
        matches!(
            self.reported_status(),
            RobotStatus::Idle | RobotStatus::Moving | RobotStatus::Working | RobotStatus::Charging
        )
    }

    /// Position at time `t`, interpolated along the current drive.
    pub fn position_at(&self, t: SimMs) -> GridCoord {
        self.travel
            .as_ref()
            .map_or(self.pos, |tr| tr.position_at(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::faults::{Comp, Fault};
    use arcadia_protocol::{FaultCause, Severity};

    fn robot() -> RobotRt {
        RobotRt::new(
            RobotSpec {
                robot_id: "r".into(),
                model: None,
                mass_kg: 100.0,
                battery_capacity_wh: 1000.0,
                max_speed_m_s: 0.5,
                drive_energy_wh_per_m: 0.5,
                idle_power_w: 10.0,
                capabilities: vec![],
            },
            GridCoord { x: 0, y: 0 },
            0,
        )
    }

    #[test]
    fn status_precedence() {
        let mut r = robot();
        r.status = RobotStatus::Working;
        assert_eq!(r.reported_status(), RobotStatus::Working);
        assert!(r.commandable());
        r.health.add_fault(
            Comp::Tool,
            Fault {
                severity: Severity::Disabled,
                cause: FaultCause::Wear,
                since: 0,
                field_repairable: true,
            },
        );
        assert_eq!(r.reported_status(), RobotStatus::Failed);
        r.offline = true;
        assert_eq!(r.reported_status(), RobotStatus::Offline);
        r.health.stuck = true;
        assert_eq!(r.reported_status(), RobotStatus::Stuck);
        r.towed = true;
        assert_eq!(r.reported_status(), RobotStatus::BeingTowed);
        r.lost = true;
        assert_eq!(r.reported_status(), RobotStatus::Lost);
        assert!(!r.commandable());
    }
}
