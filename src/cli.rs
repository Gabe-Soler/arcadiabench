//! The `arcadiabench` command line. The binary runs it, and so does the
//! Python package (`python -m arcadiabench`).
//!
//! - `run`: one run of a coordinator, with an optional event log.
//! - `bench`: a coordinator on one scenario over many seeds, in parallel,
//!   with per-run metrics, a scorecard and optional logs.
//! - `suite`: the same across presets and variants, with resilience.
//! - `--crew FILE` on any of the three: the fleet design track, with a crew
//!   of the coordinator's own in place of a preset.
//! - `score`: scorecards and resilience from saved event logs.
//! - `scenario`: the `hello` a coordinator would get, and the goal, without
//!   running anything: what a crew designer needs to know.
//! - `crew`: whether a crew file may enter the fleet design track.

use crate::bench::{self, LogFile, Options};
use crate::coordinator::CoordinatorProcess;
use crate::log::EventLog;
use crate::presets;
use crate::run::DEFAULT_DECISION_TIMEOUT;
use crate::scenario;
use crate::score::{Report, logs};
use crate::session::{self, run_session_outcome};
use crate::sim::params::{Catalog, CrewMix, Lineup, Params, Preset, Variant};
use arcadia_protocol::SimToCoordinator;
use arcadia_protocol::wire::{self, WireError};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How to use the command line.
pub const USAGE: &str = "\
usage:
  arcadiabench run   [--seed N] [--catalog C] [--preset P | --crew FILE] [--variant V]
                     [--log FILE] [--decision-timeout SECS] -- <coordinator> [args...]
  arcadiabench bench [--catalog C] [--preset P | --crew FILE] [--variant V] [--seeds LIST]
                     [--jobs N] [--out DIR] [--save-logs] [--decision-timeout SECS]
                     -- <coordinator> [args...]
  arcadiabench suite [--catalog C] [--presets LIST | --crew FILE] [--variants LIST]
                     [--seeds LIST] [--jobs N] [--out DIR] [--save-logs]
                     [--decision-timeout SECS] -- <coordinator> [args...]
  arcadiabench score [--json] <log, metrics.jsonl or directory>...
  arcadiabench scenario [--seed N] [--catalog C] [--preset P | --crew FILE]
                     [--variant V] [--goal]
  arcadiabench crew  [--catalog C] <crew file>...

  scenario runs nothing: it prints the hello message a coordinator would get,
  as one line of JSON, and with --goal the goal Earth sends at the start as a
  second line. crew says whether each crew file may enter the fleet design
  track, with its robots and its mass against the budget.

  catalog   1.1 (default), 1.0
  presets   small, medium, large (default small; suite: all)
  crew      the fleet design track: a JSON file giving your own crew in place
            of a preset, such as
            {\"scout\": 1, \"excavator\": 2, \"hauler\": 1, \"compactor_sinterer\": 6}
            (4 to 20 robots, within the catalog's mass budget)
  variants  nominal, dust_storm, conjunction, cascading_faults, solar_only,
            nuclear_heavy (default nominal; suite: all)
  seeds     seeds and ranges, such as 1-50 or 1,4,10-20 (default 1-50)
  jobs      runs at once (default: the number of CPUs)
  out       bench: results/<catalog>-<preset>-<variant>, or on catalog 1.0
            results/<preset>-<variant>; suite: results";

/// The exit code for bad usage.
const USAGE_ERROR: u8 = 2;

#[derive(Debug, Default)]
struct Args {
    command: String,
    seed: Option<u64>,
    seeds: Option<Vec<u64>>,
    catalog: Option<Catalog>,
    presets: Option<Vec<Preset>>,
    crew: Option<PathBuf>,
    variants: Option<Vec<Variant>>,
    log: Option<PathBuf>,
    out: Option<PathBuf>,
    jobs: Option<usize>,
    save_logs: bool,
    json: bool,
    goal: bool,
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
        Some(c @ ("run" | "bench" | "suite" | "score" | "scenario" | "crew")) => c.to_string(),
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
            "--catalog",
            "--preset",
            "--crew",
            "--variant",
            "--log",
            "--decision-timeout",
        ],
        "bench" => &[
            "--catalog",
            "--preset",
            "--crew",
            "--variant",
            "--seeds",
            "--jobs",
            "--out",
            "--save-logs",
            "--decision-timeout",
        ],
        "suite" => &[
            "--catalog",
            "--presets",
            "--crew",
            "--variants",
            "--seeds",
            "--jobs",
            "--out",
            "--save-logs",
            "--decision-timeout",
        ],
        "scenario" => &[
            "--seed",
            "--catalog",
            "--preset",
            "--crew",
            "--variant",
            "--goal",
        ],
        "crew" => &["--catalog"],
        _ => &["--json"],
    };
    // Commands that take files to read, and none that launch a coordinator.
    let reads_files = matches!(a.command.as_str(), "score" | "crew");
    let launches = !reads_files && a.command != "scenario";
    while let Some(arg) = argv.next() {
        if arg == "--" && launches {
            a.coordinator = argv.by_ref().collect();
            break;
        }
        if !arg.starts_with("--") && reads_files {
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
            "--catalog" => a.catalog = Some(value()?.parse()?),
            "--preset" => a.presets = Some(vec![value()?.parse()?]),
            "--variant" => a.variants = Some(vec![value()?.parse()?]),
            "--presets" => a.presets = Some(list(&value()?)?),
            "--crew" => a.crew = Some(value()?.into()),
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
            "--goal" => a.goal = true,
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
    } else if a.command == "crew" {
        if a.paths.is_empty() {
            return Err(format!("crew needs a crew file\n{USAGE}"));
        }
    } else if launches && a.coordinator.is_empty() {
        return Err(format!("missing coordinator command\n{USAGE}"));
    }
    if a.crew.is_some() && a.presets.is_some() {
        return Err(format!(
            "--crew takes the place of a preset: give one or the other\n{USAGE}"
        ));
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
        "scenario" => print_scenario(&a, &mut io::stdout().lock()),
        "crew" => check_crews(&a, &mut io::stdout().lock()),
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

/// A scenario in a few words: `<preset>/<variant>` on catalog 1.0, with the
/// catalog in front on a later one, as in its id. A crew of the
/// coordinator's own reads `crew 1.2.1.6` where the preset would be.
fn label(catalog: Catalog, who: Lineup, variant: Variant) -> String {
    let who = match who {
        Lineup::Preset(preset) => preset.to_string(),
        Lineup::Crew(crew) => format!("crew {crew}"),
    };
    match catalog {
        Catalog::V1_0 => format!("{who}/{variant}"),
        Catalog::V1_1 => format!("{catalog} {who}/{variant}"),
    }
}

/// Read a crew file and check it against the fleet design track's rules.
pub fn read_crew(path: &std::path::Path, catalog: Catalog) -> Result<CrewMix, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read the crew file {}: {e}", path.display()))?;
    crew_from_json(&text, catalog).map_err(|e| format!("{}: {e}", path.display()))
}

/// A crew from the JSON of a crew file (`schemas/crew.schema.json`), checked
/// against the fleet design track's rules for `catalog`.
pub fn crew_from_json(text: &str, catalog: Catalog) -> Result<CrewMix, String> {
    let not_a_crew = |why: String| {
        format!(
            "not a crew ({why}); expected the count of each design, such as \
             {{\"scout\": 1, \"excavator\": 2, \"hauler\": 1, \"compactor_sinterer\": 6}}"
        )
    };
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| not_a_crew(e.to_string()))?;
    // An object with the four counts by name, as the schema says: serde
    // alone would also take a bare list of four numbers.
    if !value.is_object() {
        return Err(not_a_crew("not a JSON object".into()));
    }
    let crew: CrewMix = serde_json::from_value(value).map_err(|e| not_a_crew(e.to_string()))?;
    presets::check_crew(catalog, crew).map_err(|e| e.to_string())?;
    Ok(crew)
}

/// Who builds: the crew file's crew if one was given, else the preset.
fn lineup(a: &Args, catalog: Catalog) -> Result<Lineup, String> {
    Ok(match &a.crew {
        Some(path) => Lineup::Crew(read_crew(path, catalog)?),
        None => Lineup::Preset(one(&a.presets)),
    })
}

/// `scenario`: the `hello` a coordinator would get for this scenario and
/// seed, as one line of JSON, and with `--goal` the goal Earth sends at the
/// start as a second. Nothing runs.
fn print_scenario(a: &Args, out: &mut impl Write) -> Result<(), String> {
    let catalog = a.catalog.unwrap_or(Catalog::DEFAULT);
    let params = Params::lineup(catalog, lineup(a, catalog)?, one(&a.variants));
    let seed = a.seed.unwrap_or(0);
    let hello = SimToCoordinator::Hello(Box::new(session::hello(seed, &params)));
    let mut lines = wire::send(out, &hello);
    if a.goal && lines.is_ok() {
        lines = wire::send(out, &scenario::generate(seed, &params).goal);
    }
    match lines {
        // A reader that has seen enough, as in `| head -c 300`.
        Err(WireError::Io(e)) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other.map_err(|e| format!("cannot write: {e}")),
    }
}

/// `crew`: each crew file's robots and mass against the catalog's rules.
/// Fails if any may not enter the fleet design track, after reporting all.
fn check_crews(a: &Args, out: &mut impl Write) -> Result<(), String> {
    let catalog = a.catalog.unwrap_or(Catalog::DEFAULT);
    let mut refused = Vec::new();
    for path in &a.paths {
        match read_crew(path, catalog) {
            Ok(crew) => writeln!(
                out,
                "{}: crew {crew}, {} robots, {:.0} kg of the {:.0} kg catalog {catalog} allows",
                path.display(),
                crew.total(),
                presets::crew_mass_kg(catalog, crew),
                presets::mass_budget_kg(catalog)
            )
            .map_err(|e| format!("cannot write: {e}"))?,
            Err(e) => refused.push(e),
        }
    }
    if refused.is_empty() {
        Ok(())
    } else {
        Err(refused.join("\narcadiabench: "))
    }
}

fn run(a: &Args) -> Result<(), String> {
    let catalog = a.catalog.unwrap_or(Catalog::DEFAULT);
    let (who, variant) = (lineup(a, catalog)?, one(&a.variants));
    let seed = a.seed.unwrap_or(0);
    let file = match &a.log {
        Some(path) => Some(
            LogFile::create(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?,
        ),
        None => None,
    };
    let (program, rest) = a.coordinator.split_first().expect("checked by parse");
    let params = Params::lineup(catalog, who, variant);
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
        "{} {} [{}]: {:?} after {:.2} sols, speed {} | pad {:.0}% | {} decisions, {} commands | {} faults, {} lost | {} to Earth, {} violations | link down {:.1} h",
        r.coordinator.name,
        r.coordinator.version,
        label(catalog, who, variant),
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
    let catalog = a.catalog.unwrap_or(Catalog::DEFAULT);
    // A crew file is read and checked before anything runs.
    let who = lineup(a, catalog)?;
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
        let result = match who {
            Lineup::Crew(crew) => {
                bench::suite_crew(&connect, catalog, crew, &variants, &seeds, &opts, &progress)
            }
            Lineup::Preset(_) => bench::suite(
                &connect, catalog, &presets, &variants, &seeds, &opts, &progress,
            ),
        }
        .map_err(|e| e.to_string())?;
        print!("{}", result.report.render());
        let runs: Vec<_> = result.cells.into_iter().flat_map(|c| c.runs).collect();
        (runs, out)
    } else {
        let variant = one(&a.variants);
        let out = a.out.clone().unwrap_or_else(|| {
            let scenario = Params::lineup(catalog, who, variant);
            bench::cell_dir_of(&PathBuf::from("results"), &scenario)
        });
        let opts = Options {
            jobs,
            out: Some(out.clone()),
            save_logs: a.save_logs,
        };
        let result = bench::bench(&connect, catalog, who, variant, &seeds, &opts, &progress)
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
        assert_eq!(a.catalog, None, "the default catalog");
        for command in ["run", "bench", "suite"] {
            let a = parse(args(&format!("{command} --catalog 1.1 -- c"))).unwrap();
            assert_eq!(a.catalog, Some(Catalog::V1_1), "{command}");
        }
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
            "bench --catalog 2.0 -- c",
            "run --catalog -- c",
            "score --catalog 1.1 x",
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

    #[test]
    fn a_run_is_labelled_with_its_catalog_after_1_0() {
        let (p, v) = (Lineup::Preset(Preset::Large), Variant::SolarOnly);
        assert_eq!(label(Catalog::V1_0, p, v), "large/solar_only");
        assert_eq!(label(Catalog::V1_1, p, v), "1.1 large/solar_only");
        let crew = Lineup::Crew("1.2.1.6".parse().unwrap());
        assert_eq!(label(Catalog::V1_0, crew, v), "crew 1.2.1.6/solar_only");
        assert_eq!(label(Catalog::V1_1, crew, v), "1.1 crew 1.2.1.6/solar_only");
    }

    #[test]
    fn a_crew_file_takes_the_place_of_a_preset() {
        for command in ["run", "bench", "suite"] {
            let a = parse(args(&format!("{command} --crew crew.json -- c"))).unwrap();
            assert_eq!(a.crew, Some(PathBuf::from("crew.json")), "{command}");
        }
        for bad in [
            "run --crew crew.json --preset small -- c",
            "suite --presets small,large --crew crew.json -- c",
            "bench --crew -- c",
            "score --crew crew.json x",
        ] {
            assert!(parse(args(bad)).is_err(), "{bad}");
        }
        let c = Catalog::V1_1;
        let crew = crew_from_json(
            r#"{"scout": 1, "excavator": 2, "hauler": 1, "compactor_sinterer": 6}"#,
            c,
        )
        .unwrap();
        assert_eq!(crew.to_string(), "1.2.1.6");
        for (bad, why) in [
            ("[1, 2, 1, 6]", "not a crew"),
            (r#"{"scout": 1, "excavator": 2, "hauler": 1}"#, "not a crew"),
            (
                r#"{"scout": 1, "excavator": 2, "hauler": 1, "compactor_sinterer": 6, "drone": 1}"#,
                "not a crew",
            ),
            (
                r#"{"scout": 1, "excavator": 1, "hauler": 1, "compactor_sinterer": -1}"#,
                "not a crew",
            ),
            (
                r#"{"scout": 1, "excavator": 1, "hauler": 1, "compactor_sinterer": 0}"#,
                "4 to 20 robots",
            ),
            (
                r#"{"scout": 3, "excavator": 4, "hauler": 3, "compactor_sinterer": 10}"#,
                "mass budget is 10000 kg",
            ),
        ] {
            let e = crew_from_json(bad, c).unwrap_err();
            assert!(e.contains(why), "{bad}: {e}");
        }
        // The same crew file can pass one catalog's budget and not another's.
        let heavy = r#"{"scout": 1, "excavator": 14, "hauler": 1, "compactor_sinterer": 0}"#;
        assert!(crew_from_json(heavy, c).is_ok());
        let e = crew_from_json(heavy, Catalog::V1_0).unwrap_err();
        assert!(e.contains("mass budget is 6000 kg"), "{e}");
        assert!(read_crew(std::path::Path::new("/nonexistent/crew.json"), c).is_err());
    }
    #[test]
    fn scenario_prints_what_a_coordinator_is_told_and_runs_nothing() {
        let line = |a: &str| -> Vec<serde_json::Value> {
            let mut out = Vec::new();
            print_scenario(&parse(args(a)).unwrap(), &mut out).unwrap();
            String::from_utf8(out)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect()
        };
        // One line: the hello of the default scenario, which is catalog
        // 1.1's Small crew on seed 0.
        let hello = line("scenario");
        assert_eq!(hello.len(), 1);
        assert_eq!(hello[0]["type"], "hello");
        assert_eq!(hello[0]["run_id"], "landing-pad-1.1-small-nominal-0");
        assert_eq!(hello[0]["scenario"]["robots"].as_array().unwrap().len(), 6);
        // It is the hello a run of that scenario and seed starts with.
        let params = Params::scenario(Catalog::V1_0, Preset::Medium, Variant::SolarOnly);
        let told = line("scenario --catalog 1.0 --preset medium --variant solar_only --seed 7");
        let sent = SimToCoordinator::Hello(Box::new(session::hello(7, &params)));
        assert_eq!(told[0], serde_json::to_value(&sent).unwrap());
        // With --goal, the goal Earth sends at the start follows.
        let both = line("scenario --seed 7 --goal");
        assert_eq!(both.len(), 2);
        assert_eq!(both[1]["type"], "goal");
        assert_eq!(both[1]["objective"]["kind"], "landing_pad");
        let pad = &both[1]["objective"]["pad"]["area"];
        assert_eq!(
            (pad["min"]["x"].as_u64(), pad["max"]["x"].as_u64()),
            (Some(20), Some(39))
        );
        // It takes no coordinator, and no option of the commands that run.
        for bad in [
            "scenario -- c",
            "scenario --seeds 1-3",
            "scenario --log x",
            "scenario --preset small --crew crew.json",
            "scenario x",
        ] {
            assert!(parse(args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn crew_says_whether_a_crew_file_may_enter() {
        let dir = std::env::temp_dir().join(format!("arcadiabench-crew-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, text: &str| {
            let path = dir.join(name);
            std::fs::write(&path, text).unwrap();
            path.display().to_string()
        };
        let fits = file(
            "fits.json",
            r#"{"scout": 2, "excavator": 2, "hauler": 2, "compactor_sinterer": 9}"#,
        );
        let heavy = file(
            "heavy.json",
            r#"{"scout": 2, "excavator": 2, "hauler": 2, "compactor_sinterer": 11}"#,
        );
        let check = |a: &str| {
            let mut out = Vec::new();
            let result = check_crews(&parse(args(a)).unwrap(), &mut out);
            (result, String::from_utf8(out).unwrap())
        };
        // Its robots and its mass against the budget.
        let (result, out) = check(&format!("crew {fits}"));
        assert!(result.is_ok());
        assert!(
            out.ends_with(
                "fits.json: crew 2.2.2.9, 15 robots, 9720 kg of the 10000 kg catalog 1.1 allows\n"
            ),
            "{out}"
        );
        // Every file is reported, and one that may not enter fails the
        // command with the reason.
        let (result, out) = check(&format!("crew {heavy} {fits}"));
        let e = result.unwrap_err();
        assert!(
            e.contains("heavy.json") && e.contains("this crew weighs 11520 kg"),
            "{e}"
        );
        assert!(out.contains("fits.json: crew 2.2.2.9"), "{out}");
        // The budget is the catalog's.
        let (result, _) = check(&format!("crew --catalog 1.0 {fits}"));
        assert!(result.unwrap_err().contains("mass budget is 6000 kg"));
        for bad in ["crew", "crew --seed 1 x", "crew -- c"] {
            assert!(parse(args(bad)).is_err(), "{bad}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
