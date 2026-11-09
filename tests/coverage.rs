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
    // The run metrics count what coverage cost: the two commands refused
    // while it was away, and its one return.
    assert_eq!(
        report.metrics.coverage,
        Some(CoverageMetrics {
            out_of_coverage_rejections: 2,
            lost_link_returns: 1,
        })
    );
    assert!(report.metrics.commands_rejected >= 2);

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

// ----- other robots in acceptance checks (semantics §4.1, §8.5) -----
//
// A check that looks at a robot other than the one commanded judges it by
// its true state while it is in coverage, and as it was at last contact
// while it is out of coverage. So a rejection, an estimate or a drive never
// tells the coordinator what it has not seen.

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

fn cells_apart(a: GridCoord, b: GridCoord) -> f64 {
    (f64::from(a.x) - f64::from(b.x)).hypot(f64::from(a.y) - f64::from(b.y))
}

fn rejected_because(observations: &[Observation], command: &str) -> String {
    events(observations)
        .find_map(|e| match &e.kind {
            EventKind::CommandRejected {
                command_id, detail, ..
            } if command_id == command => detail.clone(),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{command} was not rejected"))
}

fn accepted_estimate(observations: &[Observation], command: &str) -> f64 {
    events(observations)
        .find_map(|e| match &e.kind {
            EventKind::CommandAccepted {
                command_id,
                estimated_duration_s,
                ..
            } if command_id == command => Some(*estimated_duration_s),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{command} was not accepted"))
}

/// Every record of an event about `command`, delivered or not.
fn logged<'a>(records: &'a [LogRecord], command: &str) -> Vec<&'a Event> {
    records
        .iter()
        .filter_map(|r| match &r.entry {
            LogEntry::Observed { event } => Some(event),
            _ => None,
        })
        .filter(|e| match &e.kind {
            EventKind::CommandAccepted { command_id, .. }
            | EventKind::CommandRejected { command_id, .. }
            | EventKind::CommandCompleted { command_id, .. }
            | EventKind::CommandFailed { command_id, .. } => command_id == command,
            _ => false,
        })
        .collect()
}

#[test]
fn a_robot_that_fails_out_of_coverage_is_judged_as_it_was_at_last_contact() {
    // ex-1 drives north out of coverage, and breaks down out there. The site
    // last heard it moving, with its command: that is all a check may use.
    let p = Params {
        scripted: vec![fault("ex-1", 600.0, Comp::Tool, Severity::Disabled, true)],
        ..quiet()
    };
    let mut stage = 0;
    let mut asked_at = f64::NAN;
    let (_, records, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            match stage {
                0 => {
                    d.commands
                        .push(cmd("go", "ex-1", Action::MoveTo { target: NORTH }));
                    // Cannibalizing is level 3: ask, with no time to answer and fallback proceed.
                    d.to_earth.push(ask_and_proceed("q", "cannibalize", obs.t));
                    stage = 1;
                }
                1 if obs.t >= 900.0 => {
                    assert!(!robot(obs, "ex-1").in_relay_coverage);
                    asked_at = obs.t;
                    d.commands.push(cmd(
                        "fix",
                        "sc-1",
                        Action::Repair {
                            target_robot: "ex-1".into(),
                        },
                    ));
                    d.commands.push(cmd(
                        "pull",
                        "hl-1",
                        Action::Tow {
                            target_robot: "ex-1".into(),
                            to: GridCoord { x: 8, y: 14 },
                        },
                    ));
                    d.to_earth.push(notify("n", "tow", &["pull"]));
                    d.commands.push(approved(
                        cmd(
                            "strip",
                            "sc-1",
                            Action::Cannibalize {
                                target_robot: "ex-1".into(),
                                beneficiary_robot: None,
                            },
                        ),
                        "q",
                    ));
                    stage = 2;
                }
                _ => {}
            }
            d.wake_at = Some(obs.t + 300.0);
        }),
        1,
        &p,
    );
    let obs = &agent.observations;
    // In truth it has stood failed at the far end, with no command, since
    // t = 600: on its true state all three commands would have been taken.
    let truth = status_of(&records, asked_at, "ex-1");
    assert_eq!(
        (truth.status, truth.position, truth.current_command.clone()),
        (RobotStatus::Failed, NORTH, None)
    );
    assert!(!truth.in_relay_coverage);
    // The coordinator still sees it as it lost contact: moving, on its way.
    let seen = obs
        .iter()
        .find(|o| o.t == asked_at)
        .map(|o| robot(o, "ex-1").clone())
        .unwrap();
    assert_eq!(seen.status, RobotStatus::Moving);
    assert_eq!(seen.current_command.as_deref(), Some("go"));
    assert!(seen.faults.is_empty());
    // And that is what the checks judged it by.
    for id in ["fix", "pull", "strip"] {
        assert_eq!(
            rejection(obs, id),
            Some(RejectReason::InvalidTarget),
            "{id}"
        );
    }
    assert!(rejected_because(obs, "fix").contains("ex-1 is moving"));
    assert!(rejected_because(obs, "pull").contains("ex-1 is moving"));
    assert!(rejected_because(obs, "strip").contains("neither failed nor offline"));
}

