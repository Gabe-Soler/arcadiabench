//! Baseline coordinator: each ready task, in priority order, goes to the
//! capable free robot with the lowest estimated cost (travel plus work).
//! See `arcadiabench::baselines` for what every baseline shares.

use arcadiabench::baselines::{Allocation, serve};
use std::process::ExitCode;

fn main() -> ExitCode {
    serve(Allocation::Greedy)
}
