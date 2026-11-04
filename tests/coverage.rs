//! Site coverage end to end (semantics §8.4–§8.5): robots out of coverage,
//! held-back events, last-contact snapshots, lost-link returns and relays.

mod support;

use arcadia_protocol::*;
use arcadiabench::scenario;
use arcadiabench::sim::faults::Comp;
use arcadiabench::sim::params::{Incident, Params, ScriptedIncident};
use arcadiabench::sim::{Advance, Sim};
use support::*;

/// North of the pad, out of the base station's coverage.
const NORTH: GridCoord = GridCoord { x: 30, y: 45 };
/// The first covered cell on the way back from there to the base station at (30, 8).
const EDGE: GridCoord = GridCoord { x: 30, y: 41 };

fn status_of(records: &[LogRecord], t: f64, id: &str) -> RobotState {
    truth_at(records, t)
        .into_iter()
        .find(|r| r.robot_id == id)
        .unwrap()
}

#[test]
fn a_robot_out_of_coverage_keeps_working_holds_its_events_and_comes_back() {
    let p = Params {
        scripted: vec![ScriptedIncident {
            robot_id: "sc-1".into(),
            t_s: 500.0,
            incident: Incident::Fault {
                component: Comp::Sensors,
                severity: Severity::Degraded,
                cause: FaultCause::Random,
                field_repairable: true,
            },
        }],
        ..quiet()
    };
    let mut stage = 0;
    let mut lost_at = f64::NAN;
    let (report, text, agent) = run_raw(
        Script::new(|obs: &Observation, d: &mut Decision| {
            let sc = robot(obs, "sc-1");
            match stage {
                0 => {
                    d.commands
                        .push(cmd("go", "sc-1", Action::MoveTo { target: NORTH }));
                    stage = 1;
                }
                1 if !sc.in_relay_coverage => {
                    lost_at = sc.telemetry_t;
                    d.commands
                        .push(cmd("again", "sc-1", Action::MoveTo { target: EDGE }));
                    d.commands.push(cmd(
                        "stop",
                        "sc-1",
                        Action::Cancel {
                            target_command: "go".into(),
                        },
                    ));
                    // Robots in coverage still take commands.
                    d.commands
                        .push(cmd("fine", "ex-1", Action::Wait { duration_s: 10.0 }));
                    stage = 2;
                }
                _ => {}
            }
            // Look in every half minute, and just after the lost-link timeout.
            let mut wake = obs.t + 30.0;
            if stage == 2 && obs.t < lost_at + 1800.0 {
                wake = wake.min(lost_at + 1801.0);
            }
            d.wake_at = Some(wake);
        }),
        1,
        &p,
    );
    assert_valid_log(&text);
    let records: Vec<LogRecord> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let obs = &agent.observations;
    assert_eq!(rejection(obs, "again"), Some(RejectReason::OutOfCoverage));
    assert_eq!(rejection(obs, "stop"), Some(RejectReason::OutOfCoverage));
    assert_eq!(rejection(obs, "fine"), None);

    // While out of coverage the coordinator sees the robot as it lost contact.
    let out: Vec<&Observation> = obs
        .iter()
        .filter(|o| !robot(o, "sc-1").in_relay_coverage)
        .collect();
    assert!(out.len() > 50, "{}", out.len());
    let first = robot(out[0], "sc-1").clone();
    assert_eq!(first.telemetry_t, lost_at);
    assert_eq!(first.status, RobotStatus::Moving);
    assert_eq!(first.current_command.as_deref(), Some("go"));
    assert!(
        out.iter().all(|o| *robot(o, "sc-1") == first),
        "a frozen snapshot"
    );
    // Meanwhile it kept going, and finished the drive out there.
    let done = events(obs)
        .find_map(|e| match &e.kind {
            EventKind::CommandCompleted { command_id, .. } if command_id == "go" => Some(e.t),
            _ => None,
        })
        .expect("the completion is delivered eventually");
    let truth = status_of(&records, done + 60.0, "sc-1");
    assert_eq!((truth.position, truth.status), (NORTH, RobotStatus::Idle));
    assert!(!truth.in_relay_coverage);
    assert!(
        !obs.iter().any(|o| o.t == done),
        "a held event wakes nobody"
    );

    // The lost-link timeout runs out: it drives back toward the base station
    // with no command, and stops at the edge of coverage.
    let returning = status_of(&records, lost_at + 1801.0, "sc-1");
    assert_eq!(returning.status, RobotStatus::Moving);
    assert_eq!(returning.current_command, None);
    assert!(!returning.in_relay_coverage);
    let back = obs
        .iter()
        .find(|o| o.t > lost_at && robot(o, "sc-1").in_relay_coverage)
        .expect("back in coverage");
    let sc = robot(back, "sc-1");
    assert_eq!(
        (sc.position, sc.status, sc.current_command.clone()),
        (EDGE, RobotStatus::Idle, None)
    );
    assert_eq!(sc.telemetry_t, back.t);
    assert!(
        back.t > lost_at + 1800.0 && back.t < lost_at + 1830.0,
        "{}",
        back.t
    );
    assert_eq!(report.lost_link_returns, 1);

    // What it held back arrives on its return, with the times it happened, in order.
    let held: Vec<&Event> = back
        .events
        .iter()
        .filter(|e| matches!(&e.kind, EventKind::CommandCompleted { robot_id, .. } | EventKind::RobotFault { robot_id, .. } if robot_id == "sc-1"))
        .collect();
    assert_eq!(held.len(), 2, "{:?}", back.events);
    assert_eq!(held[0].t, done);
    assert_eq!(held[1].t, 500.0);
    assert!(matches!(held[1].kind, EventKind::RobotFault { .. }));
    // The log records them when they were delivered.
    let logged: Vec<f64> = records
        .iter()
        .filter_map(|r| match &r.entry {
            LogEntry::Observed { event } if event.t == 500.0 => Some(r.t),
            _ => None,
        })
        .collect();
    assert_eq!(logged, [back.t]);
}

