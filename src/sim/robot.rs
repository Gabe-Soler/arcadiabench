//! Runtime state of one robot.

use super::time::{SimMs, secs};
use arcadia_protocol::{
    CapabilityName, CapabilitySpec, Command, GridCoord, GridRect, RobotSpec, RobotStatus,
};
use std::collections::VecDeque;

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
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub command: Command,
    pub steps: VecDeque<Step>,
    pub running: Option<Running>,
    pub energy_used_wh: f64,
}

#[derive(Debug, Clone)]
pub struct Travel {
    pub from: GridCoord,
    pub to: GridCoord,
    pub start: SimMs,
    pub end: SimMs,
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
    pub status: RobotStatus,
    pub activity: Option<Activity>,
    /// Robot currently drawing from this one as a mobile charger.
    pub locked_by: Option<usize>,
    /// Bumped whenever the robot's scheduled event changes; stale events are ignored.
    pub generation: u64,
    /// Idle power is drained lazily from this time while not in a timed step.
    pub drain_since: Option<SimMs>,
}

impl RobotRt {
    pub fn new(spec: RobotSpec, pos: GridCoord) -> Self {
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
            locked_by: None,
            generation: 0,
            drain_since: Some(0),
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
        self.activity.is_some() || self.locked_by.is_some()
    }

    pub fn running_command(&self) -> Option<&str> {
        self.activity
            .as_ref()
            .map(|a| a.command.command_id.as_str())
    }

    /// Position at time `t`, interpolated along the current drive.
    pub fn position_at(&self, t: SimMs) -> GridCoord {
        let Some(tr) = &self.travel else {
            return self.pos;
        };
        let span = tr.end.saturating_sub(tr.start).max(1) as f64;
        let f = (t.saturating_sub(tr.start) as f64 / span).clamp(0.0, 1.0);
        let lerp =
            |a: u32, b: u32| (f64::from(a) + (f64::from(b) - f64::from(a)) * f).round() as u32;
        GridCoord {
            x: lerp(tr.from.x, tr.to.x),
            y: lerp(tr.from.y, tr.to.y),
        }
    }

    /// Apply idle drain up to `now`.
    pub fn settle_drain(&mut self, now: SimMs) {
        if let Some(since) = self.drain_since {
            let used = self.spec.idle_power_w * secs(now.saturating_sub(since)) / 3600.0;
            self.battery_wh = (self.battery_wh - used).max(0.0);
            self.drain_since = Some(now);
        }
    }

    /// Time until idle drain empties the battery, if draining.
    pub fn time_to_empty_s(&self) -> Option<f64> {
        if self.drain_since.is_none() || self.spec.idle_power_w <= 0.0 {
            return None;
        }
        Some(self.battery_wh / self.spec.idle_power_w * 3600.0)
    }
}
