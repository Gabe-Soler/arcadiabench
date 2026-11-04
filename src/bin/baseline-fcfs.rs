//! Baseline coordinator, first come, first served: robots in the order
//! they became free each take the oldest ready task they can do.
//! See `arcadiabench::baselines` for what every baseline shares.

use arcadiabench::baselines::{Allocation, serve};
use std::process::ExitCode;

fn main() -> ExitCode {
    serve(Allocation::Fcfs)
}
