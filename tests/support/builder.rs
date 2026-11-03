//! A naive scripted coordinator that builds the landing pad one task at a time,
//! using only what it observes. It exists to prove the default parameters
//! make the pad buildable end to end, and to seed a week-9 baseline.
//!
//! Plan: survey → dig pad (spoil to a stockpile) → dig a borrow pit → rescan →
//! haul fill into low cells → grade → compact → sinter → build the berm →
//! verification scan. Charges any robot below 35% before giving it work, and
//! retries a task after an `insufficient_energy` failure.

use arcadia_protocol::*;
use arcadiabench::coordinator::Agent;
use std::collections::{BTreeMap, VecDeque};

const STATION: &str = "st-1";
const STOCKPILE: GridCoord = GridCoord { x: 45, y: 30 };
const BORROW: GridRect = GridRect {
    min: GridCoord { x: 44, y: 20 },
    max: GridCoord { x: 47, y: 23 },
};
const FILL_THRESHOLD_M: f64 = 0.08;
const ASSUMED_DENSITY: f64 = 1550.0;

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
}

pub struct Builder {
    phase: Phase,
    pad: Option<LandingPadSpec>,
    prior: BTreeMap<(u32, u32), f64>,
    battery_capacity: BTreeMap<String, f64>,
    robots: BTreeMap<String, RobotState>,
    scanned: BTreeMap<(u32, u32), Cell>,
    tasks: VecDeque<Task>,
    active: Option<(String, Task)>,
    next_id: u64,
    pub notes: Vec<String>,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            phase: Phase::Start,
            pad: None,
            prior: BTreeMap::new(),
            battery_capacity: BTreeMap::new(),
            robots: BTreeMap::new(),
            scanned: BTreeMap::new(),
            tasks: VecDeque::new(),
            active: None,
            next_id: 0,
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
        Task { robot, action }
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
            } => {
                let task = self.active.as_ref().map(|(_, t)| t.action.clone());
                panic!("{command_id} ({task:?}) rejected: {reason:?} {detail:?}");
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
