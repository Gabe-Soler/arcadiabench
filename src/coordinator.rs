//! How the bench talks to a coordinator.
//!
//! [`CoordinatorLink`] is the transport: a subprocess speaking NDJSON over
//! stdio ([`CoordinatorProcess`]), or an in-process [`Agent`] ([`InProcess`],
//! used by tests and, from week 7, Python bindings). Either way, everything the
//! coordinator sends is checked against the JSON Schemas. Protocol rules that
//! do not depend on the transport (version, seq) live in the session.

use arcadia_protocol::validate::{Validator, validator};
use arcadia_protocol::{
    CoordinatorToSim, Decision, End, ErrorCode, ErrorMessage, Hello, Observation, SimToCoordinator,
    SoftwareInfo, Welcome, wire,
};
use serde_json::Value;
use std::fmt;
use std::io::{self, BufRead, BufReader};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum BenchError {
    /// The coordinator process could not be started.
    Launch(io::Error),
    Wire(wire::WireError),
    /// The coordinator closed its stdout before the session ended.
    CoordinatorExited,
    /// The coordinator did not reply within the decision timeout.
    Timeout(Duration),
    /// The coordinator sent something the protocol does not allow.
    Protocol(String),
    /// The coordinator sent an `error` message.
    Coordinator(ErrorMessage),
}

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BenchError::Launch(e) => write!(f, "could not launch coordinator: {e}"),
            BenchError::Wire(e) => write!(f, "{e}"),
            BenchError::CoordinatorExited => write!(f, "coordinator exited mid-session"),
            BenchError::Timeout(d) => write!(f, "coordinator did not reply within {d:?}"),
            BenchError::Protocol(m) => write!(f, "protocol violation by coordinator: {m}"),
            BenchError::Coordinator(e) => {
                write!(f, "coordinator error ({:?}): {}", e.code, e.message)
            }
        }
    }
}

impl std::error::Error for BenchError {}

impl From<wire::WireError> for BenchError {
    fn from(e: wire::WireError) -> Self {
        BenchError::Wire(e)
    }
}

/// Transport to a coordinator.
pub trait CoordinatorLink {
    fn start(&mut self, hello: Hello) -> Result<Welcome, BenchError>;
    fn decide(&mut self, obs: Observation) -> Result<Decision, BenchError>;
    fn finish(&mut self, end: End) -> Result<(), BenchError>;
    /// Tell the coordinator the session is being aborted (best effort).
    fn abort(&mut self, code: ErrorCode, message: &str);
}

fn coordinator_validator() -> Result<Validator, BenchError> {
    validator("messages.schema.json", "CoordinatorToSim")
        .map_err(|e| BenchError::Protocol(format!("schema setup failed: {e}")))
}

fn check(validator: &Validator, raw: Value) -> Result<CoordinatorToSim, String> {
    if let Err(e) = validator.validate(&raw) {
        return Err(format!("{e} at {}", e.instance_path()));
    }
    serde_json::from_value(raw).map_err(|e| format!("schema-valid but untyped: {e}"))
}

fn kind(msg: &CoordinatorToSim) -> &'static str {
    match msg {
        CoordinatorToSim::Welcome(_) => "welcome",
        CoordinatorToSim::Decision(_) => "decision",
        CoordinatorToSim::Error(_) => "error",
    }
}

// ----- subprocess -----

pub struct CoordinatorProcess {
    child: Child,
    /// `None` once closed by `finish`.
    stdin: Option<ChildStdin>,
    lines: Receiver<io::Result<String>>,
    timeout: Duration,
    validator: Validator,
}

