//! End-to-end: the harness launches real coordinator processes over stdio,
//! and runs in-process coordinators that speak JSON values under the same
//! checks.

use arcadia_protocol::validate::validator;
use arcadia_protocol::{EndReason, ErrorCode, LogEntry, LogRecord, PROTOCOL_VERSION};
use arcadiabench::coordinator::{BenchError, InProcessJson, JsonAgent};
use arcadiabench::log::EventLog;
use arcadiabench::run::run_process;
use arcadiabench::session::{RunReport, run_session_outcome};
use arcadiabench::sim::params::Params;
use serde_json::{Value, json};
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
    let result = run_process(program, &args, seed, &Params::default(), timeout, &mut log);
    (result, log.into_inner())
}

fn run(program: &str, args: &[&str], seed: u64) -> (Result<RunReport, BenchError>, Vec<u8>) {
    run_with(program, args, seed, Duration::from_secs(10))
}

/// A fake coordinator written in shell.
fn sh(script: &str) -> (String, Vec<String>) {
    ("sh".into(), vec!["-c".into(), script.into()])
}

/// Shell that reads the hello and answers with a compatible welcome.
fn welcome() -> String {
    format!(
        r#"read l; echo '{{"type":"welcome","protocol_version":"{PROTOCOL_VERSION}","coordinator":{{"name":"x","version":"0"}}}}'; "#
    )
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
fn null_coordinator_loses_its_idle_fleet_with_a_valid_log() {
    let (result, log) = run(NULL, &[], 1);
    let report = result.unwrap();
    // Idle robots drain until the last one is offline away from any station:
    // nothing can ever act again.
    assert_eq!(report.end_reason, EndReason::FleetLost);
    assert_eq!(report.commands, 0);
    assert!(report.decisions > 20, "{}", report.decisions);
    assert_eq!(report.progress, 0.0);

    let recs = records(&log);
    assert!(matches!(
        recs.first().unwrap().entry,
        LogEntry::RunStart { .. }
    ));
    assert!(matches!(
        recs.last().unwrap().entry,
        LogEntry::RunEnd {
            reason: EndReason::FleetLost,
            ..
        }
    ));
    assert!(matches!(recs[recs.len() - 2].entry, LogEntry::Truth { .. }));
    assert!(
        recs.windows(2).all(|w| w[0].t <= w[1].t),
        "log is in time order"
    );
}

/// Under a second in release builds (`cargo test --release`); debug builds
/// only get a sanity bound.
fn fast_enough() -> Duration {
    if cfg!(debug_assertions) {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(1)
    }
}

#[test]
fn a_whole_null_run_is_fast() {
    let started = Instant::now();
    let (result, _) = run(NULL, &[], 7);
    let elapsed = started.elapsed();
    assert_eq!(result.unwrap().end_reason, EndReason::FleetLost);
    eprintln!("null coordinator, whole run: {elapsed:?}");
    assert!(elapsed < fast_enough(), "{elapsed:?}");
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
    let (p, a) = sh(&format!(
        "{}{}",
        welcome(),
        r#"read l; echo '{"type":"decision","seq":0,"commands":[{"type":"teleport","command_id":"c1","robot_id":"ex-1"}],"to_earth":[]}'"#,
    ));
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let (result, log) = run(&p, &a, 0);
    assert!(matches!(result, Err(BenchError::Protocol(_))), "{result:?}");
    assert!(matches!(
        records(&log).last().unwrap().entry,
        LogEntry::RunEnd {
            reason: EndReason::CoordinatorError,
            ..
        }
    ));
}

#[test]
fn rejects_wrong_decision_seq() {
    let (p, a) = sh(&format!(
        "{}{}",
        welcome(),
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
    let (p, a) = sh(&format!("{}read l; sleep 30", welcome()));
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

/// A coordinator speaking JSON values, as the Python bindings' agents do.
struct Json<F> {
    decide: F,
    ended: bool,
}

impl<F: FnMut(&Value) -> Result<Value, String>> JsonAgent for Json<F> {
    fn welcome(&mut self, hello: Value) -> Result<Value, String> {
        assert_eq!(hello["type"], "hello");
        assert_eq!(
            hello["scenario"]["scenario_id"],
            "landing-pad-small-nominal"
        );
        // No `type` and no `protocol_version`: both are filled in.
        Ok(json!({ "coordinator": { "name": "json", "version": "0" } }))
    }
    fn decide(&mut self, obs: Value) -> Result<Value, String> {
        assert_eq!(obs["type"], "observation");
        (self.decide)(&obs)
    }
    fn end(&mut self, end: Value) -> Result<(), String> {
        assert_eq!(end["type"], "end");
        self.ended = true;
        Ok(())
    }
}

fn json_run<F: FnMut(&Value) -> Result<Value, String>>(
    decide: F,
    timeout: Option<Duration>,
) -> (arcadiabench::session::Outcome, Vec<LogRecord>, bool) {
    let mut link = InProcessJson::new(
        Json {
            decide,
            ended: false,
        },
        timeout,
    )
    .unwrap();
    let mut log = EventLog::new(Vec::new());
    let outcome = run_session_outcome(&mut link, 1, &Params::default(), &mut log).unwrap();
    (outcome, records(&log.into_inner()), link.agent.ended)
}

fn empty(obs: &Value) -> Value {
    json!({ "seq": obs["seq"], "commands": [], "to_earth": [] })
}

#[test]
fn in_process_json_agents_get_the_same_checks() {
    // Leaving out `type` is fine; such a null agent runs to the end.
    let (outcome, recs, ended) = json_run(|o| Ok(empty(o)), None);
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.report.end_reason, EndReason::FleetLost);
    assert_eq!(outcome.report.coordinator.name, "json");
    assert!(ended);
    assert!(matches!(
        recs.last().unwrap().entry,
        LogEntry::RunEnd { .. }
    ));

    // A schema-invalid decision, a failing agent, a reply of the wrong
    // type and a slow reply each end the run as a coordinator error, with
    // the log closed.
    let teleport = |o: &Value| {
        Ok(
            json!({ "seq": o["seq"], "commands": [{ "type": "teleport", "command_id": "c", "robot_id": "ex-1" }], "to_earth": [] }),
        )
    };
    let (outcome, recs, _) = json_run(teleport, None);
    assert!(
        matches!(outcome.error, Some(BenchError::Protocol(_))),
        "{:?}",
        outcome.error
    );
    assert_eq!(outcome.report.end_reason, EndReason::CoordinatorError);
    assert!(matches!(
        recs.last().unwrap().entry,
        LogEntry::RunEnd {
            reason: EndReason::CoordinatorError,
            ..
        }
    ));

    let (outcome, _, _) = json_run(|_| Err("boom".into()), None);
    match outcome.error {
        Some(BenchError::Coordinator(e)) => {
            assert_eq!((e.code, e.message.as_str()), (ErrorCode::Internal, "boom"))
        }
        other => panic!("{other:?}"),
    }

    let welcome_again = |_: &Value| {
        Ok(
            json!({ "type": "welcome", "protocol_version": PROTOCOL_VERSION, "coordinator": { "name": "x", "version": "0" } }),
        )
    };
    let (outcome, _, _) = json_run(welcome_again, None);
    match outcome.error {
        Some(BenchError::Protocol(m)) => {
            assert!(m.contains("expected decision, got welcome"), "{m}")
        }
        other => panic!("{other:?}"),
    }

    let slow = |o: &Value| {
        std::thread::sleep(Duration::from_millis(30));
        Ok(empty(o))
    };
    let (outcome, _, _) = json_run(slow, Some(Duration::from_millis(5)));
    assert!(
        matches!(outcome.error, Some(BenchError::Timeout(_))),
        "{:?}",
        outcome.error
    );
}
