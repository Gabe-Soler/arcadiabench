//! Command acceptance and execution.
//!
//! A command is validated, planned into [`Step`]s (travel, then per-cell work),
//! and executed one step at a time. Energy for a timed step is drawn when it
//! starts; if the battery cannot cover it the command fails with
//! `insufficient_energy`. Area commands work cell by cell, so partial work
//! survives cancellation and failure. A step cut short (cancel, a disabling
//! fault, getting stuck) returns the unused part of its energy.
//!
//! The landing-pad "task graph" lives here as per-cell prerequisites:
//! natural → cleared (dig) → graded → compacted → stabilized (sinter).

use super::faults::Exposure;
use super::hazards::Exposed;
use super::robot::{Activity, Running, Step, Travel, status_name};
use super::terrain::{clamp_into, distance_cells, rect_cell_count, rect_cells, serpentine};
use super::time::{SimMs, ms, secs};
use super::{Ev, Sim};
use arcadia_protocol::*;
use std::collections::VecDeque;
use std::f64::consts::PI;

pub(crate) type Rejection = (RejectReason, String);

pub(crate) enum StepStart {
    /// A timed step is under way.
    Scheduled,
    /// Waiting on something open-ended (charging, queueing, holding).
    Waiting,
    /// Nothing to do; move on.
    Skip,
    Failed(FailReason, String),
}

/// A timed step, worked out before it starts.
pub(crate) struct Timed {
    pub duration_s: f64,
    pub energy_wh: f64,
    /// Step-specific quantity fixed at start (e.g. kg to load).
    pub amount: f64,
    pub status: RobotStatus,
    /// What the step exposes the robot to, if anything.
    pub exposed: Option<Exposed>,
    /// Wear exposure of the whole step.
    pub wear: Exposure,
    /// Another robot the step engages for its duration.
    pub engage: Option<usize>,
}

pub(crate) fn reject<T>(reason: RejectReason, detail: impl Into<String>) -> Result<T, Rejection> {
    Err((reason, detail.into()))
}

fn at(c: GridCoord) -> String {
    format!("({}, {})", c.x, c.y)
}

impl Sim {
    // ----- acceptance -----

    pub(crate) fn submit(&mut self, cmd: &Command) {
        if !self.command_ids.insert(cmd.command_id.clone()) {
            return self.emit_rejected(
                cmd,
                RejectReason::InvalidTarget,
                "duplicate command_id".into(),
            );
        }
        let Some(&r) = self.robot_ix.get(&cmd.robot_id) else {
            return self.emit_rejected(
                cmd,
                RejectReason::UnknownRobot,
                format!("no robot {}", cmd.robot_id),
            );
        };
        if !self.contact[r].covered {
            return self.emit_rejected(
                cmd,
                RejectReason::OutOfCoverage,
                format!("{} is out of coverage", cmd.robot_id),
            );
        }
        let level = self.policy.level_for(cmd.action.name());
        if let Err((reason, detail)) = self.authority.check(cmd, level, self.now) {
            return self.emit_rejected(cmd, reason, detail);
        }
        if let Action::Cancel { target_command } = &cmd.action {
            return self.cancel(r, cmd, target_command, level);
        }
        let status = self.robots[r].reported_status();
        if matches!(
            status,
            RobotStatus::Failed
                | RobotStatus::Stuck
                | RobotStatus::Offline
                | RobotStatus::BeingTowed
                | RobotStatus::Lost
        ) && !self.disabled_exception(r, &cmd.action)
        {
            return self.emit_rejected(
                cmd,
                RejectReason::RobotDisabled,
                format!("{} is {}", cmd.robot_id, status_name(status)),
            );
        }
        if self.robots[r].busy() {
            let detail = match self.robots[r].running_command() {
                Some(c) => format!("running {c}"),
                None => "engaged by another robot".into(),
            };
            return self.emit_rejected(cmd, RejectReason::Busy, detail);
        }
        if let Err((reason, detail)) = self.validate(r, &cmd.action) {
            return self.emit_rejected(cmd, reason, detail);
        }

        let steps = self.plan(r, &cmd.action);
        let estimate = self.estimate_s(r, &steps);
        self.authority.accepted(cmd, level, self.now);
        self.emit(
            EventKind::CommandAccepted {
                command_id: cmd.command_id.clone(),
                robot_id: cmd.robot_id.clone(),
                estimated_duration_s: estimate,
            },
            false,
        );
        self.settle_drain(r);
        let robot = &mut self.robots[r];
        robot.drain_since = None;
        robot.activity = Some(Activity {
            command: cmd.clone(),
            steps,
            running: None,
            energy_used_wh: 0.0,
            internal: false,
        });
        self.start_next_step(r);
    }

    fn emit_rejected(&mut self, cmd: &Command, reason: RejectReason, detail: String) {
        self.tally.commands_rejected += 1;
        self.emit(
            EventKind::CommandRejected {
                command_id: cmd.command_id.clone(),
                robot_id: cmd.robot_id.clone(),
                reason,
                detail: Some(detail),
            },
            true,
        );
    }

