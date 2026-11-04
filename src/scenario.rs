//! Scenario generation. One scenario, `landing-pad-small`: a seeded
//! 60 × 60 m Arcadia Planitia site with a 20 × 20 m pad, a short blast berm,
//! four robots, one charge station, one depot and a comms base station. Its
//! power system comes from the parameters, so the power variants change it.
//! The Small/Medium/Large presets arrive with scoring in week 7.

use crate::sim::mission::Mission;
use crate::sim::params::Params;
use crate::sim::rng::Rng;
use crate::sim::terrain::{Terrain, rect_cells};
use crate::sim::time::{SOL_MS, secs};
use arcadia_protocol::*;

pub const SITE_SIZE: u32 = 60;
pub const PAD: GridRect = GridRect {
    min: GridCoord { x: 20, y: 20 },
    max: GridCoord { x: 39, y: 39 },
};
pub const BERM_Y: u32 = 42;
pub const BERM_X: (u32, u32) = (24, 35);
pub const BERM_HEIGHT_M: f64 = 0.5;
pub const TIME_LIMIT_SOLS: u64 = 60;
pub const STATION: GridCoord = GridCoord { x: 10, y: 10 };
pub const DEPOT: GridCoord = GridCoord { x: 8, y: 14 };
/// The comms hub sits between the station and the depot; with the default
/// range it covers the pad, the berm and the work areas around them, but not
/// the far corners of the site (coverage is enforced from week 6).
pub const BASE_STATION: GridCoord = GridCoord { x: 10, y: 12 };

/// Everything a run needs: what the coordinator is told, and the ground truth.
pub struct Generated {
    pub scenario: Scenario,
    pub terrain: Terrain,
    pub goal: EarthToSite,
    pub mission: Mission,
    pub starts: Vec<GridCoord>,
}

pub fn generate(seed: u64, p: &Params) -> Generated {
    let terrain = Terrain::generate(
        &mut Rng::stream(seed, "terrain"),
        SITE_SIZE,
        SITE_SIZE,
        1.0,
        p,
    );

    // Level the pad at its mean elevation, so cut and fill roughly balance.
    let pad_cells: Vec<_> = rect_cells(&PAD).collect();
    let mean = pad_cells
        .iter()
        .map(|&c| terrain.get(c).elevation_m)
        .sum::<f64>()
        / pad_cells.len() as f64;
    let target_elevation_m = (mean * 100.0).round() / 100.0;
    let pad = LandingPadSpec {
        area: PAD,
        target_elevation_m,
        surface_tolerance_m: 0.05,
        max_slope_deg: 2.0,
        stabilize: true,
        berm: Some(BermSpec {
            cells: (BERM_X.0..=BERM_X.1)
                .map(|x| GridCoord { x, y: BERM_Y })
                .collect(),
            height_m: BERM_HEIGHT_M,
        }),
    };
    let goal = EarthToSite {
        message_id: "earth-0001".into(),
        body: Uplink::Goal {
            goal_id: "pad-a".into(),
            priority: 10,
            deadline_t: None,
            objective: Objective::LandingPad { pad: pad.clone() },
        },
    };
    let mission = Mission::new("pad-a", pad, &terrain);

    // Orbital prior: elevation only, at HiRISE-DTM-like accuracy.
    let mut orbital = Rng::stream(seed, "orbital");
    let prior = SiteMap {
        site_id: "arcadia-pad-site".into(),
        name: Some("Arcadia Planitia landing pad site".into()),
        width: terrain.width,
        height: terrain.height,
        cell_size_m: terrain.cell_size_m,
        cells: terrain
            .coords()
            .map(|c| CellObservation {
                at: c,
                cell: Cell {
                    elevation_m: Some(
                        terrain.get(c).elevation_m + orbital.normal() * p.orbital_elevation_noise_m,
                    ),
                    ..Cell::default()
                },
            })
            .collect(),
    };

    let robots = crew();
    let starts = (0..robots.len() as u32)
        .map(|i| GridCoord { x: 12 + i, y: 10 })
        .collect();
    let scenario = Scenario {
        scenario_id: "landing-pad-small".into(),
        start_t: 0.0,
        time_limit_t: secs(TIME_LIMIT_SOLS * SOL_MS),
        sol_length_s: secs(SOL_MS),
        site: prior,
        robots,
        stations: vec![ChargeStation {
            station_id: "st-1".into(),
            position: STATION,
            max_power_w: 6000.0,
            slots: 2,
        }],
        depots: vec![Depot {
            depot_id: "dp-1".into(),
            position: DEPOT,
        }],
        power: PowerSystem {
            mode: p.power_mode,
            fission_w: p.fission_w,
            solar_peak_w: p.solar_peak_w,
            storage_capacity_wh: p.bank_capacity_wh,
            essential_load_w: p.essential_load_w,
        },
        base_station: BaseStation {
            position: BASE_STATION,
            range_m: p.base_station_range_m,
        },
        policy: default_policy(),
    };
    Generated {
        scenario,
        terrain,
        goal,
        mission,
        starts,
    }
}

