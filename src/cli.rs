//! The `arcadiabench` command line. The binary runs it, and so does the
//! Python package (`python -m arcadiabench`).
//!
//! - `run`: one run of a coordinator, with an optional event log.
//! - `bench`: a coordinator on one scenario over many seeds, in parallel,
//!   with per-run metrics, a scorecard and optional logs.
//! - `suite`: the same across presets and variants, with resilience.
//! - `score`: scorecards and resilience from saved event logs.

use crate::bench::{self, LogFile, Options};
use crate::coordinator::CoordinatorProcess;
use crate::log::EventLog;
use crate::run::DEFAULT_DECISION_TIMEOUT;
use crate::score::{Report, logs};
use crate::session::run_session_outcome;
use crate::sim::params::{Params, Preset, Variant};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How to use the command line.
pub const USAGE: &str = "\
usage:
  arcadiabench run   [--seed N] [--preset P] [--variant V] [--log FILE]
                     [--decision-timeout SECS] -- <coordinator> [args...]
  arcadiabench bench [--preset P] [--variant V] [--seeds LIST] [--jobs N] [--out DIR]
                     [--save-logs] [--decision-timeout SECS] -- <coordinator> [args...]
  arcadiabench suite [--presets LIST] [--variants LIST] [--seeds LIST] [--jobs N] [--out DIR]
                     [--save-logs] [--decision-timeout SECS] -- <coordinator> [args...]
  arcadiabench score [--json] <log, metrics.jsonl or directory>...

  presets   small, medium, large (default small; suite: all)
  variants  nominal, dust_storm, conjunction, cascading_faults, solar_only,
            nuclear_heavy (default nominal; suite: all)
  seeds     seeds and ranges, such as 1-50 or 1,4,10-20 (default 1-50)
  jobs      runs at once (default: the number of CPUs)
  out       bench: results/<preset>-<variant>; suite: results";

/// The exit code for bad usage.
const USAGE_ERROR: u8 = 2;

#[derive(Debug, Default)]
struct Args {
    command: String,
    seed: Option<u64>,
    seeds: Option<Vec<u64>>,
    presets: Option<Vec<Preset>>,
    variants: Option<Vec<Variant>>,
    log: Option<PathBuf>,
    out: Option<PathBuf>,
    jobs: Option<usize>,
    save_logs: bool,
    json: bool,
    timeout: Option<Duration>,
    paths: Vec<PathBuf>,
    coordinator: Vec<String>,
}

fn list<T: std::str::FromStr<Err = String>>(v: &str) -> Result<Vec<T>, String> {
    v.split(',').map(|x| x.trim().parse()).collect()
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut argv = args.into_iter();
    let command = match argv.next().as_deref() {
        Some(c @ ("run" | "bench" | "suite" | "score")) => c.to_string(),
        Some("-h" | "--help") | None => return Err(USAGE.into()),
        Some(other) => return Err(format!("unknown command '{other}'\n{USAGE}")),
    };
    let mut a = Args {
        command,
        ..Args::default()
    };
    let allowed: &[&str] = match a.command.as_str() {
        "run" => &[
            "--seed",
            "--preset",
            "--variant",
            "--log",
            "--decision-timeout",
        ],
        "bench" => &[
            "--preset",
            "--variant",
            "--seeds",
            "--jobs",
            "--out",
            "--save-logs",
            "--decision-timeout",
        ],
        "suite" => &[
            "--presets",
            "--variants",
            "--seeds",
            "--jobs",
            "--out",
            "--save-logs",
            "--decision-timeout",
        ],
        _ => &["--json"],
    };
    while let Some(arg) = argv.next() {
        if arg == "--" && a.command != "score" {
            a.coordinator = argv.by_ref().collect();
            break;
        }
        if !arg.starts_with("--") && a.command == "score" {
            a.paths.push(arg.into());
            continue;
        }
        if !allowed.contains(&arg.as_str()) {
            return Err(format!("unknown option '{arg}' for {}\n{USAGE}", a.command));
        }
        let mut value = || argv.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--seed" => {
                let v = value()?;
                a.seed = Some(v.parse().map_err(|_| format!("bad seed '{v}'"))?);
            }
            "--seeds" => a.seeds = Some(bench::parse_seeds(&value()?)?),
            "--preset" => a.presets = Some(vec![value()?.parse()?]),
            "--variant" => a.variants = Some(vec![value()?.parse()?]),
            "--presets" => a.presets = Some(list(&value()?)?),
            "--variants" => a.variants = Some(list(&value()?)?),
            "--log" => a.log = Some(value()?.into()),
            "--out" => a.out = Some(value()?.into()),
            "--jobs" => {
                let v = value()?;
                a.jobs = Some(
                    v.parse()
                        .ok()
                        .filter(|&n: &usize| n > 0)
                        .ok_or_else(|| format!("bad job count '{v}'"))?,
                );
            }
            "--save-logs" => a.save_logs = true,
            "--json" => a.json = true,
            "--decision-timeout" => {
                let v = value()?;
                let s: f64 = v.parse().map_err(|_| format!("bad timeout '{v}'"))?;
                if !(s > 0.0 && s.is_finite()) {
                    return Err(format!("bad timeout '{v}'"));
                }
                a.timeout = Some(Duration::from_secs_f64(s));
            }
            _ => unreachable!("only allowed options get here"),
        }
    }
    if a.command == "score" {
        if a.paths.is_empty() {
            return Err(format!("score needs logs or directories\n{USAGE}"));
        }
    } else if a.coordinator.is_empty() {
        return Err(format!("missing coordinator command\n{USAGE}"));
    }
    Ok(a)
}

