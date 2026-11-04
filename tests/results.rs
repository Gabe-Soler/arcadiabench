//! The published v1.0 results (`results/v1.0/`): every file matches its
//! schema, the per-scenario scorecards agree with `results.json`, and the
//! demo replays are complete, valid event logs within the size limit.

mod support;

use arcadia_protocol::*;
use arcadiabench::sim::params::{Preset, Variant};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const COORDINATORS: [&str; 4] = [
    "arcadiaos",
    "baseline-fcfs",
    "baseline-greedy",
    "baseline-random",
];

/// The most a demo replay may weigh, bytes.
const REPLAY_LIMIT: u64 = 3 * 1024 * 1024;

fn results() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("results/v1.0")
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_results_match_their_schemas_and_agree() {
    let report_schema = support::scorecard_validator("Report");
    let scorecard_schema = support::scorecard_validator("");
    let report = json(&results().join("results.json"));
    if let Err(e) = report_schema.validate(&report) {
        panic!("results.json: {e} at {}", e.instance_path());
    }
    let cards = report["scorecards"].as_array().unwrap();
    assert_eq!(cards.len(), COORDINATORS.len() * 18);
    let seeds: Vec<Value> = (1..=50).map(Value::from).collect();
    for coordinator in COORDINATORS {
        let dir = results().join("scorecards").join(coordinator);
        let files = fs::read_dir(&dir).unwrap().count();
        assert_eq!(files, 18, "{}", dir.display());
        for preset in Preset::ALL {
            for variant in Variant::ALL {
                let path = dir.join(format!("{preset}-{variant}.json"));
                let card = json(&path);
                if let Err(e) = scorecard_schema.validate(&card) {
                    panic!("{}: {e} at {}", path.display(), e.instance_path());
                }
                let id = format!("landing-pad-{preset}-{variant}");
                let same = cards
                    .iter()
                    .find(|c| c["coordinator"]["name"] == coordinator && c["scenario_id"] == id)
                    .unwrap_or_else(|| panic!("{coordinator} {id} is not in results.json"));
                assert_eq!(&card, same, "{}", path.display());
                assert_eq!(card["seeds"].as_array().unwrap(), &seeds);
                // What RESULTS.md reports of every scorecard.
                assert_eq!(
                    card["completion"]["success_rate"], 1.0,
                    "{coordinator} {id}"
                );
                assert_eq!(
                    card["autonomy"]["mean_policy_violations"], 0.0,
                    "{coordinator} {id}"
                );
            }
        }
    }
    let resilience = report["resilience"].as_array().unwrap();
    let names: Vec<&str> = resilience
        .iter()
        .map(|r| r["coordinator"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, COORDINATORS, "one resilience per coordinator");
}

#[test]
fn the_demo_replays_are_complete_valid_logs() {
    let mut replays: Vec<PathBuf> = fs::read_dir(results().join("replays"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    replays.sort();
    let names: Vec<String> = replays
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "landing-pad-large-nominal-1.jsonl.gz",
            "landing-pad-medium-cascading_faults-13.jsonl.gz",
            "landing-pad-medium-conjunction-1.jsonl.gz",
            "landing-pad-small-dust_storm-15.jsonl.gz",
        ]
    );
    for path in replays {
        let size = fs::metadata(&path).unwrap().len();
        assert!(size <= REPLAY_LIMIT, "{}: {size} bytes", path.display());
        let text = support::gunzip(&path);
        // Every record valid against the protocol's event_log.schema.json,
        // in time order, ending with a truth record and run_end.
        support::assert_valid_log(&text);
        let first: LogRecord = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        let LogEntry::RunStart {
            protocol_version,
            coordinator,
            run_id,
            ..
        } = first.entry
        else {
            panic!("{}: no run_start", path.display());
        };
        assert_eq!(protocol_version, PROTOCOL_VERSION);
        assert_eq!(coordinator.unwrap().name, "arcadiaos");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(format!("{run_id}.jsonl.gz"), name);
        let last: LogRecord = serde_json::from_str(text.lines().last().unwrap()).unwrap();
        match last.entry {
            LogEntry::RunEnd { reason, metrics } => {
                assert_eq!(reason, EndReason::GoalsComplete, "{name}");
                assert_eq!(metrics.autonomy.policy_violations, 0, "{name}");
            }
            other => panic!("{name} ends with {other:?}"),
        }
    }
}
