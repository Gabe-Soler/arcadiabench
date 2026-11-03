//! Baseline coordinator that never acts: replies to every observation with an
//! empty decision. Useful as a floor on the leaderboard and as a protocol test
//! peer. The random, greedy and first-come-first-served baselines will sit
//! alongside it.

use arcadia_protocol::*;
use std::io::{self, BufReader};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = io::stdout().lock();
    loop {
        let reply = match wire::recv::<_, SimToCoordinator>(&mut input) {
            Ok(Some(SimToCoordinator::Hello(h)))
                if is_compatible(&h.protocol_version, PROTOCOL_VERSION) =>
            {
                CoordinatorToSim::Welcome(Welcome {
                    protocol_version: PROTOCOL_VERSION.into(),
                    coordinator: SoftwareInfo {
                        name: "null-coordinator".into(),
                        version: env!("CARGO_PKG_VERSION").into(),
                    },
                })
            }
            Ok(Some(SimToCoordinator::Hello(h))) => {
                let _ = wire::send(
                    &mut output,
                    &CoordinatorToSim::Error(ErrorMessage {
                        code: ErrorCode::VersionMismatch,
                        message: format!("got {}, want {PROTOCOL_VERSION}", h.protocol_version),
                    }),
                );
                return ExitCode::FAILURE;
            }
            Ok(Some(SimToCoordinator::Observation(obs))) => {
                CoordinatorToSim::Decision(Decision::empty(obs.seq))
            }
            Ok(Some(SimToCoordinator::End(_))) | Ok(None) => return ExitCode::SUCCESS,
            Ok(Some(SimToCoordinator::Error(e))) => {
                eprintln!("null-coordinator: simulator error: {}", e.message);
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("null-coordinator: {e}");
                return ExitCode::FAILURE;
            }
        };
        if wire::send(&mut output, &reply).is_err() {
            return ExitCode::FAILURE;
        }
    }
}
