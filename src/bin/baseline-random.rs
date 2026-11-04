//! Baseline coordinator: each ready task, in priority order, goes to a
//! capable free robot drawn at random (seeded with the run's seed).
//! See `arcadiabench::baselines` for what every baseline shares.

use arcadiabench::baselines::{Allocation, serve};
use std::process::ExitCode;

fn main() -> ExitCode {
    serve(Allocation::Random)
}
