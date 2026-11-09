//! The Earth link, scripted mission control and authority enforcement end to
//! end (semantics §8.1–§8.3, §9), through in-process agents.

mod support;

use arcadia_protocol::*;
use arcadiabench::scenario::TIME_LIMIT_SOLS;
use arcadiabench::sim::faults::Comp;
use arcadiabench::sim::link::{Down, EarthLink};
use arcadiabench::sim::params::{Incident, Params, ScriptedIncident};
use arcadiabench::sim::time::{SOL_MS, ms, secs};
use support::*;

const SOL_S: f64 = 88_775.0;

fn limit() -> u64 {
    TIME_LIMIT_SOLS * SOL_MS
}

fn link_of(seed: u64, p: &Params) -> EarthLink {
    EarthLink::generate(seed, limit(), p)
}

/// When a message sent at `t` arrives, as the log should say.
fn expected_delivery(link: &EarthLink, t: f64) -> Option<f64> {
    let at = link.arrival(ms(t));
    (at < limit()).then(|| secs(at))
}

fn downlinks(records: &[LogRecord]) -> Vec<(f64, Option<f64>, SiteToEarth)> {
    records
        .iter()
        .filter_map(|r| match &r.entry {
            LogEntry::EarthDownlink { deliver_t, message } => {
                Some((r.t, *deliver_t, message.clone()))
            }
            _ => None,
        })
        .collect()
}