    fn require(&self, r: usize, cap: CapabilityName) -> Result<(), Rejection> {
        if self.robots[r].capability(cap).is_some() {
            Ok(())
        } else {
            reject(
                RejectReason::MissingCapability,
                format!("{} cannot {cap:?}", self.robots[r].spec.robot_id).to_lowercase(),
            )
        }
    }

    fn cell_ok(&self, c: GridCoord) -> Result<(), Rejection> {
        if self.terrain.in_bounds(c) {
            Ok(())
        } else {
            reject(
                RejectReason::InvalidTarget,
                format!("{} is off the site", at(c)),
            )
        }
    }

    fn area_ok(&self, a: &GridRect) -> Result<(), Rejection> {
        if self.terrain.rect_in_bounds(a) {
            Ok(())
        } else {
            reject(RejectReason::InvalidTarget, "area is empty or off the site")
        }
    }

    /// Why a cell cannot take this work right now, if it cannot.
    fn prerequisite(&self, step: &Step) -> Option<String> {
        let (c, allowed, what): (GridCoord, &[Surface], &str) = match step {
            Step::Grade { cell, target } => {
                let t = self.terrain.get(*cell);
                if t.surface == Surface::Natural {
                    return Some(format!("{} must be cleared before grading", at(*cell)));
                }
                if !t.boulders.is_empty() {
                    return Some(format!("{} still has boulders", at(*cell)));
                }
                let delta = t.elevation_m - target;
                if delta.abs() > self.params.grade_max_delta_m {
                    return Some(format!(
                        "{} is {delta:+.2} m from target; grade only levels within ±{} m (dig or haul first)",
                        at(*cell),
                        self.params.grade_max_delta_m
                    ));
                }
                return None;
            }
            Step::Compact(c) => (*c, &[Surface::Graded, Surface::Compacted], "graded"),
            Step::Sinter(c) => (*c, &[Surface::Compacted, Surface::Stabilized], "compacted"),
            _ => return None,
        };
        let s = self.terrain.get(c).surface;
        (!allowed.contains(&s))
            .then(|| format!("{} is {s:?}, must be {what} first", at(c)).to_lowercase())
    }

    fn validate(&self, r: usize, action: &Action) -> Result<(), Rejection> {
        use CapabilityName as Cap;
        match action {
            Action::MoveTo { target } => self.cell_ok(*target),
            Action::Scan { area } => {
                self.require(r, Cap::Scan)?;
                self.area_ok(area)
            }
            Action::Dig { area, spoil_to, .. } => {
                self.require(r, Cap::Dig)?;
                self.area_ok(area)?;
                spoil_to.map_or(Ok(()), |c| self.cell_ok(c))
            }
            Action::Grade {
                area,
                target_elevation_m,
            } => {
                self.require(r, Cap::Grade)?;
                self.area_ok(area)?;
                self.area_prereqs(area, |cell| Step::Grade {
                    cell,
                    target: *target_elevation_m,
                })
            }
            Action::Compact { area } => {
                self.require(r, Cap::Compact)?;
                self.area_ok(area)?;
                self.area_prereqs(area, Step::Compact)
            }
            Action::Sinter { area } => {
                self.require(r, Cap::Sinter)?;
                self.area_ok(area)?;
                self.area_prereqs(area, Step::Sinter)
            }
            Action::Haul { from, to, .. } => {
                self.require(r, Cap::Haul)?;
                self.cell_ok(*from)?;
                self.cell_ok(*to)
            }
            Action::Repair { target_robot } => {
                self.require(r, Cap::Repair)?;
                self.validate_repair(r, target_robot)
            }
            Action::Tow { target_robot, to } => {
                self.require(r, Cap::Haul)?;
                self.cell_ok(*to)?;
                self.validate_tow(r, target_robot)
            }
            Action::Cannibalize {
                target_robot,
                beneficiary_robot,
            } => {
                self.require(r, Cap::Repair)?;
                self.validate_cannibalize(r, target_robot, beneficiary_robot.as_deref())
            }
            Action::Service { depot } => self.validate_service(depot),
            Action::Charge { source, .. } => {
                if self.station_ix.contains_key(source) {
                    return Ok(());
                }
                match self.robot_ix.get(source) {
                    Some(&c) if c == r => {
                        reject(RejectReason::InvalidTarget, "a robot cannot charge itself")
                    }
                    Some(&c) if self.robots[c].capability(Cap::Charge).is_none() => reject(
                        RejectReason::InvalidTarget,
                        format!("{source} has no charge capability"),
                    ),
                    Some(_) => Ok(()),
                    None => reject(
                        RejectReason::InvalidTarget,
                        format!("no station or robot {source}"),
                    ),
                }
            }
            Action::Relay { position } => {
                self.require(r, Cap::Relay)?;
                self.cell_ok(*position)
            }
            Action::Wait { .. } => Ok(()),
            Action::Cancel { .. } => unreachable!("handled in submit"),
        }
    }