/// Four robots covering every v1 capability.
fn crew() -> Vec<RobotSpec> {
    use CapabilityName::*;
    vec![
        robot(
            "sc-1",
            "scout",
            90.0,
            1500.0,
            0.5,
            0.2,
            8.0,
            vec![
                cap(Scan, 4.0, 0.05, None, Some(10.0)),
                cap(Repair, 0.0005, 400.0, None, None),
            ],
        ),
        robot(
            "ex-1",
            "excavator",
            420.0,
            6000.0,
            0.15,
            0.9,
            35.0,
            vec![
                cap(Dig, 0.004, 100.0, None, None),
                cap(Grade, 0.05, 8.0, None, None),
                cap(Haul, 2.0, 0.01, Some(250.0), None),
            ],
        ),
        robot(
            "hl-1",
            "hauler",
            300.0,
            12000.0,
            0.3,
            0.7,
            20.0,
            vec![
                cap(Haul, 3.0, 0.008, Some(400.0), None),
                cap(Charge, 0.5, 0.0, Some(8000.0), None),
                cap(Relay, 0.0, 0.0, None, Some(150.0)),
            ],
        ),
        robot(
            "cp-1",
            "compactor-sinterer",
            500.0,
            10000.0,
            0.15,
            1.0,
            30.0,
            vec![
                cap(Compact, 0.1, 4.0, None, None),
                cap(Sinter, 0.01, 60.0, None, None),
            ],
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn robot(
    id: &str,
    model: &str,
    mass_kg: f64,
    battery_wh: f64,
    speed: f64,
    drive_wh_per_m: f64,
    idle_w: f64,
    capabilities: Vec<CapabilitySpec>,
) -> RobotSpec {
    RobotSpec {
        robot_id: id.into(),
        model: Some(model.into()),
        mass_kg,
        battery_capacity_wh: battery_wh,
        max_speed_m_s: speed,
        drive_energy_wh_per_m: drive_wh_per_m,
        idle_power_w: idle_w,
        capabilities,
    }
}

fn cap(
    name: CapabilityName,
    rate: f64,
    energy_wh_per_unit: f64,
    capacity: Option<f64>,
    range_m: Option<f64>,
) -> CapabilitySpec {
    CapabilitySpec {
        name,
        rate,
        energy_wh_per_unit,
        capacity,
        range_m,
    }
}

/// Authority levels from the project outline.
pub fn default_policy() -> Policy {
    let levels = [
        ("move_to", 1),
        ("scan", 1),
        ("dig", 1),
        ("grade", 1),
        ("haul", 1),
        ("compact", 1),
        ("sinter", 1),
        ("charge", 1),
        ("relay", 1),
        ("wait", 1),
        ("cancel", 1),
        ("repair", 1),
        ("service", 1),
        ("tow", 2),
        ("replan", 2),
        ("reassign_task", 2),
        ("abandon_section", 3),
        ("cannibalize", 3),
        ("commit_energy_reserve", 4),
    ]
    .into_iter()
    .map(|(a, l)| (a.to_string(), AuthorityLevel::try_from(l).expect("1-4")))
    .collect();
    Policy {
        policy_version: 1,
        default_level: AuthorityLevel::NOTIFY,
        levels,
        level3: Level3Rule {
            max_wait_s: 3600.0,
            default_fallback: Fallback::Abort,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_world_and_prior_is_noisy() {
        let p = Params::default();
        let (a, b) = (generate(5, &p), generate(5, &p));
        assert_eq!(a.terrain, b.terrain);
        assert_eq!(a.scenario, b.scenario);
        let truth = a.terrain.get(GridCoord { x: 3, y: 3 }).elevation_m;
        let prior = a.scenario.site.cells[3 * 60 + 3].cell.elevation_m.unwrap();
        assert_ne!(truth, prior);
        assert!((truth - prior).abs() < 1.5);
    }

    #[test]
    fn crew_covers_every_landing_pad_capability() {
        use CapabilityName::*;
        let crew = crew();
        for c in [
            Scan, Dig, Grade, Haul, Compact, Sinter, Charge, Relay, Repair,
        ] {
            assert!(crew.iter().any(|r| r.capability(c).is_some()), "{c:?}");
        }
    }
}
