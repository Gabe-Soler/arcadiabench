//! When robots fail (semantics §6.4): exposure, the failure processes, and
//! what happens when one fires.
//!
//! Each process draws from its own named RNG stream. When a robot starts an
//! exposed step, every process samples the time of its next event from the
//! hazard at that moment; an event that would land after the step ends is
//! never scheduled. A new exposure bumps the robot's hazard generation, so
//! events left over from an earlier step are ignored when they fire.

use super::faults::{Comp, Fault, exp_threshold, exp_wait_s, softness};
use super::params::Incident;
use super::robot::Step;
use super::terrain::distance_cells;
use super::time::{SimMs, ms};
use super::{Ev, Sim};
use arcadia_protocol::*;

/// The failure processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HazardKind {
    /// A component fails at a rate that rises with its wear.
    Wear,
    /// Any component fails at a constant base rate.
    Random,
    /// Dust knocks out the sensors of a robot scanning or driving in a storm.
    Storm,
    /// The robot gets stuck in soft regolith on a drive.
    Stuck,
}

/// What exposes a robot to failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exposed {
    Drive,
    Scan,
    Work,
    Hold,
}

impl Sim {
    /// What the robot's running step exposes it to, and until when.
    fn exposure_of(&self, r: usize) -> Option<(Exposed, SimMs)> {
        let running = self.robots[r].activity.as_ref()?.running.as_ref()?;
        let kind = match running.step {
            Step::Travel(_) | Step::Tow { .. } => Exposed::Drive,
            Step::Scan(_) => Exposed::Scan,
            Step::Dig { .. }
            | Step::Grade { .. }
            | Step::Compact(_)
            | Step::Sinter(_)
            | Step::Load { .. }
            | Step::Dump(_)
            | Step::Repair { .. }
            | Step::Strip { .. }
            | Step::Install { .. } => Exposed::Work,
            Step::Hold => Exposed::Hold,
            _ => return None,
        };
        Some((kind, running.end.unwrap_or(self.time_limit)))
    }

    /// A robot starts an exposed step lasting until `end` (open-ended for a
    /// hold). Schedules the first event of each failure process that falls
    /// inside it; `path` is the drive's start and end cells.
    pub(crate) fn start_exposure(
        &mut self,
        r: usize,
        kind: Exposed,
        end: Option<SimMs>,
        path: Option<(GridCoord, GridCoord)>,
    ) {
        self.end_exposure(r);
        if !self.params.faults_enabled {
            return;
        }
        let end = end.unwrap_or(self.time_limit);
        for process in [HazardKind::Wear, HazardKind::Random, HazardKind::Storm] {
            self.schedule_hazard(r, process, kind, end);
        }
        if let Some((from, to)) = path {
            self.schedule_stuck(r, from, to, end);
        }
    }

    /// The robot's exposure ended: pending hazard events become stale.
    pub(crate) fn end_exposure(&mut self, r: usize) {
        self.robots[r].hazard_generation += 1;
    }

    fn hazard_rate_per_h(&self, r: usize, process: HazardKind, kind: Exposed) -> f64 {
        let p = &self.params;
        match process {
            HazardKind::Wear => self.robots[r].health.wear_hazards_per_h(p).iter().sum(),
            HazardKind::Random => p.random_fault_per_h,
            HazardKind::Storm => {
                if matches!(kind, Exposed::Drive | Exposed::Scan) && self.env.storm_active(self.now)
                {
                    p.storm_sensor_fault_per_h
                        * (self.env.tau(self.now) - self.env.clear_tau()).max(0.0)
                } else {
                    0.0
                }
            }
            HazardKind::Stuck => 0.0,
        }
    }

    fn rng_for(&mut self, process: HazardKind) -> &mut super::rng::Rng {
        match process {
            HazardKind::Wear => &mut self.rng_wear_faults,
            HazardKind::Random => &mut self.rng_random_faults,
            HazardKind::Storm => &mut self.rng_storm_faults,
            HazardKind::Stuck => &mut self.rng_stuck,
        }
    }

    fn schedule_hazard(&mut self, r: usize, process: HazardKind, kind: Exposed, end: SimMs) {
        let rate = self.hazard_rate_per_h(r, process, kind);
        if rate <= 0.0 {
            return;
        }
        let u = self.rng_for(process).uniform();
        let at = self.now.saturating_add(ms(exp_wait_s(rate, u)).max(1));
        if at < end {
            let generation = self.robots[r].hazard_generation;
            self.queue.push(
                at,
                Ev::Hazard {
                    robot: r,
                    generation,
                    kind: process,
                },
            );
        }
    }

