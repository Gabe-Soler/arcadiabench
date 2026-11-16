//! The fleet design track end to end: a crew of the coordinator's own in
//! place of a preset, from the crew file on the command line to scorecards
//! that carry the crew and measure it against the track's one reference.

mod support;

use arcadia_protocol::validate::validator;
use arcadia_protocol::*;
use arcadiabench::bench::{self, Options};
use arcadiabench::cli::crew_from_json;
use arcadiabench::coordinator::{BenchError, CoordinatorLink, InProcess};
use arcadiabench::presets;
use arcadiabench::score::{Report, RunRecord, Scorecard, logs};
use arcadiabench::session::hello;
use arcadiabench::sim::params::{Catalog, CrewMix, FLEET_OUTPOST, Params, Preset, Variant};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use support::builder::Builder;

const NULL: &str = env!("CARGO_BIN_EXE_null-coordinator");
const CLI: &str = env!("CARGO_BIN_EXE_arcadiabench");

fn mix(sc: u32, ex: u32, hl: u32, cp: u32) -> CrewMix {
    CrewMix {
        scout: sc,
        excavator: ex,
        hauler: hl,
        compactor_sinterer: cp,
    }
}

fn builder() -> Result<Box<dyn CoordinatorLink>, BenchError> {
    Ok(Box::new(InProcess::new(Builder::new())?))
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn check(v: &jsonschema::Validator, json: &Value) {
    if let Err(e) = v.validate(json) {
        panic!("{e} at {}: {json}", e.instance_path());
    }
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
fn the_crew_schema_and_the_reader_agree() {
    let schema: Value = serde_json::from_str(presets::CREW_SCHEMA).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let v = jsonschema::options().offline().build(&schema).unwrap();
    // Crews of the right shape: the schema and the reader both take them.
    for crew in [mix(1, 1, 1, 1), mix(1, 2, 1, 6), mix(20, 0, 0, 0)] {
        let json = serde_json::to_value(crew).unwrap();
        check(&v, &json);
        for c in Catalog::ALL {
            assert_eq!(crew_from_json(&json.to_string(), c), Ok(crew));
        }
    }
    // Not crews: both refuse them.
    for bad in [
        r#"[1, 2, 1, 6]"#,
        r#"{"scout": 1, "excavator": 2, "hauler": 1}"#,
        r#"{"scout": 1, "excavator": 2, "hauler": 1, "compactor_sinterer": 6, "drone": 1}"#,
        r#"{"scout": 1, "excavator": 2, "hauler": 1, "compactor_sinterer": -1}"#,
        r#"{"scout": 1, "excavator": 2, "hauler": 1, "compactor_sinterer": 1.5}"#,
        r#"{"scout": "1", "excavator": 2, "hauler": 1, "compactor_sinterer": 6}"#,
        r#"{"scout": 21, "excavator": 0, "hauler": 0, "compactor_sinterer": 0}"#,
    ] {
        let json: Value = serde_json::from_str(bad).unwrap();
        assert!(v.validate(&json).is_err(), "the schema takes {bad}");
        for c in Catalog::ALL {
            assert!(crew_from_json(bad, c).is_err(), "the reader takes {bad}");
        }
    }
    // Two rules go beyond the schema: the size of the crew and its mass.
    let small = serde_json::to_value(mix(1, 1, 1, 0)).unwrap();
    check(&v, &small);
    let e = crew_from_json(&small.to_string(), Catalog::V1_1).unwrap_err();
    assert!(e.contains("4 to 20 robots"), "{e}");
    let heavy = serde_json::to_value(mix(3, 4, 3, 10)).unwrap();
    check(&v, &heavy);
    let e = crew_from_json(&heavy.to_string(), Catalog::V1_1).unwrap_err();
    assert!(e.contains("11850 kg") && e.contains("10000 kg"), "{e}");
}

#[test]
fn every_admitted_crew_says_a_valid_hello() {
    let v = validator("messages.schema.json", "SimToCoordinator").unwrap();
    for catalog in Catalog::ALL {
        for crew in [
            mix(1, 1, 1, 1),
            mix(2, 1, 1, 6),
            mix(0, 0, 0, 4),
            mix(20, 0, 0, 0),
        ] {
            assert_eq!(presets::check_crew(catalog, crew), Ok(()));
            for variant in Variant::ALL {
                let p = Params::fleet(catalog, crew, variant);
                let h = hello(7, &p);
                let json = serde_json::to_value(SimToCoordinator::Hello(Box::new(h.clone())));
                check(&v, &json.unwrap());
                let s = &h.scenario;
                let name = match catalog {
                    Catalog::V1_0 => format!("landing-pad-crew.{crew}-{variant}"),
                    Catalog::V1_1 => format!("landing-pad-1.1-crew.{crew}-{variant}"),
                };
                assert_eq!(s.scenario_id, name);
                assert_eq!(h.run_id, format!("{name}-7"));
                // The crew asked for, on the outpost every crew gets.
                assert_eq!(s.robots.len() as u32, crew.total());
                let of = |prefix: &str| {
                    s.robots
                        .iter()
                        .filter(|r| r.robot_id.starts_with(prefix))
                        .count() as u32
                };
                assert_eq!([of("sc-"), of("ex-"), of("hl-"), of("cp-")], crew.counts());
                assert_eq!(s.stations, presets::stations(catalog, FLEET_OUTPOST));
                assert_eq!(s.depots, presets::depots(FLEET_OUTPOST));
                // The variant's power plant, as for any preset.
                assert_eq!(s.power.fission_w, p.fission_w);
            }
        }
    }
}

#[test]
fn a_crew_of_its_own_builds_and_is_scored_against_the_track_reference() {
    // Catalog 1.0, where the scripted builder finishes in sols: the Small
    // crew with a second sinterer it never uses.
    let (catalog, crew) = (Catalog::V1_0, mix(1, 1, 1, 2));
    let out = support::scratch_dir("fleet-bench");
    let opts = Options {
        jobs: 2,
        out: Some(out.clone()),
        save_logs: true,
    };
    let seeds = [1, 2];
    let cell = bench::bench(
        &builder,
        catalog,
        crew,
        Variant::Nominal,
        &seeds,
        &opts,
        &|_, _| true,
    )
    .unwrap();
    assert_eq!((cell.crew, cell.preset), (Some(crew), FLEET_OUTPOST));
    let card = &cell.scorecard;
    assert_eq!(card.scenario_id, "landing-pad-crew.1.1.1.2-nominal");
    assert_eq!(
        (card.catalog, card.preset, card.crew),
        (Some(catalog), None, Some(crew))
    );
    assert_eq!(card.completion.completed, 2, "the builder finishes");
    // One reference for every crew: twelve sinterers fit in catalog 1.0's
    // budget, whatever this crew brought.
    let reference = 400.0 / (12.0 * 0.01) / 88_775.0;
    assert!((card.reference_sols.unwrap() - reference).abs() < 1e-12);
    for r in &cell.runs {
        assert_eq!(r.reason, EndReason::GoalsComplete);
        assert!((r.speed.unwrap() - reference / r.metrics.sols).abs() < 1e-12);
    }
    // The same crew measured against itself would score far higher: on this
    // track, bringing few sinterers costs speed.
    let own = 400.0 / (2.0 * 0.01) / 88_775.0;
    assert!(card.headline < reference / own);

    // What it wrote matches the schema, and the logs score back to it.
    check(
        &support::scorecard_validator(""),
        &read(&out.join("scorecard.json")),
    );
    let record = support::scorecard_validator("RunRecord");
    let lines = fs::read_to_string(out.join("metrics.jsonl")).unwrap();
    for line in lines.lines() {
        let json: Value = serde_json::from_str(line).unwrap();
        check(&record, &json);
        assert_eq!(json["crew"]["compactor_sinterer"], 2);
        assert!(json["preset"].is_null());
    }
    for seed in seeds {
        let log = out.join(format!(
            "logs/landing-pad-crew.1.1.1.2-nominal-{seed}.jsonl.gz"
        ));
        support::assert_valid_log(&support::gunzip(&log));
    }
    let rescored: Vec<RunRecord> = logs::read_logs(std::slice::from_ref(&out)).unwrap();
    assert_eq!(Scorecard::from_runs(&rescored).unwrap(), *card);
}

#[test]
fn a_crew_suite_gives_the_crew_its_own_resilience() {
    let (catalog, crew) = (Catalog::V1_1, mix(1, 1, 1, 3));
    let out = support::scratch_dir("fleet-suite");
    let opts = Options {
        jobs: 4,
        out: Some(out.clone()),
        save_logs: false,
    };
    let connect = bench::command(vec![NULL.into()], Duration::from_secs(10));
    let variants = [
        Variant::Nominal,
        Variant::DustStorm,
        Variant::Conjunction,
        Variant::CascadingFaults,
    ];
    let suite = bench::suite_crew(&connect, catalog, crew, &variants, &[3], &opts, &|_, _| {
        true
    })
    .unwrap();
    assert_eq!(suite.cells.len(), 4);
    for (cell, variant) in suite.cells.iter().zip(variants) {
        assert_eq!((cell.crew, cell.variant), (Some(crew), variant));
        let dir = out.join(format!("1.1-crew.1.1.1.3-{variant}"));
        check(
            &support::scorecard_validator(""),
            &read(&dir.join("scorecard.json")),
        );
        // Nothing is built, so nothing is scored.
        assert_eq!(cell.scorecard.headline, 0.0);
    }
    // The same world as the Small preset's on this seed: a crew changes who
    // builds, not what they meet.
    let preset = bench::bench(
        &connect,
        catalog,
        Preset::Small,
        Variant::DustStorm,
        &[3],
        &Options {
            jobs: 1,
            out: None,
            save_logs: false,
        },
        &|_, _| true,
    )
    .unwrap();
    let storm = |r: &RunRecord| r.metrics.conditions.storm_s;
    assert_eq!(storm(&suite.cells[1].runs[0]), storm(&preset.runs[0]));

    let json = read(&out.join("suite.json"));
    check(&support::scorecard_validator("Report"), &json);
    let report: Report = serde_json::from_value(json).unwrap();
    assert_eq!(report, suite.report);
    let [res] = report.resilience.as_slice() else {
        panic!("one crew, one resilience: {:?}", report.resilience);
    };
    assert_eq!((res.catalog, res.crew), (catalog, Some(crew)));
    assert_eq!(res.presets.len(), 1);
    assert_eq!(res.presets[0].preset, None);
    // A nominal headline of zero keeps nothing to compare.
    assert_eq!(res.resilience, None);
}

#[test]
fn the_command_line_takes_a_crew_file() {
    let dir = support::scratch_dir("fleet-cli");
    let file = |name: &str, crew: CrewMix| {
        let path = dir.join(name);
        fs::write(&path, serde_json::to_string_pretty(&crew).unwrap()).unwrap();
        path.to_str().unwrap().to_string()
    };
    let crew = file("crew.json", mix(1, 1, 1, 3));
    let log = dir.join("run.jsonl");
    let (code, stdout, stderr) = cli(&[
        "run",
        "--seed",
        "2",
        "--crew",
        &crew,
        "--variant",
        "solar_only",
        "--log",
        log.to_str().unwrap(),
        "--",
        NULL,
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("[1.1 crew 1.1.1.3/solar_only]: "),
        "{stdout}"
    );
    let record = &logs::read_logs(&[log]).unwrap()[0];
    assert_eq!(
        record.scenario_id,
        "landing-pad-1.1-crew.1.1.1.3-solar_only"
    );
    assert_eq!(record.crew, Some(mix(1, 1, 1, 3)));

    // A batch and a suite, on catalog 1.0.
    let out = dir.join("bench");
    let (code, stdout, stderr) = cli(&[
        "bench",
        "--catalog",
        "1.0",
        "--crew",
        &crew,
        "--seeds",
        "1-2",
        "--out",
        out.to_str().unwrap(),
        "--",
        NULL,
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("on landing-pad-crew.1.1.1.3-nominal (2 runs, seeds 1-2)"),
        "{stdout}"
    );
    assert!(out.join("scorecard.json").is_file());
    let suite = dir.join("suite");
    let (code, stdout, stderr) = cli(&[
        "suite",
        "--catalog",
        "1.0",
        "--crew",
        &crew,
        "--variants",
        "nominal,dust_storm",
        "--seeds",
        "1",
        "--out",
        suite.to_str().unwrap(),
        "--",
        NULL,
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("crew 1.1.1.3"), "{stdout}");
    for cell in ["crew.1.1.1.3-nominal", "crew.1.1.1.3-dust_storm"] {
        assert!(suite.join(cell).join("scorecard.json").is_file(), "{cell}");
    }
    assert!(suite.join("suite.json").is_file());

    // A crew over the budget is refused before anything runs, and says why.
    let heavy = file("heavy.json", mix(3, 4, 3, 10));
    let refused = dir.join("refused");
    let (code, _, stderr) = cli(&[
        "bench",
        "--crew",
        &heavy,
        "--seeds",
        "1",
        "--out",
        refused.to_str().unwrap(),
        "--",
        NULL,
    ]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("heavy.json")
            && stderr.contains("weighs 11850 kg")
            && stderr.contains("mass budget is 10000 kg"),
        "{stderr}"
    );
    assert!(!refused.exists(), "nothing ran");
    // The same file fits no better on catalog 1.0, and three robots are too few.
    assert_eq!(
        cli(&["run", "--catalog", "1.0", "--crew", &heavy, "--", NULL]).0,
        1
    );
    let few = file("few.json", mix(1, 1, 1, 0));
    let (code, _, stderr) = cli(&["run", "--crew", &few, "--", NULL]);
    assert_eq!(code, 1);
    assert!(stderr.contains("4 to 20 robots"), "{stderr}");
    // A file that is not a crew, a file that is not there, and a crew
    // beside a preset.
    let junk = dir.join("junk.json");
    fs::write(&junk, "{\"robots\": 6}").unwrap();
    let (code, _, stderr) = cli(&["run", "--crew", junk.to_str().unwrap(), "--", NULL]);
    assert_eq!(code, 1);
    assert!(stderr.contains("not a crew"), "{stderr}");
    let (code, _, stderr) = cli(&["run", "--crew", "/nonexistent/crew.json", "--", NULL]);
    assert_eq!(code, 1);
    assert!(stderr.contains("cannot read the crew file"), "{stderr}");
    let (code, _, stderr) = cli(&["run", "--crew", &crew, "--preset", "small", "--", NULL]);
    assert_eq!(code, 2);
    assert!(stderr.contains("one or the other"), "{stderr}");
}
