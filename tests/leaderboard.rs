//! The leaderboard's entries (`leaderboard/entries/<id>/<version>/`): every
//! entry file matches its schema, its report is a valid report that agrees
//! with its per-seed scores, and the board built from the entries matches
//! the board's schema.

mod support;

use arcadiabench::presets;
use arcadiabench::sim::params::{Catalog, CrewMix};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn validator(schema: &str) -> jsonschema::Validator {
    jsonschema::validator_for(&json(&root().join("schemas").join(schema))).unwrap()
}

fn dirs(path: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    found.sort();
    found
}

/// Every entry's directory, with its id and version from the path.
fn entries() -> Vec<(String, String, PathBuf)> {
    let mut found = Vec::new();
    for id in dirs(&root().join("leaderboard/entries")) {
        for version in dirs(&id) {
            let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
            found.push((name(&id), name(&version), version));
        }
    }
    found
}

#[test]
fn every_entry_matches_its_schema_and_its_results_agree() {
    let entry_schema = validator("leaderboard-entry.schema.json");
    let report_schema = support::scorecard_validator("Report");
    let found = entries();
    let ids: Vec<&str> = found.iter().map(|e| e.0.as_str()).collect();
    assert_eq!(
        ids,
        [
            "arcadiaos",
            "baseline-fcfs",
            "baseline-greedy",
            "baseline-random"
        ]
    );
    for (id, version, dir) in &found {
        let entry = json(&dir.join("entry.json"));
        if let Err(e) = entry_schema.validate(&entry) {
            panic!("{id} {version} entry.json: {e} at {}", e.instance_path());
        }
        // An entry is where its file says it is.
        assert_eq!(
            (entry["id"].as_str(), entry["version"].as_str()),
            (Some(id.as_str()), Some(version.as_str()))
        );
        let tracks: Vec<&str> = entry["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .collect();

        let report = json(&dir.join("public/report.json"));
        if let Err(e) = report_schema.validate(&report) {
            panic!("{id} {version} report.json: {e} at {}", e.instance_path());
        }
        let cards = report["scorecards"].as_array().unwrap();
        let fleet_cards = cards.iter().filter(|c| !c["crew"].is_null()).count();
        assert_eq!(cards.len() - fleet_cards, 18, "{id}: the Standard track");
        assert_eq!(
            fleet_cards,
            if tracks.contains(&"fleet") { 6 } else { 0 },
            "{id}"
        );
        for card in cards {
            // Its own runs, on the catalog the board is on.
            assert_eq!(card["coordinator"]["name"], id.as_str(), "{id}");
            assert_eq!(card["coordinator"]["version"], version.as_str(), "{id}");
            assert_eq!(card["catalog"], "1.1", "{id}");
            assert_eq!(card["autonomy"]["mean_policy_violations"], 0.0, "{id}");
        }

        // The per-seed scores are the runs behind the nominal headlines.
        let runs = json(&dir.join("public/runs.json"));
        let scenarios = runs["scenarios"].as_object().unwrap();
        assert_eq!(
            scenarios.len(),
            3 + usize::from(tracks.contains(&"fleet")),
            "{id}"
        );
        for (scenario, run) in scenarios {
            let seeds = run["seeds"].as_array().unwrap();
            let scores: Vec<f64> = run["scores"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_f64().unwrap())
                .collect();
            let expected: Vec<Value> = (1..=50).map(Value::from).collect();
            assert_eq!(seeds, &expected, "{id} {scenario}");
            assert_eq!(scores.len(), 50, "{id} {scenario}");
            assert!(
                scores.iter().all(|s| (0.0..=1.0).contains(s)),
                "{id} {scenario}"
            );
            let card = cards
                .iter()
                .find(|c| c["scenario_id"] == scenario.as_str())
                .unwrap_or_else(|| panic!("{id}: no scorecard for {scenario}"));
            assert_eq!(card["variant"], "nominal", "{id} {scenario}");
            let mean = scores.iter().sum::<f64>() / scores.len() as f64;
            let headline = card["headline"].as_f64().unwrap();
            assert!(
                (mean - headline).abs() < 1e-12,
                "{id} {scenario}: {mean} against {headline}"
            );
        }

        // A crew is one the track admits, at the mass the entry states.
        if tracks.contains(&"fleet") {
            let crew: CrewMix = serde_json::from_value(entry["fleet"]["crew"].clone()).unwrap();
            presets::check_crew(Catalog::V1_1, crew).unwrap();
            assert_eq!(
                entry["fleet"]["mass_kg"].as_f64().unwrap(),
                presets::crew_mass_kg(Catalog::V1_1, crew),
                "{id}"
            );
            for card in cards.iter().filter(|c| !c["crew"].is_null()) {
                assert_eq!(card["crew"], entry["fleet"]["crew"], "{id}");
            }
        } else {
            assert!(entry.get("fleet").is_none(), "{id}");
        }
        // A replay an entry names is a file in the repository.
        if let Some(replays) = entry.get("replays").and_then(Value::as_object) {
            for path in replays.values() {
                assert!(
                    root().join(path.as_str().unwrap()).is_file(),
                    "{id}: {path}"
                );
            }
        }
    }
}

#[test]
fn the_entry_schema_refuses_what_the_tools_must_not_be_given() {
    let schema = validator("leaderboard-entry.schema.json");
    let good = json(&root().join("leaderboard/entries/baseline-random/1.1.0/entry.json"));
    assert!(schema.is_valid(&good));
    let with = |path: &[&str], value: Value| {
        let mut entry = good.clone();
        let mut at = &mut entry;
        for key in &path[..path.len() - 1] {
            at = &mut at[*key];
        }
        at[path[path.len() - 1]] = value;
        entry
    };
    for (what, bad) in [
        ("an id with a path in it", with(&["id"], "../x".into())),
        ("an id with a space", with(&["id"], "my entry".into())),
        (
            "a branch for a commit",
            with(&["source", "commit"], "main".into()),
        ),
        (
            "a short hash",
            with(&["source", "commit"], "7db45ce".into()),
        ),
        (
            "a repository elsewhere",
            with(&["source", "repository"], "https://example.com/a/b".into()),
        ),
        (
            "a repository with a command in it",
            with(
                &["source", "repository"],
                "https://github.com/a/b; rm -rf /".into(),
            ),
        ),
        (
            "plain http",
            with(&["source", "repository"], "http://github.com/a/b".into()),
        ),
        (
            "an unknown track",
            with(&["tracks"], serde_json::json!(["standard", "speedrun"])),
        ),
        ("no track", with(&["tracks"], serde_json::json!([]))),
        (
            "a field nobody defined",
            with(&["script"], "<script>".into()),
        ),
        (
            "a name of 200 characters",
            with(&["name"], "x".repeat(200).into()),
        ),
        (
            "a replay outside results",
            with(
                &["replays"],
                serde_json::json!({"standard": "https://example.com/x.jsonl.gz"}),
            ),
        ),
    ] {
        assert!(!schema.is_valid(&bad), "{what} was accepted");
    }
}

#[test]
fn the_board_built_from_the_entries_matches_its_schema() {
    // The board is built by a Node script, as the Pages workflow does it.
    let dir = support::scratch_dir("leaderboard");
    let out = dir.join("board.json");
    let built = Command::new("node")
        .arg(root().join("site/tools/build-board.mjs"))
        .arg(&out)
        .output();
    let built = match built {
        Ok(o) => o,
        Err(e) => {
            eprintln!(
                "the_board_built_from_the_entries_matches_its_schema: skipped, node is not available ({e})"
            );
            return;
        }
    };
    assert!(
        built.status.success(),
        "build-board failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let board = json(&out);
    if let Err(e) = validator("leaderboard.schema.json").validate(&board) {
        panic!("board.json: {e} at {}", e.instance_path());
    }
    let rows = |track: &str| board["tracks"][track]["rows"].as_array().unwrap().clone();
    let ids = |track: &str| -> Vec<String> {
        rows(track)
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        ids("standard"),
        [
            "arcadiaos",
            "baseline-greedy",
            "baseline-random",
            "baseline-fcfs"
        ]
    );
    assert_eq!(ids("fleet"), ["arcadiaos", "baseline-greedy"]);
    // Ranks start at one, never go down, and the first is not shared here.
    let ranks: Vec<u64> = rows("standard")
        .iter()
        .map(|r| r["rank"].as_u64().unwrap())
        .collect();
    assert_eq!(ranks[0], 1);
    assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "{ranks:?}");
    assert_eq!(rows("standard")[0]["tied"], false);
    // The same entries give the same file.
    let again = dir.join("board-again.json");
    let status = Command::new("node")
        .arg(root().join("site/tools/build-board.mjs"))
        .arg(&again)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(fs::read(&out).unwrap(), fs::read(&again).unwrap());
}
