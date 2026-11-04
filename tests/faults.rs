//! Faults, wear and the repair options end to end (semantics §6), through
//! in-process agents. Most tests turn the random failure processes off and
//! script the incidents they need.

mod support;

use arcadia_protocol::*;
use arcadiabench::coordinator::Agent;
use arcadiabench::sim::faults::Comp;
use arcadiabench::sim::params::{Incident, Params, ScriptedIncident};
use support::*;

const DEPOT: GridCoord = GridCoord { x: 8, y: 14 };
const STATION: GridCoord = GridCoord { x: 10, y: 10 };

fn fault(
    robot: &str,
    t_s: f64,
    component: Comp,
    severity: Severity,
    field_repairable: bool,
) -> ScriptedIncident {
    ScriptedIncident {
        robot_id: robot.into(),
        t_s,
        incident: Incident::Fault {
            component,
            severity,
            cause: FaultCause::Random,
            field_repairable,
        },
    }
}

/// Wear grows but nothing fails on its own.
fn wear_only() -> Params {
    Params {
        wear_hazard_per_h: 0.0,
        random_fault_per_h: 0.0,
        storm_sensor_fault_per_h: 0.0,
        stuck_per_km: 0.0,
        ..Params::default()
    }
}

fn health_is_new(p: &Params, s: &RobotState) -> bool {
    s.health.wheel_slip == p.slip_base
        && s.health.motor_current_ratio == 1.0
        && s.health.dust_coverage == 0.0
}

#[test]
fn repair_clears_a_field_repairable_fault() {
    let params = Params {
        scripted: vec![fault("ex-1", 100.0, Comp::Tool, Severity::Disabled, true)],
        ..quiet()
    };
    let mut sent = false;
    let (report, _, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            if !sent && robot(obs, "ex-1").status == RobotStatus::Failed {
                sent = true;
                d.commands.push(cmd(
                    "move",
                    "ex-1",
                    Action::MoveTo {
                        target: GridCoord { x: 20, y: 20 },
                    },
                ));
                d.commands.push(cmd(
                    "fix",
                    "sc-1",
                    Action::Repair {
                        target_robot: "ex-1".into(),
                    },
                ));
            }
        }),
        1,
        &params,
    );
    let obs = &agent.observations;
    let failed = obs
        .iter()
        .find(|o| robot(o, "ex-1").status == RobotStatus::Failed)
        .unwrap();
    assert_eq!(failed.t, 100.0);
    let ex = robot(failed, "ex-1");
    assert_eq!(ex.faults.len(), 1);
    assert_eq!(
        (
            ex.faults[0].component.as_str(),
            ex.faults[0].severity,
            ex.faults[0].field_repairable
        ),
        ("tool", Severity::Disabled, true)
    );
    assert!(failed.events.iter().any(|e| matches!(
        &e.kind,
        EventKind::RobotFault { robot_id, component, field_repairable: true, .. }
            if robot_id == "ex-1" && component == "tool"
    )));
    assert_eq!(rejection(obs, "move"), Some(RejectReason::RobotDisabled));
    let done = completed(obs, "fix").expect("the repair completes");
    let after = obs.iter().find(|o| o.t >= done).unwrap();
    assert_eq!(robot(after, "ex-1").status, RobotStatus::Idle);
    assert!(robot(after, "ex-1").faults.is_empty());
    assert_eq!(report.metrics.fleet.faults, 1);
}

#[test]
fn repair_frees_a_stuck_robot_and_getting_stuck_is_a_decision_point() {
    let params = Params {
        scripted: vec![ScriptedIncident {
            robot_id: "cp-1".into(),
            t_s: 100.0,
            incident: Incident::Stuck,
        }],
        ..quiet()
    };
    let mut sent = false;
    let (report, _, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            if !sent && robot(obs, "cp-1").status == RobotStatus::Stuck {
                sent = true;
                d.commands.push(cmd(
                    "fix",
                    "sc-1",
                    Action::Repair {
                        target_robot: "cp-1".into(),
                    },
                ));
            }
        }),
        2,
        &params,
    );
    let obs = &agent.observations;
    // An idle robot getting stuck wakes the coordinator with no event at all.
    let stuck = obs
        .iter()
        .find(|o| o.t == 100.0)
        .expect("observation at 100 s");
    assert_eq!(robot(stuck, "cp-1").status, RobotStatus::Stuck);
    assert!(stuck.events.is_empty(), "{:?}", stuck.events);
    assert!(
        robot(stuck, "cp-1").faults.is_empty(),
        "stuck is not a fault"
    );
    let done = completed(obs, "fix").expect("the repair completes");
    let after = obs.iter().find(|o| o.t >= done).unwrap();
    assert_eq!(robot(after, "cp-1").status, RobotStatus::Idle);
    assert_eq!(report.metrics.fleet.faults, 0);
}

