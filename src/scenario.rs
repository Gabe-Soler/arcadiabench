//! Scenarios. Week 1 ships a fixed placeholder; seeded generation of the
//! Arcadia Planitia landing-pad site arrives with the simulator in week 2.

use arcadia_protocol::*;

const PAD_ELEVATION_M: f64 = -4102.0;

/// A small fixed site with three robots, one charge station and one depot.
pub fn placeholder() -> Scenario {
    Scenario {
        scenario_id: "placeholder".into(),
        start_t: 0.0,
        time_limit_t: 60.0 * SOL_S,
        sol_length_s: SOL_S,
        site: SiteMap {
            site_id: "arcadia-placeholder".into(),
            name: Some("Arcadia Planitia placeholder site".into()),
            width: 40,
            height: 40,
            cell_size_m: 1.0,
            cells: vec![],
        },
        robots: vec![
            robot(
                "ex-1",
                6000.0,
                &[
                    cap(CapabilityName::Dig, 0.002, 120.0),
                    cap(CapabilityName::Grade, 0.05, 8.0),
                ],
            ),
            robot(
                "hl-1",
                12000.0,
                &[CapabilitySpec {
                    capacity: Some(400.0),
                    ..cap(CapabilityName::Haul, 3.0, 0.008)
                }],
            ),
            robot(
                "sc-1",
                1500.0,
                &[CapabilitySpec {
                    range_m: Some(10.0),
                    ..cap(CapabilityName::Scan, 4.0, 0.05)
                }],
            ),
        ],
        stations: vec![ChargeStation {
            station_id: "st-1".into(),
            position: GridCoord { x: 2, y: 2 },
            max_power_w: 3000.0,
            slots: 2,
        }],
        depots: vec![Depot {
            depot_id: "dp-1".into(),
            position: GridCoord { x: 1, y: 5 },
        }],
        policy: default_policy(),
    }
}

/// Initial robot states for [`placeholder`]: full batteries, parked near the station.
pub fn placeholder_states(scenario: &Scenario) -> Vec<RobotState> {
    scenario
        .robots
        .iter()
        .zip(3u32..)
        .map(|(r, x)| RobotState {
            robot_id: r.robot_id.clone(),
            position: GridCoord { x, y: 3 },
            status: RobotStatus::Idle,
            battery_wh: r.battery_capacity_wh,
            payload_kg: 0.0,
            current_command: None,
            health: HealthTelemetry {
                wheel_slip: 0.0,
                motor_current_ratio: 1.0,
                dust_coverage: 0.0,
                temperature_c: -40.0,
            },
            in_relay_coverage: true,
        })
        .collect()
}

/// The landing-pad goal mission control sends at t = 0.
pub fn placeholder_goal() -> EarthToSite {
    EarthToSite {
        message_id: "earth-0001".into(),
        body: Uplink::Goal {
            goal_id: "pad-a".into(),
            priority: 10,
            deadline_t: None,
            objective: Objective::LandingPad {
                pad: LandingPadSpec {
                    area: GridRect {
                        min: GridCoord { x: 10, y: 10 },
                        max: GridCoord { x: 29, y: 29 },
                    },
                    target_elevation_m: PAD_ELEVATION_M,
                    surface_tolerance_m: 0.05,
                    max_slope_deg: 2.0,
                    stabilize: true,
                    berm: None,
                },
            },
        },
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

fn cap(name: CapabilityName, rate: f64, energy_wh_per_unit: f64) -> CapabilitySpec {
    CapabilitySpec {
        name,
        rate,
        energy_wh_per_unit,
        capacity: None,
        range_m: None,
    }
}

fn robot(id: &str, battery_wh: f64, caps: &[CapabilitySpec]) -> RobotSpec {
    RobotSpec {
        robot_id: id.into(),
        model: None,
        mass_kg: 300.0,
        battery_capacity_wh: battery_wh,
        max_speed_m_s: 0.2,
        drive_energy_wh_per_m: 0.6,
        idle_power_w: 20.0,
        capabilities: caps.to_vec(),
    }
}
