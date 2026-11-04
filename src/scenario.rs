//! Scenario generation: the landing-pad scenario of the catalog,
//! `landing-pad-<preset>-<variant>`. Every scenario has the same seeded
//! 60 × 60 m Arcadia Planitia site, a 20 × 20 m pad with a short blast berm,
//! and a comms base station. The preset ([`crate::presets`]) brings the crew,
//! the charge stations and the depots; the variant's parameters bring the
//! power system and the base station's range.

use crate::presets;
use crate::sim::mission::Mission;
use crate::sim::params::{Params, Preset, Variant};
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
/// The first charge station, `st-1`, in every preset.
pub const STATION: GridCoord = GridCoord { x: 10, y: 10 };
/// The first depot, `dp-1`, in every preset.
pub const DEPOT: GridCoord = GridCoord { x: 8, y: 14 };
/// The comms hub stands between the outpost and the pad. With the default
/// 33 m range it covers the outpost (stations, depots and start area), the
/// whole pad and the ground east and west of it, but not the blast berm 3 m
/// north of the pad, nor the north of the site: building the berm needs a
/// relay, or work planned from inside coverage. It is the same in every
/// preset.
pub const BASE_STATION: GridCoord = GridCoord { x: 30, y: 8 };

/// The id of a scenario in the catalog: `landing-pad-<preset>-<variant>`.
pub fn scenario_id(preset: Preset, variant: Variant) -> String {
    format!("landing-pad-{preset}-{variant}")
}

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

    let robots = presets::crew(p.preset);
    let starts = presets::starts(p.preset);
    let scenario = Scenario {
        scenario_id: scenario_id(p.preset, p.variant),
        start_t: 0.0,
        time_limit_t: secs(TIME_LIMIT_SOLS * SOL_MS),
        sol_length_s: secs(SOL_MS),
        site: prior,
        robots,
        stations: presets::stations(p.preset),
        depots: presets::depots(p.preset),
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
    fn the_base_station_covers_the_outpost_and_the_pad_but_not_the_berm() {
        use crate::sim::coverage::within;
        for preset in Preset::ALL {
            let p = Params::scenario(preset, Variant::Nominal);
            let g = generate(1, &p);
            let base = &g.scenario.base_station;
            assert_eq!(base.position, BASE_STATION);
            let covered = |c: GridCoord| within(c, base.position, base.range_m, 1.0);
            assert!(rect_cells(&PAD).all(covered));
            assert!(g.scenario.stations.iter().all(|s| covered(s.position)));
            assert!(g.scenario.depots.iter().all(|d| covered(d.position)));
            assert!(g.starts.iter().all(|&c| covered(c)));
            let Uplink::Goal {
                objective: Objective::LandingPad { pad },
                ..
            } = &g.goal.body
            else {
                panic!("the goal is a landing pad");
            };
            assert!(
                pad.berm
                    .as_ref()
                    .unwrap()
                    .cells
                    .iter()
                    .all(|&c| !covered(c))
            );
            // Every hauler's relay reaches the whole site from anywhere on it.
            let relays: Vec<f64> = g
                .scenario
                .robots
                .iter()
                .filter_map(|r| r.capability(CapabilityName::Relay)?.range_m)
                .collect();
            assert!(!relays.is_empty());
            assert!(
                relays
                    .iter()
                    .all(|&r| r >= f64::from(SITE_SIZE) * std::f64::consts::SQRT_2)
            );
        }
    }

    #[test]
    fn every_scenario_in_the_catalog_has_its_id_crew_and_world() {
        let small = generate(3, &Params::default());
        assert_eq!(small.scenario.scenario_id, "landing-pad-small-nominal");
        for preset in Preset::ALL {
            for variant in Variant::ALL {
                let p = Params::scenario(preset, variant);
                let g = generate(3, &p);
                assert_eq!(
                    g.scenario.scenario_id,
                    format!("landing-pad-{}-{}", preset.name(), variant.name())
                );
                assert_eq!(g.scenario.robots, presets::crew(preset));
                assert_eq!(g.starts, presets::starts(preset));
                assert_eq!(g.scenario.stations, presets::stations(preset));
                assert_eq!(g.scenario.depots, presets::depots(preset));
                assert_eq!(g.scenario.power.fission_w, p.fission_w);
                // The same site and goal everywhere: a seed is one world.
                assert_eq!(g.terrain, small.terrain);
                assert_eq!(g.scenario.site, small.scenario.site);
                assert_eq!(g.goal, small.goal);
            }
        }
    }
}
