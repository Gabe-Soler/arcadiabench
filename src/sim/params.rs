//! Every simulation tunable in one place.
//!
//! These are placeholder values chosen to be physically plausible, to make a
//! 20 m pad buildable in a few sols, and to make faults matter without
//! crippling a crew. `docs/simulator.md` documents each one with its rationale
//! and the functional forms that use it. Sourcing realistic numbers is an open
//! question in the project outline. TODO(post-v1.0): cite a source for each field.

use super::faults::Comp;
use arcadia_protocol::{FaultCause, PowerMode, Severity};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    // --- the scenario catalog ---
    /// The crew and the outpost it works from (a Standard track preset).
    pub preset: Preset,
    /// The catalog variant these parameters were built from. It names the
    /// scenario (`landing-pad-<preset>-<variant>`); a parameter set edited by
    /// hand keeps the name of the variant it started from.
    pub variant: Variant,

    // --- terrain (Arcadia Planitia: smooth, low rock abundance) ---
    /// Mean surface elevation of the site, m (MOLA datum).
    pub base_elevation_m: f64,
    /// Std-dev of the regional tilt, m per m.
    pub tilt_sigma: f64,
    /// Value-noise octaves: (lattice spacing in cells, amplitude in m).
    pub noise_octaves: [(f64, f64); 3],
    /// Range of loose regolith depth, m.
    pub regolith_depth_m: (f64, f64),
    /// Range of regolith bulk density, kg/m³.
    pub regolith_density_kg_m3: (f64, f64),
    /// Range of regolith cohesion, kPa.
    pub regolith_cohesion_kpa: (f64, f64),
    /// Mean boulders per cell.
    pub boulders_per_cell: f64,
    /// Power-law boulder diameters: d = d_min * u^(-1/alpha), capped at d_max.
    pub boulder_d_min_m: f64,
    /// Largest boulder diameter, m.
    pub boulder_d_max_m: f64,
    /// Power-law exponent of the boulder size distribution.
    pub boulder_alpha: f64,
    /// Boulder rock density, kg/m³.
    pub rock_density_kg_m3: f64,
    /// Std-dev of the orbital prior's elevation error, m.
    pub orbital_elevation_noise_m: f64,

    // --- actions ---
    /// Log-sd of multiplicative noise on work durations.
    pub work_duration_sigma: f64,
    /// Log-sd of multiplicative noise on travel durations.
    pub travel_duration_sigma: f64,
    /// Floor on any per-cell work step or scan, s.
    pub min_work_s: f64,
    /// Std-dev of where a dig cut ends relative to its target, m.
    pub dig_cut_noise_m: f64,
    /// Grade only does fine levelling within this distance of the target, m.
    pub grade_max_delta_m: f64,
    /// Std-dev of graded elevation around the target, m.
    pub grade_noise_m: f64,
    /// Mean settlement when a cell is compacted, m.
    pub compact_settle_m: f64,
    /// Std-dev of compaction settlement, m.
    pub compact_settle_noise_m: f64,

    // --- sensing ---
    /// Std-dev of scanned elevation, m.
    pub scan_elevation_noise_m: f64,
    /// Std-dev of scanned regolith depth, m.
    pub scan_depth_noise_m: f64,
    /// Relative std-dev of scanned bulk density.
    pub scan_density_noise_frac: f64,
    /// Smallest boulder a scan detects, m.
    pub scan_min_boulder_m: f64,
    /// Relative std-dev of scanned boulder diameters.
    pub scan_boulder_noise_frac: f64,
    /// Battery telemetry noise as a fraction of capacity.
    pub telemetry_battery_noise_frac: f64,

    // --- site power ---
    /// Power architecture reported in `Scenario.power.mode`.
    pub power_mode: PowerMode,
    /// Constant fission output, W.
    pub fission_w: f64,
    /// Clear-sky noon solar output, W.
    pub solar_peak_w: f64,
    /// Always-on site load, served before any charging, W.
    pub essential_load_w: f64,
    /// Site battery bank capacity, Wh.
    pub bank_capacity_wh: f64,
    /// Bank charge at the start of the run, as a fraction of capacity.
    pub bank_initial_frac: f64,
    /// Bank-alert threshold, as a fraction of capacity.
    pub bank_low_frac: f64,
    /// Internal power-accounting step, s (not a decision point).
    pub power_tick_s: f64,

    // --- environment ---
    /// Clear-sky dust optical depth (tau).
    pub clear_tau: f64,
    /// Chance a dust storm starts in a given sol (when none is raging).
    pub storm_chance_per_sol: f64,
    /// Range of storm length, sols.
    pub storm_sols: (f64, f64),
    /// Range of storm peak optical depth.
    pub storm_peak_tau: (f64, f64),
    /// Solar transmittance = exp(-extinction * (tau - clear_tau)).
    pub dust_extinction: f64,
    /// The run contains a guaranteed regional dust storm early on (the
    /// `dust_storm` variant), drawn from the `dust_storm` stream. Seasonal
    /// storms that would overlap it are dropped.
    pub dust_storm: bool,
    /// Range of when that storm's front arrives, sols into the run (uniform).
    pub dust_storm_start_sols: (f64, f64),
    /// Time from the front's arrival to peak opacity, sols.
    pub dust_storm_rise_sols: f64,
    /// Range of that storm's length, sols.
    pub dust_storm_sols: (f64, f64),
    /// Range of that storm's peak optical depth.
    pub dust_storm_peak_tau: (f64, f64),

    // --- Earth link (one-way delay geometry, outages, conjunction) ---
    /// Shortest one-way light time, at opposition, s.
    pub delay_min_s: f64,
    /// Longest one-way light time, at solar conjunction, s.
    pub delay_max_s: f64,
    /// Earth–Mars synodic period, sols.
    pub synodic_period_sols: f64,
    /// One-way delay at the start of the run, s, with Mars receding (the
    /// delay growing). Ignored when the run contains a conjunction.
    pub start_delay_s: f64,
    /// The run contains a solar conjunction: a blackout, and a delay near its maximum.
    pub conjunction: bool,
    /// Range of the blackout's start, sols into the run (uniform, from the `comms` stream).
    pub conjunction_start_sols: (f64, f64),
    /// Length of the conjunction blackout, sols.
    pub conjunction_sols: f64,
    /// Rate of unplanned link outages while the link is up, per sol (Poisson).
    pub outage_per_sol: f64,
    /// Range of an unplanned outage's length, hours (uniform).
    pub outage_hours: (f64, f64),

    // --- site comms (coverage and relays) ---
    /// Coverage radius of the base station, m.
    pub base_station_range_m: f64,
    /// Radio power a robot draws while holding as an active relay, W (added to idle drain).
    pub relay_radio_w: f64,
    /// An idle robot out of coverage this long drives back toward the base station, s.
    pub lost_link_timeout_s: f64,

    // --- scripted mission control ---
    /// Mean human response time to an approval request, s (lognormal).
    pub earth_response_mean_s: f64,
    /// Log-sd of the human response time.
    pub earth_response_sigma: f64,
    /// Chance Earth approves a request for an action without its own rule.
    pub earth_approve_p: f64,
    /// Chance Earth approves `cannibalize`.
    pub earth_approve_cannibalize_p: f64,
    /// Chance Earth approves `abandon_section`.
    pub earth_approve_abandon_p: f64,
    /// Earth approves `commit_energy_reserve` (with `earth_approve_p`) only if site
    /// storage was at or below this fraction of capacity when the request was sent.
    pub earth_reserve_low_frac: f64,
    /// Incident-free sols after which Earth loosens level-2 actions to level 1.
    pub trust_incident_free_sols: f64,
    /// Time Earth takes to issue a policy update once it learns what prompts it, s.
    pub policy_reaction_s: f64,

    // --- wear: growth (wear is in [0, 1] per component) ---
    /// Master switch for wear growth and every random failure process (wear,
    /// random, storm and getting stuck). Scripted incidents happen either way.
    pub faults_enabled: bool,
    /// `wheels` wear per km driven.
    pub wear_wheels_per_km: f64,
    /// `drive_motor` wear per km driven.
    pub wear_drive_motor_per_km: f64,
    /// `tool` wear per hour of work.
    pub wear_tool_per_work_h: f64,
    /// `joints` wear per hour of work.
    pub wear_joints_per_work_h: f64,
    /// `joints` wear per hour of dust exposure (driving or working, scaled by tau / clear tau).
    pub wear_joints_per_dust_h: f64,
    /// `sensors` wear per hour of dust exposure (scaled like joints).
    pub wear_sensors_per_dust_h: f64,
    /// `battery` wear per kWh drawn from the battery.
    pub wear_battery_per_kwh: f64,
    /// Multiplier on all wear growth while a dust storm is active.
    pub storm_wear_factor: f64,

    // --- wear: effects (fractional change at wear 1.0, linear in wear) ---
    /// Drive speed lost to `wheels` wear.
    pub wear_speed_loss: f64,
    /// Extra drive energy from `drive_motor` wear.
    pub wear_drive_energy_gain: f64,
    /// Work rate lost to `joints` wear, and again to `tool` wear.
    pub wear_work_rate_loss: f64,
    /// Extra work energy from `tool` wear.
    pub wear_work_energy_gain: f64,
    /// Scan rate lost to `sensors` wear.
    pub wear_scan_rate_loss: f64,
    /// Extra energy for everything from `battery` wear.
    pub wear_energy_gain: f64,

    // --- degraded faults: multipliers while the fault is active ---
    /// Drive speed with a degraded `wheels` fault.
    pub degraded_speed_factor: f64,
    /// Drive energy with a degraded `drive_motor` fault.
    pub degraded_drive_energy_factor: f64,
    /// Work rate with a degraded `joints` or `tool` fault (each).
    pub degraded_work_rate_factor: f64,
    /// Work energy with a degraded `tool` fault.
    pub degraded_work_energy_factor: f64,
    /// Scan rate with a degraded `sensors` fault.
    pub degraded_scan_rate_factor: f64,
    /// Energy for everything with a degraded `battery` fault.
    pub degraded_energy_factor: f64,

    // --- failure processes (hazards apply only while a robot is exposed) ---
    /// Wear hazard scale: a component fails at `wear_hazard_per_h * wear^wear_hazard_exponent` per hour.
    pub wear_hazard_per_h: f64,
    /// Exponent of the wear hazard.
    pub wear_hazard_exponent: f64,
    /// Constant random hazard per robot, per exposed hour.
    pub random_fault_per_h: f64,
    /// `sensors` hazard while scanning or driving in a storm, per hour per unit of tau above clear.
    pub storm_sensor_fault_per_h: f64,
    /// Chance of getting stuck per km driven over the softest ground.
    pub stuck_per_km: f64,
    /// Regolith depth at which ground starts to be soft, m.
    pub soft_depth_m: f64,
    /// Extra depth over which softness ramps from 0 to 1, m.
    pub soft_depth_span_m: f64,
    /// Reference cohesion, kPa: softness scales with `soft_cohesion_kpa / cohesion`.
    pub soft_cohesion_kpa: f64,
    /// Cap on the cohesion factor of softness.
    pub soft_cohesion_cap: f64,
    /// Chance a wear fault is disabling.
    pub p_disabled_wear: f64,
    /// Chance a random fault is disabling.
    pub p_disabled_random: f64,
    /// Chance an environmental (storm) fault is disabling.
    pub p_disabled_environmental: f64,
    /// Chance a disabling fault can be repaired in the field.
    pub p_field_repairable: f64,

    // --- repair, tow, cannibalize, service ---
    /// Repair work, in health-fraction units, per degraded field-repairable fault.
    pub repair_units_degraded: f64,
    /// Repair work per disabling field-repairable fault.
    pub repair_units_disabled: f64,
    /// Repair work to free a stuck robot.
    pub repair_units_stuck: f64,
    /// Repair work per unit of wear removed.
    pub repair_units_per_wear: f64,
    /// Fraction of each component's wear a field repair removes.
    pub repair_wear_cut: f64,
    /// Tow speed as a fraction of the tower's speed.
    pub tow_speed_factor: f64,
    /// Tow drive energy as a multiple of the tower's normal drive energy.
    pub tow_energy_factor: f64,
    /// Work to strip a robot for parts, in repair units.
    pub strip_units: f64,
    /// Work to install cannibalized parts, in repair units.
    pub install_units: f64,
    /// Depot service duration, s.
    pub service_s: f64,
    /// Robots a depot services at once; the rest wait first come, first served.
    pub depot_capacity: u32,

    // --- cold nights ---
    /// Ambient temperature below which robot heaters run, °C.
    pub heater_threshold_c: f64,
    /// Heater draw as a multiple of the robot's idle power (added to idle drain).
    pub heater_idle_factor: f64,
    /// How much warmer a heated robot reads than ambient, °C.
    pub heater_offset_c: f64,

    // --- health telemetry (noise-free model, then Gaussian noise in observations) ---
    /// `wheel_slip` of a new robot.
    pub slip_base: f64,
    /// `wheel_slip` added per unit of `wheels` wear.
    pub slip_per_wear: f64,
    /// `wheel_slip` added by an active `wheels` fault.
    pub slip_fault: f64,
    /// `motor_current_ratio` added per unit of `drive_motor` wear.
    pub current_per_wear: f64,
    /// `motor_current_ratio` added by an active `drive_motor` fault.
    pub current_fault: f64,
    /// `dust_coverage` per unit of `joints` wear.
    pub dust_per_joint_wear: f64,
    /// `dust_coverage` per unit of `sensors` wear.
    pub dust_per_sensor_wear: f64,
    /// Std-dev of observed `wheel_slip`.
    pub telemetry_slip_noise: f64,
    /// Std-dev of observed `motor_current_ratio`.
    pub telemetry_current_noise: f64,
    /// Std-dev of observed `dust_coverage`.
    pub telemetry_dust_noise: f64,
    /// Std-dev of observed `temperature_c`, °C.
    pub telemetry_temp_noise_c: f64,

    // --- session ---
    /// Ends the run if a coordinator keeps sim time frozen this many observations in a row.
    pub max_observations_at_same_time: u64,

    /// Incidents that happen at fixed times regardless of exposure, for tests
    /// and demo scenarios. Empty in every built-in variant.
    pub scripted: Vec<ScriptedIncident>,
}