#[test]
fn tow_to_the_depot_then_service_clears_everything_and_resets_wear() {
    let params = Params {
        scripted: vec![
            fault("ex-1", 900.0, Comp::Wheels, Severity::Degraded, true),
            fault("ex-1", 1000.0, Comp::DriveMotor, Severity::Disabled, false),
        ],
        ..wear_only()
    };
    let mut stage = 0;
    let mut towed_seen = false;
    let (report, records, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            let ex = robot(obs, "ex-1");
            towed_seen |= ex.status == RobotStatus::BeingTowed;
            match stage {
                0 => {
                    // Drive a little first so there is wear to reset (staying in coverage).
                    d.commands.push(cmd(
                        "drive",
                        "ex-1",
                        Action::MoveTo {
                            target: GridCoord { x: 13, y: 30 },
                        },
                    ));
                    stage = 1;
                }
                1 if ex.status == RobotStatus::Failed => {
                    d.commands.push(cmd(
                        "svc-early",
                        "ex-1",
                        Action::Service {
                            depot: "dp-1".into(),
                        },
                    ));
                    d.commands.push(cmd(
                        "tow",
                        "hl-1",
                        Action::Tow {
                            target_robot: "ex-1".into(),
                            to: DEPOT,
                        },
                    ));
                    // Towing is level 2 in the default policy.
                    d.to_earth.push(notify("n-tow", "tow", &["tow"]));
                    stage = 2;
                }
                2 if completed(std::slice::from_ref(obs), "tow").is_some() => {
                    assert_eq!(ex.position, DEPOT);
                    assert_eq!(robot(obs, "hl-1").position, DEPOT);
                    assert_eq!(
                        ex.status,
                        RobotStatus::Failed,
                        "a failed robot stays failed"
                    );
                    d.commands.push(cmd(
                        "svc",
                        "ex-1",
                        Action::Service {
                            depot: "dp-1".into(),
                        },
                    ));
                    stage = 3;
                }
                _ => {}
            }
            // Look in on the tow while it moves.
            if stage == 2 {
                d.wake_at = Some(obs.t + 20.0);
            }
        }),
        3,
        &params,
    );
    let obs = &agent.observations;
    assert_eq!(
        rejection(obs, "svc-early"),
        Some(RejectReason::RobotDisabled)
    );
    assert!(towed_seen, "the target shows being_towed during the tow");
    let done = completed(obs, "svc").expect("the service completes");
    let before = truth_at(&records, done - 1.0);
    let ex_before = before.iter().find(|r| r.robot_id == "ex-1").unwrap();
    assert_eq!(ex_before.faults.len(), 2);
    assert!(!health_is_new(&params, ex_before));
    let after = truth_at(&records, done);
    let ex_after = after.iter().find(|r| r.robot_id == "ex-1").unwrap();
    assert!(ex_after.faults.is_empty());
    assert!(health_is_new(&params, ex_after), "{:?}", ex_after.health);
    assert_eq!(ex_after.status, RobotStatus::Idle);
    assert_eq!(
        report.metrics.autonomy.policy_violations, 0,
        "the tow was notified"
    );
}

