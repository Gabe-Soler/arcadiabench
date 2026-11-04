//! The repair options (semantics §4.3, §6.6): field repair, tow, cannibalize
//! and depot service, plus the exceptions that let a disabled robot still
//! accept `service` on a depot's cell and `charge` at its source (§6.7).

use super::actions::{Rejection, StepStart, Timed, reject};
use super::faults::{Exposure, Health};
use super::hazards::Exposed;
use super::robot::status_name;
use super::terrain::distance_cells;
use super::time::ms;
use super::{Ev, REACH_CELLS, Sim};
use arcadia_protocol::*;

fn blocked(detail: String) -> StepStart {
    StepStart::Failed(FailReason::Blocked, detail)
}

impl Sim {
    // ----- acceptance -----

    /// Whether a disabled robot may still accept this command.
    pub(crate) fn disabled_exception(&self, r: usize, action: &Action) -> bool {
        let robot = &self.robots[r];
        let status = robot.reported_status();
        match action {
            Action::Service { depot } => {
                matches!(
                    status,
                    RobotStatus::Failed | RobotStatus::Stuck | RobotStatus::Offline
                ) && self
                    .depot_ix
                    .get(depot)
                    .is_some_and(|&d| self.depots[d].spec.position == robot.pos)
            }
            Action::Charge { source, .. } => {
                status == RobotStatus::Offline && self.at_source(r, source)
            }
            _ => false,
        }
    }

    /// Already at a charging source: on the station's cell, or within reach
    /// of a mobile charger.
    pub(crate) fn at_source(&self, r: usize, source: &str) -> bool {
        let pos = self.robots[r].pos;
        if let Some(&st) = self.station_ix.get(source) {
            return self.stations[st].spec.position == pos;
        }
        match self.robot_ix.get(source) {
            Some(&c) if c != r && self.robots[c].capability(CapabilityName::Charge).is_some() => {
                distance_cells(self.robots[c].position_at(self.now), pos) <= REACH_CELLS
            }
            _ => false,
        }
    }

    fn other_robot(&self, r: usize, id: &str) -> Result<usize, Rejection> {
        match self.robot_ix.get(id) {
            None => reject(RejectReason::InvalidTarget, format!("no robot {id}")),
            Some(&t) if t == r => reject(
                RejectReason::InvalidTarget,
                format!("{id} cannot do this to itself"),
            ),
            Some(&t) => Ok(t),
        }
    }

    /// Repair targets: idle, stuck, failed or offline, with no command, not
    /// engaged, and with something to repair.
    fn repairable_now(&self, t: usize) -> Result<(), String> {
        let target = &self.robots[t];
        let status = target.reported_status();
        let id = &target.spec.robot_id;
        if !matches!(
            status,
            RobotStatus::Idle | RobotStatus::Stuck | RobotStatus::Failed | RobotStatus::Offline
        ) {
            return Err(format!("{id} is {}", status_name(status)));
        }
        if target.busy() {
            return Err(format!("{id} is busy"));
        }
        if !target.health.needs_repair() {
            return Err(format!("{id} has nothing to repair"));
        }
        Ok(())
    }

    /// Tow targets: failed, stuck or offline, with no command and not engaged.
    fn towable_now(&self, t: usize) -> Result<(), String> {
        let target = &self.robots[t];
        let status = target.reported_status();
        let id = &target.spec.robot_id;
        if !matches!(
            status,
            RobotStatus::Failed | RobotStatus::Stuck | RobotStatus::Offline
        ) {
            return Err(format!(
                "{id} is {}; only failed, stuck or offline robots can be towed",
                status_name(status)
            ));
        }
        if target.busy() {
            return Err(format!("{id} is busy"));
        }
        Ok(())
    }

    /// Cannibalize targets: a disabling fault or an empty battery, whatever
    /// the status shows; not lost, with no command and not engaged.
    fn strippable_now(&self, t: usize) -> Result<(), String> {
        let target = &self.robots[t];
        let id = &target.spec.robot_id;
        if target.lost {
            return Err(format!("{id} is lost"));
        }
        if !(target.health.has_disabled() || target.offline) {
            return Err(format!("{id} is neither failed nor offline"));
        }
        if target.busy() {
            return Err(format!("{id} is busy"));
        }
        Ok(())
    }

    pub(crate) fn validate_repair(&self, r: usize, target: &str) -> Result<(), Rejection> {
        let t = self.other_robot(r, target)?;
        self.repairable_now(t)
            .or_else(|why| reject(RejectReason::InvalidTarget, why))
    }

    pub(crate) fn validate_tow(&self, r: usize, target: &str) -> Result<(), Rejection> {
        let t = self.other_robot(r, target)?;
        self.towable_now(t)
            .or_else(|why| reject(RejectReason::InvalidTarget, why))
    }