/// An incident forced on a robot at a fixed time.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptedIncident {
    pub robot_id: String,
    /// Sim time, s.
    pub t_s: f64,
    pub incident: Incident,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Incident {
    /// A fault, subject to the one-fault-per-component rule.
    Fault {
        component: Comp,
        severity: Severity,
        cause: FaultCause,
        field_repairable: bool,
    },
    /// The robot becomes stuck where it is.
    Stuck,
}

impl Default for Params {
    fn default() -> Self {
        Self::nominal()
    }
}

impl Params {
    /// The default variant: hybrid power and the nominal fault model.
    pub fn nominal() -> Self {
        Self {
            preset: Preset::Small,
            variant: Variant::Nominal,

            base_elevation_m: -4102.0,
            tilt_sigma: 0.004,
            noise_octaves: [(16.0, 0.30), (8.0, 0.12), (4.0, 0.04)],
            regolith_depth_m: (0.4, 1.5),
            regolith_density_kg_m3: (1400.0, 1700.0),
            regolith_cohesion_kpa: (0.5, 3.0),
            boulders_per_cell: 0.06,
            boulder_d_min_m: 0.1,
            boulder_d_max_m: 1.2,
            boulder_alpha: 2.5,
            rock_density_kg_m3: 2800.0,
            orbital_elevation_noise_m: 0.25,

            work_duration_sigma: 0.15,
            travel_duration_sigma: 0.05,
            min_work_s: 5.0,
            dig_cut_noise_m: 0.02,
            grade_max_delta_m: 0.15,
            grade_noise_m: 0.005,
            compact_settle_m: 0.01,
            compact_settle_noise_m: 0.002,

            scan_elevation_noise_m: 0.01,
            scan_depth_noise_m: 0.05,
            scan_density_noise_frac: 0.03,
            scan_min_boulder_m: 0.1,
            scan_boulder_noise_frac: 0.05,
            telemetry_battery_noise_frac: 0.005,

            power_mode: PowerMode::Hybrid,
            fission_w: 10_000.0,
            solar_peak_w: 20_000.0,
            essential_load_w: 2_000.0,
            bank_capacity_wh: 100_000.0,
            bank_initial_frac: 0.6,
            bank_low_frac: 0.2,
            power_tick_s: 900.0,

            clear_tau: 0.5,
            storm_chance_per_sol: 0.03,
            storm_sols: (2.0, 10.0),
            storm_peak_tau: (2.0, 4.0),
            dust_extinction: 0.6,
            dust_storm: false,
            dust_storm_start_sols: (0.0, 0.02),
            dust_storm_rise_sols: 0.03,
            dust_storm_sols: (4.0, 8.0),
            dust_storm_peak_tau: (3.5, 5.0),

            delay_min_s: 240.0,
            delay_max_s: 1440.0,
            synodic_period_sols: 759.07,
            start_delay_s: 720.0,
            conjunction: false,
            conjunction_start_sols: (0.01, 0.03),
            conjunction_sols: 14.0,
            outage_per_sol: 0.15,
            outage_hours: (2.0, 8.0),

            base_station_range_m: 33.0,
            relay_radio_w: 25.0,
            lost_link_timeout_s: 1800.0,

            earth_response_mean_s: 1200.0,
            earth_response_sigma: 0.5,
            earth_approve_p: 0.9,
            earth_approve_cannibalize_p: 0.75,
            earth_approve_abandon_p: 0.6,
            earth_reserve_low_frac: 0.25,
            trust_incident_free_sols: 2.0,
            policy_reaction_s: 3600.0,

            faults_enabled: true,
            wear_wheels_per_km: 0.08,
            wear_drive_motor_per_km: 0.06,
            wear_tool_per_work_h: 0.02,
            wear_joints_per_work_h: 0.012,
            wear_joints_per_dust_h: 0.004,
            wear_sensors_per_dust_h: 0.006,
            wear_battery_per_kwh: 0.006,
            storm_wear_factor: 2.0,

            wear_speed_loss: 0.3,
            wear_drive_energy_gain: 0.3,
            wear_work_rate_loss: 0.2,
            wear_work_energy_gain: 0.2,
            wear_scan_rate_loss: 0.3,
            wear_energy_gain: 0.15,

            degraded_speed_factor: 0.7,
            degraded_drive_energy_factor: 1.4,
            degraded_work_rate_factor: 0.75,
            degraded_work_energy_factor: 1.25,
            degraded_scan_rate_factor: 0.5,
            degraded_energy_factor: 1.25,

            wear_hazard_per_h: 0.05,
            wear_hazard_exponent: 3.0,
            random_fault_per_h: 0.008,
            storm_sensor_fault_per_h: 0.02,
            stuck_per_km: 0.35,
            soft_depth_m: 0.9,
            soft_depth_span_m: 0.6,
            soft_cohesion_kpa: 1.0,
            soft_cohesion_cap: 2.0,
            p_disabled_wear: 0.4,
            p_disabled_random: 0.3,
            p_disabled_environmental: 0.2,
            p_field_repairable: 0.6,

            repair_units_degraded: 0.25,
            repair_units_disabled: 1.0,
            repair_units_stuck: 0.5,
            repair_units_per_wear: 0.3,
            repair_wear_cut: 0.5,
            tow_speed_factor: 0.5,
            tow_energy_factor: 2.0,
            strip_units: 0.5,
            install_units: 1.0,
            service_s: 7_200.0,
            depot_capacity: 1,

            heater_threshold_c: -60.0,
            heater_idle_factor: 1.0,
            heater_offset_c: 30.0,

            slip_base: 0.02,
            slip_per_wear: 0.3,
            slip_fault: 0.15,
            current_per_wear: 0.6,
            current_fault: 0.3,
            dust_per_joint_wear: 0.6,
            dust_per_sensor_wear: 0.4,
            telemetry_slip_noise: 0.01,
            telemetry_current_noise: 0.02,
            telemetry_dust_noise: 0.02,
            telemetry_temp_noise_c: 0.5,

            max_observations_at_same_time: 1000,

            scripted: Vec::new(),
        }
    }