#[test]
fn cannibalize_loses_the_target_and_restores_the_beneficiary() {
    let params = Params {
        scripted: vec![
            fault("ex-1", 1000.0, Comp::Tool, Severity::Disabled, false),
            fault("cp-1", 1000.0, Comp::Wheels, Severity::Disabled, false),
        ],
        ..wear_only()
    };
    let mut stage = 0;
    let (report, records, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| match stage {
            0 => {
                d.commands.push(cmd(
                    "drive",
                    "cp-1",
                    Action::MoveTo {
                        target: GridCoord { x: 15, y: 30 },
                    },
                ));
                // Cannibalizing is level 3: ask, with no time to answer and fallback proceed.
                d.to_earth.push(ask_and_proceed("q", "cannibalize", obs.t));
                stage = 1;
            }
            1 if robot(obs, "ex-1").status == RobotStatus::Failed
                && robot(obs, "cp-1").status == RobotStatus::Failed =>
            {
                d.commands.push(approved(
                    cmd(
                        "self",
                        "sc-1",
                        Action::Cannibalize {
                            target_robot: "ex-1".into(),
                            beneficiary_robot: Some("ex-1".into()),
                        },
                    ),
                    "q",
                ));
                d.commands.push(approved(
                    cmd(
                        "strip",
                        "sc-1",
                        Action::Cannibalize {
                            target_robot: "ex-1".into(),
                            beneficiary_robot: Some("cp-1".into()),
                        },
                    ),
                    "q",
                ));
                stage = 2;
            }
            2 if completed(std::slice::from_ref(obs), "strip").is_some() => {
                d.commands.push(cmd(
                    "zombie",
                    "ex-1",
                    Action::MoveTo {
                        target: GridCoord { x: 1, y: 1 },
                    },
                ));
                stage = 3;
            }
            _ => {}
        }),
        4,
        &params,
    );
    let obs = &agent.observations;
    assert_eq!(rejection(obs, "self"), Some(RejectReason::InvalidTarget));
    assert_eq!(rejection(obs, "zombie"), Some(RejectReason::RobotDisabled));
    let done = completed(obs, "strip").expect("cannibalize completes");
    let after = obs.iter().find(|o| o.t >= done).unwrap();
    assert_eq!(robot(after, "ex-1").status, RobotStatus::Lost);
    assert_eq!(robot(after, "cp-1").status, RobotStatus::Idle);
    assert!(robot(after, "cp-1").faults.is_empty());
    let truth = truth_at(&records, done);
    assert!(health_is_new(
        &params,
        truth.iter().find(|r| r.robot_id == "cp-1").unwrap()
    ));
    // A lost robot keeps reporting the state it was stripped in.
    let last = obs.last().unwrap();
    assert_eq!(robot(last, "ex-1").position, robot(after, "ex-1").position);
    assert_eq!(robot(last, "ex-1").faults, robot(after, "ex-1").faults);
    assert_eq!(report.metrics.fleet.robots_lost, 1);
}

#[test]
fn commands_are_checked_against_capability_and_target() {
    let params = Params {
        scripted: vec![fault("ex-1", 100.0, Comp::Tool, Severity::Disabled, false)],
        ..quiet()
    };
    let mut stage = 0;
    let (_, _, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| match stage {
            0 => {
                let tow_cp1 = Action::Tow {
                    target_robot: "cp-1".into(),
                    to: GridCoord { x: 5, y: 5 },
                };
                d.commands.push(cmd("no-haul", "sc-1", tow_cp1.clone()));
                d.to_earth.push(ask_and_proceed("q", "cannibalize", obs.t));
                d.commands.push(cmd(
                    "healthy",
                    "sc-1",
                    Action::Repair {
                        target_robot: "cp-1".into(),
                    },
                ));
                d.commands.push(cmd("tow-healthy", "hl-1", tow_cp1));
                d.commands.push(cmd(
                    "itself",
                    "sc-1",
                    Action::Repair {
                        target_robot: "sc-1".into(),
                    },
                ));
                d.commands.push(cmd(
                    "nowhere",
                    "cp-1",
                    Action::Service {
                        depot: "dp-9".into(),
                    },
                ));
                stage = 1;
            }
            1 if robot(obs, "ex-1").status == RobotStatus::Failed => {
                // The capability check comes before the target checks.
                d.commands.push(approved(
                    cmd(
                        "no-repair",
                        "hl-1",
                        Action::Cannibalize {
                            target_robot: "ex-1".into(),
                            beneficiary_robot: None,
                        },
                    ),
                    "q",
                ));
                d.commands.push(approved(
                    cmd(
                        "strip-live",
                        "sc-1",
                        Action::Cannibalize {
                            target_robot: "cp-1".into(),
                            beneficiary_robot: None,
                        },
                    ),
                    "q",
                ));
                d.commands.push(cmd(
                    "serviced-elsewhere",
                    "ex-1",
                    Action::Service {
                        depot: "dp-1".into(),
                    },
                ));
                stage = 2;
            }
            _ => {}
        }),
        5,
        &params,
    );
    let obs = &agent.observations;
    for (id, reason) in [
        ("no-haul", RejectReason::MissingCapability),
        ("no-repair", RejectReason::MissingCapability),
        ("healthy", RejectReason::InvalidTarget),
        ("tow-healthy", RejectReason::InvalidTarget),
        ("itself", RejectReason::InvalidTarget),
        ("nowhere", RejectReason::InvalidTarget),
        ("strip-live", RejectReason::InvalidTarget),
        ("serviced-elsewhere", RejectReason::RobotDisabled),
    ] {
        assert_eq!(rejection(obs, id), Some(reason), "{id}");
    }
}

