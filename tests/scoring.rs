//! Scoring end to end: what a batch writes does not depend on `--jobs`,
//! everything it writes matches `schemas/scorecard.schema.json`, saved logs
//! score to the same scorecards, and the command line ties it together.

mod support;

use arcadia_protocol::EndReason;
use arcadiabench::bench::{self, Options};
use arcadiabench::coordinator::{BenchError, CoordinatorLink, InProcess};
use arcadiabench::score::{Report, RunRecord, Scorecard, logs};
use arcadiabench::sim::params::{Preset, Variant};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use support::builder::Builder;

const NULL: &str = env!("CARGO_BIN_EXE_null-coordinator");
const CLI: &str = env!("CARGO_BIN_EXE_arcadiabench");

fn builder() -> Result<Box<dyn CoordinatorLink>, BenchError> {
    Ok(Box::new(InProcess::new(Builder::new())?))
}

fn null() -> impl Fn() -> Result<Box<dyn CoordinatorLink>, BenchError> + Sync {
    bench::command(vec![NULL.into()], Duration::from_secs(10))
}

/// Every file under `dir`, by relative path, with its bytes.
fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.insert(p.strip_prefix(root).unwrap().into(), fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn opts(out: &Path, jobs: usize) -> Options {
    Options {
        jobs,
        out: Some(out.into()),
        save_logs: true,
    }
}

#[test]
fn what_a_batch_writes_does_not_depend_on_jobs() {
    let seeds = [1, 2, 3, 4, 5];
    let (one, many) = (
        support::scratch_dir("jobs-1"),
        support::scratch_dir("jobs-4"),
    );
    let a = bench::bench(
        &builder,
        Preset::Small,
        Variant::Nominal,
        &seeds,
        &opts(&one, 1),
        &|_, _| true,
    )
    .unwrap();
    let b = bench::bench(
        &builder,
        Preset::Small,
        Variant::Nominal,
        &seeds,
        &opts(&many, 4),
        &|_, _| true,
    )
    .unwrap();
    assert_eq!(a.scorecard, b.scorecard);
    assert_eq!(
        a.scorecard.completion.completed, 5,
        "the builder finishes every seed"
    );
    let (fa, fb) = (files(&one), files(&many));
    assert_eq!(fa.len(), 2 + seeds.len(), "{:?}", fa.keys());
    assert_eq!(fa, fb, "byte-identical outputs");

    // A suite of subprocess runs, unevenly spread over the threads.
    let (one, many) = (
        support::scratch_dir("suite-jobs-1"),
        support::scratch_dir("suite-jobs-3"),
    );
    let presets = [Preset::Small, Preset::Large];
    let variants = [
        Variant::Nominal,
        Variant::CascadingFaults,
        Variant::DustStorm,
    ];
    let connect = null();
    let a = bench::suite(
        &connect,
        &presets,
        &variants,
        &[1, 2],
        &opts(&one, 1),
        &|_, _| true,
    )
    .unwrap();
    let b = bench::suite(
        &connect,
        &presets,
        &variants,
        &[1, 2],
        &opts(&many, 3),
        &|_, _| true,
    )
    .unwrap();
    assert_eq!(a.report, b.report);
    let (fa, fb) = (files(&one), files(&many));
    assert_eq!(fa.len(), 1 + 6 * (2 + 2));
    assert_eq!(fa, fb, "byte-identical outputs");
}

#[test]
fn everything_a_batch_writes_matches_the_schema() {
    let out = support::scratch_dir("schema");
    let built = bench::bench(
        &builder,
        Preset::Small,
        Variant::Nominal,
        &[1, 2, 3],
        &opts(&out, 3),
        &|_, _| true,
    )
    .unwrap();
    let card = support::scorecard_validator("");
    let record = support::scorecard_validator("RunRecord");
    let report = support::scorecard_validator("Report");
    let read =
        |p: &Path| -> Value { serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap() };
    let check = |v: &jsonschema::Validator, json: &Value| {
        if let Err(e) = v.validate(json) {
            panic!("{e} at {}: {json}", e.instance_path());
        }
    };
    check(&card, &read(&out.join("scorecard.json")));
    for line in fs::read_to_string(out.join("metrics.jsonl"))
        .unwrap()
        .lines()
    {
        check(&record, &serde_json::from_str(line).unwrap());
    }
    // A suite where nothing finishes: every optional number is null.
    let suite_out = support::scratch_dir("schema-suite");
    let suite = bench::suite(
        &null(),
        &[Preset::Medium],
        &[
            Variant::Nominal,
            Variant::DustStorm,
            Variant::Conjunction,
            Variant::CascadingFaults,
        ],
        &[1],
        &Options {
            jobs: 4,
            out: Some(suite_out.clone()),
            save_logs: false,
        },
        &|_, _| true,
    )
    .unwrap();
    check(&report, &read(&suite_out.join("suite.json")));
    assert_eq!(suite.report.scorecards[0].completion.mean_sols, None);
    assert_eq!(suite.report.resilience.len(), 1);
    assert_eq!(
        suite.report.resilience[0].presets[0].dust_storm, None,
        "nominal scored 0"
    );
    // And a report mixing finished and unfinished scenarios.
    let mut runs: Vec<RunRecord> = built.runs.clone();
    runs.extend(suite.cells.iter().flat_map(|c| c.runs.iter().cloned()));
    check(
        &report,
        &serde_json::to_value(Report::from_runs(&runs).unwrap()).unwrap(),
    );

    // The schema is strict.
    let good = serde_json::to_value(&built.scorecard).unwrap();
    for (what, broken) in [
        ("headline above 1", {
            let mut b = good.clone();
            b["headline"] = 1.5.into();
            b
        }),
        ("a missing group", {
            let mut b = good.clone();
            b.as_object_mut().unwrap().remove("fleet");
            b
        }),
        ("an unknown field", {
            let mut b = good.clone();
            b["extra"] = 1.into();
            b
        }),
        ("an unknown variant", {
            let mut b = good.clone();
            b["variant"] = "sandstorm".into();
            b
        }),
        ("no seeds", {
            let mut b = good.clone();
            b["seeds"] = Value::Array(Vec::new());
            b
        }),
    ] {
        assert!(card.validate(&broken).is_err(), "{what} passed");
    }
}

#[test]
fn saved_logs_score_to_the_same_scorecards() {
    let out = support::scratch_dir("rescore");
    let result = bench::bench(
        &builder,
        Preset::Medium,
        Variant::CascadingFaults,
        &[2, 4, 6],
        &opts(&out, 3),
        &|_, _| true,
    )
    .unwrap();
    let records = logs::read_logs(std::slice::from_ref(&out)).unwrap();
    assert_eq!(records.len(), 3, "metrics.jsonl is not a log");
    assert_eq!(records, result.runs);
    let report = Report::from_runs(&records).unwrap();
    assert_eq!(report.scorecards, std::slice::from_ref(&result.scorecard));
    // A log named on its own scores too, and a truncated one is refused.
    let one = out
        .join("logs")
        .join("landing-pad-medium-cascading_faults-4.jsonl.gz");
    assert_eq!(
        logs::read_logs(std::slice::from_ref(&one)).unwrap()[0].seed,
        4
    );
    let text = support::gunzip(&one);
    let cut = out.join("cut.jsonl");
    fs::write(&cut, &text[..text.len() / 2]).unwrap();
    assert!(logs::read_logs(&[cut]).is_err());
    let scorecard: Scorecard =
        serde_json::from_str(&fs::read_to_string(out.join("scorecard.json")).unwrap()).unwrap();
    assert_eq!(scorecard, result.scorecard, "scorecard.json round-trips");
    // Named as a file, metrics.jsonl gives the bench's run records, without
    // the logs; something else named as a file is refused.
    let metrics = out.join("metrics.jsonl");
    assert_eq!(
        logs::read_logs(std::slice::from_ref(&metrics)).unwrap(),
        result.runs
    );
    let other = out.join("other.jsonl");
    fs::write(&other, "{\"hello\":1}\n").unwrap();
    assert!(logs::read_logs(&[other]).is_err());
}

fn cli(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(CLI).args(args).output().unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    )
}