    /// Nominal power with much faster wear and far more failures.
    pub fn cascading_faults() -> Self {
        let n = Self::nominal();
        Self {
            variant: Variant::CascadingFaults,
            wear_wheels_per_km: n.wear_wheels_per_km * 4.0,
            wear_drive_motor_per_km: n.wear_drive_motor_per_km * 4.0,
            wear_tool_per_work_h: n.wear_tool_per_work_h * 4.0,
            wear_joints_per_work_h: n.wear_joints_per_work_h * 4.0,
            wear_joints_per_dust_h: n.wear_joints_per_dust_h * 4.0,
            wear_sensors_per_dust_h: n.wear_sensors_per_dust_h * 4.0,
            wear_battery_per_kwh: n.wear_battery_per_kwh * 4.0,
            wear_hazard_per_h: n.wear_hazard_per_h * 4.0,
            random_fault_per_h: n.random_fault_per_h * 5.0,
            storm_sensor_fault_per_h: n.storm_sensor_fault_per_h * 3.0,
            stuck_per_km: n.stuck_per_km * 3.0,
            p_field_repairable: 0.4,
            ..n
        }
    }

    /// Solar arrays and storage only: day-night swings and storms dominate.
    pub fn solar_only() -> Self {
        Self {
            variant: Variant::SolarOnly,
            power_mode: PowerMode::SolarOnly,
            fission_w: 0.0,
            solar_peak_w: 30_000.0,
            bank_capacity_wh: 150_000.0,
            ..Self::nominal()
        }
    }

