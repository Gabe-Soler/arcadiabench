//! Wear, faults and health telemetry: the per-robot model behind semantics §6.
//!
//! Everything here is a pure function of a robot's hidden condition and the
//! parameters, so it is unit-testable without a simulator. The simulator
//! decides *when* things happen (`hazards.rs`); this module decides what
//! they do.

use super::params::Params;
use super::time::SimMs;
use arcadia_protocol::{ActiveFault, FaultCause, HealthTelemetry, Severity};

/// The v1 component vocabulary, in name order, which is also the order of a
/// robot's `faults` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Comp {
    Battery,
    DriveMotor,
    Joints,
    Sensors,
    Tool,
    Wheels,
}

impl Comp {
    pub const ALL: [Comp; 6] = [
        Comp::Battery,
        Comp::DriveMotor,
        Comp::Joints,
        Comp::Sensors,
        Comp::Tool,
        Comp::Wheels,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Comp::Battery => "battery",
            Comp::DriveMotor => "drive_motor",
            Comp::Joints => "joints",
            Comp::Sensors => "sensors",
            Comp::Tool => "tool",
            Comp::Wheels => "wheels",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_name(s: &str) -> Option<Comp> {
        Comp::ALL.into_iter().find(|c| c.name() == s)
    }
}

/// An active fault on one component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fault {
    pub severity: Severity,
    pub cause: FaultCause,
    pub since: SimMs,
    pub field_repairable: bool,
}

/// What a step exposed a robot to, for wear growth.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Exposure {
    /// Distance driven, km.
    pub km: f64,
    /// Hours of work (any work step, scans included).
    pub work_h: f64,
    /// Hours outdoors driving or working, weighted by tau / clear tau.
    pub dust_h: f64,
    /// Energy drawn from the battery, kWh.
    pub kwh: f64,
    /// A dust storm was active when the step started.
    pub storm: bool,
}

impl Exposure {
    /// The part of the exposure that happened before an interruption.
    pub fn scaled(self, f: f64) -> Self {
        Self {
            km: self.km * f,
            work_h: self.work_h * f,
            dust_h: self.dust_h * f,
            kwh: self.kwh * f,
            storm: self.storm,
        }
    }
}

/// Hidden condition of one robot: wear per component, active faults, and
/// whether it is stuck. Indexed by [`Comp::index`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Health {
    pub wear: [f64; 6],
    pub faults: [Option<Fault>; 6],
    pub stuck: bool,
}

impl Health {
    pub fn wear(&self, c: Comp) -> f64 {
        self.wear[c.index()]
    }

    pub fn fault(&self, c: Comp) -> Option<&Fault> {
        self.faults[c.index()].as_ref()
    }

    pub fn has_faults(&self) -> bool {
        self.faults.iter().any(Option::is_some)
    }

    pub fn has_disabled(&self) -> bool {
        self.faults
            .iter()
            .flatten()
            .any(|f| f.severity == Severity::Disabled)
    }

    /// Something a field repair could work on: stuck, a fault, or wear.
    pub fn needs_repair(&self) -> bool {
        self.stuck || self.has_faults() || self.wear.iter().any(|&w| w > 0.0)
    }

    /// Adds a fault under the one-active-fault-per-component rule: a degraded
    /// fault can only be replaced by a disabling one. Returns whether the new
    /// fault took effect.
    pub fn add_fault(&mut self, c: Comp, f: Fault) -> bool {
        let slot = &mut self.faults[c.index()];
        let takes = match slot {
            None => true,
            Some(old) => old.severity == Severity::Degraded && f.severity == Severity::Disabled,
        };
        if takes {
            *slot = Some(f);
        }
        takes
    }

    pub fn clear_field_repairable(&mut self) {
        for slot in &mut self.faults {
            if slot.is_some_and(|f| f.field_repairable) {
                *slot = None;
            }
        }
    }

    pub fn clear_all_faults(&mut self) {
        self.faults = [None; 6];
    }

    pub fn cut_wear(&mut self, fraction: f64) {
        for w in &mut self.wear {
            *w *= 1.0 - fraction;
        }
    }

    pub fn reset_wear(&mut self) {
        self.wear = [0.0; 6];
    }

    /// Grows wear for what a step exposed the robot to.
    pub fn grow(&mut self, p: &Params, e: &Exposure) {
        if !p.faults_enabled {
            return;
        }
        let k = if e.storm { p.storm_wear_factor } else { 1.0 };
        let mut add = |c: Comp, amount: f64| {
            let w = &mut self.wear[c.index()];
            *w = (*w + amount * k).clamp(0.0, 1.0);
        };
        add(Comp::Wheels, p.wear_wheels_per_km * e.km);
        add(Comp::DriveMotor, p.wear_drive_motor_per_km * e.km);
        add(Comp::Tool, p.wear_tool_per_work_h * e.work_h);
        add(
            Comp::Joints,
            p.wear_joints_per_work_h * e.work_h + p.wear_joints_per_dust_h * e.dust_h,
        );
        add(Comp::Sensors, p.wear_sensors_per_dust_h * e.dust_h);
        add(Comp::Battery, p.wear_battery_per_kwh * e.kwh);
    }

