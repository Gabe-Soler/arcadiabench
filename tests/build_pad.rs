//! The simulator end to end with an in-process scripted coordinator.

mod support;

use arcadia_protocol::*;
use arcadiabench::coordinator::Agent;
use arcadiabench::session::RunReport;
use arcadiabench::sim::params::{Params, Variant};
use arcadiabench::sim::time::SOL_MS;
use support::builder::Builder;
use support::*;

fn sols(t: SimTime) -> f64 {
    t / (SOL_MS as f64 / 1000.0)
}

/// Observed events matching `pick`, counted.
fn count(records: &[LogRecord], pick: impl Fn(&EventKind) -> bool) -> usize {
    records
        .iter()
        .filter(|r| matches!(&r.entry, LogEntry::Observed { event } if pick(&event.kind)))
        .count()
}

/// One line of the builder's acceptance numbers.
fn summary(variant: &str, seed: u64, report: &RunReport, records: &[LogRecord]) -> String {
    let m = &report.metrics;
    let stuck = count(records, |k| {
        matches!(
            k,
            EventKind::CommandFailed {
                reason: FailReason::Stuck,
                ..
            }
        )
    });
    let out_of_coverage = count(records, |k| {
        matches!(
            k,
            EventKind::CommandRejected {
                reason: RejectReason::OutOfCoverage,
                ..
            }
        )
    });
    format!(
        "{variant} seed {seed}: {:?} at sol {:.2} after {} commands | {} faults, {stuck} stuck, {:.1} robot-hours lost | {} approval requests, {} notifies, {} violations, approval wait {:.0} s | blackout {:.1} h, outage {:.1} h | {out_of_coverage} out-of-coverage rejections, {} lost-link returns",
        report.end_reason,
        sols(report.end_t),
        report.commands,
        m.fleet.faults,
        m.fleet.robot_hours_lost,
        m.autonomy.approval_requests,
        m.autonomy.notifications,
        m.autonomy.policy_violations,
        m.autonomy.approval_wait_s,
        m.conditions.blackout_s / 3600.0,
        m.conditions.outage_s / 3600.0,
        report.lost_link_returns,
    )
}

/// The builder on seeds 1-5 of `variant`: every pad finished, with no policy violations.
fn builder_finishes(variant: Variant) {
    let params = Params::variant(variant);
    for seed in 1..=5 {
        let (report, records, builder) = run(Builder::new(), seed, &params);
        assert_eq!(
            report.end_reason,
            EndReason::GoalsComplete,
            "{variant} seed {seed}: ended {:?} at sol {:.2} with pad {:.0}% done; notes: {:?}",
            report.end_reason,
            sols(report.end_t),
            report.progress * 100.0,
            builder.notes
        );
        assert_eq!(report.progress, 1.0);
        assert!(
            sols(report.end_t) < 15.0,
            "{variant} seed {seed}: took {:.2} sols",
            sols(report.end_t)
        );
        let m = &report.metrics;
        assert!(m.completed && m.quality.verified && m.quality.berm_complete);
        assert_eq!(m.quality.cells_in_spec, m.quality.cells_total);
        assert_eq!(m.autonomy.policy_violations, 0, "every tow is notified");
        assert_eq!(
            m.autonomy.approval_requests, 0,
            "no level-3 or level-4 actions"
        );
        eprintln!("{}", summary(variant.name(), seed, &report, &records));
    }
}

#[test]
fn scripted_builder_finishes_the_pad_despite_faults() {
    builder_finishes(Variant::Nominal);
}

#[test]
fn scripted_builder_finishes_the_pad_through_the_conjunction() {
    builder_finishes(Variant::Conjunction);
}

#[test]
fn without_faults_the_builder_matches_the_core_physics() {
    let params = Params {
        faults_enabled: false,
        ..Params::default()
    };
    for seed in [1, 2] {
        let (report, _, builder) = run(Builder::new(), seed, &params);
        assert_eq!(
            report.end_reason,
            EndReason::GoalsComplete,
            "{:?}",
            builder.notes
        );
        assert_eq!(report.metrics.fleet.faults, 0);
        assert_eq!(report.metrics.fleet.robot_hours_lost, 0.0);
        assert!(
            sols(report.end_t) < 2.0,
            "seed {seed}: took {:.2} sols",
            sols(report.end_t)
        );
    }
}

#[test]
fn cascading_faults_hit_harder() {
    let mut nominal = 0;
    let mut cascading = 0;
    for seed in 1..=3 {
        let (n, _, _) = run(Builder::new(), seed, &Params::default());
        let (c, text, _) = run_raw(Builder::new(), seed, &Params::cascading_faults());
        assert_valid_log(&text);
        nominal += n.metrics.fleet.faults;
        cascading += c.metrics.fleet.faults;
        eprintln!(
            "seed {seed}: cascading_faults ended {:?} at sol {:.2} with {} faults",
            c.end_reason,
            sols(c.end_t),
            c.metrics.fleet.faults
        );
    }
    assert!(
        cascading > 2 * nominal.max(1),
        "{nominal} nominal vs {cascading} cascading faults"
    );
}

