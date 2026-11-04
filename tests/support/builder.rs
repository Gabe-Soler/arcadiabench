//! A naive scripted coordinator that builds the landing pad one task at a time,
//! using only what it observes. It exists to prove the default parameters
//! make the pad buildable end to end, and to seed a week-9 baseline.
//!
//! Plan: survey → dig pad (spoil to a stockpile) → dig a borrow pit → rescan →
//! haul fill into low cells → grade → compact → sinter → build the berm →
//! verification scan. Charges any robot below 35% before giving it work, and
//! retries a task after an `insufficient_energy` failure.
//!
//! Faults get simple handling before any other work: the repair-capable
//! scout repairs field-repairable faults and frees stuck robots; robots with
//! faults that need the depot are towed there and serviced; offline robots
//! are recharged by the mobile charger, or towed to the station when that
//! does not work. A task cut short by a fault is retried afterwards.

use arcadia_protocol::*;
use arcadiabench::coordinator::Agent;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const STATION: &str = "st-1";
const DEPOT: &str = "dp-1";
const REPAIRER: &str = "sc-1";
const CHARGER: &str = "hl-1";
/// Robots that can tow (they have `haul`), in order of preference.
const TOWERS: [&str; 2] = ["hl-1", "ex-1"];
/// Order in which disabled robots are rescued: helpers first.
const RESCUE_ORDER: [&str; 4] = ["sc-1", "hl-1", "ex-1", "cp-1"];
const STOCKPILE: GridCoord = GridCoord { x: 45, y: 30 };
const BORROW: GridRect = GridRect {
    min: GridCoord { x: 44, y: 20 },
    max: GridCoord { x: 47, y: 23 },
};
const FILL_THRESHOLD_M: f64 = 0.08;
const ASSUMED_DENSITY: f64 = 1550.0;
/// After a rejection, wait this long before trying again, s.
const BACKOFF_S: f64 = 600.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    Survey,
    Dig,
    Rescan,
    Fill,
    Finish,
    Berm,
    Verify,
    Done,
}

#[derive(Debug, Clone)]
struct Task {
    robot: &'static str,
    action: Action,
    /// A rescue: not retried if it fails, because the next one is worked out afresh.
    rescue: bool,
}