impl CoordinatorProcess {
    /// Spawn `program args...`. Its stderr is inherited so its logs stay visible.
    /// Every reply must arrive within `timeout`.
    pub fn spawn(program: &str, args: &[String], timeout: Duration) -> Result<Self, BenchError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(BenchError::Launch)?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        // A reader thread lets us wait for a line with a deadline.
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            lines,
            timeout,
            validator: coordinator_validator()?,
        })
    }

    fn send(&mut self, msg: &SimToCoordinator) -> Result<(), BenchError> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(BenchError::CoordinatorExited);
        };
        wire::send(stdin, msg).map_err(|e| match e {
            wire::WireError::Io(io) if io.kind() == io::ErrorKind::BrokenPipe => {
                BenchError::CoordinatorExited
            }
            e => e.into(),
        })
    }

    fn recv(&mut self) -> Result<CoordinatorToSim, BenchError> {
        let deadline = Instant::now() + self.timeout;
        let line = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(Ok(line)) if line.trim().is_empty() => continue,
                Ok(Ok(line)) => break line,
                Ok(Err(e)) => return Err(BenchError::Wire(e.into())),
                Err(RecvTimeoutError::Timeout) => {
                    let _ = self.child.kill();
                    return Err(BenchError::Timeout(self.timeout));
                }
                Err(RecvTimeoutError::Disconnected) => return Err(BenchError::CoordinatorExited),
            }
        };
        let raw: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let message = format!("not JSON: {e}");
                self.abort(ErrorCode::InvalidMessage, &message);
                return Err(BenchError::Protocol(message));
            }
        };
        match check(&self.validator, raw) {
            Ok(CoordinatorToSim::Error(e)) => Err(BenchError::Coordinator(e)),
            Ok(msg) => Ok(msg),
            Err(message) => {
                self.abort(ErrorCode::InvalidMessage, &message);
                Err(BenchError::Protocol(message))
            }
        }
    }

    fn unexpected(&mut self, wanted: &str, got: &CoordinatorToSim) -> BenchError {
        let message = format!("expected {wanted}, got {}", kind(got));
        self.abort(ErrorCode::UnexpectedMessage, &message);
        BenchError::Protocol(message)
    }
}

impl CoordinatorLink for CoordinatorProcess {
    fn start(&mut self, hello: Hello) -> Result<Welcome, BenchError> {
        self.send(&SimToCoordinator::Hello(Box::new(hello)))?;
        match self.recv()? {
            CoordinatorToSim::Welcome(w) => Ok(w),
            other => Err(self.unexpected("welcome", &other)),
        }
    }

    fn decide(&mut self, obs: Observation) -> Result<Decision, BenchError> {
        self.send(&SimToCoordinator::Observation(obs))?;
        match self.recv()? {
            CoordinatorToSim::Decision(d) => Ok(d),
            other => Err(self.unexpected("decision", &other)),
        }
    }

    /// Send `end`, close the pipe, and give the process `timeout` to exit.
    fn finish(&mut self, end: End) -> Result<(), BenchError> {
        self.send(&SimToCoordinator::End(end))?;
        drop(self.stdin.take());
        let deadline = Instant::now() + self.timeout;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(BenchError::Wire(e.into())),
            }
        }
        let _ = self.child.kill();
        Ok(())
    }

    fn abort(&mut self, code: ErrorCode, message: &str) {
        let _ = self.send(&SimToCoordinator::Error(ErrorMessage {
            code,
            message: message.into(),
        }));
    }
}

impl Drop for CoordinatorProcess {
    fn drop(&mut self) {
        // Never leave a coordinator running after an aborted session.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ----- in-process -----

/// A coordinator implemented in Rust, run inside the bench.
pub trait Agent {
    fn welcome(&mut self, hello: &Hello) -> SoftwareInfo;
    fn decide(&mut self, obs: &Observation) -> Decision;
    fn end(&mut self, _end: &End) {}
}

pub struct InProcess<A: Agent> {
    pub agent: A,
    validator: Validator,
}

impl<A: Agent> InProcess<A> {
    pub fn new(agent: A) -> Result<Self, BenchError> {
        Ok(Self {
            agent,
            validator: coordinator_validator()?,
        })
    }

    /// Same schema check a subprocess gets, so in-process agents can't cheat.
    fn checked(&self, msg: CoordinatorToSim) -> Result<CoordinatorToSim, BenchError> {
        let raw = serde_json::to_value(&msg).map_err(|e| BenchError::Protocol(e.to_string()))?;
        check(&self.validator, raw).map_err(BenchError::Protocol)
    }
}

impl<A: Agent> CoordinatorLink for InProcess<A> {
    fn start(&mut self, hello: Hello) -> Result<Welcome, BenchError> {
        let coordinator = self.agent.welcome(&hello);
        match self.checked(CoordinatorToSim::Welcome(Welcome {
            protocol_version: arcadia_protocol::PROTOCOL_VERSION.into(),
            coordinator,
        }))? {
            CoordinatorToSim::Welcome(w) => Ok(w),
            _ => unreachable!(),
        }
    }

    fn decide(&mut self, obs: Observation) -> Result<Decision, BenchError> {
        let d = self.agent.decide(&obs);
        match self.checked(CoordinatorToSim::Decision(d))? {
            CoordinatorToSim::Decision(d) => Ok(d),
            _ => unreachable!(),
        }
    }

    fn finish(&mut self, end: End) -> Result<(), BenchError> {
        self.agent.end(&end);
        Ok(())
    }

    fn abort(&mut self, _code: ErrorCode, _message: &str) {}
}