    fn area_prereqs(
        &self,
        area: &GridRect,
        step: impl Fn(GridCoord) -> Step,
    ) -> Result<(), Rejection> {
        match rect_cells(area).find_map(|c| self.prerequisite(&step(c))) {
            Some(why) => reject(RejectReason::InvalidTarget, why),
            None => Ok(()),
        }
    }

    fn plan(&self, r: usize, action: &Action) -> VecDeque<Step> {
        let per_cell = |area: &GridRect, work: &dyn Fn(GridCoord) -> Step| {
            serpentine(area)
                .into_iter()
                .flat_map(|c| [Step::Travel(c), work(c)])
                .collect()
        };
        match action {
            Action::MoveTo { target } => VecDeque::from([Step::Travel(*target)]),
            Action::Scan { area } => {
                let entry = clamp_into(self.robots[r].pos, area);
                VecDeque::from([Step::Travel(entry), Step::Scan(*area)])
            }
            Action::Dig {
                area,
                target_elevation_m,
                spoil_to,
            } => per_cell(area, &|cell| Step::Dig {
                cell,
                target: *target_elevation_m,
                spoil_to: *spoil_to,
            }),
            Action::Grade {
                area,
                target_elevation_m,
            } => per_cell(area, &|cell| Step::Grade {
                cell,
                target: *target_elevation_m,
            }),
            Action::Compact { area } => per_cell(area, &Step::Compact),
            Action::Sinter { area } => per_cell(area, &Step::Sinter),
            Action::Haul { from, to, mass_kg } => VecDeque::from([
                Step::Travel(*from),
                Step::Load {
                    at: *from,
                    kg: *mass_kg,
                },
                Step::Travel(*to),
                Step::Dump(*to),
            ]),
            Action::Charge { source, until_wh } => {
                if let Some(&st) = self.station_ix.get(source) {
                    VecDeque::from([
                        Step::Travel(self.stations[st].spec.position),
                        Step::ChargeStation {
                            station: st,
                            until_wh: *until_wh,
                        },
                    ])
                } else {
                    let c = self.robot_ix[source];
                    let charge = Step::ChargeMobile {
                        charger: c,
                        until_wh: *until_wh,
                    };
                    // An offline robot is already at its source and cannot drive.
                    if self.robots[r].offline {
                        VecDeque::from([charge])
                    } else {
                        VecDeque::from([Step::TravelToRobot(c), charge])
                    }
                }
            }
            Action::Relay { position } => VecDeque::from([Step::Travel(*position), Step::Hold]),
            Action::Wait { duration_s } => VecDeque::from([Step::Wait(ms(*duration_s))]),
            Action::Repair { target_robot } => {
                let target = self.robot_ix[target_robot];
                VecDeque::from([Step::TravelToRobot(target), Step::Repair { target }])
            }
            Action::Tow { target_robot, to } => {
                let target = self.robot_ix[target_robot];
                VecDeque::from([Step::TravelToRobot(target), Step::Tow { target, to: *to }])
            }
            Action::Cannibalize {
                target_robot,
                beneficiary_robot,
            } => {
                let target = self.robot_ix[target_robot];
                let beneficiary = beneficiary_robot.as_ref().map_or(r, |b| self.robot_ix[b]);
                let mut steps =
                    VecDeque::from([Step::TravelToRobot(target), Step::Strip { target }]);
                if beneficiary != r {
                    steps.push_back(Step::TravelToRobot(beneficiary));
                }
                steps.push_back(Step::Install { beneficiary });
                steps
            }
            Action::Service { depot } => {
                let d = self.depot_ix[depot];
                VecDeque::from([
                    Step::Travel(self.depots[d].spec.position),
                    Step::Service { depot: d },
                ])
            }
            Action::Cancel { .. } => VecDeque::new(),
        }
    }

    // ----- step costs -----

    fn rate(&self, r: usize, cap: CapabilityName) -> (f64, f64) {
        self.robots[r]
            .capability(cap)
            .map_or((0.0, 0.0), |c| (c.rate, c.energy_wh_per_unit))
    }

    /// Work units a step needs, with the capability that does it.
    fn work_units(&self, step: &Step) -> Option<(CapabilityName, f64)> {
        let area = self.terrain.cell_area_m2();
        Some(match step {
            Step::Dig { cell, target, .. } => {
                let cut = (self.terrain.get(*cell).elevation_m - target).max(0.0) * area;
                (
                    CapabilityName::Dig,
                    cut + self.terrain.boulder_volume_m3(*cell),
                )
            }
            Step::Grade { .. } => (CapabilityName::Grade, area),
            Step::Compact(_) => (CapabilityName::Compact, area),
            Step::Sinter(_) => (CapabilityName::Sinter, area),
            Step::Scan(a) => (CapabilityName::Scan, rect_cell_count(a) as f64 * area),
            _ => return None,
        })
    }

