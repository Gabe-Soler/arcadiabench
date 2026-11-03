//! End-to-end: the harness launches real coordinator processes over stdio.

use arcadia_protocol::LogRecord;
use arcadia_protocol::validate::validator;
use arcadiabench::coordinator::BenchError;
use arcadiabench::log::EventLog;
use arcadiabench::run::run_placeholder;

const NULL: &str = env!("CARGO_BIN_EXE_null-coordinator");

fn run(program: &str, args: &[&str], seed: u64) -> (Result<u64, BenchError>, Vec<u8>) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut log = EventLog::new(Vec::new());
    let result = run_placeholder(program, &args, seed, &mut log).map(|r| r.decisions);
    (result, log.into_inner())
}

/// A fake coordinator written in shell: reads one line, prints `reply` lines.
fn sh(script: &str) -> (String, Vec<String>) {
    ("sh".into(), vec!["-c".into(), script.into()])
}

#[test]
fn null_coordinator_completes_a_run_with_a_schema_valid_log() {
    let (result, log) = run(NULL, &[], 1);
    assert_eq!(result.unwrap(), 1);

    let v = validator("event_log.schema.json", "LogRecord").unwrap();
    let lines: Vec<&str> = std::str::from_utf8(&log).unwrap().lines().collect();
    assert!(lines.len() >= 3);
    for line in &lines {
        let json: serde_json::Value = serde_json::from_str(line).unwrap();
        if let Err(e) = v.validate(&json) {
            panic!("invalid log record {line}: {e}");
        }
    }
    let first: LogRecord = serde_json::from_str(lines[0]).unwrap();
    let last: LogRecord = serde_json::from_str(lines[lines.len() - 1]).unwrap();
    assert!(matches!(
        first.entry,
        arcadia_protocol::LogEntry::RunStart { .. }
    ));
    assert!(matches!(
        last.entry,
        arcadia_protocol::LogEntry::RunEnd { .. }
    ));
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
    let (result, _) = run(&p, &a, 0);
    match result {
        Err(BenchError::Protocol(m)) => assert!(m.contains("9.0.0"), "{m}"),
        other => panic!("expected protocol error, got {other:?}"),
    }
}

#[test]
fn rejects_schema_invalid_decision() {
    let (p, a) = sh(concat!(
        r#"read l; echo '{"type":"welcome","protocol_version":"0.1.0","coordinator":{"name":"x","version":"0"}}'; "#,
        r#"read l; echo '{"type":"decision","seq":0,"commands":[{"type":"teleport","command_id":"c1","robot_id":"ex-1"}],"to_earth":[]}'"#,
    ));
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let (result, _) = run(&p, &a, 0);
    assert!(matches!(result, Err(BenchError::Protocol(_))), "{result:?}");
}

#[test]
fn rejects_wrong_decision_seq() {
    let (p, a) = sh(concat!(
        r#"read l; echo '{"type":"welcome","protocol_version":"0.1.0","coordinator":{"name":"x","version":"0"}}'; "#,
        r#"read l; echo '{"type":"decision","seq":5,"commands":[],"to_earth":[]}'"#,
    ));
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let (result, _) = run(&p, &a, 0);
    match result {
        Err(BenchError::Protocol(m)) => assert!(m.contains("seq 5"), "{m}"),
        other => panic!("expected protocol error, got {other:?}"),
    }
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
