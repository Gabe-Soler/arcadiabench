//! Every simulation tunable in one place.
//!
//! These are placeholder values chosen to be physically plausible and to make
//! a 20 m pad buildable in a few sols. Sourcing realistic numbers is an open
//! question in the project outline. TODO(params): cite sources per field.

#[derive(Debug, Clone)]
pub struct Params {
    // --- terrain (Arcadia Planitia: smooth, low rock abundance) ---
    pub base_elevation_m: f64,
    /// Std-dev of the regional tilt, m per m.
    pub tilt_sigma: f64,
    /// Value-noise octaves: (lattice spacing in cells, amplitude in m).
    pub noise_octaves: [(f64, f64); 3],
    pub regolith_depth_m: (f64, f64),
    pub regolith_density_kg_m3: (f64, f64),
    pub regolith_cohesion_kpa: (f64, f64),
    /// Mean boulders per cell.
    pub boulders_per_cell: f64,
    /// Power-law boulder diameters: d = d_min * u^(-1/alpha), capped at d_max.
    pub boulder_d_min_m: f64,
    pub boulder_d_max_m: f64,
    pub boulder_alpha: f64,
    pub rock_density_kg_m3: f64,
    pub orbital_elevation_noise_m: f64,

    // --- actions ---
    /// Log-sd of multiplicative noise on work durations.
    pub work_duration_sigma: f64,
    /// Log-sd of multiplicative noise on travel durations.
    pub travel_duration_sigma: f64,
    /// Floor on any per-cell work step, s.
    pub min_work_s: f64,
    pub dig_cut_noise_m: f64,
    /// Grade only does fine levelling within this distance of the target.
    pub grade_max_delta_m: f64,
    pub grade_noise_m: f64,
    pub compact_settle_m: f64,
    pub compact_settle_noise_m: f64,

    // --- sensing ---
    pub scan_elevation_noise_m: f64,
    pub scan_depth_noise_m: f64,
    pub scan_density_noise_frac: f64,
    pub scan_min_boulder_m: f64,
    pub scan_boulder_noise_frac: f64,
    /// Battery telemetry noise as a fraction of capacity.
    pub telemetry_battery_noise_frac: f64,

    // --- hybrid power ---
    pub fission_w: f64,
    pub solar_peak_w: f64,
    pub base_load_w: f64,
    pub bank_capacity_wh: f64,
    pub bank_initial_frac: f64,
    pub bank_low_frac: f64,
    /// Internal power-accounting step, s (not a decision point).
    pub power_tick_s: f64,

    // --- environment ---
    pub clear_tau: f64,
    pub storm_chance_per_sol: f64,
    pub storm_sols: (f64, f64),
    pub storm_peak_tau: (f64, f64),
    /// Solar transmittance = exp(-extinction * (tau - clear_tau)).
    pub dust_extinction: f64,

    // --- comms (fixed until week 6) ---
    pub one_way_delay_s: f64,

    // --- session ---
    /// Ends the run if a coordinator keeps sim time frozen this many observations in a row.
    pub max_observations_at_same_time: u64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
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

            fission_w: 10_000.0,
            solar_peak_w: 20_000.0,
            base_load_w: 2_000.0,
            bank_capacity_wh: 100_000.0,
            bank_initial_frac: 0.6,
            bank_low_frac: 0.2,
            power_tick_s: 900.0,

            clear_tau: 0.5,
            storm_chance_per_sol: 0.03,
            storm_sols: (2.0, 10.0),
            storm_peak_tau: (2.0, 4.0),
            dust_extinction: 0.6,

            one_way_delay_s: 760.0,

            max_observations_at_same_time: 1000,
        }
    }
}
