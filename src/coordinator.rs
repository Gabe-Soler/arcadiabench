//! Launch a coordinator subprocess and speak arcadia-protocol with it.
//!
//! Everything the coordinator sends is checked against the JSON Schemas before
//! it is accepted, so a coordinator in any language gets precise errors.

use arcadia_protocol::validate::validator;
use arcadia_protocol::{
    CoordinatorToSim, Decision, End, ErrorCode, ErrorMessage, Hello, Observation, PROTOCOL_VERSION,
    SimToCoordinator, SoftwareInfo, is_compatible, wire,
};
use serde_json::Value;
use std::fmt;
use std::io::BufReader;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

#[derive(Debug)]
pub enum BenchError {
    /// The coordinator process could not be started.
    Launch(std::io::Error),
    Wire(wire::WireError),
    /// The coordinator closed its stdout before the session ended.
    CoordinatorExited,
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

pub struct CoordinatorProcess {
    child: Child,
    /// `None` once closed by [`CoordinatorProcess::finish`].
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    validator: arcadia_protocol::validate::Validator,
    /// Name and version from the coordinator's `welcome`.
    pub info: SoftwareInfo,
}

impl CoordinatorProcess {
    /// Spawn `program args...`, send `hello` and wait for a compatible `welcome`.
    /// The coordinator's stderr is inherited so its logs stay visible.
    pub fn launch(program: &str, args: &[String], hello: Hello) -> Result<Self, BenchError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(BenchError::Launch)?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        let validator = validator("messages.schema.json", "CoordinatorToSim")
            .map_err(|e| BenchError::Protocol(format!("schema setup failed: {e}")))?;
        let mut proc = Self {
            child,
            stdin: Some(stdin),
            stdout,
            validator,
            info: SoftwareInfo {
                name: String::new(),
                version: String::new(),
            },
        };

        proc.send(&SimToCoordinator::Hello(Box::new(hello)))?;
        match proc.recv()? {
            CoordinatorToSim::Welcome(w) => {
                if !is_compatible(&w.protocol_version, PROTOCOL_VERSION) {
                    let message = format!(
                        "coordinator speaks {}, arcadiabench speaks {PROTOCOL_VERSION}",
                        w.protocol_version
                    );
                    proc.send_error(ErrorCode::VersionMismatch, &message);
                    return Err(BenchError::Protocol(message));
                }
                proc.info = w.coordinator;
                Ok(proc)
            }
            other => Err(proc.unexpected("welcome", &other)),
        }
    }

    /// Send an observation and wait for the matching decision.
    pub fn decide(&mut self, obs: Observation) -> Result<Decision, BenchError> {
        let seq = obs.seq;
        self.send(&SimToCoordinator::Observation(obs))?;
        match self.recv()? {
            CoordinatorToSim::Decision(d) if d.seq == seq => Ok(d),
            CoordinatorToSim::Decision(d) => {
                let message = format!(
                    "decision seq {} does not match observation seq {seq}",
                    d.seq
                );
                self.send_error(ErrorCode::UnexpectedMessage, &message);
                Err(BenchError::Protocol(message))
            }
            other => Err(self.unexpected("decision", &other)),
        }
    }

    /// Send `end`, close the pipe and wait for the process to exit.
    pub fn finish(mut self, end: End) -> Result<(), BenchError> {
        self.send(&SimToCoordinator::End(end))?;
        drop(self.stdin.take());
        self.child.wait().map_err(|e| BenchError::Wire(e.into()))?;
        Ok(())
    }

    fn send(&mut self, msg: &SimToCoordinator) -> Result<(), BenchError> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(BenchError::CoordinatorExited);
        };
        wire::send(stdin, msg).map_err(|e| match e {
            wire::WireError::Io(io) if io.kind() == std::io::ErrorKind::BrokenPipe => {
                BenchError::CoordinatorExited
            }
            e => e.into(),
        })
    }

    fn recv(&mut self) -> Result<CoordinatorToSim, BenchError> {
        let raw: Value = match wire::recv(&mut self.stdout) {
            Ok(Some(v)) => v,
            Ok(None) => return Err(BenchError::CoordinatorExited),
            Err(wire::WireError::Json(e)) => {
                let message = format!("not JSON: {e}");
                self.send_error(ErrorCode::InvalidMessage, &message);
                return Err(BenchError::Protocol(message));
            }
            Err(e) => return Err(e.into()),
        };
        if let Err(e) = self.validator.validate(&raw) {
            let message = format!("{e} at {}", e.instance_path());
            self.send_error(ErrorCode::InvalidMessage, &message);
            return Err(BenchError::Protocol(message));
        }
        let msg: CoordinatorToSim = serde_json::from_value(raw)
            .map_err(|e| BenchError::Protocol(format!("schema-valid but untyped: {e}")))?;
        if let CoordinatorToSim::Error(e) = msg {
            return Err(BenchError::Coordinator(e));
        }
        Ok(msg)
    }

    fn send_error(&mut self, code: ErrorCode, message: &str) {
        // Best effort: the coordinator may already be gone.
        let _ = self.send(&SimToCoordinator::Error(ErrorMessage {
            code,
            message: message.into(),
        }));
    }

    fn unexpected(&mut self, wanted: &str, got: &CoordinatorToSim) -> BenchError {
        let got = match got {
            CoordinatorToSim::Welcome(_) => "welcome",
            CoordinatorToSim::Decision(_) => "decision",
            CoordinatorToSim::Error(_) => "error",
        };
        let message = format!("expected {wanted}, got {got}");
        self.send_error(ErrorCode::UnexpectedMessage, &message);
        BenchError::Protocol(message)
    }
}

impl Drop for CoordinatorProcess {
    fn drop(&mut self) {
        // Never leave a coordinator running after an aborted session.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
