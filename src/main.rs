//! `arcadiabench run [--seed N] [--log FILE] -- <coordinator> [args...]`

use arcadiabench::log::EventLog;
use arcadiabench::run::run_placeholder;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

const USAGE: &str = "usage: arcadiabench run [--seed N] [--log FILE] -- <coordinator> [args...]";

struct Args {
    seed: u64,
    log: Option<String>,
    coordinator: Vec<String>,
}

fn parse(mut argv: impl Iterator<Item = String>) -> Result<Args, String> {
    match argv.next().as_deref() {
        Some("run") => {}
        Some("-h" | "--help") | None => return Err(USAGE.into()),
        Some(other) => return Err(format!("unknown command '{other}'\n{USAGE}")),
    }
    let mut args = Args {
        seed: 0,
        log: None,
        coordinator: Vec::new(),
    };
    while let Some(a) = argv.next() {
        match a.as_str() {
            "--seed" => {
                let v = argv.next().ok_or("--seed needs a value")?;
                args.seed = v.parse().map_err(|_| format!("bad seed '{v}'"))?;
            }
            "--log" => args.log = Some(argv.next().ok_or("--log needs a path")?),
            "--" => {
                args.coordinator = argv.collect();
                break;
            }
            other => return Err(format!("unknown option '{other}'\n{USAGE}")),
        }
    }
    if args.coordinator.is_empty() {
        return Err(format!("missing coordinator command\n{USAGE}"));
    }
    Ok(args)
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let sink: Box<dyn Write> = match &args.log {
        Some(path) => match File::create(path) {
            Ok(f) => Box::new(BufWriter::new(f)),
            Err(e) => {
                eprintln!("cannot create {path}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => Box::new(io::sink()),
    };
    let mut log = EventLog::new(sink);
    let (program, rest) = args.coordinator.split_first().expect("non-empty");
    match run_placeholder(program, rest, args.seed, &mut log) {
        Ok(report) => {
            println!(
                "{} {}: {} decision(s), {} command(s)",
                report.coordinator.name,
                report.coordinator.version,
                report.decisions,
                report.commands
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("arcadiabench: {e}");
            ExitCode::FAILURE
        }
    }
}