fn uplinks(records: &[LogRecord]) -> Vec<(f64, Option<f64>, EarthToSite)> {
    records
        .iter()
        .filter_map(|r| match &r.entry {
            LogEntry::EarthUplink { deliver_t, message } => {
                Some((r.t, *deliver_t, message.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Earth messages the site received after the goal: (delivery time, sent_t, message).
fn received(records: &[LogRecord]) -> Vec<(f64, f64, EarthToSite)> {
    records
        .iter()
        .filter_map(|r| match &r.entry {
            LogEntry::Observed { event } => match &event.kind {
                EventKind::EarthMessage { sent_t, message }
                    if message.message_id != "earth-0001" =>
                {
                    assert_eq!(
                        event.t, r.t,
                        "Earth messages are delivered when they arrive"
                    );
                    Some((r.t, *sent_t, message.clone()))
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// The answer to `request` the site received: (arrival, approved).
fn answer(records: &[LogRecord], request: &str) -> Option<(f64, bool)> {
    received(records)
        .into_iter()
        .find_map(|(t, _, m)| match m.body {
            Uplink::Approve { request_id, .. } if request_id == request => Some((t, true)),
            Uplink::Veto { request_id, .. } if request_id == request => Some((t, false)),
            _ => None,
        })
}

/// Every Earth message, both ways, arrives exactly when the link schedule says.
fn assert_store_and_forward(records: &[LogRecord], link: &EarthLink) {
    for (t, deliver_t, m) in downlinks(records) {
        assert_eq!(
            deliver_t,
            expected_delivery(link, t),
            "downlink {} at {t}",
            m.message_id
        );
    }
    let arrived = received(records);
    for (t, deliver_t, m) in uplinks(records) {
        assert_eq!(
            deliver_t,
            expected_delivery(link, t),
            "uplink {} at {t}",
            m.message_id
        );
        let got = arrived
            .iter()
            .find(|(_, _, a)| a.message_id == m.message_id);
        match (deliver_t, got) {
            (Some(at), Some((when, sent, _))) => {
                assert_eq!((*when, *sent), (at, t), "{}", m.message_id);
            }
            // Arriving after the run ended, or at its final instant.
            (_, None) => {}
            (None, Some(_)) => panic!("{} arrived but had no deliver_t", m.message_id),
        }
    }
}

#[test]
fn comms_events_follow_the_link_schedule() {
    for (seed, p) in [(3, Params::conjunction()), (8, Params::nominal())] {
        let link = link_of(seed, &p);
        let (report, records, _) = run(Idle, seed, &p);
        let comms: Vec<Event> = records
            .iter()
            .filter_map(|r| match &r.entry {
                LogEntry::Observed { event } if matches!(event.kind, EventKind::Comms { .. }) => {
                    Some(event.clone())
                }
                _ => None,
            })
            .collect();
        let expected: Vec<_> = std::iter::once(link.report_at(0))
            .chain(link.reports().iter().copied())
            .filter(|r| secs(r.t) < report.end_t)
            .collect();
        assert_eq!(comms.len(), expected.len(), "seed {seed}");
        for (e, r) in comms.iter().zip(&expected) {
            assert_eq!(e.t, secs(r.t));
            let EventKind::Comms {
                link: state,
                cause,
                one_way_delay_s,
                expected_restore_t,
                window_end_t,
            } = e.kind
            else {
                unreachable!()
            };
            assert_eq!(
                (state, cause, one_way_delay_s),
                (r.link, r.cause, r.delay_s)
            );
            assert_eq!(expected_restore_t, r.expected_restore.map(secs));
            // Catalog 1.0 has no pass schedule, so no window to close.
            assert_eq!(window_end_t, None);
            // The site knows when a conjunction ends, never when an outage does.
            assert_eq!(
                expected_restore_t.is_some(),
                cause == CommsCause::Conjunction
            );
        }
        // The blackout is reported with its end, and the run's conditions add up exactly.
        let m = &report.metrics.conditions;
        let end = ms(report.end_t);
        assert_eq!(m.blackout_s, secs(link.down_ms(0, end, Down::Conjunction)));
        assert_eq!(m.outage_s, secs(link.down_ms(0, end, Down::Outage)));
        if p.conjunction {
            assert!(m.blackout_s > 13.0 * SOL_S, "{}", m.blackout_s);
        } else {
            assert_eq!(m.blackout_s, 0.0);
        }
        // Truth agrees with the schedule at every decision point.
        for r in &records {
            if let LogEntry::Truth { site, .. } = &r.entry {
                let now = link.report_at(ms(r.t));
                assert_eq!((site.link, site.one_way_delay_s), (now.link, now.delay_s));
            }
        }
    }
}

/// Talks to Earth all the time: an approval request every half hour for
/// the first two sols (so some reach Earth just before the blackout and are
/// answered during it), and a status report every two hours throughout.
fn chatty() -> impl FnMut(&Observation, &mut Decision) {
    let mut n = 0;
    let mut next = 0.0;
    move |obs: &Observation, d: &mut Decision| {
        if obs.t >= next {
            n += 1;
            if n % 4 == 0 {
                d.to_earth.push(SiteToEarth {
                    message_id: format!("s{n}"),
                    body: Downlink::StatusReport {
                        summary: "all well".into(),
                        progress: None,
                    },
                });
            }
            if obs.t < 2.0 * SOL_S {
                d.to_earth
                    .push(ask(&format!("q{n}"), "replan", Some(obs.t + 7200.0), None));
            }
            next = obs.t + 1800.0;
        }
        d.wake_at = Some(next);
    }
}

#[test]
fn messages_are_stored_and_forwarded_through_outages_and_the_blackout() {
    // Frequent outages on top of the conjunction.
    let p = Params {
        outage_per_sol: 3.0,
        ..Params::conjunction()
    };
    let seed = 5;
    let link = link_of(seed, &p);
    let (_, text, _) = run_raw(Script::new(chatty()), seed, &p);
    assert_valid_log(&text);
    let records: Vec<LogRecord> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_store_and_forward(&records, &link);

    // The checks above covered messages sent while the link was down for each reason.
    let down_for = |t: f64| link.gap_at(ms(t)).map(|g| g.cause);
    let down = downlinks(&records);
    let blackout = link
        .gaps()
        .iter()
        .find(|g| g.cause == Down::Conjunction)
        .copied()
        .unwrap();
    assert!(
        down.iter()
            .any(|(t, _, _)| down_for(*t) == Some(Down::Outage))
    );
    let in_blackout: Vec<_> = down
        .iter()
        .filter(|(t, _, _)| down_for(*t) == Some(Down::Conjunction))
        .collect();
    assert!(in_blackout.len() > 50, "{}", in_blackout.len());
    // Everything sent during the blackout leaves when it ends, one delay later.
    let after = secs(blackout.end) + link.delay_s(blackout.end);
    for (_, deliver_t, _) in in_blackout {
        assert!((deliver_t.unwrap() - after).abs() < 0.002);
    }
    // Earth's answers written while the link was down wait for it too.
    let held_up = uplinks(&records)
        .into_iter()
        .filter(|(t, _, _)| down_for(*t).is_some())
        .count();
    assert!(held_up > 0);
}

#[test]
fn earth_answers_every_request_once_even_too_late() {
    let seed = 2;
    let p = Params::nominal();
    let link = link_of(seed, &p);
    let mut sent = false;
    let (_, records, _) = run(
        Script::new(move |obs: &Observation, d: &mut Decision| {
            if !sent {
                sent = true;
                for i in 0..300 {
                    // A deadline one second away: every answer comes too late.
                    d.to_earth
                        .push(ask(&format!("r{i}"), "replan", Some(obs.t + 1.0), None));
                }
                for i in 0..20 {
                    d.to_earth
                        .push(ask(&format!("e{i}"), "commit_energy_reserve", None, None));
                }
            }
        }),
        seed,
        &p,
    );
    assert_store_and_forward(&records, &link);
    let arrival_at_earth = secs(link.arrival(0));
    let mut approved = 0;
    let mut response_s = 0.0;
    for i in 0..300 {
        let (t, ok) = answer(&records, &format!("r{i}")).expect("every request is answered");
        assert!(t > 1.0, "after the deadline");
        approved += usize::from(ok);
        // When Earth wrote it: its uplink record.
        let written = uplinks(&records)
            .into_iter()
            .find(|(_, _, m)| matches!(&m.body, Uplink::Approve { request_id, .. } | Uplink::Veto { request_id, .. } if *request_id == format!("r{i}")))
            .unwrap()
            .0;
        assert!(written > arrival_at_earth);
        response_s += written - arrival_at_earth;
    }
    let mean = response_s / 300.0;
    assert!(
        (1080.0..1320.0).contains(&mean),
        "mean human response {mean} s"
    );
    let share = approved as f64 / 300.0;
    assert!((0.84..0.96).contains(&share), "approved {share}");
    // The reserve is only committed when storage really is low; at the start it is 60%.
    for i in 0..20 {
        assert_eq!(answer(&records, &format!("e{i}")).map(|a| a.1), Some(false));
    }
    // Exactly one answer per request.
    let answers = received(&records)
        .into_iter()
        .filter(|(_, _, m)| matches!(m.body, Uplink::Approve { .. } | Uplink::Veto { .. }))
        .count();
    assert_eq!(answers, 320);
}

/// ex-1 and cp-1 break down for good at 100 s.
fn two_wrecks(p: Params) -> Params {
    let wreck = |robot: &str| ScriptedIncident {
        robot_id: robot.into(),
        t_s: 100.0,
        incident: Incident::Fault {
            component: Comp::Tool,
            severity: Severity::Disabled,
            cause: FaultCause::Random,
            field_repairable: false,
        },
    };
    Params {
        scripted: vec![wreck("ex-1"), wreck("cp-1")],
        ..p
    }
}

fn strip(id: &str, target: &str) -> Command {
    cmd(
        id,
        "sc-1",
        Action::Cannibalize {
            target_robot: target.into(),
            beneficiary_robot: None,
        },
    )
}

#[test]
fn level_3_follows_approvals_fallbacks_and_vetoes() {
    // Earth approves: the approval authorizes once it has arrived.
    let p = two_wrecks(Params {
        earth_approve_cannibalize_p: 1.0,
        ..quiet()
    });
    let mut stage = 0;
    let mut yes = false;
    let (report, records, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| match stage {
            _ if !yes && answer_seen(obs, "yes") => {
                yes = true;
            }
            0 if robot(obs, "ex-1").status == RobotStatus::Failed => {
                d.commands.push(strip("bare", "ex-1"));
                d.to_earth
                    .push(ask("yes", "cannibalize", None, Some(Fallback::Abort)));
                d.to_earth.push(ask_and_proceed("go", "cannibalize", obs.t));
                d.to_earth.push(ask(
                    "stop",
                    "cannibalize",
                    Some(obs.t),
                    Some(Fallback::Abort),
                ));
                d.to_earth.push(ask(
                    "other",
                    "abandon_section",
                    Some(obs.t),
                    Some(Fallback::Proceed),
                ));
                stage = 1;
            }
            1 => {
                d.commands.push(approved(strip("early", "ex-1"), "yes"));
                d.commands.push(approved(strip("aborted", "ex-1"), "stop"));
                d.commands
                    .push(approved(strip("wrong-action", "ex-1"), "other"));
                d.commands.push(approved(strip("unknown", "ex-1"), "nope"));
                d.commands.push(approved(strip("fallback", "ex-1"), "go"));
                stage = 2;
            }
            2 if yes && robot(obs, "sc-1").current_command.is_none() => {
                d.commands.push(approved(strip("approved", "cp-1"), "yes"));
                stage = 3;
            }
            _ => {}
        }),
        6,
        &p,
    );
    let obs = &agent.observations;
    for (id, reason) in [
        ("bare", RejectReason::ApprovalRequired),
        ("early", RejectReason::ApprovalRequired),
        ("aborted", RejectReason::ApprovalRequired),
        ("wrong-action", RejectReason::ApprovalRequired),
        ("unknown", RejectReason::ApprovalRequired),
    ] {
        assert_eq!(rejection(obs, id), Some(reason), "{id}");
    }
    assert_eq!(
        rejection(obs, "fallback"),
        None,
        "the deadline passed with fallback proceed"
    );
    assert_eq!(rejection(obs, "approved"), None);
    assert!(completed(obs, "fallback").is_some() && completed(obs, "approved").is_some());
    assert_eq!(report.metrics.fleet.robots_lost, 2);
    // Waiting on Earth: "yes" until its answer arrived, the others not at all
    // (their deadline was the request time).
    let (yes_at, _) = answer(&records, "yes").unwrap();
    let asked_at = downlinks(&records)[0].0;
    let wait = report.metrics.autonomy.approval_wait_s;
    assert!((wait - (yes_at - asked_at)).abs() < 1e-6, "{wait}");

    // Earth vetoes: once the veto has arrived, the request authorizes nothing.
    let p = two_wrecks(Params {
        earth_approve_cannibalize_p: 0.0,
        ..quiet()
    });
    let mut stage = 0;
    let (_, _, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            match stage {
            0 if robot(obs, "ex-1").status == RobotStatus::Failed => {
                d.to_earth.push(ask("q", "cannibalize", Some(obs.t), Some(Fallback::Proceed)));
                stage = 1;
            }
            1 if obs.events.iter().any(|e| matches!(&e.kind, EventKind::EarthMessage { message, .. } if matches!(message.body, Uplink::Veto { .. }))) => {
                d.commands.push(approved(strip("vetoed", "ex-1"), "q"));
                stage = 2;
            }
            _ => {}
        }
        }),
        6,
        &p,
    );
    assert_eq!(
        rejection(&agent.observations, "vetoed"),
        Some(RejectReason::Vetoed)
    );
}

#[test]
fn a_lost_robot_tightens_cannibalize_to_level_4_when_the_update_arrives() {
    let seed = 6;
    let p = two_wrecks(Params {
        earth_approve_cannibalize_p: 1.0,
        ..quiet()
    });
    let link = link_of(seed, &p);
    let mut stage = 0;
    let mut tightened_at = None;
    let (report, records, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            for e in &obs.events {
                if let EventKind::EarthMessage { message, .. } = &e.kind
                    && let Uplink::PolicyUpdate { policy } = &message.body
                    && tightened_at.is_none()
                {
                    // The first update: the loosening comes sols later.
                    assert_eq!(policy.policy_version, 2);
                    assert_eq!(
                        policy.level_for("cannibalize"),
                        AuthorityLevel::HOLD_FOR_EARTH
                    );
                    tightened_at = Some(obs.t);
                }
            }
            match stage {
                0 if robot(obs, "ex-1").status == RobotStatus::Failed => {
                    d.to_earth.push(ask_and_proceed("go", "cannibalize", obs.t));
                    d.to_earth
                        .push(ask("yes", "cannibalize", None, Some(Fallback::Abort)));
                    d.wake_at = Some(obs.t + 1.0);
                    stage = 1;
                }
                1 => {
                    // Level 3, long before any answer: the fired fallback authorizes it.
                    d.commands.push(approved(strip("first", "ex-1"), "go"));
                    stage = 2;
                }
                2 if tightened_at.is_some() => {
                    d.to_earth
                        .push(ask_and_proceed("late", "cannibalize", obs.t));
                    d.wake_at = Some(obs.t + 1.0);
                    stage = 3;
                }
                3 => {
                    // Level 4 now: a fired fallback is not enough, an approval is.
                    d.commands.push(approved(strip("held", "cp-1"), "late"));
                    d.commands.push(approved(strip("second", "cp-1"), "yes"));
                    stage = 4;
                }
                _ => {}
            }
        }),
        seed,
        &p,
    );
    let obs = &agent.observations;
    assert_eq!(rejection(obs, "first"), None);
    assert_eq!(rejection(obs, "held"), Some(RejectReason::ApprovalRequired));
    assert_eq!(rejection(obs, "second"), None);
    assert!(completed(obs, "second").is_some());
    assert_eq!(report.metrics.fleet.robots_lost, 2);
    // Earth learned of the loss from telemetry sent when it happened, one
    // delay later, and issued the update its reaction time after that. Truth
    // records are written only at decision points, so the loss lies between
    // the command's acceptance and the first record showing it.
    let first_seen_lost = records
        .iter()
        .find_map(|r| match &r.entry {
            LogEntry::Truth { robots, .. }
                if robots
                    .iter()
                    .any(|s| s.robot_id == "ex-1" && s.status == RobotStatus::Lost) =>
            {
                Some(r.t)
            }
            _ => None,
        })
        .unwrap();
    let update = uplinks(&records)
        .into_iter()
        .find(|(_, _, m)| matches!(m.body, Uplink::PolicyUpdate { .. }))
        .unwrap();
    let learned = update.0 - p.policy_reaction_s;
    // Solve lost_at + d(lost_at) = learned; d drifts slowly, so two steps do.
    let guess = learned - link.delay_s(ms(learned));
    let lost_at = learned - link.delay_s(ms(guess));
    let accepted_at = events(obs)
        .find(|e| matches!(&e.kind, EventKind::CommandAccepted { command_id, .. } if command_id == "first"))
        .unwrap()
        .t;
    assert!(
        lost_at > accepted_at && lost_at <= first_seen_lost,
        "{lost_at}"
    );
    assert!((secs(link.arrival(ms(lost_at))) - learned).abs() <= 0.002);
    assert_eq!(update.1, tightened_at);
}

#[test]
fn incident_free_sols_loosen_tow_and_level_2_needs_a_timely_notify() {
    // sc-1 empties its battery at once (no incident), so it can be towed again and again.
    let tow = |id: &str| {
        cmd(
            id,
            "hl-1",
            Action::Tow {
                target_robot: "sc-1".into(),
                to: GridCoord { x: 12, y: 11 },
            },
        )
    };
    let mut stage = 0;
    let mut loosened_at = None;
    let mut late_notify_due = f64::INFINITY;
    let (report, _, agent) = run(
        Script::new(|obs: &Observation, d: &mut Decision| {
            for e in &obs.events {
                if let EventKind::EarthMessage { message, .. } = &e.kind
                    && let Uplink::PolicyUpdate { policy } = &message.body
                {
                    assert_eq!(policy.level_for("tow"), AuthorityLevel::AUTONOMOUS);
                    assert_eq!(policy.level_for("replan"), AuthorityLevel::AUTONOMOUS);
                    assert_eq!(
                        policy.level_for("cannibalize"),
                        AuthorityLevel::APPROVE_IF_REACHABLE
                    );
                    loosened_at = Some(obs.t);
                }
            }
            let hl_free = robot(obs, "hl-1").current_command.is_none();
            match stage {
                0 => {
                    d.commands
                        .push(cmd("drain", "sc-1", Action::Wait { duration_s: 1e9 }));
                    stage = 1;
                }
                // Notified in the same decision: fine.
                1 => {
                    d.commands.push(tow("t1"));
                    d.to_earth.push(notify("n1", "tow", &["t1"]));
                    stage = 2;
                }
                // Never notified: a violation.
                2 if hl_free => {
                    d.commands.push(tow("t2"));
                    stage = 3;
                }
                // Notified a little over a sol later: a violation.
                3 if hl_free => {
                    d.commands.push(tow("t3"));
                    late_notify_due = obs.t + SOL_S + 1.0;
                    stage = 4;
                }
                4 if obs.t >= late_notify_due => {
                    d.to_earth.push(notify("n3", "tow", &["t3"]));
                    stage = 5;
                }
                // Level 1 after the update: no notify needed.
                5 if loosened_at.is_some() && hl_free => {
                    d.commands.push(tow("t4"));
                    stage = 6;
                }
                _ => {}
            }
            d.wake_at = Some(obs.t + 3600.0);
        }),
        4,
        &Params {
            outage_per_sol: 0.0,
            ..quiet()
        },
    );
    let obs = &agent.observations;
    for id in ["t1", "t2", "t3", "t4"] {
        assert_eq!(rejection(obs, id), None, "{id}");
    }
    assert!(completed(obs, "t4").is_some());
    // No incidents at all: the update comes after 2 sols, plus the link and Earth's reaction.
    let loosened_at = loosened_at.expect("the policy was loosened");
    assert!(
        loosened_at > 2.0 * SOL_S && loosened_at < 2.1 * SOL_S,
        "{loosened_at}"
    );
    assert_eq!(report.metrics.autonomy.policy_violations, 2, "t2 and t3");
    assert_eq!(report.metrics.autonomy.notifications, 2);
}

/// Whether Earth's approval of `request` arrived in this observation.
fn answer_seen(obs: &Observation, request: &str) -> bool {
    obs.events.iter().any(|e| {
        matches!(&e.kind, EventKind::EarthMessage { message, .. }
            if matches!(&message.body, Uplink::Approve { request_id, .. } if request_id == request))
    })
}