#[test]
fn a_repair_is_accepted_and_estimated_on_last_contact_state_and_done_on_the_truth() {
    // ex-1 goes north under a relay's coverage with a degraded sensor, and
    // stops there. The relay is withdrawn, and then its tool breaks down.
    // The site last heard it idle, with the one degraded fault.
    let p = Params {
        scripted: vec![
            fault("ex-1", 10.0, Comp::Sensors, Severity::Degraded, true),
            fault("ex-1", 1000.0, Comp::Tool, Severity::Disabled, true),
        ],
        ..quiet()
    };
    let mut stage = 0;
    let mut asked: Option<Observation> = None;
    let (_, records, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            match stage {
                0 => {
                    // hl-1's relay reaches 150 m: the whole site.
                    d.commands.push(cmd(
                        "relay",
                        "hl-1",
                        Action::Relay {
                            position: robot(obs, "hl-1").position,
                        },
                    ));
                    d.commands
                        .push(cmd("go", "ex-1", Action::MoveTo { target: NORTH }));
                    stage = 1;
                }
                1 if completed(std::slice::from_ref(obs), "go").is_some() => {
                    assert!(robot(obs, "ex-1").in_relay_coverage);
                    d.commands.push(cmd(
                        "cut",
                        "hl-1",
                        Action::Cancel {
                            target_command: "relay".into(),
                        },
                    ));
                    stage = 2;
                }
                2 if obs.t >= 1200.0 => {
                    asked = Some(obs.clone());
                    d.commands.push(cmd(
                        "fix",
                        "sc-1",
                        Action::Repair {
                            target_robot: "ex-1".into(),
                        },
                    ));
                    stage = 3;
                }
                _ => {}
            }
            d.wake_at = Some(obs.t + 300.0);
        }),
        1,
        &p,
    );
    let obs = &agent.observations;
    let asked = asked.expect("the repair was sent");
    // What the site knew: idle at the far end, one degraded fault.
    let seen = robot(&asked, "ex-1");
    assert!(!seen.in_relay_coverage);
    assert_eq!((seen.status, seen.position), (RobotStatus::Idle, NORTH));
    assert_eq!(seen.faults.len(), 1);
    assert_eq!(seen.faults[0].severity, Severity::Degraded);
    // What was true: failed since t = 1000, with two faults.
    let truth = status_of(&records, asked.t, "ex-1");
    assert_eq!(truth.status, RobotStatus::Failed);
    assert_eq!(truth.faults.len(), 2);

    // Accepted on what the site knew, and estimated from it: the drive, and
    // the work for the one fault it could see (0.25 units at 0.0005 a
    // second), not for the two there really are (1.25 units).
    assert_eq!(rejection(obs, "fix"), None);
    let drive_s = cells_apart(robot(&asked, "sc-1").position, NORTH) / 0.5;
    let estimate = accepted_estimate(obs, "fix");
    assert!(
        (estimate - (drive_s + 0.25 / 0.0005)).abs() < 1e-6,
        "{estimate} vs {drive_s} + 500"
    );
    // On arrival the scout finds the truth and repairs all of it. It is out
    // of coverage itself by then, so the log has what the site never heard.
    let done = logged(&records, "fix")
        .into_iter()
        .find(|e| matches!(e.kind, EventKind::CommandCompleted { .. }))
        .expect("the repair completes")
        .t;
    assert!(
        done - asked.t > drive_s + 1.25 / 0.0005 * 0.6,
        "the work was for both faults: {} s",
        done - asked.t
    );
    // Nobody is woken by an event out of coverage, so the truth record to
    // look at is the next wake-up's, 300 s on at most.
    let repaired = status_of(&records, done + 300.0, "ex-1");
    assert_eq!(repaired.status, RobotStatus::Idle);
    assert!(repaired.faults.is_empty(), "{:?}", repaired.faults);
}