    /// Expected duration of a planned command, from the advertised spec:
    /// no noise, wear, faults, queueing or grid throttling.
    fn estimate_s(&self, r: usize, steps: &VecDeque<Step>) -> f64 {
        let robot = &self.robots[r];
        let p = &self.params;
        let mut pos = robot.pos;
        let mut payload = robot.payload_kg;
        let mut total = 0.0;
        let drive =
            |a, b| distance_cells(a, b) * self.terrain.cell_size_m / robot.spec.max_speed_m_s;
        let repair_rate = self.rate(r, CapabilityName::Repair).0.max(1e-9);
        for step in steps {
            total += match step {
                Step::Travel(to) => {
                    let d = drive(pos, *to);
                    pos = *to;
                    d
                }
                Step::TravelToRobot(c) => {
                    let to = self.robots[*c].position_at(self.now);
                    let d = drive(pos, to);
                    pos = to;
                    d
                }
                Step::Load { kg, .. } => {
                    let amount = (kg - payload)
                        .max(0.0)
                        .min(robot.haul_capacity_kg() - payload);
                    payload += amount;
                    amount / self.rate(r, CapabilityName::Haul).0.max(1e-9)
                }
                Step::Dump(_) => {
                    let d = payload / self.rate(r, CapabilityName::Haul).0.max(1e-9);
                    payload = 0.0;
                    d
                }
                Step::Wait(d) => secs(*d),
                Step::ChargeStation { station, until_wh } => {
                    let need =
                        until_wh.unwrap_or(robot.spec.battery_capacity_wh) - robot.battery_wh;
                    let spec = &self.stations[*station].spec;
                    need.max(0.0) / (spec.max_power_w / f64::from(spec.slots.max(1))) * 3600.0
                }
                Step::ChargeMobile { charger, until_wh } => {
                    let need =
                        until_wh.unwrap_or(robot.spec.battery_capacity_wh) - robot.battery_wh;
                    let rate = self.rate(*charger, CapabilityName::Charge).0;
                    need.max(0.0) / rate.max(1e-9)
                }
                Step::Hold => 0.0,
                Step::Repair { target } => {
                    (self.visible_repair_units(*target) / repair_rate).max(p.min_work_s)
                }
                Step::Tow { target, to } => {
                    let from = self.robots[*target].position_at(self.now);
                    pos = *to;
                    distance_cells(from, *to) * self.terrain.cell_size_m
                        / (robot.spec.max_speed_m_s * p.tow_speed_factor)
                }
                Step::Strip { .. } => (p.strip_units / repair_rate).max(p.min_work_s),
                Step::Install { .. } => (p.install_units / repair_rate).max(p.min_work_s),
                Step::Service { .. } => p.service_s,
                work => match self.work_units(work) {
                    Some((cap, units)) => (units / self.rate(r, cap).0.max(1e-9)).max(p.min_work_s),
                    None => 0.0,
                },
            };
        }
        total
    }

    // ----- execution -----

    pub(crate) fn start_next_step(&mut self, r: usize) {
        loop {
            let Some(step) = self.robots[r]
                .activity
                .as_mut()
                .and_then(|a| a.steps.pop_front())
            else {
                if self.robots[r].activity.is_some() {
                    self.complete_command(r);
                }
                return;
            };
            match self.begin_step(r, step) {
                StepStart::Scheduled | StepStart::Waiting => return,
                StepStart::Skip => continue,
                StepStart::Failed(reason, detail) => {
                    self.fail_command(r, reason, Some(detail));
                    return;
                }
            }
        }
    }

    fn begin_step(&mut self, r: usize, step: Step) -> StepStart {
        let step = match step {
            Step::TravelToRobot(c) => Step::Travel(self.robots[c].position_at(self.now)),
            s => s,
        };
        let timed = match &step {
            Step::Hold => {
                let now = self.now;
                // Settle any drain owed at the old rate before the radio adds to it.
                self.settle_drain(r);
                let radio_w = self.params.relay_radio_w;
                let robot = &mut self.robots[r];
                robot.status = RobotStatus::Working;
                robot.drain_since = Some(now);
                robot.radio_w = radio_w;
                self.set_running(r, step, None, 0.0, 0.0, Exposure::default());
                self.schedule_depletion(r);
                self.start_exposure(r, Exposed::Hold, None, None);
                return StepStart::Waiting;
            }
            Step::ChargeStation { station, until_wh } => {
                let (station, until_wh) = (*station, *until_wh);
                self.set_running(r, step, None, 0.0, 0.0, Exposure::default());
                self.end_exposure(r);
                let st = &self.stations[station];
                if st.occupants.len() < st.spec.slots as usize {
                    self.start_station_session(r, station, until_wh);
                } else {
                    // Queue for a slot; idle drain continues while waiting.
                    self.stations[station].waiting.push_back(r);
                    let now = self.now;
                    let robot = &mut self.robots[r];
                    robot.status = RobotStatus::Charging;
                    if !robot.offline {
                        robot.drain_since = Some(now);
                    }
                    self.schedule_depletion(r);
                }
                return StepStart::Waiting;
            }
            Step::ChargeMobile { charger, until_wh } => {
                let (c, until_wh) = (*charger, *until_wh);
                let ch = &self.robots[c];
                if ch.busy() || ch.reported_status() != RobotStatus::Idle {
                    return StepStart::Failed(
                        FailReason::Blocked,
                        format!("{} is not available", ch.spec.robot_id),
                    );
                }
                if distance_cells(ch.pos, self.robots[r].pos) > super::REACH_CELLS {
                    return StepStart::Failed(
                        FailReason::TargetLost,
                        format!("{} moved away", ch.spec.robot_id),
                    );
                }
                self.set_running(r, step, None, 0.0, 0.0, Exposure::default());
                self.end_exposure(r);
                self.start_mobile_session(r, c, until_wh);
                return StepStart::Waiting;
            }
            Step::Service { depot } => return self.begin_service(r, *depot),
            Step::Repair { target } => self.begin_repair(r, *target),
            Step::Tow { target, to } => self.begin_tow(r, *target, *to),
            Step::Strip { target } => self.begin_strip(r, *target),
            Step::Install { beneficiary } => self.begin_install(r, *beneficiary),
            Step::TravelToRobot(_) => unreachable!("resolved above"),
            other => self.basic_step(r, other),
        };
        match timed {
            Ok(timed) => self.run_timed(r, step, timed),
            Err(start) => start,
        }
    }