    /// Walk the drive cell by cell, accumulating the stuck hazard of the
    /// ground under it; the robot gets stuck where it passes a threshold
    /// drawn once per drive.
    fn schedule_stuck(&mut self, r: usize, from: GridCoord, to: GridCoord, end: SimMs) {
        let cells = distance_cells(from, to);
        let span = end.saturating_sub(self.now);
        if self.params.stuck_per_km <= 0.0 || cells <= 0.0 || span < 2 {
            return;
        }
        let threshold = exp_threshold(self.rng_stuck.uniform());
        let p = &self.params;
        let n = cells.ceil() as usize;
        let step_km = cells * self.terrain.cell_size_m / n as f64 / 1000.0;
        let lerp = |a: u32, b: u32, f: f64| {
            (f64::from(a) + (f64::from(b) - f64::from(a)) * f).round() as u32
        };
        let mut hazard = 0.0;
        for i in 0..n {
            let f = (i as f64 + 0.5) / n as f64;
            let cell = self.terrain.get(GridCoord {
                x: lerp(from.x, to.x, f),
                y: lerp(from.y, to.y, f),
            });
            // Compacted and stabilized ground is firm.
            let soft = if matches!(cell.surface, Surface::Compacted | Surface::Stabilized) {
                0.0
            } else {
                softness(p, cell.regolith_depth_m, cell.cohesion_kpa)
            };
            hazard += p.stuck_per_km * soft * step_km;
            if hazard >= threshold {
                // Drives move at a constant speed, so the robot reaches this point at fraction f.
                let at = (self.now + (span as f64 * f) as SimMs).clamp(self.now + 1, end - 1);
                let generation = self.robots[r].hazard_generation;
                self.queue.push(
                    at,
                    Ev::Hazard {
                        robot: r,
                        generation,
                        kind: HazardKind::Stuck,
                    },
                );
                return;
            }
        }
    }

    /// A failure process fired for a robot that is still exposed.
    pub(crate) fn on_hazard(&mut self, r: usize, process: HazardKind) {
        if process == HazardKind::Stuck {
            self.get_stuck(r);
            return;
        }
        let Some((kind, end)) = self.exposure_of(r) else {
            return;
        };
        let rates = self.robots[r].health.wear_hazards_per_h(&self.params);
        let (p_disabled, p_repairable) = (
            match process {
                HazardKind::Wear => self.params.p_disabled_wear,
                HazardKind::Random => self.params.p_disabled_random,
                _ => self.params.p_disabled_environmental,
            },
            self.params.p_field_repairable,
        );
        let rng = self.rng_for(process);
        let (u_comp, disabled, repairable) = (
            rng.uniform(),
            rng.chance(p_disabled),
            rng.chance(p_repairable),
        );
        let (component, cause) = match process {
            HazardKind::Wear => {
                let total: f64 = rates.iter().sum();
                let mut pick = u_comp * total;
                let mut chosen = Comp::ALL[0];
                for c in Comp::ALL {
                    chosen = c;
                    if pick < rates[c.index()] {
                        break;
                    }
                    pick -= rates[c.index()];
                }
                (chosen, FaultCause::Wear)
            }
            HazardKind::Random => (
                Comp::ALL[((u_comp * Comp::ALL.len() as f64) as usize).min(Comp::ALL.len() - 1)],
                FaultCause::Random,
            ),
            _ => (Comp::Sensors, FaultCause::Environmental),
        };
        let severity = if disabled {
            Severity::Disabled
        } else {
            Severity::Degraded
        };
        let generation = self.robots[r].hazard_generation;
        self.apply_fault(r, component, severity, cause, !disabled || repairable);
        // Still exposed (a degraded fault, or none took effect): the process
        // keeps running for the rest of the step.
        if self.robots[r].hazard_generation == generation {
            self.schedule_hazard(r, process, kind, end);
        }
    }

    /// Give a robot a fault, subject to the one-fault-per-component rule.
    /// A disabling fault stops the robot's command. Returns whether the
    /// fault took effect.
    pub(crate) fn apply_fault(
        &mut self,
        r: usize,
        component: Comp,
        severity: Severity,
        cause: FaultCause,
        field_repairable: bool,
    ) -> bool {
        if self.robots[r].lost {
            return false;
        }
        let fault = Fault {
            severity,
            cause,
            since: self.now,
            field_repairable,
        };
        if !self.robots[r].health.add_fault(component, fault) {
            return false;
        }
        self.tally.faults += 1;
        self.incident();
        let robot_id = self.robots[r].spec.robot_id.clone();
        self.emit(
            EventKind::RobotFault {
                robot_id,
                cause,
                component: component.name().into(),
                severity,
                field_repairable,
                detail: None,
            },
            true,
        );
        if severity == Severity::Disabled && self.robots[r].activity.is_some() {
            self.fail_command(
                r,
                FailReason::Fault,
                Some(format!("{} disabled", component.name())),
            );
        }
        true
    }

    /// The robot is immobilized where it is: a decision point, unless the
    /// robot is out of coverage and nobody can hear of it yet.
    pub(crate) fn get_stuck(&mut self, r: usize) {
        if self.robots[r].lost {
            return;
        }
        let at = self.robots[r].position_at(self.now);
        self.robots[r].health.stuck = true;
        self.incident();
        if self.robots[r].activity.is_some() {
            self.fail_command(
                r,
                FailReason::Stuck,
                Some(format!("soft regolith at ({}, {})", at.x, at.y)),
            );
        } else {
            self.robot_changed(r);
        }
    }

    pub(crate) fn on_scripted(&mut self, i: usize) {
        let s = self.params.scripted[i].clone();
        let Some(&r) = self.robot_ix.get(&s.robot_id) else {
            return;
        };
        match s.incident {
            Incident::Fault {
                component,
                severity,
                cause,
                field_repairable,
            } => {
                self.apply_fault(r, component, severity, cause, field_repairable);
            }
            Incident::Stuck => self.get_stuck(r),
        }
    }
}