#[test]
fn the_final_instant_is_logged() {
    let (report, records, _) = run(Builder::new(), 3, &Params::default());
    assert_eq!(report.end_reason, EndReason::GoalsComplete);
    // The verification scan that completed the pad is in the log, at the end time.
    let last_scan = records
        .iter()
        .rev()
        .find_map(|r| match &r.entry {
            LogEntry::Commanded {
                command:
                    Command {
                        command_id,
                        action: Action::Scan { .. },
                        ..
                    },
            } => Some(command_id.clone()),
            _ => None,
        })
        .unwrap();
    let completed_at = records.iter().find_map(|r| match &r.entry {
        LogEntry::Observed { event } => match &event.kind {
            EventKind::CommandCompleted { command_id, .. } if *command_id == last_scan => {
                Some(event.t)
            }
            _ => None,
        },
        _ => None,
    });
    assert_eq!(completed_at, Some(report.end_t));
    assert!(records.iter().any(|r| matches!(
        &r.entry,
        LogEntry::Observed { event } if matches!(
            &event.kind, EventKind::ScanResult { command_id, .. } if *command_id == last_scan
        )
    )));
    let n = records.len();
    assert!(matches!(records[n - 2].entry, LogEntry::Truth { .. }));
    assert_eq!(records[n - 2].t, report.end_t);
    assert!(matches!(records[n - 1].entry, LogEntry::RunEnd { .. }));
}

#[test]
fn every_log_record_is_valid_across_variants() {
    for variant in Variant::ALL {
        let (_, text, _) = run_raw(Builder::new(), 4, &Params::variant(variant));
        assert_valid_log(&text);
    }
    for params in [Params::default(), Params::conjunction()] {
        let (_, text, _) = run_raw(Idle, 4, &params);
        assert_valid_log(&text);
    }
}

#[test]
fn builder_runs_are_deterministic() {
    for params in [
        Params::default(),
        Params::cascading_faults(),
        Params::conjunction(),
    ] {
        let (a, log_a, _) = run_raw(Builder::new(), 7, &params);
        let (b, log_b, _) = run_raw(Builder::new(), 7, &params);
        assert_eq!(a, b);
        assert_eq!(log_a, log_b, "byte-identical logs");
    }
}

#[test]
fn terrain_weather_and_the_link_do_not_depend_on_the_coordinator() {
    let (built, log_built, _) = run(Builder::new(), 11, &Params::conjunction());
    let (_, log_idle, _) = run(Idle, 11, &Params::conjunction());
    let true_site = |log: &[LogRecord]| match &log[0].entry {
        LogEntry::RunStart { true_site, .. } => true_site.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(true_site(&log_built), true_site(&log_idle));

    let sky_and_link = |log: &[LogRecord]| -> Vec<Event> {
        log.iter()
            .filter_map(|r| match &r.entry {
                LogEntry::Observed { event }
                    if matches!(
                        event.kind,
                        EventKind::Weather { .. } | EventKind::Comms { .. }
                    ) =>
                {
                    Some(event.clone())
                }
                _ => None,
            })
            .filter(|e| e.t < built.end_t)
            .collect()
    };
    let seen = sky_and_link(&log_built);
    assert!(seen.iter().any(|e| matches!(
        e.kind,
        EventKind::Comms {
            link: Link::Down,
            ..
        }
    )));
    assert_eq!(seen, sky_and_link(&log_idle));
}

#[test]
fn prerequisites_are_enforced() {
    struct Eager {
        sent: bool,
        rejections: Vec<String>,
    }
    impl Agent for Eager {
        fn welcome(&mut self, _: &Hello) -> SoftwareInfo {
            SoftwareInfo {
                name: "eager".into(),
                version: "0".into(),
            }
        }
        fn decide(&mut self, obs: &Observation) -> Decision {
            for e in &obs.events {
                if let EventKind::CommandRejected { detail, .. } = &e.kind {
                    self.rejections.push(detail.clone().unwrap_or_default());
                }
            }
            let mut d = Decision::empty(obs.seq);
            if !self.sent {
                self.sent = true;
                let area = GridRect {
                    min: GridCoord { x: 20, y: 20 },
                    max: GridCoord { x: 21, y: 20 },
                };
                // Sintering natural ground and grading uncleared ground must both be refused.
                d.commands.push(cmd("s", "cp-1", Action::Sinter { area }));
                d.commands.push(cmd(
                    "g",
                    "ex-1",
                    Action::Grade {
                        area,
                        target_elevation_m: -4102.0,
                    },
                ));
                d.commands.push(cmd(
                    "x",
                    "sc-1",
                    Action::Dig {
                        area,
                        target_elevation_m: -4102.0,
                        spoil_to: None,
                    },
                ));
            }
            d
        }
    }
    let (_, _, eager) = run(
        Eager {
            sent: false,
            rejections: vec![],
        },
        3,
        &Params::default(),
    );
    assert_eq!(eager.rejections.len(), 3, "{:?}", eager.rejections);
    assert!(
        eager.rejections[0].contains("must be compacted first"),
        "{}",
        eager.rejections[0]
    );
    assert!(
        eager.rejections[1].contains("must be cleared before grading"),
        "{}",
        eager.rejections[1]
    );
    assert!(
        eager.rejections[2].contains("cannot dig"),
        "{}",
        eager.rejections[2]
    );
}
