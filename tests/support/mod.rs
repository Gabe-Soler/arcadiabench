//! Shared helpers for the integration tests. Each test crate uses a subset.
#![allow(dead_code)]

pub mod builder;

use arcadia_protocol::validate::validator;
use arcadia_protocol::*;
use arcadiabench::coordinator::{Agent, InProcess};
use arcadiabench::log::EventLog;
use arcadiabench::session::{RunReport, run_session};
use arcadiabench::sim::params::Params;

/// Run an in-process agent; returns the report, the raw log and the agent.
pub fn run_raw<A: Agent>(agent: A, seed: u64, params: &Params) -> (RunReport, String, A) {
    let mut link = InProcess::new(agent).unwrap();
    let mut log = EventLog::new(Vec::new());
    let report = run_session(&mut link, seed, params, &mut log).unwrap();
    (
        report,
        String::from_utf8(log.into_inner()).unwrap(),
        link.agent,
    )
}

/// Run an in-process agent; returns the report, the parsed log and the agent.
pub fn run<A: Agent>(agent: A, seed: u64, params: &Params) -> (RunReport, Vec<LogRecord>, A) {
    let (report, text, agent) = run_raw(agent, seed, params);
    let records = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (report, records, agent)
}

/// Every record validates against the protocol schemas, the log is in time
/// order, and it ends with a truth record followed by run_end.
pub fn assert_valid_log(text: &str) {
    let v = validator("event_log.schema.json", "LogRecord").unwrap();
    let mut last_t = f64::NEG_INFINITY;
    let lines: Vec<&str> = text.lines().collect();
    for line in &lines {
        let json: serde_json::Value = serde_json::from_str(line).unwrap();
        if let Err(e) = v.validate(&json) {
            panic!("invalid log record {line}: {e}");
        }
        let t = json["t"].as_f64().unwrap();
        assert!(t >= last_t, "log goes back in time at {line}");
        last_t = t;
    }
    let kind =
        |line: &str| serde_json::from_str::<serde_json::Value>(line).unwrap()["kind"].clone();
    assert_eq!(kind(lines[lines.len() - 1]), "run_end");
    assert_eq!(kind(lines[lines.len() - 2]), "truth");
}

/// A coordinator that never acts.
pub struct Idle;

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

/// A coordinator driven by a closure that fills in each decision. It keeps
/// every observation for the test to inspect afterwards.
pub struct Script<F> {
    step: F,
    pub observations: Vec<Observation>,
}

impl<F: FnMut(&Observation, &mut Decision)> Script<F> {
    pub fn new(step: F) -> Self {
        Self {
            step,
            observations: Vec::new(),
        }
    }
}

impl<F: FnMut(&Observation, &mut Decision)> Agent for Script<F> {
    fn welcome(&mut self, _: &Hello) -> SoftwareInfo {
        SoftwareInfo {
            name: "script".into(),
            version: "0".into(),
        }
    }
    fn decide(&mut self, obs: &Observation) -> Decision {
        let mut d = Decision::empty(obs.seq);
        (self.step)(obs, &mut d);
        self.observations.push(obs.clone());
        d
    }
}

pub fn cmd(id: &str, robot: &str, action: Action) -> Command {
    Command {
        command_id: id.into(),
        robot_id: robot.into(),
        approval_id: None,
        action,
    }
}

pub fn robot<'a>(obs: &'a Observation, id: &str) -> &'a RobotState {
    obs.robots.iter().find(|r| r.robot_id == id).unwrap()
}

/// Events of one kind, from the observations an agent saw.
pub fn events(observations: &[Observation]) -> impl Iterator<Item = &Event> {
    observations.iter().flat_map(|o| o.events.iter())
}

pub fn rejection(observations: &[Observation], command: &str) -> Option<RejectReason> {
    events(observations).find_map(|e| match &e.kind {
        EventKind::CommandRejected {
            command_id, reason, ..
        } if command_id == command => Some(*reason),
        _ => None,
    })
}

pub fn completed(observations: &[Observation], command: &str) -> Option<f64> {
    events(observations).find_map(|e| match &e.kind {
        EventKind::CommandCompleted { command_id, .. } if command_id == command => Some(e.t),
        _ => None,
    })
}

/// Parameters with every random failure process off, so tests choose the incidents.
pub fn quiet() -> Params {
    Params {
        faults_enabled: false,
        ..Params::default()
    }
}

/// Ground-truth robot states from the last truth record at or before `t`.
pub fn truth_at(records: &[LogRecord], t: f64) -> Vec<RobotState> {
    records
        .iter()
        .rev()
        .filter(|r| r.t <= t)
        .find_map(|r| match &r.entry {
            LogEntry::Truth { robots, .. } => Some(robots.clone()),
            _ => None,
        })
        .unwrap()
}
