//! The published results (`results/v1.0/` and `results/v1.1/`): every file
//! matches its schema, the per-scenario scorecards agree with
//! `results.json`, and the demo replays are complete, valid event logs
//! within the size limit.

mod support;

use arcadia_protocol::*;
use arcadiabench::cli::crew_from_json;
use arcadiabench::sim::params::{Catalog, Preset, Variant};
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

/// One published set of results.
struct Published {
    /// Its directory under `results/`.
    version: &'static str,
    /// What comes before the preset in a scenario's directory name and
    /// after `landing-pad-` in its id: nothing on catalog 1.0.
    prefix: &'static str,
    /// The protocol version its replays were written on.
    protocol: &'static str,
    /// The fleet design track's crew, and the coordinators that ran it.
    fleet: Option<(&'static str, &'static [&'static str])>,
    /// The demo replays, in name order.
    replays: &'static [&'static str],
}

const V1_0: Published = Published {
    version: "v1.0",
    prefix: "",
    protocol: "1.0.0",
    fleet: None,
    replays: &[
        "landing-pad-large-nominal-1.jsonl.gz",
        "landing-pad-medium-cascading_faults-13.jsonl.gz",
        "landing-pad-medium-conjunction-1.jsonl.gz",
        "landing-pad-small-dust_storm-15.jsonl.gz",
    ],
};

const V1_1: Published = Published {
    version: "v1.1",
    prefix: "1.1-",
    protocol: "1.1.0",
    fleet: Some(("2.1.1.10", &["arcadiaos", "baseline-greedy"])),
    replays: &[
        "landing-pad-1.1-crew.2.1.1.10-nominal-1.jsonl.gz",
        "landing-pad-1.1-large-dust_storm-1.jsonl.gz",
        "landing-pad-1.1-medium-cascading_faults-13.jsonl.gz",
        "landing-pad-1.1-medium-conjunction-1.jsonl.gz",
        "landing-pad-1.1-medium-nominal-1.jsonl.gz",
    ],
};

fn results(p: &Published) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("results")
        .join(p.version)
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// Every scorecard file of `p` is valid, is the same as its entry in
/// `results.json`, and covers seeds 1 to 50. Returns the scorecards.
fn checked_scorecards(p: &Published) -> Vec<Value> {
    let report_schema = support::scorecard_validator("Report");
    let scorecard_schema = support::scorecard_validator("");
    let report = json(&results(p).join("results.json"));
    if let Err(e) = report_schema.validate(&report) {
        panic!("{} results.json: {e} at {}", p.version, e.instance_path());
    }
    let cards = report["scorecards"].as_array().unwrap();
    let seeds: Vec<Value> = (1..=50).map(Value::from).collect();
    let mut seen = 0;
    for coordinator in COORDINATORS {
        // The scenarios this coordinator ran: every preset and variant,
        // and the fleet design track's crew if it took it.
        let mut names: Vec<String> = Vec::new();
        for preset in Preset::ALL {
            for variant in Variant::ALL {
                names.push(format!("{}{preset}-{variant}", p.prefix));
            }
        }
        if let Some((crew, who)) = p.fleet
            && who.contains(&coordinator)
        {
            for variant in Variant::ALL {
                names.push(format!("{}crew.{crew}-{variant}", p.prefix));
            }
        }
        let dir = results(p).join("scorecards").join(coordinator);
        let files = fs::read_dir(&dir).unwrap().count();
        assert_eq!(files, names.len(), "{}", dir.display());
        for name in names {
            let path = dir.join(format!("{name}.json"));
            let card = json(&path);
            if let Err(e) = scorecard_schema.validate(&card) {
                panic!("{}: {e} at {}", path.display(), e.instance_path());
            }
            let id = format!("landing-pad-{name}");
            let same = cards
                .iter()
                .find(|c| c["coordinator"]["name"] == coordinator && c["scenario_id"] == id)
                .unwrap_or_else(|| panic!("{coordinator} {id} is not in results.json"));
            assert_eq!(&card, same, "{}", path.display());
            assert_eq!(card["seeds"].as_array().unwrap(), &seeds);
            seen += 1;
        }
    }
    assert_eq!(
        cards.len(),
        seen,
        "{}: scorecards in results.json",
        p.version
    );
    // One resilience per coordinator, and one more for each that ran the
    // fleet design track's crew.
    let entries = report["resilience"].as_array().unwrap();
    let fleet = p.fleet.map_or(0, |f| f.1.len());
    assert_eq!(entries.len(), COORDINATORS.len() + fleet, "{}", p.version);
    for coordinator in COORDINATORS {
        let of = |with_crew: bool| {
            entries
                .iter()
                .filter(|r| r["coordinator"]["name"] == coordinator)
                .filter(|r| r.get("crew").is_some_and(|c| !c.is_null()) == with_crew)
                .count()
        };
        assert_eq!(of(false), 1, "{} {coordinator}", p.version);
        let entered = p.fleet.is_some_and(|f| f.1.contains(&coordinator));
        assert_eq!(
            of(true),
            usize::from(entered),
            "{} {coordinator}",
            p.version
        );
    }
    cards.clone()
}

#[test]
fn the_v1_0_results_match_their_schemas_and_agree() {
    let cards = checked_scorecards(&V1_0);
    assert_eq!(cards.len(), COORDINATORS.len() * 18);
    // What RESULTS.md reports of every scorecard.
    for card in &cards {
        let who = format!("{} {}", card["coordinator"]["name"], card["scenario_id"]);
        assert_eq!(card["completion"]["success_rate"], 1.0, "{who}");
        assert_eq!(card["autonomy"]["mean_policy_violations"], 0.0, "{who}");
    }
}

#[test]
fn the_v1_1_results_match_their_schemas_and_agree() {
    let cards = checked_scorecards(&V1_1);
    // Four coordinators on 18 scenarios, and two on the six variants of
    // the fleet design track.
    assert_eq!(cards.len(), COORDINATORS.len() * 18 + 2 * 6);
    // What RESULTS.md reports of them.
    let mut incomplete = Vec::new();
    for card in &cards {
        let who = format!("{} {}", card["coordinator"]["name"], card["scenario_id"]);
        assert_eq!(card["catalog"], "1.1", "{who}");
        assert_eq!(card["simulator"]["version"], "1.1.0", "{who}");
        assert_eq!(card["autonomy"]["mean_policy_violations"], 0.0, "{who}");
        assert_eq!(card["fleet"]["robots_lost"], 0, "{who}");
        assert_eq!(card["completion"]["end_reasons"]["time_limit"], 0, "{who}");
        if card["coordinator"]["name"] == "arcadiaos" {
            assert_eq!(card["coordinator"]["version"], "1.1.0", "{who}");
            assert_eq!(card["completion"]["success_rate"], 1.0, "{who}");
        }
        if card["completion"]["success_rate"] != 1.0 {
            incomplete.push((who, card["completion"]["completed"].clone()));
        }
    }
    // One run in all did not complete.
    assert_eq!(incomplete.len(), 1, "{incomplete:?}");
    assert_eq!(
        incomplete[0],
        (
            "\"baseline-greedy\" \"landing-pad-1.1-small-cascading_faults\"".to_string(),
            Value::from(49)
        )
    );
    // ArcadiaOS has the best headline in every scenario of the catalog.
    let headline = |coordinator: &str, id: &str| -> f64 {
        cards
            .iter()
            .find(|c| c["coordinator"]["name"] == coordinator && c["scenario_id"] == id)
            .unwrap()["headline"]
            .as_f64()
            .unwrap()
    };
    for preset in Preset::ALL {
        for variant in Variant::ALL {
            let id = format!("landing-pad-1.1-{preset}-{variant}");
            for baseline in &COORDINATORS[1..] {
                assert!(headline("arcadiaos", &id) > headline(baseline, &id), "{id}");
            }
        }
    }
    // The fleet design track's entry: the crew file is one the track
    // admits, and it is the crew the scorecards name.
    let (crew, _) = V1_1.fleet.unwrap();
    let file = results(&V1_1).join("fleet/crew.json");
    let designed = crew_from_json(&fs::read_to_string(&file).unwrap(), Catalog::V1_1).unwrap();
    assert_eq!(designed.to_string(), crew);
    for variant in Variant::ALL {
        let id = format!("landing-pad-1.1-crew.{crew}-{variant}");
        assert!(
            headline("arcadiaos", &id) > headline("baseline-greedy", &id),
            "{id}"
        );
    }
}

/// The replays of `p` are the ones named, each a complete valid log within
/// the size limit that ends with the goal met and no policy violation.
fn check_replays(p: &Published) {
    let mut replays: Vec<PathBuf> = fs::read_dir(results(p).join("replays"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    replays.sort();
    let names: Vec<String> = replays
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, p.replays, "{}", p.version);
    for path in replays {
        let size = fs::metadata(&path).unwrap().len();
        assert!(size <= REPLAY_LIMIT, "{}: {size} bytes", path.display());
        let text = support::gunzip(&path);
        // Every record valid against the protocol's event_log.schema.json,
        // in time order, ending with a truth record and run_end. A minor
        // version only adds, so a log written on protocol 1.0 is still
        // valid under the schemas this build carries.
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
        assert_eq!(protocol_version, p.protocol, "{}", path.display());
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

#[test]
fn the_v1_0_demo_replays_are_complete_valid_logs() {
    check_replays(&V1_0);
}

#[test]
fn the_v1_1_demo_replays_are_complete_valid_logs() {
    check_replays(&V1_1);
}
