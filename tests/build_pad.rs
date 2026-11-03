//! The simulator end to end with an in-process scripted coordinator.

mod support;

use arcadia_protocol::*;
use arcadiabench::coordinator::{Agent, InProcess};
use arcadiabench::log::EventLog;
use arcadiabench::session::{RunReport, run_session};
use arcadiabench::sim::params::Params;
use arcadiabench::sim::time::SOL_MS;
use support::builder::Builder;

fn run<A: Agent>(agent: A, seed: u64) -> (RunReport, Vec<LogRecord>, A) {
    let mut link = InProcess::new(agent).unwrap();
    let mut log = EventLog::new(Vec::new());
    let report = run_session(&mut link, seed, &Params::default(), &mut log).unwrap();
    let records = std::str::from_utf8(&log.into_inner())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (report, records, link.agent)
}

struct Idle;

impl Agent for Idle {
    fn welcome(&mut self, _: &Hello) -> SoftwareInfo {
        SoftwareInfo {
            name: "idle".into(),
            version: "0".into(),
        }
    }
    fn decide(&mut self, obs: &Observation) -> Decision {
        Decision::empty(obs.seq)
    }
}

fn sols(t: SimTime) -> f64 {
    t / (SOL_MS as f64 / 1000.0)
}

#[test]
fn scripted_builder_finishes_the_pad() {
    for seed in [1, 2] {
        let (report, _, builder) = run(Builder::new(), seed);
        assert_eq!(
            report.end_reason,
            EndReason::GoalsComplete,
            "seed {seed}: ended {:?} at sol {:.2} with pad {:.0}% done; notes: {:?}",
            report.end_reason,
            sols(report.end_t),
            report.progress * 100.0,
            builder.notes
        );
        assert_eq!(report.progress, 1.0);
        assert!(
            sols(report.end_t) < 15.0,
            "seed {seed}: took {:.2} sols",
            sols(report.end_t)
        );
        eprintln!(
            "seed {seed}: pad done at sol {:.2} after {} commands ({})",
            sols(report.end_t),
            report.commands,
            builder.notes.join("; ")
        );
    }
}

#[test]
fn builder_runs_are_deterministic() {
    let (a, log_a, _) = run(Builder::new(), 7);
    let (b, log_b, _) = run(Builder::new(), 7);
    assert_eq!(a, b);
    assert_eq!(log_a, log_b);
}

#[test]
fn terrain_and_weather_do_not_depend_on_the_coordinator() {
    let (built, log_built, _) = run(Builder::new(), 11);
    let (_, log_idle, _) = run(Idle, 11);
    let true_site = |log: &[LogRecord]| match &log[0].entry {
        LogEntry::RunStart { true_site, .. } => true_site.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(true_site(&log_built), true_site(&log_idle));

    let weather = |log: &[LogRecord]| -> Vec<Event> {
        log.iter()
            .filter_map(|r| match &r.entry {
                LogEntry::Observed { event } if matches!(event.kind, EventKind::Weather { .. }) => {
                    Some(event.clone())
                }
                _ => None,
            })
            .filter(|e| e.t <= built.end_t)
            .collect()
    };
    assert_eq!(weather(&log_built), weather(&log_idle));
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
                d.commands.push(Command {
                    command_id: "s".into(),
                    robot_id: "cp-1".into(),
                    approval_id: None,
                    action: Action::Sinter { area },
                });
                d.commands.push(Command {
                    command_id: "g".into(),
                    robot_id: "ex-1".into(),
                    approval_id: None,
                    action: Action::Grade {
                        area,
                        target_elevation_m: -4102.0,
                    },
                });
                d.commands.push(Command {
                    command_id: "x".into(),
                    robot_id: "sc-1".into(),
                    approval_id: None,
                    action: Action::Dig {
                        area,
                        target_elevation_m: -4102.0,
                        spoil_to: None,
                    },
                });
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