#[test]
fn offline_robots_charge_only_at_their_source() {
    let mut stage = 0;
    let (_, _, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| match stage {
            0 => {
                // A step the battery cannot cover empties it: both go offline at once.
                for (id, robot) in [("drain-sc", "sc-1"), ("drain-cp", "cp-1")] {
                    d.commands
                        .push(cmd(id, robot, Action::Wait { duration_s: 1e9 }));
                }
                stage = 1;
            }
            1 => {
                assert_eq!(robot(obs, "sc-1").status, RobotStatus::Offline);
                assert_eq!(robot(obs, "cp-1").status, RobotStatus::Offline);
                d.commands.push(cmd(
                    "far-station",
                    "sc-1",
                    Action::Charge {
                        source: "st-1".into(),
                        until_wh: None,
                    },
                ));
                d.commands.push(cmd(
                    "far-charger",
                    "sc-1",
                    Action::Charge {
                        source: "hl-1".into(),
                        until_wh: None,
                    },
                ));
                d.commands.push(cmd(
                    "bring",
                    "hl-1",
                    Action::MoveTo {
                        target: robot(obs, "sc-1").position,
                    },
                ));
                stage = 2;
            }
            2 if completed(std::slice::from_ref(obs), "bring").is_some() => {
                d.commands.push(cmd(
                    "mobile",
                    "sc-1",
                    Action::Charge {
                        source: "hl-1".into(),
                        until_wh: Some(1000.0),
                    },
                ));
                stage = 3;
            }
            3 if completed(std::slice::from_ref(obs), "mobile").is_some() => {
                d.commands.push(cmd(
                    "tow",
                    "hl-1",
                    Action::Tow {
                        target_robot: "cp-1".into(),
                        to: STATION,
                    },
                ));
                stage = 4;
            }
            4 if completed(std::slice::from_ref(obs), "tow").is_some() => {
                d.commands.push(cmd(
                    "station",
                    "cp-1",
                    Action::Charge {
                        source: "st-1".into(),
                        until_wh: Some(2000.0),
                    },
                ));
                stage = 5;
            }
            _ => {}
        }),
        6,
        &quiet(),
    );
    let obs = &agent.observations;
    assert!(events(obs).any(|e| matches!(
        &e.kind,
        EventKind::CommandFailed { command_id, reason: FailReason::InsufficientEnergy, .. }
            if command_id == "drain-sc"
    )));
    assert_eq!(
        rejection(obs, "far-station"),
        Some(RejectReason::RobotDisabled)
    );
    assert_eq!(
        rejection(obs, "far-charger"),
        Some(RejectReason::RobotDisabled)
    );
    let mobile = completed(obs, "mobile").expect("charges from the mobile charger");
    let after = obs.iter().find(|o| o.t >= mobile).unwrap();
    assert_eq!(robot(after, "sc-1").status, RobotStatus::Idle);
    assert!(robot(after, "sc-1").battery_wh > 900.0);
    let station = completed(obs, "station").expect("charges at the station after the tow");
    let after = obs.iter().find(|o| o.t >= station).unwrap();
    assert_eq!(robot(after, "cp-1").status, RobotStatus::Idle);
    assert_eq!(robot(after, "cp-1").position, STATION);
}

#[test]
fn getting_stuck_fails_the_drive_where_the_robot_stands() {
    let params = Params {
        stuck_per_km: 1e6,
        ..wear_only()
    };
    let target = GridCoord { x: 13, y: 58 };
    let mut sent = false;
    let (report, _, agent) = run(
        Script::new(|_: &Observation, d: &mut Decision| {
            if !sent {
                sent = true;
                d.commands
                    .push(cmd("drive", "ex-1", Action::MoveTo { target }));
            }
        }),
        1,
        &params,
    );
    let obs = &agent.observations;
    let (t, detail) = events(obs)
        .find_map(|e| match &e.kind {
            EventKind::CommandFailed {
                command_id,
                reason: FailReason::Stuck,
                detail,
                ..
            } if command_id == "drive" => Some((e.t, detail.clone())),
            _ => None,
        })
        .expect("gets stuck on soft ground");
    let at = obs.iter().find(|o| o.t >= t).unwrap();
    let ex = robot(at, "ex-1");
    assert_eq!(ex.status, RobotStatus::Stuck);
    assert_ne!(ex.position, target);
    assert!(detail.unwrap().contains("soft regolith"));
    assert!(ex.faults.is_empty(), "stuck is a condition, not a fault");
    assert_eq!(report.metrics.fleet.faults, 0);
}