    /// Mostly fission: a steady supply with a small solar top-up.
    pub fn nuclear_heavy() -> Self {
        Self {
            variant: Variant::NuclearHeavy,
            power_mode: PowerMode::NuclearHeavy,
            fission_w: 15_000.0,
            solar_peak_w: 5_000.0,
            bank_capacity_wh: 50_000.0,
            ..Self::nominal()
        }
    }

    /// Nominal, around solar conjunction: the Earth link goes down for about
    /// two weeks between 15 and 45 minutes into the run, so the blackout
    /// covers most of even the fastest build, and the delay is near its
    /// longest.
    pub fn conjunction() -> Self {
        Self {
            variant: Variant::Conjunction,
            conjunction: true,
            ..Self::nominal()
        }
    }

    /// Nominal, with a severe regional dust storm from the start of the run:
    /// its front arrives within the first half hour and opacity peaks about
    /// 45 minutes later, so even the fastest build spends most of its time
    /// in it, and it then dims the sun for days.
    pub fn dust_storm() -> Self {
        Self {
            variant: Variant::DustStorm,
            dust_storm: true,
            ..Self::nominal()
        }
    }

    /// The parameters of a variant, for the Small preset.
    pub fn variant(v: Variant) -> Self {
        match v {
            Variant::Nominal => Self::nominal(),
            Variant::DustStorm => Self::dust_storm(),
            Variant::Conjunction => Self::conjunction(),
            Variant::CascadingFaults => Self::cascading_faults(),
            Variant::SolarOnly => Self::solar_only(),
            Variant::NuclearHeavy => Self::nuclear_heavy(),
        }
    }