/// Run the command line with `args` (without the program name). Returns the
/// exit code: 0 on success, 1 on failure, 2 on bad usage.
pub fn main(args: impl IntoIterator<Item = String>) -> u8 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return USAGE_ERROR;
        }
    };
    let result = match a.command.as_str() {
        "run" => run(&a),
        "bench" => batch(&a, false),
        "suite" => batch(&a, true),
        _ => score(&a),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("arcadiabench: {e}");
            1
        }
    }
}

fn one<T: Copy + Default>(xs: &Option<Vec<T>>) -> T {
    xs.as_ref()
        .and_then(|v| v.first().copied())
        .unwrap_or_default()
}

fn run(a: &Args) -> Result<(), String> {
    let (preset, variant) = (one(&a.presets), one(&a.variants));
    let seed = a.seed.unwrap_or(0);
    let file = match &a.log {
        Some(path) => Some(
            LogFile::create(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?,
        ),
        None => None,
    };
    let (program, rest) = a.coordinator.split_first().expect("checked by parse");
    let params = Params::scenario(preset, variant);
    let timeout = a.timeout.unwrap_or(DEFAULT_DECISION_TIMEOUT);
    let mut link = CoordinatorProcess::spawn(program, rest, timeout).map_err(|e| e.to_string())?;
    let outcome = match file {
        Some(file) => {
            let mut log = EventLog::new(file);
            let outcome = run_session_outcome(&mut link, seed, &params, &mut log);
            log.into_inner()
                .finish()
                .map_err(|e| format!("cannot write the log: {e}"))?;
            outcome
        }
        None => run_session_outcome(&mut link, seed, &params, &mut EventLog::new(io::sink())),
    }
    .map_err(|e| e.to_string())?;
    if let Some(e) = &outcome.error {
        return Err(e.to_string());
    }
    let r = bench::record(&params, seed, outcome);
    let m = &r.metrics;
    // Sols since the start of the run, which need not be t = 0.
    println!(
        "{} {} [{preset}/{variant}]: {:?} after {:.2} sols, speed {} | pad {:.0}% | {} decisions, {} commands | {} faults, {} lost | {} to Earth, {} violations | link down {:.1} h",
        r.coordinator.name,
        r.coordinator.version,
        r.reason,
        m.sols,
        r.speed.map_or_else(|| "-".into(), |s| format!("{s:.3}")),
        m.quality.cells_in_spec as f64 / m.quality.cells_total.max(1) as f64 * 100.0,
        m.decisions,
        m.commands,
        m.fleet.faults,
        m.fleet.robots_lost,
        m.autonomy.downlink_messages,
        m.autonomy.policy_violations,
        (m.conditions.blackout_s
            + m.conditions.outage_s
            + m.conditions.between_passes_s.unwrap_or(0.0))
            / 3600.0
    );
    Ok(())
}

/// A progress counter on stderr, only when it is a terminal.
fn progress(done: usize, total: usize) -> bool {
    let mut err = io::stderr();
    if err.is_terminal() {
        let _ = write!(err, "\r{done}/{total} runs");
        if done == total {
            let _ = write!(err, "\r{}\r", " ".repeat(24));
        }
        let _ = err.flush();
    }
    true
}

fn batch(a: &Args, is_suite: bool) -> Result<(), String> {
    let seeds = a.seeds.clone().unwrap_or_else(|| (1..=50).collect());
    let jobs = a
        .jobs
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let timeout = a.timeout.unwrap_or(DEFAULT_DECISION_TIMEOUT);
    let connect = bench::command(a.coordinator.clone(), timeout);
    let started = Instant::now();
    let (runs, out) = if is_suite {
        let presets = a.presets.clone().unwrap_or_else(|| Preset::ALL.to_vec());
        let variants = a.variants.clone().unwrap_or_else(|| Variant::ALL.to_vec());
        let out = a.out.clone().unwrap_or_else(|| PathBuf::from("results"));
        let opts = Options {
            jobs,
            out: Some(out.clone()),
            save_logs: a.save_logs,
        };
        let result = bench::suite(&connect, &presets, &variants, &seeds, &opts, &progress)
            .map_err(|e| e.to_string())?;
        print!("{}", result.report.render());
        let runs: Vec<_> = result.cells.into_iter().flat_map(|c| c.runs).collect();
        (runs, out)
    } else {
        let (preset, variant) = (one(&a.presets), one(&a.variants));
        let out = a
            .out
            .clone()
            .unwrap_or_else(|| bench::cell_dir(&PathBuf::from("results"), preset, variant));
        let opts = Options {
            jobs,
            out: Some(out.clone()),
            save_logs: a.save_logs,
        };
        let result = bench::bench(&connect, preset, variant, &seeds, &opts, &progress)
            .map_err(|e| e.to_string())?;
        print!("{}", result.scorecard.render());
        (result.runs, out)
    };
    let elapsed = started.elapsed().as_secs_f64();
    for r in runs.iter().filter(|r| r.error.is_some()) {
        eprintln!(
            "warning: {} ended {:?}: {}",
            r.run_id,
            r.reason,
            r.error.as_deref().unwrap_or("")
        );
    }
    println!(
        "{} runs in {elapsed:.2} s, {} at a time; results in {}",
        runs.len(),
        jobs.min(runs.len().max(1)),
        out.display()
    );
    Ok(())
}

fn score(a: &Args) -> Result<(), String> {
    let runs = logs::read_logs(&a.paths).map_err(|e| e.to_string())?;
    if runs.is_empty() {
        return Err("no event logs found".into());
    }
    let report = Report::from_runs(&runs).map_err(|e| e.to_string())?;
    if a.json {
        print!("{}", bench::to_pretty(&report));
    } else {
        print!("{}", report.render());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_each_command() {
        let a = parse(args(
            "run --seed 4 --preset large --variant dust_storm -- coord --x",
        ))
        .unwrap();
        assert_eq!(
            (a.seed, one(&a.presets), one(&a.variants)),
            (Some(4), Preset::Large, Variant::DustStorm)
        );
        assert_eq!(a.coordinator, ["coord", "--x"]);
        let a = parse(args("bench --seeds 1-3,9 --jobs 2 --save-logs -- c")).unwrap();
        assert_eq!(a.seeds.unwrap(), [1, 2, 3, 9]);
        assert_eq!((a.jobs, a.save_logs), (Some(2), true));
        let a = parse(args(
            "suite --presets small,medium --variants nominal,conjunction -- c",
        ))
        .unwrap();
        assert_eq!(a.presets.unwrap(), [Preset::Small, Preset::Medium]);
        assert_eq!(
            a.variants.unwrap(),
            [Variant::Nominal, Variant::Conjunction]
        );
        let a = parse(args("score --json a.jsonl.gz dir")).unwrap();
        assert!(a.json);
        assert_eq!(a.paths.len(), 2);
        // `run` keeps its week-6 form.
        let a = parse(args(
            "run --seed 42 --variant solar_only --log r.jsonl --decision-timeout 2.5 -- c",
        ))
        .unwrap();
        assert_eq!(a.timeout, Some(Duration::from_millis(2500)));
    }

    #[test]
    fn rejects_bad_usage() {
        for bad in [
            "",
            "fly",
            "run --seed 1",
            "run --seeds 1-3 -- c",
            "bench --preset huge -- c",
            "bench --jobs 0 -- c",
            "suite --variant nominal -- c",
            "score",
            "score --save-logs x",
            "run --decision-timeout -1 -- c",
        ] {
            assert!(parse(args(bad)).is_err(), "{bad}");
        }
        assert_eq!(main(args("bench --seeds 3-1 -- c")), USAGE_ERROR);
    }
}