    /// Drives, loading and dumping, waits and per-cell work, with the
    /// robot's wear and fault penalties applied.
    fn basic_step(&mut self, r: usize, step: &Step) -> Result<Timed, StepStart> {
        let dust = self.dust_factor();
        let storm = self.env.storm_active(self.now);
        let p = &self.params;
        let robot = &self.robots[r];
        let health = &robot.health;
        let ef = health.energy_factor(p);
        let idle_w = robot.spec.idle_power_w;
        let exposure = |d: f64, e: f64, km: f64, work: bool| Exposure {
            km,
            work_h: if work { d / 3600.0 } else { 0.0 },
            dust_h: d / 3600.0 * dust,
            kwh: e / 1000.0,
            storm,
        };
        match step {
            Step::Travel(to) => {
                let from = robot.pos;
                if from == *to {
                    return Err(StepStart::Skip);
                }
                let dist_m = distance_cells(from, *to) * self.terrain.cell_size_m;
                let speed = robot.spec.max_speed_m_s * health.speed_factor(p);
                let drive_wh =
                    dist_m * robot.spec.drive_energy_wh_per_m * health.drive_energy_factor(p);
                let d = dist_m / speed
                    * self
                        .rng_work
                        .lognormal_mean1(self.params.travel_duration_sigma);
                let e = (drive_wh + d * idle_w / 3600.0) * ef;
                Ok(Timed {
                    duration_s: d,
                    energy_wh: e,
                    amount: 0.0,
                    status: RobotStatus::Moving,
                    exposed: Some(Exposed::Drive),
                    wear: exposure(d, e, dist_m / 1000.0, false),
                    engage: None,
                })
            }
            Step::Load { at: cell, kg } => {
                let room = robot.haul_capacity_kg() - robot.payload_kg;
                let amount = (kg - robot.payload_kg)
                    .min(room)
                    .min(self.terrain.available_mass_kg(*cell))
                    .max(0.0);
                if amount <= 0.0 {
                    if robot.payload_kg > 0.0 {
                        return Err(StepStart::Skip);
                    }
                    return Err(StepStart::Failed(
                        FailReason::Blocked,
                        format!("nothing to load at {}", at(*cell)),
                    ));
                }
                let (rate, e_unit) = self.rate(r, CapabilityName::Haul);
                let (rate_f, work_ef) = (health.work_rate_factor(p), health.work_energy_factor(p));
                let d = amount / (rate * rate_f)
                    * self
                        .rng_work
                        .lognormal_mean1(self.params.work_duration_sigma);
                let e = (amount * e_unit * work_ef + d * idle_w / 3600.0) * ef;
                Ok(Timed {
                    duration_s: d,
                    energy_wh: e,
                    amount,
                    status: RobotStatus::Working,
                    exposed: Some(Exposed::Work),
                    wear: exposure(d, e, 0.0, true),
                    engage: None,
                })
            }
            Step::Dump(_) => {
                let payload = robot.payload_kg;
                if payload <= 0.0 {
                    return Err(StepStart::Skip);
                }
                let (rate, e_unit) = self.rate(r, CapabilityName::Haul);
                let (rate_f, work_ef) = (health.work_rate_factor(p), health.work_energy_factor(p));
                let d = payload / (rate * rate_f)
                    * self
                        .rng_work
                        .lognormal_mean1(self.params.work_duration_sigma);
                let e = (payload * e_unit * work_ef + d * idle_w / 3600.0) * ef;
                Ok(Timed {
                    duration_s: d,
                    energy_wh: e,
                    amount: payload,
                    status: RobotStatus::Working,
                    exposed: Some(Exposed::Work),
                    wear: exposure(d, e, 0.0, true),
                    engage: None,
                })
            }
            Step::Wait(d) => {
                let d = secs(*d);
                let e = d * idle_w / 3600.0 * ef;
                Ok(Timed {
                    duration_s: d,
                    energy_wh: e,
                    amount: 0.0,
                    status: RobotStatus::Idle,
                    exposed: None,
                    wear: Exposure {
                        kwh: e / 1000.0,
                        ..Exposure::default()
                    },
                    engage: None,
                })
            }
            work => {
                if let Some(why) = self.prerequisite(work) {
                    return Err(StepStart::Failed(FailReason::Blocked, why));
                }
                let (cap, units) = self.work_units(work).expect("work step");
                let (rate, e_unit) = self.rate(r, cap);
                let scan = cap == CapabilityName::Scan;
                let (rate_f, work_ef) = if scan {
                    (health.scan_rate_factor(p), 1.0)
                } else {
                    (health.work_rate_factor(p), health.work_energy_factor(p))
                };
                let d = (units / (rate * rate_f)
                    * self
                        .rng_work
                        .lognormal_mean1(self.params.work_duration_sigma))
                .max(self.params.min_work_s);
                let e = (units * e_unit * work_ef + d * idle_w / 3600.0) * ef;
                Ok(Timed {
                    duration_s: d,
                    energy_wh: e,
                    amount: units,
                    status: RobotStatus::Working,
                    exposed: Some(if scan { Exposed::Scan } else { Exposed::Work }),
                    wear: exposure(d, e, 0.0, !scan),
                    engage: None,
                })
            }
        }
    }