#[test]
fn dust_storms_knock_out_the_sensors_of_scanning_robots() {
    let params = Params {
        storm_chance_per_sol: 1.0,
        storm_sensor_fault_per_h: 50.0,
        ..wear_only()
    };
    let pad = GridRect {
        min: GridCoord { x: 20, y: 20 },
        max: GridCoord { x: 39, y: 39 },
    };
    let mut storm = false;
    let mut n = 0;
    let (_, _, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            storm |= obs.events.iter().any(|e| {
                matches!(
                    e.kind,
                    EventKind::Weather {
                        condition: WeatherCondition::DustStorm,
                        ..
                    }
                )
            });
            let sc = robot(obs, "sc-1");
            if storm && sc.status == RobotStatus::Idle && sc.current_command.is_none() {
                n += 1;
                d.commands.push(cmd(
                    &format!("scan-{n}"),
                    "sc-1",
                    Action::Scan { area: pad },
                ));
            }
        }),
        7,
        &params,
    );
    let obs = &agent.observations;
    let storm_start = events(obs)
        .find(|e| {
            matches!(
                e.kind,
                EventKind::Weather {
                    condition: WeatherCondition::DustStorm,
                    ..
                }
            )
        })
        .map(|e| e.t)
        .expect("a storm comes");
    let fault_t = events(obs)
        .find_map(|e| match &e.kind {
            EventKind::RobotFault {
                robot_id,
                cause: FaultCause::Environmental,
                component,
                ..
            } if robot_id == "sc-1" && component == "sensors" => Some(e.t),
            _ => None,
        })
        .expect("a sensors fault during the storm");
    assert!(fault_t >= storm_start);
    assert!(!events(obs).any(|e| matches!(
        &e.kind,
        EventKind::RobotFault { cause, .. } if *cause != FaultCause::Environmental
    )));
}

#[test]
fn cold_nights_drain_idle_robots_faster() {
    let offline_at = |p: &Params| {
        let (_, records, agent) = run(Script::new(|_: &Observation, _: &mut Decision| {}), 1, p);
        let o = agent
            .observations
            .iter()
            .find(|o| robot(o, "sc-1").status == RobotStatus::Offline)
            .unwrap()
            .clone();
        (o, records)
    };
    let (cold, records) = offline_at(&Params::default());
    let (warm, _) = offline_at(&Params {
        heater_idle_factor: 0.0,
        ..Params::default()
    });
    // 1500 Wh at 8 W of idle power lasts exactly 187.5 h, and running out is a decision point.
    assert_eq!(warm.t, 1500.0 / 8.0 * 3600.0);
    assert!(warm.events.is_empty(), "{:?}", warm.events);
    assert!(cold.t < warm.t * 0.9, "{} vs {}", cold.t, warm.t);
    // The heater shows in telemetry: at midnight a robot reads warmer than the air.
    let p = Params::default();
    let midnight = truth_at(&records, 0.0);
    let sc = midnight.iter().find(|r| r.robot_id == "sc-1").unwrap();
    let ambient = -50.0 + 30.0 * (2.0 * std::f64::consts::PI * -0.375).sin();
    assert!((sc.health.temperature_c - (ambient + p.heater_offset_c)).abs() < 1e-9);
}

#[test]
fn the_run_ends_fleet_lost_when_no_robot_can_act() {
    let params = Params {
        scripted: ["sc-1", "ex-1", "hl-1", "cp-1"]
            .into_iter()
            .map(|r| fault(r, 100.0, Comp::DriveMotor, Severity::Disabled, false))
            .collect(),
        ..quiet()
    };
    let (report, _, _) = run(Idle, 1, &params);
    assert_eq!(report.end_reason, EndReason::FleetLost);
    assert_eq!(report.end_t, 100.0);
    assert_eq!(report.metrics.fleet.faults, 4);
}

