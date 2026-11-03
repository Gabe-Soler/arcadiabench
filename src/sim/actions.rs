//! Command acceptance and execution.
//!
//! A command is validated, planned into [`Step`]s (travel, then per-cell work),
//! and executed one step at a time. Energy for a timed step is drawn when it
//! starts; if the battery cannot cover it the command fails with
//! `insufficient_energy`. Area commands work cell by cell, so partial work
//! survives cancellation and failure.
//!
//! The landing-pad "task graph" lives here as per-cell prerequisites:
//! natural → cleared (dig) → graded → compacted → stabilized (sinter).

use super::robot::{Activity, Running, Step, Travel};
use super::terrain::{clamp_into, distance_cells, rect_cell_count, rect_cells, serpentine};
use super::time::{SimMs, ms, secs};
use super::{Ev, Sim};
use arcadia_protocol::*;
use std::collections::VecDeque;
use std::f64::consts::PI;

type Rejection = (RejectReason, String);

enum StepStart {
    /// A timed step is under way.
    Scheduled,
    /// Waiting on something open-ended (charging, queueing, holding).
    Waiting,
    /// Nothing to do; move on.
    Skip,
    Failed(FailReason, String),
}

fn reject(reason: RejectReason, detail: impl Into<String>) -> Result<(), Rejection> {
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
        if let Action::Cancel { target_command } = &cmd.action {
            return self.cancel(r, cmd, target_command);
        }
        if self.robots[r].status == RobotStatus::Offline {
            return self.emit_rejected(cmd, RejectReason::RobotDisabled, "battery depleted".into());
        }
        if self.robots[r].busy() {
            let detail = match self.robots[r].running_command() {
                Some(c) => format!("running {c}"),
                None => "serving as a mobile charger".into(),
            };
            return self.emit_rejected(cmd, RejectReason::Busy, detail);
        }
        if let Err((reason, detail)) = self.validate(r, &cmd.action) {
            return self.emit_rejected(cmd, reason, detail);
        }

        let steps = self.plan(r, &cmd.action);
        let estimate = self.estimate_s(r, &steps);
        self.emit(
            EventKind::CommandAccepted {
                command_id: cmd.command_id.clone(),
                robot_id: cmd.robot_id.clone(),
                estimated_duration_s: estimate,
            },
            false,
        );
        let now = self.now;
        let robot = &mut self.robots[r];
        robot.settle_drain(now);
        robot.drain_since = None;
        robot.activity = Some(Activity {
            command: cmd.clone(),
            steps,
            running: None,
            energy_used_wh: 0.0,
        });
        self.start_next_step(r);
    }

    fn emit_rejected(&mut self, cmd: &Command, reason: RejectReason, detail: String) {
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
            Action::Repair { target_robot } | Action::Cannibalize { target_robot, .. } => {
                self.require(r, Cap::Repair)?;
                self.disabled_target(target_robot)
            }
            Action::Tow { target_robot, to } => {
                self.require(r, Cap::Haul)?;
                self.cell_ok(*to)?;
                self.disabled_target(target_robot)
            }
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

    /// Faults arrive in week 5; until then no robot is ever disabled.
    fn disabled_target(&self, target: &str) -> Result<(), Rejection> {
        match self.robot_ix.get(target) {
            None => reject(RejectReason::InvalidTarget, format!("no robot {target}")),
            Some(_) => reject(
                RejectReason::InvalidTarget,
                format!("{target} is not disabled"),
            ),
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
                    VecDeque::from([
                        Step::TravelToRobot(c),
                        Step::ChargeMobile {
                            charger: c,
                            until_wh: *until_wh,
                        },
                    ])
                }
            }
            Action::Relay { position } => VecDeque::from([Step::Travel(*position), Step::Hold]),
            Action::Wait { duration_s } => VecDeque::from([Step::Wait(ms(*duration_s))]),
            Action::Repair { .. }
            | Action::Tow { .. }
            | Action::Cannibalize { .. }
            | Action::Cancel { .. } => VecDeque::new(),
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

    /// Expected duration of a planned command, without noise.
    fn estimate_s(&self, r: usize, steps: &VecDeque<Step>) -> f64 {
        let robot = &self.robots[r];
        let mut pos = robot.pos;
        let mut payload = robot.payload_kg;
        let mut total = 0.0;
        let drive =
            |a, b| distance_cells(a, b) * self.terrain.cell_size_m / robot.spec.max_speed_m_s;
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
                work => match self.work_units(work) {
                    Some((cap, units)) => {
                        (units / self.rate(r, cap).0.max(1e-9)).max(self.params.min_work_s)
                    }
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
        let idle_w = self.robots[r].spec.idle_power_w;
        let (duration_s, energy_wh, amount, status) = match &step {
            Step::Travel(to) => {
                let from = self.robots[r].pos;
                if from == *to {
                    return StepStart::Skip;
                }
                let dist_m = distance_cells(from, *to) * self.terrain.cell_size_m;
                let spec = &self.robots[r].spec;
                let d = dist_m / spec.max_speed_m_s
                    * self
                        .rng_work
                        .lognormal_mean1(self.params.travel_duration_sigma);
                let e = dist_m * spec.drive_energy_wh_per_m + d * idle_w / 3600.0;
                (d, e, 0.0, RobotStatus::Moving)
            }
            Step::Load { at: cell, kg } => {
                let robot = &self.robots[r];
                let room = robot.haul_capacity_kg() - robot.payload_kg;
                let amount = (kg - robot.payload_kg)
                    .min(room)
                    .min(self.terrain.available_mass_kg(*cell))
                    .max(0.0);
                if amount <= 0.0 {
                    if robot.payload_kg > 0.0 {
                        return StepStart::Skip;
                    }
                    return StepStart::Failed(
                        FailReason::Blocked,
                        format!("nothing to load at {}", at(*cell)),
                    );
                }
                let (rate, e_unit) = self.rate(r, CapabilityName::Haul);
                let d = amount / rate
                    * self
                        .rng_work
                        .lognormal_mean1(self.params.work_duration_sigma);
                (
                    d,
                    amount * e_unit + d * idle_w / 3600.0,
                    amount,
                    RobotStatus::Working,
                )
            }
            Step::Dump(_) => {
                let payload = self.robots[r].payload_kg;
                if payload <= 0.0 {
                    return StepStart::Skip;
                }
                let (rate, e_unit) = self.rate(r, CapabilityName::Haul);
                let d = payload / rate
                    * self
                        .rng_work
                        .lognormal_mean1(self.params.work_duration_sigma);
                (
                    d,
                    payload * e_unit + d * idle_w / 3600.0,
                    payload,
                    RobotStatus::Working,
                )
            }
            Step::Wait(d) => {
                let d = secs(*d);
                (d, d * idle_w / 3600.0, 0.0, RobotStatus::Idle)
            }
            Step::Hold => {
                let now = self.now;
                let robot = &mut self.robots[r];
                robot.status = RobotStatus::Working;
                robot.drain_since = Some(now);
                self.set_running(r, step, None, 0.0, 0.0);
                self.schedule_depletion(r);
                return StepStart::Waiting;
            }
            Step::ChargeStation { station, until_wh } => {
                let (station, until_wh) = (*station, *until_wh);
                self.set_running(r, step, None, 0.0, 0.0);
                let st = &self.stations[station];
                if st.occupants.len() < st.spec.slots as usize {
                    self.start_station_session(r, station, until_wh);
                } else {
                    // Queue for a slot; idle drain continues while waiting.
                    self.stations[station].waiting.push_back(r);
                    let now = self.now;
                    let robot = &mut self.robots[r];
                    robot.status = RobotStatus::Charging;
                    robot.drain_since = Some(now);
                    self.schedule_depletion(r);
                }
                return StepStart::Waiting;
            }
            Step::ChargeMobile { charger, until_wh } => {
                let (c, until_wh) = (*charger, *until_wh);
                let ch = &self.robots[c];
                if ch.busy() || ch.status == RobotStatus::Offline {
                    return StepStart::Failed(
                        FailReason::Blocked,
                        format!("{} is not available", ch.spec.robot_id),
                    );
                }
                if distance_cells(ch.pos, self.robots[r].pos) > 1.5 {
                    return StepStart::Failed(
                        FailReason::TargetLost,
                        format!("{} moved away", ch.spec.robot_id),
                    );
                }
                self.set_running(r, step, None, 0.0, 0.0);
                self.start_mobile_session(r, c, until_wh);
                return StepStart::Waiting;
            }
            Step::TravelToRobot(_) => unreachable!("resolved above"),
            work => {
                if let Some(why) = self.prerequisite(work) {
                    return StepStart::Failed(FailReason::Blocked, why);
                }
                let (cap, units) = self.work_units(work).expect("work step");
                let (rate, e_unit) = self.rate(r, cap);
                let d = (units / rate
                    * self
                        .rng_work
                        .lognormal_mean1(self.params.work_duration_sigma))
                .max(self.params.min_work_s);
                (
                    d,
                    units * e_unit + d * idle_w / 3600.0,
                    units,
                    RobotStatus::Working,
                )
            }
        };

        let now = self.now;
        let robot = &mut self.robots[r];
        if robot.battery_wh < energy_wh {
            robot.battery_wh = 0.0;
            return StepStart::Failed(
                FailReason::InsufficientEnergy,
                format!("needed {energy_wh:.0} Wh for the next step"),
            );
        }
        robot.battery_wh -= energy_wh;
        robot.status = status;
        let end = now + ms(duration_s).max(1);
        if let Step::Travel(to) = step {
            robot.travel = Some(Travel {
                from: robot.pos,
                to,
                start: now,
                end,
            });
        }
        if let Some(a) = robot.activity.as_mut() {
            a.energy_used_wh += energy_wh;
        }
        self.set_running(r, step, Some(end), energy_wh, amount);
        self.schedule(r, end, |robot, generation| Ev::StepDone {
            robot,
            generation,
        });
        StepStart::Scheduled
    }

    fn set_running(
        &mut self,
        r: usize,
        step: Step,
        end: Option<SimMs>,
        energy_wh: f64,
        amount: f64,
    ) {
        let start = self.now;
        if let Some(a) = self.robots[r].activity.as_mut() {
            a.running = Some(Running {
                step,
                start,
                end,
                energy_wh,
                amount,
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
        robot.drain_since = Some(now);
        self.emit(
            EventKind::CommandCompleted {
                command_id: activity.command.command_id,
                robot_id: activity.command.robot_id,
                energy_used_wh: activity.energy_used_wh,
            },
            true,
        );
        self.schedule_depletion(r);
    }

    pub(crate) fn fail_command(&mut self, r: usize, reason: FailReason, detail: Option<String>) {
        self.leave_charging(r);
        let now = self.now;
        let robot = &mut self.robots[r];
        let Some(activity) = robot.activity.take() else {
            return;
        };
        if robot.travel.is_some() {
            robot.pos = robot.position_at(now);
            robot.travel = None;
        }
        robot.settle_drain(now);
        if robot.battery_wh <= 0.0 {
            robot.status = RobotStatus::Offline;
            robot.drain_since = None;
        } else {
            robot.status = RobotStatus::Idle;
            robot.drain_since = Some(now);
        }
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
        self.schedule_depletion(r);
    }

    fn cancel(&mut self, r: usize, cmd: &Command, target: &str) {
        if self.robots[r].running_command() != Some(target) {
            return self.emit_rejected(
                cmd,
                RejectReason::InvalidTarget,
                format!("{target} is not running on {}", cmd.robot_id),
            );
        }
        // Refund the unused part of a timed step in progress; its work is abandoned.
        let now = self.now;
        let robot = &mut self.robots[r];
        if let Some(a) = robot.activity.as_mut()
            && let Some(Running {
                start,
                end: Some(end),
                energy_wh,
                ..
            }) = a.running
        {
            let left = (end.saturating_sub(now)) as f64 / (end - start).max(1) as f64;
            let refund = energy_wh * left;
            robot.battery_wh += refund;
            a.energy_used_wh -= refund;
        }
        self.emit(
            EventKind::CommandAccepted {
                command_id: cmd.command_id.clone(),
                robot_id: cmd.robot_id.clone(),
                estimated_duration_s: 0.0,
            },
            false,
        );
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