    /// The parameters of a scenario in the catalog: a preset's crew and
    /// outpost, under a variant.
    pub fn scenario(preset: Preset, variant: Variant) -> Self {
        Self {
            preset,
            ..Self::variant(variant)
        }
    }
}

/// A Standard track preset: a crew with a defined capability mix and the
/// outpost it works from. `src/presets.rs` defines them.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// Four robots, one of each design.
    #[default]
    Small,
    /// Ten robots.
    Medium,
    /// Twenty robots.
    Large,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Small, Preset::Medium, Preset::Large];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Small => "small",
            Preset::Medium => "medium",
            Preset::Large => "large",
        }
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Preset {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Preset::ALL
            .into_iter()
            .find(|p| p.name() == s)
            .ok_or_else(|| {
                let names: Vec<&str> = Preset::ALL.iter().map(|p| p.name()).collect();
                format!("unknown preset '{s}' (expected {})", names.join(", "))
            })
    }
}

/// A named scenario variant: what the run throws at the crew, as a fixed
/// change to the nominal parameters. Every variant combines with every preset.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    #[default]
    Nominal,
    DustStorm,
    Conjunction,
    CascadingFaults,
    SolarOnly,
    NuclearHeavy,
}

impl Variant {
    pub const ALL: [Variant; 6] = [
        Variant::Nominal,
        Variant::DustStorm,
        Variant::Conjunction,
        Variant::CascadingFaults,
        Variant::SolarOnly,
        Variant::NuclearHeavy,
    ];