#[test]
fn the_command_line_benches_scores_and_suites() {
    let dir = support::scratch_dir("cli");
    let out = dir.join("bench");
    let o = out.to_str().unwrap();
    let (code, stdout, stderr) = cli(&[
        "bench",
        "--preset",
        "medium",
        "--variant",
        "dust_storm",
        "--seeds",
        "1-2",
        "--jobs",
        "2",
        "--out",
        o,
        "--save-logs",
        "--",
        NULL,
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("on landing-pad-medium-dust_storm (2 runs, seeds 1-2)"),
        "{stdout}"
    );
    assert!(stdout.contains("headline       0.000"), "{stdout}");
    for f in [
        "metrics.jsonl",
        "scorecard.json",
        "logs/landing-pad-medium-dust_storm-2.jsonl.gz",
    ] {
        assert!(out.join(f).is_file(), "{f}");
    }
    let (code, json, stderr) = cli(&["score", "--json", o]);
    assert_eq!(code, 0, "{stderr}");
    let report: Report = serde_json::from_str(&json).unwrap();
    let written: Scorecard =
        serde_json::from_str(&fs::read_to_string(out.join("scorecard.json")).unwrap()).unwrap();
    assert_eq!(report.scorecards, [written]);
    let (code, table, _) = cli(&["score", o]);
    assert_eq!(code, 0);
    assert!(table.contains("fleet health"), "{table}");

    let suite = dir.join("suite");
    let (code, stdout, stderr) = cli(&[
        "suite",
        "--presets",
        "small",
        "--variants",
        "nominal,conjunction",
        "--seeds",
        "5",
        "--out",
        suite.to_str().unwrap(),
        "--",
        NULL,
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("landing-pad-small-conjunction"), "{stdout}");
    assert!(stdout.contains("resilience"), "{stdout}");
    assert!(suite.join("suite.json").is_file());
    assert!(
        suite
            .join("small-conjunction")
            .join("scorecard.json")
            .is_file()
    );

    // `run` keeps its week-6 form and takes a preset.
    let log = dir.join("run.jsonl");
    let (code, stdout, stderr) = cli(&[
        "run",
        "--seed",
        "3",
        "--preset",
        "large",
        "--variant",
        "solar_only",
        "--log",
        log.to_str().unwrap(),
        "--",
        NULL,
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("[large/solar_only]: FleetLost"), "{stdout}");
    let record = &logs::read_logs(&[log]).unwrap()[0];
    assert_eq!(record.scenario_id, "landing-pad-large-solar_only");
    assert_eq!(record.reason, EndReason::FleetLost);
    let (code, _, _) = cli(&["run", "--seed", "1", "--", NULL]);
    assert_eq!(code, 0);

    // Bad usage, and a coordinator that cannot start.
    assert_eq!(cli(&["bench", "--preset", "huge", "--", NULL]).0, 2);
    assert_eq!(cli(&["score"]).0, 2);
    let (code, _, stderr) = cli(&[
        "bench",
        "--seeds",
        "1",
        "--out",
        o,
        "--",
        "/nonexistent/coordinator",
    ]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("seed 1: could not launch coordinator"),
        "{stderr}"
    );
}

#[test]
fn a_batch_can_be_cancelled() {
    let opts = Options {
        jobs: 1,
        out: None,
        save_logs: false,
    };
    let seen = std::sync::atomic::AtomicUsize::new(0);
    let result = bench::bench(
        &null(),
        Preset::Small,
        Variant::Nominal,
        &[1, 2, 3, 4],
        &opts,
        &|done, total| {
            seen.store(done, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(total, 4);
            done < 2
        },
    );
    assert!(matches!(result, Err(bench::Error::Cancelled)), "{result:?}");
    assert_eq!(
        seen.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "no run starts after the cancel"
    );
}
