//! End-to-end: the harness launches real coordinator processes over stdio.

use arcadia_protocol::validate::validator;
use arcadia_protocol::{EndReason, LogEntry, LogRecord};
use arcadiabench::coordinator::BenchError;
use arcadiabench::log::EventLog;
use arcadiabench::run::run_process;
use arcadiabench::session::RunReport;
use std::time::{Duration, Instant};

const NULL: &str = env!("CARGO_BIN_EXE_null-coordinator");

fn run_with(
    program: &str,
    args: &[&str],
    seed: u64,
    timeout: Duration,
) -> (Result<RunReport, BenchError>, Vec<u8>) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut log = EventLog::new(Vec::new());
    let result = run_process(program, &args, seed, timeout, &mut log);
    (result, log.into_inner())
}

fn run(program: &str, args: &[&str], seed: u64) -> (Result<RunReport, BenchError>, Vec<u8>) {
    run_with(program, args, seed, Duration::from_secs(10))
}

/// A fake coordinator written in shell.
fn sh(script: &str) -> (String, Vec<String>) {
    ("sh".into(), vec!["-c".into(), script.into()])
}

fn records(log: &[u8]) -> Vec<LogRecord> {
    let v = validator("event_log.schema.json", "LogRecord").unwrap();
    std::str::from_utf8(log)
        .unwrap()
        .lines()
        .map(|line| {
            let json: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Err(e) = v.validate(&json) {
                panic!("invalid log record {line}: {e}");
            }
            serde_json::from_value(json).unwrap()
        })
        .collect()
}

#[test]
fn null_coordinator_runs_to_the_time_limit_with_a_valid_log() {
    let (result, log) = run(NULL, &[], 1);
    let report = result.unwrap();
    assert_eq!(report.end_reason, EndReason::TimeLimit);
    assert_eq!(report.commands, 0);
    assert!(report.decisions > 100, "{}", report.decisions);
    assert_eq!(report.progress, 0.0);

    let recs = records(&log);
    assert!(matches!(
        recs.first().unwrap().entry,
        LogEntry::RunStart { .. }
    ));
    assert!(matches!(
        recs.last().unwrap().entry,
        LogEntry::RunEnd {
            reason: EndReason::TimeLimit
        }
    ));
    assert!(
        recs.windows(2).all(|w| w[0].t <= w[1].t),
        "log is in time order"
    );
}

#[test]
fn same_seed_gives_identical_logs() {
    let (_, a) = run(NULL, &[], 42);
    let (_, b) = run(NULL, &[], 42);
    assert_eq!(a, b);
}

#[test]
fn rejects_incompatible_welcome() {
    let (p, a) = sh(
        r#"read l; echo '{"type":"welcome","protocol_version":"9.0.0","coordinator":{"name":"x","version":"0"}}'"#,
    );
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    match run(&p, &a, 0).0 {
        Err(BenchError::Protocol(m)) => assert!(m.contains("9.0.0"), "{m}"),
        other => panic!("expected protocol error, got {other:?}"),
    }
}

#[test]
fn rejects_schema_invalid_decision_and_still_closes_the_log() {
    let (p, a) = sh(concat!(
        r#"read l; echo '{"type":"welcome","protocol_version":"0.1.0","coordinator":{"name":"x","version":"0"}}'; "#,
        r#"read l; echo '{"type":"decision","seq":0,"commands":[{"type":"teleport","command_id":"c1","robot_id":"ex-1"}],"to_earth":[]}'"#,
    ));
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let (result, log) = run(&p, &a, 0);
    assert!(matches!(result, Err(BenchError::Protocol(_))), "{result:?}");
    assert!(matches!(
        records(&log).last().unwrap().entry,
        LogEntry::RunEnd {
            reason: EndReason::CoordinatorError
        }
    ));
}

#[test]
fn rejects_wrong_decision_seq() {
    let (p, a) = sh(concat!(
        r#"read l; echo '{"type":"welcome","protocol_version":"0.1.0","coordinator":{"name":"x","version":"0"}}'; "#,
        r#"read l; echo '{"type":"decision","seq":5,"commands":[],"to_earth":[]}'"#,
    ));
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    match run(&p, &a, 0).0 {
        Err(BenchError::Protocol(m)) => assert!(m.contains("seq 5"), "{m}"),
        other => panic!("expected protocol error, got {other:?}"),
    }
}

#[test]
fn times_out_a_hung_coordinator() {
    let (p, a) = sh(concat!(
        r#"read l; echo '{"type":"welcome","protocol_version":"0.1.0","coordinator":{"name":"x","version":"0"}}'; "#,
        "read l; sleep 30",
    ));
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let started = Instant::now();
    let (result, _) = run_with(&p, &a, 0, Duration::from_millis(500));
    assert!(matches!(result, Err(BenchError::Timeout(_))), "{result:?}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn reports_coordinator_that_exits_early() {
    let (result, _) = run("true", &[], 0);
    assert!(
        matches!(result, Err(BenchError::CoordinatorExited)),
        "{result:?}"
    );
}

#[test]
fn reports_missing_coordinator_binary() {
    let (result, _) = run("/nonexistent/coordinator", &[], 0);
    assert!(matches!(result, Err(BenchError::Launch(_))), "{result:?}");
}