#[test]
fn a_drive_to_a_robot_out_of_coverage_heads_for_its_last_contact_cell() {
    // The scout strips failed cp-1 for parts and takes them to ex-1, which
    // has meanwhile driven north out of coverage. The second drive starts
    // long after the command was accepted, and still heads for where the
    // site last heard ex-1, not for where it is.
    let p = Params {
        scripted: vec![fault("cp-1", 10.0, Comp::Wheels, Severity::Disabled, false)],
        ..quiet()
    };
    let mut stage = 0;
    let mut asked: Option<Observation> = None;
    let (report, records, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            match stage {
                0 => {
                    d.commands
                        .push(cmd("go", "ex-1", Action::MoveTo { target: NORTH }));
                    d.to_earth.push(ask_and_proceed("q", "cannibalize", obs.t));
                    stage = 1;
                }
                1 if !robot(obs, "ex-1").in_relay_coverage => {
                    assert_eq!(robot(obs, "cp-1").status, RobotStatus::Failed);
                    asked = Some(obs.clone());
                    d.commands.push(approved(
                        cmd(
                            "parts",
                            "sc-1",
                            Action::Cannibalize {
                                target_robot: "cp-1".into(),
                                beneficiary_robot: Some("ex-1".into()),
                            },
                        ),
                        "q",
                    ));
                    stage = 2;
                }
                _ => {}
            }
            d.wake_at = Some(obs.t + 30.0);
        }),
        1,
        &p,
    );
    let obs = &agent.observations;
    let asked = asked.expect("the command was sent");
    // Where the site last heard ex-1: just past the edge of coverage, on its way.
    let last_heard = robot(&asked, "ex-1").position;
    assert!(cells_apart(last_heard, NORTH) > 3.0, "{last_heard:?}");
    let (sc, cp) = (
        robot(&asked, "sc-1").position,
        robot(&asked, "cp-1").position,
    );
    // The estimate is planned on that cell: both drives at 0.5 m/s, and 0.5
    // + 1.0 units of work at 0.0005 a second.
    let estimate = accepted_estimate(obs, "parts");
    let planned = (cells_apart(sc, cp) + cells_apart(cp, last_heard)) / 0.5 + (0.5 + 1.0) / 0.0005;
    assert!((estimate - planned).abs() < 1e-6, "{estimate} vs {planned}");

    // The strip goes ahead: cp-1 is lost. Then the scout drives to where
    // ex-1 was last heard, and finds nobody within reach.
    let failed = logged(&records, "parts")
        .into_iter()
        .find_map(|e| match &e.kind {
            EventKind::CommandFailed { reason, .. } => Some((e.t, *reason)),
            _ => None,
        })
        .expect("the command fails");
    assert_eq!(failed.1, FailReason::TargetLost);
    // It failed out of coverage, which wakes nobody: the truth record to
    // look at is the next wake-up's, 30 s on at most. The scout stays put
    // there, and ex-1 at the far end, until their lost-link timeouts.
    let at_failure = truth_at(&records, failed.0 + 30.0);
    let of = |id: &str| at_failure.iter().find(|r| r.robot_id == id).unwrap();
    assert_eq!(
        of("sc-1").position,
        last_heard,
        "it went to the last-contact cell"
    );
    assert_eq!(of("ex-1").position, NORTH, "ex-1 was somewhere else");
    assert_eq!(of("cp-1").status, RobotStatus::Lost);
    assert_eq!(report.metrics.fleet.robots_lost, 1);
}