#[test]
fn an_idle_fleet_drains_and_counts_its_lost_hours() {
    let (report, _, agent) = run(
        Script::new(|_: &Observation, _: &mut Decision| {}),
        1,
        &Params::default(),
    );
    assert_eq!(report.end_reason, EndReason::FleetLost);
    let m = &report.metrics;
    // Every battery ran dry and nothing was charged.
    assert!((m.energy.robot_energy_wh - (1500.0 + 6000.0 + 12000.0 + 10000.0)).abs() < 1e-6);
    assert_eq!(m.energy.grid_energy_wh, 0.0);
    // Hours lost: each robot from the moment it went offline to the end.
    let obs = &agent.observations;
    let expected: f64 = ["sc-1", "ex-1", "hl-1", "cp-1"]
        .iter()
        .map(|id| {
            let off = obs
                .iter()
                .find(|o| robot(o, id).status == RobotStatus::Offline)
                .map_or(report.end_t, |o| o.t);
            (report.end_t - off) / 3600.0
        })
        .sum();
    assert!(
        (m.fleet.robot_hours_lost - expected).abs() < 1e-6,
        "{} vs {expected}",
        m.fleet.robot_hours_lost
    );
    assert!(m.fleet.robot_hours_lost > 0.0);
}

/// Keeps every robot driving or working, charging when low.
struct Busy {
    n: u64,
}

impl Agent for Busy {
    fn welcome(&mut self, _: &Hello) -> SoftwareInfo {
        SoftwareInfo {
            name: "busy".into(),
            version: "0".into(),
        }
    }
    fn decide(&mut self, obs: &Observation) -> Decision {
        let mut d = Decision::empty(obs.seq);
        for r in &obs.robots {
            if r.current_command.is_some() || r.status != RobotStatus::Idle {
                continue;
            }
            self.n += 1;
            let k = (self.n % 7) as u32;
            let low = match r.robot_id.as_str() {
                "sc-1" => 500.0,
                "ex-1" => 2000.0,
                _ => 3500.0,
            };
            let action = if r.battery_wh < low {
                Action::Charge {
                    source: "st-1".into(),
                    until_wh: None,
                }
            } else {
                match r.robot_id.as_str() {
                    "ex-1" => Action::Dig {
                        area: GridRect {
                            min: GridCoord { x: 44, y: 20 + k },
                            max: GridCoord { x: 50, y: 20 + k },
                        },
                        target_elevation_m: -4105.0,
                        spoil_to: Some(GridCoord { x: 52, y: 40 }),
                    },
                    "hl-1" => Action::Haul {
                        from: GridCoord { x: 52, y: 40 },
                        to: GridCoord { x: 30 + k, y: 45 },
                        mass_kg: 400.0,
                    },
                    _ => Action::MoveTo {
                        target: GridCoord {
                            x: 20 + 3 * k,
                            y: if self.n.is_multiple_of(2) { 20 } else { 39 },
                        },
                    },
                }
            };
            d.commands
                .push(cmd(&format!("c{}", self.n), &r.robot_id, action));
        }
        d.wake_at = Some(obs.t + 600.0);
        d
    }
}

#[test]
fn more_wear_means_more_failures() {
    let wear_faults =
        |p: &Params| -> usize {
            (1..=5)
                .map(|seed| {
                    let (_, records, _) = run(Busy { n: 0 }, seed, p);
                    records
                    .iter()
                    .filter(|r| r.t < 2.0 * 88_775.0)
                    .filter(|r| matches!(
                        &r.entry,
                        LogEntry::Observed { event } if matches!(
                            event.kind, EventKind::RobotFault { cause: FaultCause::Wear, .. }
                        )
                    ))
                    .count()
                })
                .sum()
        };
    // Coverage of the whole site, so wear is the only thing being compared.
    let base = Params {
        random_fault_per_h: 0.0,
        storm_sensor_fault_per_h: 0.0,
        stuck_per_km: 0.0,
        base_station_range_m: 1000.0,
        ..Params::default()
    };
    let worn = Params {
        wear_wheels_per_km: base.wear_wheels_per_km * 4.0,
        wear_drive_motor_per_km: base.wear_drive_motor_per_km * 4.0,
        wear_tool_per_work_h: base.wear_tool_per_work_h * 4.0,
        wear_joints_per_work_h: base.wear_joints_per_work_h * 4.0,
        wear_joints_per_dust_h: base.wear_joints_per_dust_h * 4.0,
        wear_sensors_per_dust_h: base.wear_sensors_per_dust_h * 4.0,
        wear_battery_per_kwh: base.wear_battery_per_kwh * 4.0,
        ..base.clone()
    };
    let (few, many) = (wear_faults(&base), wear_faults(&worn));
    assert!(
        many > few * 2,
        "{few} wear faults at nominal wear, {many} at 4x"
    );
}