    /// The variants that stress a crew, against which resilience is measured:
    /// the outline's dust storms, blackouts and faults.
    pub const STRESS: [Variant; 3] = [
        Variant::DustStorm,
        Variant::Conjunction,
        Variant::CascadingFaults,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Variant::Nominal => "nominal",
            Variant::DustStorm => "dust_storm",
            Variant::Conjunction => "conjunction",
            Variant::CascadingFaults => "cascading_faults",
            Variant::SolarOnly => "solar_only",
            Variant::NuclearHeavy => "nuclear_heavy",
        }
    }
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Variant {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Variant::ALL
            .into_iter()
            .find(|v| v.name() == s)
            .ok_or_else(|| {
                let names: Vec<&str> = Variant::ALL.iter().map(|v| v.name()).collect();
                format!("unknown variant '{s}' (expected {})", names.join(", "))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_parse_and_differ_where_they_should() {
        for v in Variant::ALL {
            assert_eq!(v.name().parse::<Variant>().unwrap(), v);
            assert_eq!(Params::variant(v).variant, v);
        }
        assert!("sandstorm".parse::<Variant>().is_err());
        assert!(Params::conjunction().conjunction && !Params::nominal().conjunction);
        assert_eq!(
            Params {
                variant: Variant::Nominal,
                conjunction: false,
                ..Params::conjunction()
            },
            Params::nominal(),
            "conjunction changes only the link"
        );
        assert_eq!(
            Params {
                variant: Variant::Nominal,
                dust_storm: false,
                ..Params::dust_storm()
            },
            Params::nominal(),
            "dust_storm adds only the storm"
        );
        for v in Variant::ALL {
            assert!(Params::variant(v).outage_per_sol > 0.0, "{v} has outages");
        }
        let (n, c) = (Params::nominal(), Params::cascading_faults());
        assert!(c.random_fault_per_h > n.random_fault_per_h * 4.0);
        assert!(c.wear_wheels_per_km > n.wear_wheels_per_km * 3.0);
        assert_eq!(c.fission_w, n.fission_w);
        assert_eq!(Params::solar_only().fission_w, 0.0);
        assert_eq!(Params::solar_only().power_mode, PowerMode::SolarOnly);
        assert_eq!(Params::nuclear_heavy().power_mode, PowerMode::NuclearHeavy);
        assert!(Params::nuclear_heavy().fission_w > n.fission_w);
        assert_eq!(Params::default(), n);
        assert!(Variant::STRESS.iter().all(|v| Variant::ALL.contains(v)));
        for v in Variant::ALL {
            assert_eq!(serde_json::to_value(v).unwrap(), v.name());
        }
        for p in Preset::ALL {
            assert_eq!(serde_json::to_value(p).unwrap(), p.name());
        }
    }

    #[test]
    fn presets_parse_and_change_only_the_crew() {
        for p in Preset::ALL {
            assert_eq!(p.name().parse::<Preset>().unwrap(), p);
            for v in Variant::ALL {
                let params = Params::scenario(p, v);
                assert_eq!((params.preset, params.variant), (p, v));
                assert_eq!(
                    Params {
                        preset: Preset::Small,
                        ..params
                    },
                    Params::variant(v)
                );
            }
        }
        assert!("huge".parse::<Preset>().is_err());
        assert_eq!(Params::default().preset, Preset::Small);
    }
}