    pub(crate) fn validate_cannibalize(
        &self,
        r: usize,
        target: &str,
        beneficiary: Option<&str>,
    ) -> Result<(), Rejection> {
        let t = self.other_robot(r, target)?;
        self.strippable_now(t)
            .or_else(|why| reject(RejectReason::InvalidTarget, why))?;
        if let Some(b) = beneficiary {
            match self.robot_ix.get(b) {
                None => return reject(RejectReason::InvalidTarget, format!("no robot {b}")),
                Some(&bi) if bi == t => {
                    return reject(
                        RejectReason::InvalidTarget,
                        "the beneficiary cannot be the robot being stripped",
                    );
                }
                Some(&bi) if self.robots[bi].lost => {
                    return reject(RejectReason::InvalidTarget, format!("{b} is lost"));
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(crate) fn validate_service(&self, depot: &str) -> Result<(), Rejection> {
        if self.depot_ix.contains_key(depot) {
            Ok(())
        } else {
            reject(RejectReason::InvalidTarget, format!("no depot {depot}"))
        }
    }

    // ----- execution -----

    /// On arrival: the robot the step is for must still exist and be within reach.
    fn within_reach(&self, r: usize, t: usize) -> Result<(), StepStart> {
        let target = &self.robots[t];
        let id = &target.spec.robot_id;
        if target.lost {
            return Err(StepStart::Failed(
                FailReason::TargetLost,
                format!("{id} was lost"),
            ));
        }
        if distance_cells(target.position_at(self.now), self.robots[r].pos) > REACH_CELLS {
            return Err(StepStart::Failed(
                FailReason::TargetLost,
                format!("{id} moved away"),
            ));
        }
        Ok(())
    }

    /// Work done with the repair capability: `units` of health fraction.
    fn maintenance_work(&mut self, r: usize, units: f64, engage: Option<usize>) -> Timed {
        let p = &self.params;
        let robot = &self.robots[r];
        let (rate, e_unit) = robot
            .capability(CapabilityName::Repair)
            .map_or((0.0, 0.0), |c| (c.rate, c.energy_wh_per_unit));
        let health = &robot.health;
        let (rate_f, work_ef, ef) = (
            health.work_rate_factor(p),
            health.work_energy_factor(p),
            health.energy_factor(p),
        );
        let (idle_w, min_s, sigma) = (robot.spec.idle_power_w, p.min_work_s, p.work_duration_sigma);
        let dust = self.dust_factor();
        let storm = self.env.storm_active(self.now);
        let d = (units / (rate * rate_f).max(1e-9) * self.rng_maintenance.lognormal_mean1(sigma))
            .max(min_s);
        let e = (units * e_unit * work_ef + d * idle_w / 3600.0) * ef;
        Timed {
            duration_s: d,
            energy_wh: e,
            amount: units,
            status: RobotStatus::Working,
            exposed: Some(Exposed::Work),
            wear: Exposure {
                km: 0.0,
                work_h: d / 3600.0,
                dust_h: d / 3600.0 * dust,
                kwh: e / 1000.0,
                storm,
            },
            engage,
        }
    }

    pub(crate) fn begin_repair(&mut self, r: usize, t: usize) -> Result<Timed, StepStart> {
        self.within_reach(r, t)?;
        self.repairable_now(t).map_err(blocked)?;
        let units = self.robots[t].health.repair_units(&self.params);
        Ok(self.maintenance_work(r, units, Some(t)))
    }

    pub(crate) fn begin_tow(
        &mut self,
        r: usize,
        t: usize,
        to: GridCoord,
    ) -> Result<Timed, StepStart> {
        self.within_reach(r, t)?;
        self.towable_now(t).map_err(blocked)?;
        let p = &self.params;
        let tower = &self.robots[r];
        let dist_m = distance_cells(self.robots[t].pos, to) * self.terrain.cell_size_m;
        let health = &tower.health;
        let speed = tower.spec.max_speed_m_s * health.speed_factor(p) * p.tow_speed_factor;
        let drive_wh = dist_m
            * tower.spec.drive_energy_wh_per_m
            * health.drive_energy_factor(p)
            * p.tow_energy_factor;
        let (idle_w, ef, sigma) = (
            tower.spec.idle_power_w,
            health.energy_factor(p),
            p.travel_duration_sigma,
        );
        let dust = self.dust_factor();
        let storm = self.env.storm_active(self.now);
        let d = dist_m / speed.max(1e-9) * self.rng_maintenance.lognormal_mean1(sigma);
        let e = (drive_wh + d * idle_w / 3600.0) * ef;
        Ok(Timed {
            duration_s: d,
            energy_wh: e,
            amount: 0.0,
            status: RobotStatus::Moving,
            exposed: Some(Exposed::Drive),
            wear: Exposure {
                km: dist_m / 1000.0,
                work_h: 0.0,
                dust_h: d / 3600.0 * dust,
                kwh: e / 1000.0,
                storm,
            },
            engage: Some(t),
        })
    }

    pub(crate) fn begin_strip(&mut self, r: usize, t: usize) -> Result<Timed, StepStart> {
        self.within_reach(r, t)?;
        self.strippable_now(t).map_err(blocked)?;
        let units = self.params.strip_units;
        Ok(self.maintenance_work(r, units, Some(t)))
    }

    pub(crate) fn begin_install(&mut self, r: usize, b: usize) -> Result<Timed, StepStart> {
        let units = self.params.install_units;
        if b == r {
            return Ok(self.maintenance_work(r, units, None));
        }
        self.within_reach(r, b)?;
        let beneficiary = &self.robots[b];
        if beneficiary.busy() {
            return Err(blocked(format!("{} is busy", beneficiary.spec.robot_id)));
        }
        Ok(self.maintenance_work(r, units, Some(b)))
    }

    /// Wait for a service bay, then be serviced. The robot draws no energy meanwhile.
    pub(crate) fn begin_service(&mut self, r: usize, depot: usize) -> StepStart {
        self.settle_drain(r);
        let robot = &mut self.robots[r];
        robot.drain_since = None;
        robot.status = RobotStatus::Working;
        self.set_running(
            r,
            super::robot::Step::Service { depot },
            None,
            0.0,
            0.0,
            Exposure::default(),
        );
        self.end_exposure(r);
        let d = &mut self.depots[depot];
        if d.occupants.len() < self.params.depot_capacity.max(1) as usize {
            self.start_service(r, depot);
        } else {
            d.waiting.push_back(r);
            self.unschedule(r);
        }
        StepStart::Waiting
    }

    fn start_service(&mut self, r: usize, depot: usize) {
        self.depots[depot].occupants.push(r);
        let (start, end) = (self.now, self.now + ms(self.params.service_s).max(1));
        if let Some(run) = self.robots[r]
            .activity
            .as_mut()
            .and_then(|a| a.running.as_mut())
        {
            run.start = start;
            run.end = Some(end);
        }
        self.schedule(r, end, |robot, generation| Ev::StepDone {
            robot,
            generation,
        });
    }

    /// Leave a depot's queue or bay, admitting the next robot in line.
    pub(crate) fn leave_depot(&mut self, r: usize, depot: usize) {
        let d = &mut self.depots[depot];
        let had_bay = d.occupants.contains(&r);
        d.occupants.retain(|&o| o != r);
        d.waiting.retain(|&w| w != r);
        if had_bay && let Some(next) = self.depots[depot].waiting.pop_front() {
            self.start_service(next, depot);
        }
    }

    /// Release a robot a step had engaged.
    pub(crate) fn release(&mut self, t: usize, by: usize) {
        if self.robots[t].engaged_by == Some(by) {
            self.robots[t].engaged_by = None;
        }
    }

    /// Change a robot's health, settling its idle drain first so energy use
    /// is accounted at the old rate, and re-predicting when it runs dry.
    pub(crate) fn change_health(&mut self, t: usize, change: impl FnOnce(&mut Health)) {
        let draining = self.robots[t].drain_since.is_some();
        self.settle_drain(t);
        change(&mut self.robots[t].health);
        if draining {
            self.schedule_depletion(t);
        }
    }

    // ----- effects -----

    pub(crate) fn finish_repair(&mut self, r: usize, t: usize) {
        let cut = self.params.repair_wear_cut;
        self.change_health(t, |h| {
            h.clear_field_repairable();
            h.cut_wear(cut);
            h.stuck = false;
        });
        self.release(t, r);
    }

    pub(crate) fn finish_tow(&mut self, r: usize, t: usize, to: GridCoord) {
        let tower = &mut self.robots[r];
        tower.pos = to;
        tower.travel = None;
        let target = &mut self.robots[t];
        target.pos = to;
        target.travel = None;
        target.towed = false;
        target.health.stuck = false;
        self.release(t, r);
    }

    pub(crate) fn finish_strip(&mut self, r: usize, t: usize) {
        self.settle_drain(t);
        let mut snapshot = self.truth_robots().swap_remove(t);
        snapshot.status = RobotStatus::Lost;
        self.release(t, r);
        let target = &mut self.robots[t];
        target.lost = true;
        target.drain_since = None;
        target.frozen = Some(snapshot);
        self.unschedule(t);
        self.end_exposure(t);
        self.robot_lost();
    }

    pub(crate) fn finish_install(&mut self, r: usize, b: usize) {
        self.change_health(b, |h| {
            h.clear_all_faults();
            h.reset_wear();
        });
        if b != r {
            self.release(b, r);
        }
    }

    pub(crate) fn finish_service(&mut self, r: usize, depot: usize) {
        self.change_health(r, |h| {
            h.clear_all_faults();
            h.reset_wear();
            h.stuck = false;
        });
        self.leave_depot(r, depot);
    }

    /// Work-content estimate for a field repair, from what the coordinator
    /// can see (faults and stuck), ignoring hidden wear.
    pub(crate) fn visible_repair_units(&self, t: usize) -> f64 {
        let h = &self.robots[t].health;
        let visible = Health {
            wear: [0.0; 6],
            ..h.clone()
        };
        visible.repair_units(&self.params)
    }

    /// Dust exposure weight right now: tau over clear tau.
    pub(crate) fn dust_factor(&self) -> f64 {
        self.env.tau(self.now) / self.env.clear_tau().max(1e-9)
    }
}