/// Run a simulator directly, with `decide` answering each observation.
fn drive(mut sim: Sim, mut decide: impl FnMut(&Observation, &Sim) -> Decision) -> Vec<Observation> {
    let mut seen = Vec::new();
    while let Advance::Observe(obs) = sim.advance() {
        let d = decide(&obs, &sim);
        sim.apply(&d);
        seen.push(obs);
    }
    seen
}

#[test]
fn relays_extend_coverage_transitively() {
    let p = quiet();
    let mut g = scenario::generate(1, &p);
    // Short relays, so it takes a chain: hl-1's cut to 15 m, and one for the scout.
    for r in &mut g.scenario.robots {
        for c in &mut r.capabilities {
            if c.name == CapabilityName::Relay {
                c.range_m = Some(15.0);
            }
        }
    }
    g.scenario.robots[0].capabilities.push(CapabilitySpec {
        name: CapabilityName::Relay,
        rate: 0.0,
        energy_wh_per_unit: 0.0,
        capacity: None,
        range_m: Some(15.0),
    });
    // hl-1 holds inside the base station's 33 m, the scout 13 m beyond it,
    // and cp-1 goes 7 m past the scout: 20 m from hl-1 and 51 m from the base.
    let first_relay = GridCoord { x: 30, y: 39 };
    let second_relay = GridCoord { x: 30, y: 52 };
    let far = GridCoord { x: 30, y: 59 };
    let mut stage = 0;
    let mut chain_up = None;
    let mut cut = None;
    let obs = drive(Sim::new(g, 1, p), |obs, sim| {
        let mut d = Decision::empty(obs.seq);
        let cp = robot(obs, "cp-1");
        match stage {
            0 => {
                d.commands.push(cmd(
                    "r1",
                    "hl-1",
                    Action::Relay {
                        position: first_relay,
                    },
                ));
                d.commands.push(cmd(
                    "r2",
                    "sc-1",
                    Action::Relay {
                        position: second_relay,
                    },
                ));
                d.commands
                    .push(cmd("go", "cp-1", Action::MoveTo { target: far }));
                stage = 1;
            }
            1 if completed(std::slice::from_ref(obs), "go").is_some() => {
                assert!(cp.in_relay_coverage && cp.position == far);
                chain_up = Some(obs.t);
                let truth = sim.truth_robots();
                assert!(truth.iter().all(|r| r.in_relay_coverage), "{truth:?}");
                // Break the chain at its root: everything past it is cut off,
                // even for the next command in the same decision.
                d.commands.push(cmd(
                    "cut",
                    "hl-1",
                    Action::Cancel {
                        target_command: "r1".into(),
                    },
                ));
                d.commands
                    .push(cmd("late", "cp-1", Action::Wait { duration_s: 1.0 }));
                stage = 2;
            }
            2 => {
                cut = Some(obs.clone());
                stage = 3;
            }
            _ => {}
        }
        d
    });
    assert!(chain_up.is_some(), "cp-1 got there in coverage");
    assert_eq!(rejection(&obs, "late"), Some(RejectReason::OutOfCoverage));
    let cut = cut.expect("an observation after the cut");
    assert!(robot(&cut, "hl-1").in_relay_coverage);
    assert!(
        !robot(&cut, "sc-1").in_relay_coverage,
        "a relay out of coverage extends nothing"
    );
    assert!(!robot(&cut, "cp-1").in_relay_coverage);
    // The scout is still holding: its relay never completes on its own.
    assert_eq!(robot(&cut, "sc-1").current_command.as_deref(), Some("r2"));
}