    /// Start a timed step: draw its energy, schedule its end, and expose the
    /// robot to failures for its duration.
    fn run_timed(&mut self, r: usize, step: Step, t: Timed) -> StepStart {
        let now = self.now;
        let robot = &mut self.robots[r];
        if robot.battery_wh < t.energy_wh {
            robot.battery_wh = 0.0;
            return StepStart::Failed(
                FailReason::InsufficientEnergy,
                format!("needed {:.0} Wh for the next step", t.energy_wh),
            );
        }
        robot.battery_wh -= t.energy_wh;
        robot.status = t.status;
        let end = now + ms(t.duration_s).max(1);
        let from = robot.pos;
        let mut path = None;
        if let Step::Travel(to) | Step::Tow { to, .. } = step {
            robot.travel = Some(Travel {
                from,
                to,
                start: now,
                end,
            });
            path = Some((from, to));
        }
        if let Some(a) = robot.activity.as_mut() {
            a.energy_used_wh += t.energy_wh;
        }
        if let Step::Tow { target, to } = step {
            let towed = &mut self.robots[target];
            towed.travel = Some(Travel {
                from: towed.pos,
                to,
                start: now,
                end,
            });
            towed.towed = true;
        }
        if let Some(e) = t.engage {
            self.robots[e].engaged_by = Some(r);
        }
        self.set_running(r, step, Some(end), t.energy_wh, t.amount, t.wear);
        self.schedule(r, end, |robot, generation| Ev::StepDone {
            robot,
            generation,
        });
        match t.exposed {
            Some(kind) => self.start_exposure(r, kind, Some(end), path),
            None => self.end_exposure(r),
        }
        StepStart::Scheduled
    }

    pub(crate) fn set_running(
        &mut self,
        r: usize,
        step: Step,
        end: Option<SimMs>,
        energy_wh: f64,
        amount: f64,
        exposure: Exposure,
    ) {
        let start = self.now;
        if let Some(a) = self.robots[r].activity.as_mut() {
            a.running = Some(Running {
                step,
                start,
                end,
                energy_wh,
                amount,
                exposure,
            });
        }
    }

    pub(crate) fn finish_step(&mut self, r: usize) {
        let Some(running) = self.robots[r]
            .activity
            .as_mut()
            .and_then(|a| a.running.take())
        else {
            return;
        };
        self.end_exposure(r);
        self.robots[r].health.grow(&self.params, &running.exposure);
        match running.step {
            Step::Travel(to) => {
                let robot = &mut self.robots[r];
                robot.pos = to;
                robot.travel = None;
            }
            Step::Dig {
                cell,
                target,
                spoil_to,
            } => self.apply_dig(r, cell, target, spoil_to),
            Step::Grade { cell, target } => {
                let noise = self.rng_work.normal() * self.params.grade_noise_m;
                let t = self.terrain.get_mut(cell);
                t.elevation_m = target + noise;
                t.surface = Surface::Graded;
                self.mark_changed(cell);
            }
            Step::Compact(cell) => {
                let settle = self.params.compact_settle_m
                    + self.rng_work.normal() * self.params.compact_settle_noise_m;
                let t = self.terrain.get_mut(cell);
                t.elevation_m -= settle.max(0.0);
                t.surface = Surface::Compacted;
                self.mark_changed(cell);
            }
            Step::Sinter(cell) => {
                self.terrain.get_mut(cell).surface = Surface::Stabilized;
                self.mark_changed(cell);
            }
            Step::Scan(area) => self.complete_scan(r, area, running.start),
            Step::Load { at: cell, .. } => {
                let got = self.terrain.remove_mass(cell, running.amount);
                self.robots[r].payload_kg += got;
                self.mark_changed(cell);
            }
            Step::Dump(cell) => {
                let kg = std::mem::take(&mut self.robots[r].payload_kg);
                self.dump(cell, kg);
            }
            Step::Repair { target } => self.finish_repair(r, target),
            Step::Tow { target, to } => self.finish_tow(r, target, to),
            Step::Strip { target } => self.finish_strip(r, target),
            Step::Install { beneficiary } => self.finish_install(r, beneficiary),
            Step::Service { depot } => self.finish_service(r, depot),
            Step::Wait(_) | Step::Hold | Step::ChargeStation { .. } | Step::ChargeMobile { .. } => {
            }
            Step::TravelToRobot(_) => unreachable!("resolved when started"),
        }
        self.start_next_step(r);
    }