    /// Active faults in protocol form, ordered by component name.
    pub fn active_faults(&self) -> Vec<ActiveFault> {
        Comp::ALL
            .into_iter()
            .filter_map(|c| {
                self.fault(c).map(|f| ActiveFault {
                    component: c.name().into(),
                    severity: f.severity,
                    cause: f.cause,
                    since_t: super::time::secs(f.since),
                    field_repairable: f.field_repairable,
                })
            })
            .collect()
    }

    /// `factor` when the component has an active fault, else 1.
    fn penalty(&self, c: Comp, factor: f64) -> f64 {
        if self.fault(c).is_some() { factor } else { 1.0 }
    }

    /// Multiplier on drive speed.
    pub fn speed_factor(&self, p: &Params) -> f64 {
        (1.0 - p.wear_speed_loss * self.wear(Comp::Wheels))
            * self.penalty(Comp::Wheels, p.degraded_speed_factor)
    }

    /// Multiplier on drive energy (before the battery factor).
    pub fn drive_energy_factor(&self, p: &Params) -> f64 {
        (1.0 + p.wear_drive_energy_gain * self.wear(Comp::DriveMotor))
            * self.penalty(Comp::DriveMotor, p.degraded_drive_energy_factor)
    }

    /// Multiplier on the rate of work steps other than scans.
    pub fn work_rate_factor(&self, p: &Params) -> f64 {
        (1.0 - p.wear_work_rate_loss * self.wear(Comp::Joints))
            * (1.0 - p.wear_work_rate_loss * self.wear(Comp::Tool))
            * self.penalty(Comp::Joints, p.degraded_work_rate_factor)
            * self.penalty(Comp::Tool, p.degraded_work_rate_factor)
    }

    /// Multiplier on the energy of work steps other than scans (before the battery factor).
    pub fn work_energy_factor(&self, p: &Params) -> f64 {
        (1.0 + p.wear_work_energy_gain * self.wear(Comp::Tool))
            * self.penalty(Comp::Tool, p.degraded_work_energy_factor)
    }

    /// Multiplier on scan rate.
    pub fn scan_rate_factor(&self, p: &Params) -> f64 {
        (1.0 - p.wear_scan_rate_loss * self.wear(Comp::Sensors))
            * self.penalty(Comp::Sensors, p.degraded_scan_rate_factor)
    }

    /// Multiplier on every energy draw from the battery.
    pub fn energy_factor(&self, p: &Params) -> f64 {
        (1.0 + p.wear_energy_gain * self.wear(Comp::Battery))
            * self.penalty(Comp::Battery, p.degraded_energy_factor)
    }

    /// Wear hazard per component, per exposed hour.
    pub fn wear_hazards_per_h(&self, p: &Params) -> [f64; 6] {
        let mut out = [0.0; 6];
        if p.faults_enabled {
            for c in Comp::ALL {
                out[c.index()] =
                    p.wear_hazard_per_h * libm::pow(self.wear(c), p.wear_hazard_exponent);
            }
        }
        out
    }

    /// Work content of a field repair, in health-fraction units.
    pub fn repair_units(&self, p: &Params) -> f64 {
        let faults: f64 = self
            .faults
            .iter()
            .flatten()
            .filter(|f| f.field_repairable)
            .map(|f| match f.severity {
                Severity::Degraded => p.repair_units_degraded,
                Severity::Disabled => p.repair_units_disabled,
            })
            .sum();
        let stuck = if self.stuck {
            p.repair_units_stuck
        } else {
            0.0
        };
        let wear: f64 = self.wear.iter().sum::<f64>() * p.repair_wear_cut;
        faults + stuck + p.repair_units_per_wear * wear
    }

    /// Noise-free health telemetry.
    pub fn telemetry(&self, p: &Params, ambient_c: f64, heater_on: bool) -> HealthTelemetry {
        let slip = p.slip_base
            + p.slip_per_wear * self.wear(Comp::Wheels)
            + if self.fault(Comp::Wheels).is_some() {
                p.slip_fault
            } else {
                0.0
            };
        let current = 1.0
            + p.current_per_wear * self.wear(Comp::DriveMotor)
            + if self.fault(Comp::DriveMotor).is_some() {
                p.current_fault
            } else {
                0.0
            };
        let dust = p.dust_per_joint_wear * self.wear(Comp::Joints)
            + p.dust_per_sensor_wear * self.wear(Comp::Sensors);
        HealthTelemetry {
            wheel_slip: slip.clamp(0.0, 1.0),
            motor_current_ratio: current.max(0.0),
            dust_coverage: dust.clamp(0.0, 1.0),
            temperature_c: ambient_c + if heater_on { p.heater_offset_c } else { 0.0 },
        }
    }
}