#[test]
fn a_relay_holds_until_cancelled_and_draws_radio_power() {
    let battery_at = |radio_w: f64| {
        let p = Params {
            relay_radio_w: radio_w,
            ..quiet()
        };
        let mut stage = 0;
        let (_, records, agent) = run(
            Script::new(|obs: &Observation, d: &mut Decision| {
                match stage {
                    0 => {
                        // Already there: it holds at once.
                        d.commands.push(cmd(
                            "relay",
                            "hl-1",
                            Action::Relay {
                                position: GridCoord { x: 14, y: 10 },
                            },
                        ));
                        stage = 1;
                    }
                    1 if obs.t >= 40_000.0 => {
                        d.commands.push(cmd(
                            "stop",
                            "hl-1",
                            Action::Cancel {
                                target_command: "relay".into(),
                            },
                        ));
                        stage = 2;
                    }
                    _ => {}
                }
                d.wake_at = Some(obs.t + 10_000.0);
            }),
            2,
            &p,
        );
        let obs = &agent.observations;
        // Holding the whole time, and only the cancel ends it.
        assert!(
            obs.iter()
                .filter(|o| o.t > 0.0 && o.t < 40_000.0)
                .all(|o| robot(o, "hl-1").status == RobotStatus::Working
                    && robot(o, "hl-1").current_command.as_deref() == Some("relay"))
        );
        assert!(events(obs).any(|e| matches!(&e.kind,
            EventKind::CommandFailed { command_id, reason: FailReason::Cancelled, .. } if command_id == "relay")));
        // The last ground truth before the cancel, and when it was taken.
        let t = records
            .iter()
            .filter(|r| r.t < 40_000.0 && matches!(r.entry, LogEntry::Truth { .. }))
            .map(|r| r.t)
            .next_back()
            .unwrap();
        (t, status_of(&records, t, "hl-1").battery_wh)
    };
    let ((t, with), (t0, without)) = (battery_at(25.0), battery_at(0.0));
    assert_eq!(t, t0);
    assert!(t > 30_000.0);
    // Holding since t = 0: the radio is the only difference.
    let expected = 25.0 * t / 3600.0;
    assert!(
        ((without - with) - expected).abs() < 1e-6,
        "{} vs {expected}",
        without - with
    );
}