    fn complete_command(&mut self, r: usize) {
        let now = self.now;
        let robot = &mut self.robots[r];
        let Some(activity) = robot.activity.take() else {
            return;
        };
        robot.status = RobotStatus::Idle;
        robot.travel = None;
        robot.drain_since = if robot.offline { None } else { Some(now) };
        if !activity.internal {
            self.emit(
                EventKind::CommandCompleted {
                    command_id: activity.command.command_id,
                    robot_id: activity.command.robot_id,
                    energy_used_wh: activity.energy_used_wh,
                },
                true,
            );
        }
        self.schedule_depletion(r);
    }

    /// Stop the robot's running step now: return the unused part of its
    /// energy, grow wear for the part done, stop moving (with any robot it
    /// tows), release robots it engages, leave any depot queue or bay, and
    /// switch off a relay's radio.
    pub(crate) fn interrupt_step(&mut self, r: usize) {
        self.end_exposure(r);
        let now = self.now;
        if matches!(self.robots[r].running_step(), Some(Step::Hold)) {
            // The radio has drawn power up to now; it stops here.
            self.settle_drain(r);
            self.robots[r].radio_w = 0.0;
        }
        let Some(running) = self.robots[r]
            .activity
            .as_mut()
            .and_then(|a| a.running.take())
        else {
            return;
        };
        let (done, refund) = match running.end {
            Some(end) => {
                let span = end.saturating_sub(running.start).max(1) as f64;
                let left = end.saturating_sub(now) as f64 / span;
                ((1.0 - left).clamp(0.0, 1.0), running.energy_wh * left)
            }
            None => (1.0, 0.0),
        };
        let robot = &mut self.robots[r];
        robot.battery_wh += refund;
        if let Some(a) = robot.activity.as_mut() {
            a.energy_used_wh -= refund;
        }
        if robot.travel.is_some() {
            robot.pos = robot.position_at(now);
            robot.travel = None;
        }
        robot
            .health
            .grow(&self.params, &running.exposure.scaled(done));
        match running.step {
            Step::Tow { target, .. } => {
                let towed = &mut self.robots[target];
                towed.pos = towed.position_at(now);
                towed.travel = None;
                towed.towed = false;
                self.release(target, r);
            }
            Step::Repair { target } | Step::Strip { target } => self.release(target, r),
            Step::Install { beneficiary } => self.release(beneficiary, r),
            Step::Service { depot } => self.leave_depot(r, depot),
            _ => {}
        }
    }

    pub(crate) fn fail_command(&mut self, r: usize, reason: FailReason, detail: Option<String>) {
        self.leave_charging(r);
        self.interrupt_step(r);
        let Some(activity) = self.robots[r].activity.take() else {
            return;
        };
        self.settle_drain(r);
        let now = self.now;
        let robot = &mut self.robots[r];
        robot.status = RobotStatus::Idle;
        if robot.battery_wh <= 0.0 {
            robot.offline = true;
            robot.drain_since = None;
        } else {
            robot.drain_since = Some(now);
        }
        if !activity.internal {
            self.emit(
                EventKind::CommandFailed {
                    command_id: activity.command.command_id,
                    robot_id: activity.command.robot_id,
                    reason,
                    detail,
                    energy_used_wh: activity.energy_used_wh,
                },
                true,
            );
        }
        self.schedule_depletion(r);
    }

    fn cancel(&mut self, r: usize, cmd: &Command, target: &str, level: AuthorityLevel) {
        if self.robots[r].running_command() != Some(target) {
            return self.emit_rejected(
                cmd,
                RejectReason::InvalidTarget,
                format!("{target} is not running on {}", cmd.robot_id),
            );
        }
        self.authority.accepted(cmd, level, self.now);
        self.emit(
            EventKind::CommandAccepted {
                command_id: cmd.command_id.clone(),
                robot_id: cmd.robot_id.clone(),
                estimated_duration_s: 0.0,
            },
            false,
        );
        // The unused part of the step in progress is refunded; its work is abandoned.
        self.fail_command(
            r,
            FailReason::Cancelled,
            Some(format!("cancelled by {}", cmd.command_id)),
        );
        self.emit(
            EventKind::CommandCompleted {
                command_id: cmd.command_id.clone(),
                robot_id: cmd.robot_id.clone(),
                energy_used_wh: 0.0,
            },
            true,
        );
    }

