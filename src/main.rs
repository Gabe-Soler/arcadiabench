//! `arcadiabench run | bench | suite | score`: see [`arcadiabench::cli`].

use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(arcadiabench::cli::main(std::env::args().skip(1)))
}
