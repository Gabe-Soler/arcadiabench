//! Run a coordinator subprocess through one benchmark scenario.

use crate::coordinator::{BenchError, CoordinatorProcess};
use crate::log::EventLog;
use crate::session::{RunReport, run_session};
use crate::sim::params::Params;
use std::io::Write;
use std::time::Duration;

pub const DEFAULT_DECISION_TIMEOUT: Duration = Duration::from_secs(10);

pub fn run_process<W: Write>(
    program: &str,
    args: &[String],
    seed: u64,
    decision_timeout: Duration,
    log: &mut EventLog<W>,
) -> Result<RunReport, BenchError> {
    let mut link = CoordinatorProcess::spawn(program, args, decision_timeout)?;
    run_session(&mut link, seed, &Params::default(), log)
}