    // ----- physical effects -----

    fn mark_changed(&mut self, c: GridCoord) {
        self.changed.insert((c.x, c.y));
        if let Some(m) = self.mission.as_mut() {
            m.on_change(c, self.now);
        }
        self.mission_dirty = true;
    }

    fn apply_dig(&mut self, r: usize, cell: GridCoord, target: f64, spoil_to: Option<GridCoord>) {
        let boulder_kg: f64 = self
            .terrain
            .get(cell)
            .boulders
            .iter()
            .map(|d| PI / 6.0 * d * d * d * self.params.rock_density_kg_m3)
            .sum();
        let density = self.terrain.get(cell).density_kg_m3;
        let above = self.terrain.get(cell).elevation_m - target;
        let mut cut_kg = 0.0;
        if above > 0.0 {
            let noisy_target = (target + self.rng_work.normal() * self.params.dig_cut_noise_m)
                .min(self.terrain.get(cell).elevation_m);
            cut_kg = self.terrain.cut_to(cell, noisy_target) * density;
        }
        let t = self.terrain.get_mut(cell);
        t.boulders.clear();
        t.surface = Surface::Cleared;
        self.mark_changed(cell);

        let spoil = cut_kg + boulder_kg;
        let robot = &mut self.robots[r];
        let to_payload = spoil.min((robot.haul_capacity_kg() - robot.payload_kg).max(0.0));
        robot.payload_kg += to_payload;
        let overflow = spoil - to_payload;
        if overflow > 0.0 {
            let dest = spoil_to.unwrap_or_else(|| self.default_spoil_cell(cell));
            self.dump(dest, overflow);
        }
    }

    /// Deposit material. Berm cells become berm; untouched ground or piles
    /// become stockpile; prepared ground reverts to cleared and needs regrading.
    fn dump(&mut self, c: GridCoord, kg: f64) {
        if kg <= 0.0 {
            return;
        }
        self.terrain.add_mass(c, kg);
        let is_berm = self.mission.as_ref().is_some_and(|m| m.is_berm(c));
        let t = self.terrain.get_mut(c);
        t.surface = if is_berm {
            Surface::Berm
        } else if matches!(t.surface, Surface::Natural | Surface::Stockpile) {
            Surface::Stockpile
        } else {
            Surface::Cleared
        };
        self.mark_changed(c);
    }

    /// Nearest cell outside the pad, searching outward ring by ring.
    fn default_spoil_cell(&self, from: GridCoord) -> GridCoord {
        let Some(m) = &self.mission else {
            return from;
        };
        let (w, h) = (self.terrain.width as i64, self.terrain.height as i64);
        for radius in 1..w.max(h) {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx.abs() != radius && dy.abs() != radius {
                        continue;
                    }
                    let (x, y) = (i64::from(from.x) + dx, i64::from(from.y) + dy);
                    if (0..w).contains(&x) && (0..h).contains(&y) {
                        let c = GridCoord {
                            x: x as u32,
                            y: y as u32,
                        };
                        if !m.in_pad(c) {
                            return c;
                        }
                    }
                }
            }
        }
        from
    }

    fn complete_scan(&mut self, r: usize, area: GridRect, started: SimMs) {
        let p = self.params.clone();
        let cells = rect_cells(&area)
            .map(|c| {
                let t = self.terrain.get(c).clone();
                let mut n = || self.rng_scan.normal();
                let boulders = t
                    .boulders
                    .iter()
                    .filter(|&&d| d >= p.scan_min_boulder_m)
                    .map(|&d| Boulder {
                        diameter_m: d * (1.0 + n() * p.scan_boulder_noise_frac),
                        mass_kg: None,
                    })
                    .collect();
                CellObservation {
                    at: c,
                    cell: Cell {
                        elevation_m: Some(t.elevation_m + n() * p.scan_elevation_noise_m),
                        regolith: Some(Regolith {
                            depth_m: Some(
                                (t.regolith_depth_m + n() * p.scan_depth_noise_m).max(0.0),
                            ),
                            bulk_density_kg_m3: Some(
                                t.density_kg_m3 * (1.0 + n() * p.scan_density_noise_frac),
                            ),
                            cohesion_kpa: None,
                        }),
                        boulders: Some(boulders),
                        surface: Some(t.surface),
                        ice_depth_m: None,
                    },
                }
            })
            .collect();
        if let Some(m) = self.mission.as_mut() {
            m.on_scan(&area, started);
        }
        self.mission_dirty = true;
        let a = self.robots[r]
            .activity
            .as_ref()
            .expect("scan belongs to a command");
        let (command_id, robot_id) = (a.command.command_id.clone(), a.command.robot_id.clone());
        self.emit(
            EventKind::ScanResult {
                command_id,
                robot_id,
                cells,
            },
            true,
        );
    }
}