/// How soft a cell is for getting stuck: 0 on firm ground, rising with
/// regolith depth and falling with cohesion.
pub fn softness(p: &Params, depth_m: f64, cohesion_kpa: f64) -> f64 {
    let depth = ((depth_m - p.soft_depth_m) / p.soft_depth_span_m).clamp(0.0, 1.0);
    let cohesion = (p.soft_cohesion_kpa / cohesion_kpa.max(1e-6)).min(p.soft_cohesion_cap);
    depth * cohesion
}

/// Seconds until the next event of a Poisson process with `rate_per_h`, from
/// a uniform draw in [0, 1).
pub fn exp_wait_s(rate_per_h: f64, u: f64) -> f64 {
    -libm::log(1.0 - u) * 3600.0 / rate_per_h
}

/// Cumulative hazard at which an event fires, from a uniform draw in [0, 1).
pub fn exp_threshold(u: f64) -> f64 {
    -libm::log(1.0 - u)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fault(severity: Severity, field_repairable: bool) -> Fault {
        Fault {
            severity,
            cause: FaultCause::Random,
            since: 1000,
            field_repairable,
        }
    }

    #[test]
    fn names_are_the_protocol_vocabulary_in_order() {
        let names: Vec<&str> = Comp::ALL.iter().map(|c| c.name()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "faults are listed in name order");
        let mut vocab = arcadia_protocol::COMPONENTS.to_vec();
        vocab.sort();
        assert_eq!(names, vocab);
        for c in Comp::ALL {
            assert_eq!(Comp::from_name(c.name()), Some(c));
        }
    }

    #[test]
    fn one_fault_per_component_and_escalation_only() {
        let mut h = Health::default();
        assert!(h.add_fault(Comp::Wheels, fault(Severity::Degraded, true)));
        assert!(!h.add_fault(Comp::Wheels, fault(Severity::Degraded, true)));
        assert!(h.add_fault(Comp::Wheels, fault(Severity::Disabled, false)));
        assert!(!h.add_fault(Comp::Wheels, fault(Severity::Degraded, true)));
        assert!(!h.add_fault(Comp::Wheels, fault(Severity::Disabled, true)));
        assert!(h.add_fault(Comp::Battery, fault(Severity::Degraded, true)));
        let listed: Vec<String> = h.active_faults().into_iter().map(|f| f.component).collect();
        assert_eq!(listed, ["battery", "wheels"]);
        assert!(h.has_disabled());
        h.clear_field_repairable();
        assert_eq!(
            h.active_faults().len(),
            1,
            "the depot-only wheels fault stays"
        );
        h.clear_all_faults();
        assert!(!h.has_faults());
    }

    #[test]
    fn wear_grows_with_exposure_and_faster_in_storms() {
        let p = Params::default();
        let e = Exposure {
            km: 1.0,
            work_h: 2.0,
            dust_h: 3.0,
            kwh: 4.0,
            storm: false,
        };
        let mut clear = Health::default();
        clear.grow(&p, &e);
        assert!((clear.wear(Comp::Wheels) - p.wear_wheels_per_km).abs() < 1e-12);
        assert!((clear.wear(Comp::Tool) - 2.0 * p.wear_tool_per_work_h).abs() < 1e-12);
        let joints = 2.0 * p.wear_joints_per_work_h + 3.0 * p.wear_joints_per_dust_h;
        assert!((clear.wear(Comp::Joints) - joints).abs() < 1e-12);
        let mut storm = Health::default();
        storm.grow(&p, &Exposure { storm: true, ..e });
        for c in Comp::ALL {
            assert!(storm.wear(c) > clear.wear(c), "{c:?}");
        }
        let mut off = Health::default();
        off.grow(
            &Params {
                faults_enabled: false,
                ..p.clone()
            },
            &e,
        );
        assert_eq!(off, Health::default());
        let mut big = Health::default();
        big.grow(&p, &Exposure { km: 1e6, ..e });
        assert_eq!(big.wear(Comp::Wheels), 1.0, "wear is capped at 1");
    }

    #[test]
    fn worn_and_faulty_robots_are_slower_and_hungrier() {
        let p = Params::default();
        let fresh = Health::default();
        for f in [
            fresh.speed_factor(&p),
            fresh.drive_energy_factor(&p),
            fresh.work_rate_factor(&p),
            fresh.work_energy_factor(&p),
            fresh.scan_rate_factor(&p),
            fresh.energy_factor(&p),
        ] {
            assert_eq!(f, 1.0);
        }
        let worn = Health {
            wear: [0.5; 6],
            ..Health::default()
        };
        assert!(worn.speed_factor(&p) < 1.0);
        assert!(worn.drive_energy_factor(&p) > 1.0);
        assert!(worn.work_rate_factor(&p) < 1.0);
        assert!(worn.work_energy_factor(&p) > 1.0);
        assert!(worn.scan_rate_factor(&p) < 1.0);
        assert!(worn.energy_factor(&p) > 1.0);
        let mut faulty = worn.clone();
        faulty.add_fault(Comp::Wheels, fault(Severity::Degraded, true));
        assert!(faulty.speed_factor(&p) < worn.speed_factor(&p));
        // Modest ranges: even fully worn with a degraded fault, a robot keeps moving.
        let wrecked = Health {
            wear: [1.0; 6],
            faults: [Some(fault(Severity::Degraded, true)); 6],
            stuck: false,
        };
        assert!(wrecked.speed_factor(&p) > 0.4);
        assert!(wrecked.work_rate_factor(&p) > 0.3);
    }

    #[test]
    fn wear_hazard_rises_steeply_with_wear() {
        let p = Params::default();
        let at = |w: f64| {
            Health {
                wear: [w; 6],
                ..Health::default()
            }
            .wear_hazards_per_h(&p)[0]
        };
        assert_eq!(at(0.0), 0.0);
        assert!(at(0.2) < at(0.5) && at(0.5) < at(0.9));
        assert!(at(0.9) / at(0.3) > 20.0);
        let off = Params {
            faults_enabled: false,
            ..p.clone()
        };
        assert_eq!(
            Health {
                wear: [1.0; 6],
                ..Health::default()
            }
            .wear_hazards_per_h(&off),
            [0.0; 6]
        );
    }

    #[test]
    fn repair_work_counts_repairable_faults_stuck_and_wear() {
        let p = Params::default();
        let mut h = Health::default();
        assert!(!h.needs_repair());
        assert_eq!(h.repair_units(&p), 0.0);
        h.add_fault(Comp::Tool, fault(Severity::Disabled, true));
        h.add_fault(Comp::Wheels, fault(Severity::Disabled, false));
        h.stuck = true;
        h.wear = [0.2; 6];
        let expected = p.repair_units_disabled
            + p.repair_units_stuck
            + p.repair_units_per_wear * 1.2 * p.repair_wear_cut;
        assert!((h.repair_units(&p) - expected).abs() < 1e-12);
        assert!(h.needs_repair());
    }

    #[test]
    fn telemetry_tracks_wear_faults_and_the_heater() {
        let p = Params::default();
        let fresh = Health::default().telemetry(&p, -70.0, false);
        assert_eq!(fresh.wheel_slip, p.slip_base);
        assert_eq!(fresh.motor_current_ratio, 1.0);
        assert_eq!(fresh.dust_coverage, 0.0);
        assert_eq!(fresh.temperature_c, -70.0);
        let mut worn = Health {
            wear: [0.6; 6],
            ..Health::default()
        };
        let t = worn.telemetry(&p, -70.0, true);
        assert!(t.wheel_slip > fresh.wheel_slip);
        assert!(t.motor_current_ratio > 1.0);
        assert!(t.dust_coverage > 0.0);
        assert_eq!(t.temperature_c, -70.0 + p.heater_offset_c);
        worn.add_fault(Comp::Wheels, fault(Severity::Degraded, true));
        assert!(worn.telemetry(&p, -70.0, true).wheel_slip > t.wheel_slip);
    }

    #[test]
    fn soft_ground_is_deep_and_loose() {
        let p = Params::default();
        assert_eq!(softness(&p, 0.5, 1.0), 0.0);
        assert!(softness(&p, 1.4, 0.6) > softness(&p, 1.4, 2.5));
        assert!(softness(&p, 1.4, 1.0) > softness(&p, 1.0, 1.0));
        assert!(softness(&p, 50.0, 0.01) <= p.soft_cohesion_cap);
    }

    #[test]
    fn exponential_waits_have_the_right_mean() {
        let mut rng = super::super::rng::Rng::new(3);
        let n = 20_000;
        let mean = (0..n).map(|_| exp_wait_s(2.0, rng.uniform())).sum::<f64>() / n as f64;
        assert!((mean - 1800.0).abs() < 40.0, "{mean}");
    }
}