pub struct Builder {
    phase: Phase,
    pad: Option<LandingPadSpec>,
    prior: BTreeMap<(u32, u32), f64>,
    battery_capacity: BTreeMap<String, f64>,
    station: GridCoord,
    depot: GridCoord,
    robots: BTreeMap<String, RobotState>,
    scanned: BTreeMap<(u32, u32), Cell>,
    tasks: VecDeque<Task>,
    active: Option<(String, Task)>,
    next_id: u64,
    /// Robots the mobile charger already failed to revive.
    charger_tried: BTreeSet<String>,
    backoff_until: f64,
    pub notes: Vec<String>,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            phase: Phase::Start,
            pad: None,
            prior: BTreeMap::new(),
            battery_capacity: BTreeMap::new(),
            station: GridCoord { x: 0, y: 0 },
            depot: GridCoord { x: 0, y: 0 },
            robots: BTreeMap::new(),
            scanned: BTreeMap::new(),
            tasks: VecDeque::new(),
            active: None,
            next_id: 0,
            charger_tried: BTreeSet::new(),
            backoff_until: 0.0,
            notes: Vec::new(),
        }
    }

    fn rows(area: &GridRect) -> Vec<GridRect> {
        (area.min.y..=area.max.y)
            .map(|y| GridRect {
                min: GridCoord { x: area.min.x, y },
                max: GridCoord { x: area.max.x, y },
            })
            .collect()
    }

    fn task(robot: &'static str, action: Action) -> Task {
        Task {
            robot,
            action,
            rescue: false,
        }
    }

    fn rescue(robot: &'static str, action: Action) -> Task {
        Task {
            robot,
            action,
            rescue: true,
        }
    }

    fn hauls(&mut self, to: GridCoord, kg: f64) {
        let mut left = kg;
        while left > 1.0 {
            let chunk = left.min(400.0);
            self.tasks.push_back(Self::task(
                "hl-1",
                Action::Haul {
                    from: STOCKPILE,
                    to,
                    mass_kg: chunk,
                },
            ));
            left -= chunk;
        }
    }

    /// Queue the next phase's tasks once the current phase is drained.
    fn advance_phase(&mut self) {
        let Some(pad) = self.pad.clone() else { return };
        let target = pad.target_elevation_m;
        self.phase = match self.phase {
            Phase::Start => {
                self.tasks
                    .push_back(Self::task("sc-1", Action::Scan { area: pad.area }));
                Phase::Survey
            }
            Phase::Survey => {
                for row in Self::rows(&pad.area) {
                    self.tasks.push_back(Self::task(
                        "ex-1",
                        Action::Dig {
                            area: row,
                            target_elevation_m: target,
                            spoil_to: Some(STOCKPILE),
                        },
                    ));
                }
                let lowest = (BORROW.min.y..=BORROW.max.y)
                    .flat_map(|y| (BORROW.min.x..=BORROW.max.x).map(move |x| (x, y)))
                    .map(|k| self.prior[&k])
                    .fold(f64::INFINITY, f64::min);
                for row in Self::rows(&BORROW) {
                    self.tasks.push_back(Self::task(
                        "ex-1",
                        Action::Dig {
                            area: row,
                            target_elevation_m: lowest - 0.6,
                            spoil_to: Some(STOCKPILE),
                        },
                    ));
                }
                Phase::Dig
            }
            Phase::Dig => {
                self.tasks
                    .push_back(Self::task("sc-1", Action::Scan { area: pad.area }));
                Phase::Rescan
            }
            Phase::Rescan => {
                let low: Vec<(GridCoord, f64)> = self
                    .scanned
                    .iter()
                    .filter(|((x, y), _)| {
                        (pad.area.min.x..=pad.area.max.x).contains(x)
                            && (pad.area.min.y..=pad.area.max.y).contains(y)
                    })
                    .filter_map(|(&(x, y), cell)| {
                        let elev = cell.elevation_m?;
                        let density = cell
                            .regolith
                            .as_ref()
                            .and_then(|r| r.bulk_density_kg_m3)
                            .unwrap_or(ASSUMED_DENSITY);
                        (elev < target - FILL_THRESHOLD_M)
                            .then_some((GridCoord { x, y }, (target - elev) * density))
                    })
                    .collect();
                self.notes.push(format!("filling {} low cells", low.len()));
                for (c, kg) in low {
                    self.hauls(c, kg);
                }
                Phase::Fill
            }
            Phase::Fill => {
                for row in Self::rows(&pad.area) {
                    self.tasks.push_back(Self::task(
                        "ex-1",
                        Action::Grade {
                            area: row,
                            target_elevation_m: target,
                        },
                    ));
                }
                for row in Self::rows(&pad.area) {
                    self.tasks
                        .push_back(Self::task("cp-1", Action::Compact { area: row }));
                }
                for row in Self::rows(&pad.area) {
                    self.tasks
                        .push_back(Self::task("cp-1", Action::Sinter { area: row }));
                }
                Phase::Finish
            }
            Phase::Finish => {
                if let Some(berm) = &pad.berm {
                    // Enough for 0.5 m even at the densest regolith, plus margin.
                    let kg = berm.height_m * 1700.0 * 1.1;
                    for &c in &berm.cells.clone() {
                        self.hauls(c, kg);
                    }
                }
                Phase::Berm
            }
            Phase::Berm => {
                self.tasks
                    .push_back(Self::task("sc-1", Action::Scan { area: pad.area }));
                Phase::Verify
            }
            Phase::Verify | Phase::Done => Phase::Done,
        };
    }

    fn battery_low(&self, robot: &str) -> bool {
        let cap = self.battery_capacity[robot];
        self.robots
            .get(robot)
            .is_some_and(|s| s.battery_wh < 0.35 * cap)
    }

    fn status(&self, robot: &str) -> Option<RobotStatus> {
        self.robots.get(robot).map(|s| s.status)
    }

    /// Free to take a command: idle and not running one.
    fn available(&self, robot: &str) -> bool {
        self.robots
            .get(robot)
            .is_some_and(|s| s.status == RobotStatus::Idle && s.current_command.is_none())
    }

    fn near(a: GridCoord, b: GridCoord) -> bool {
        let (dx, dy) = (
            f64::from(a.x) - f64::from(b.x),
            f64::from(a.y) - f64::from(b.y),
        );
        (dx * dx + dy * dy).sqrt() <= 1.5
    }

    fn tower_for(&self, robot: &str) -> Option<&'static str> {
        TOWERS
            .into_iter()
            .find(|&t| t != robot && self.available(t))
    }

    /// The next step of rescuing a disabled robot, helpers first.
    fn rescue_task(&mut self) -> Option<Task> {
        for robot in RESCUE_ORDER {
            let Some(state) = self.robots.get(robot).cloned() else {
                continue;
            };
            let pos = state.position;
            let task = match state.status {
                RobotStatus::Offline => {
                    let charger = self.robots.get(CHARGER).map(|c| c.position);
                    if robot != CHARGER
                        && self.available(CHARGER)
                        && !self.charger_tried.contains(robot)
                    {
                        let at = charger.expect("charger state");
                        if Self::near(at, pos) {
                            self.charger_tried.insert(robot.into());
                            Self::rescue(
                                robot,
                                Action::Charge {
                                    source: CHARGER.into(),
                                    until_wh: None,
                                },
                            )
                        } else {
                            Self::rescue(CHARGER, Action::MoveTo { target: pos })
                        }
                    } else if pos == self.station {
                        Self::rescue(
                            robot,
                            Action::Charge {
                                source: STATION.into(),
                                until_wh: None,
                            },
                        )
                    } else if let Some(tower) = self.tower_for(robot) {
                        Self::rescue(
                            tower,
                            Action::Tow {
                                target_robot: robot.into(),
                                to: self.station,
                            },
                        )
                    } else {
                        continue;
                    }
                }
                RobotStatus::Stuck => {
                    if robot != REPAIRER && self.available(REPAIRER) {
                        Self::rescue(
                            REPAIRER,
                            Action::Repair {
                                target_robot: robot.into(),
                            },
                        )
                    } else if let Some(tower) = self.tower_for(robot) {
                        Self::rescue(
                            tower,
                            Action::Tow {
                                target_robot: robot.into(),
                                to: self.depot,
                            },
                        )
                    } else {
                        continue;
                    }
                }
                RobotStatus::Failed => {
                    let repairable = state
                        .faults
                        .iter()
                        .filter(|f| f.severity == Severity::Disabled)
                        .all(|f| f.field_repairable);
                    if repairable && robot != REPAIRER && self.available(REPAIRER) {
                        Self::rescue(
                            REPAIRER,
                            Action::Repair {
                                target_robot: robot.into(),
                            },
                        )
                    } else if pos == self.depot {
                        Self::rescue(
                            robot,
                            Action::Service {
                                depot: DEPOT.into(),
                            },
                        )
                    } else if let Some(tower) = self.tower_for(robot) {
                        Self::rescue(
                            tower,
                            Action::Tow {
                                target_robot: robot.into(),
                                to: self.depot,
                            },
                        )
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };
            self.notes.push(format!(
                "rescue {robot} ({:?}): {:?}",
                state.status, task.action
            ));
            return Some(task);
        }
        None
    }

    fn issue(&mut self, task: Task) -> Command {
        self.next_id += 1;
        let id = format!("b-{:05}", self.next_id);
        self.active = Some((id.clone(), task.clone()));
        Command {
            command_id: id,
            robot_id: task.robot.into(),
            approval_id: None,
            action: task.action,
        }
    }

    fn handle(&mut self, event: &Event) {
        let active_id = self.active.as_ref().map(|(id, _)| id.clone());
        match &event.kind {
            EventKind::EarthMessage { message, .. } => {
                if let Uplink::Goal {
                    objective: Objective::LandingPad { pad },
                    ..
                } = &message.body
                {
                    self.pad = Some(pad.clone());
                }
            }
            EventKind::ScanResult { cells, .. } => {
                for c in cells {
                    self.scanned.insert((c.at.x, c.at.y), c.cell.clone());
                }
            }
            EventKind::CommandCompleted { command_id, .. }
                if Some(command_id) == active_id.as_ref() =>
            {
                self.active = None;
            }
            EventKind::CommandFailed {
                command_id,
                reason,
                detail,
                ..
            } if Some(command_id) == active_id.as_ref() => {
                let (_, task) = self.active.take().unwrap();
                match reason {
                    _ if task.rescue => {
                        self.notes
                            .push(format!("rescue {command_id} failed: {reason:?} {detail:?}"));
                    }
                    FailReason::InsufficientEnergy => {
                        self.notes
                            .push(format!("{} ran out of energy; recharging", task.robot));
                        let robot = task.robot;
                        self.tasks.push_front(task);
                        self.tasks.push_front(Self::task(
                            robot,
                            Action::Charge {
                                source: STATION.into(),
                                until_wh: None,
                            },
                        ));
                    }
                    FailReason::Fault
                    | FailReason::Stuck
                    | FailReason::Blocked
                    | FailReason::TargetLost => {
                        self.notes.push(format!(
                            "{command_id} ({}) failed: {reason:?} {detail:?}; will retry",
                            task.robot
                        ));
                        self.tasks.push_front(task);
                    }
                    other => panic!(
                        "{command_id} ({:?}) failed: {other:?} {detail:?}",
                        task.action
                    ),
                }
            }
            EventKind::CommandRejected {
                command_id,
                reason,
                detail,
                ..
            } if Some(command_id) == active_id.as_ref() => {
                let (_, task) = self.active.take().unwrap();
                match reason {
                    // The world changed under us (a fault at the same instant, a
                    // target that moved on): back off and try again later.
                    _ if task.rescue => {}
                    RejectReason::RobotDisabled | RejectReason::Busy => {
                        self.tasks.push_front(task);
                    }
                    other => panic!(
                        "{command_id} ({:?}) rejected: {other:?} {detail:?}",
                        task.action
                    ),
                }
                self.notes.push(format!(
                    "{command_id} rejected: {reason:?} {detail:?}; backing off"
                ));
                self.backoff_until = event.t + BACKOFF_S;
            }
            _ => {}
        }
    }
}

impl Agent for Builder {
    fn welcome(&mut self, hello: &Hello) -> SoftwareInfo {
        for c in &hello.scenario.site.cells {
            if let Some(e) = c.cell.elevation_m {
                self.prior.insert((c.at.x, c.at.y), e);
            }
        }
        for r in &hello.scenario.robots {
            self.battery_capacity
                .insert(r.robot_id.clone(), r.battery_capacity_wh);
        }
        self.station = hello.scenario.stations[0].position;
        self.depot = hello.scenario.depots[0].position;
        SoftwareInfo {
            name: "scripted-builder".into(),
            version: "0".into(),
        }
    }

    fn decide(&mut self, obs: &Observation) -> Decision {
        for r in &obs.robots {
            self.robots.insert(r.robot_id.clone(), r.clone());
        }
        for e in &obs.events {
            self.handle(e);
        }
        let mut decision = Decision::empty(obs.seq);
        if self.active.is_some() {
            return decision;
        }
        if obs.t < self.backoff_until {
            decision.wake_at = Some(self.backoff_until);
            return decision;
        }
        // A revived robot can be tried with the charger again later.
        let revived: Vec<String> = self
            .charger_tried
            .iter()
            .filter(|r| self.status(r) != Some(RobotStatus::Offline))
            .cloned()
            .collect();
        for r in revived {
            self.charger_tried.remove(&r);
        }
        if let Some(task) = self.rescue_task() {
            decision.commands.push(self.issue(task));
            return decision;
        }
        while self.tasks.is_empty() && self.phase != Phase::Done && self.pad.is_some() {
            self.advance_phase();
        }
        if let Some(task) = self.tasks.pop_front() {
            let is_charge = matches!(task.action, Action::Charge { .. });
            let task = if !is_charge && self.battery_low(task.robot) {
                let robot = task.robot;
                self.tasks.push_front(task);
                Self::task(
                    robot,
                    Action::Charge {
                        source: STATION.into(),
                        until_wh: None,
                    },
                )
            } else {
                task
            };
            decision.commands.push(self.issue(task));
        }
        decision
    }
}
