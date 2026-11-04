//! `arcadiabench run [--seed N] [--variant NAME] [--log FILE] [--decision-timeout SECS] -- <coordinator> [args...]`

use arcadiabench::log::EventLog;
use arcadiabench::run::{DEFAULT_DECISION_TIMEOUT, run_process};
use arcadiabench::sim::params::{Params, Variant};
use arcadiabench::sim::time::SOL_MS;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "usage: arcadiabench run [--seed N] [--variant nominal|cascading_faults|solar_only|nuclear_heavy|conjunction] [--log FILE] [--decision-timeout SECS] -- <coordinator> [args...]";

struct Args {
    seed: u64,
    variant: Variant,
    log: Option<String>,
    timeout: Duration,
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
        variant: Variant::default(),
        log: None,
        timeout: DEFAULT_DECISION_TIMEOUT,
        coordinator: Vec::new(),
    };
    while let Some(a) = argv.next() {
        match a.as_str() {
            "--seed" => {
                let v = argv.next().ok_or("--seed needs a value")?;
                args.seed = v.parse().map_err(|_| format!("bad seed '{v}'"))?;
            }
            "--variant" => {
                let v = argv.next().ok_or("--variant needs a name")?;
                args.variant = v.parse()?;
            }
            "--log" => args.log = Some(argv.next().ok_or("--log needs a path")?),
            "--decision-timeout" => {
                let v = argv.next().ok_or("--decision-timeout needs seconds")?;
                let s: f64 = v.parse().map_err(|_| format!("bad timeout '{v}'"))?;
                if !(s > 0.0 && s.is_finite()) {
                    return Err(format!("bad timeout '{v}'"));
                }
                args.timeout = Duration::from_secs_f64(s);
            }
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
    let params = Params::variant(args.variant);
    match run_process(program, rest, args.seed, &params, args.timeout, &mut log) {
        Ok(r) => {
            let m = &r.metrics;
            println!(
                "{} {} [{}]: {:?} at sol {:.2} | pad {:.0}% | {} decisions, {} commands | {} faults, {} lost | {} to Earth, {} violations | link down {:.1} h",
                r.coordinator.name,
                r.coordinator.version,
                args.variant,
                r.end_reason,
                r.end_t / (SOL_MS as f64 / 1000.0),
                r.progress * 100.0,
                r.decisions,
                r.commands,
                m.fleet.faults,
                m.fleet.robots_lost,
                m.autonomy.downlink_messages,
                m.autonomy.policy_violations,
                (m.conditions.blackout_s + m.conditions.outage_s) / 3600.0
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("arcadiabench: {e}");
            ExitCode::FAILURE
        }
    }
}
